import AppKit
import SwiftUI

enum PRFilter: String, CaseIterable, Identifiable {
    case reviewRequested, mine, assigned, mentioned, reviewed

    var id: String { rawValue }

    var title: String {
        switch self {
        case .reviewRequested: return t("Ждут моего ревью", "Review requests")
        case .mine: return t("Мои", "Created by me")
        case .assigned: return t("Назначены мне", "Assigned to me")
        case .mentioned: return t("Упоминания", "Mentioned")
        case .reviewed: return t("Я проверял", "Reviewed by me")
        }
    }

    var icon: String {
        switch self {
        case .reviewRequested: return "eye"
        case .mine: return "person"
        case .assigned: return "person.crop.circle.badge.checkmark"
        case .mentioned: return "at"
        case .reviewed: return "checkmark.bubble"
        }
    }

    var query: String {
        switch self {
        case .reviewRequested: return Demo.query("review-requested:@me")
        case .mine: return Demo.query("author:@me")
        case .assigned: return Demo.query("assignee:@me")
        case .mentioned: return Demo.query("mentions:@me")
        case .reviewed: return Demo.query("reviewed-by:@me -author:@me")
        }
    }
}

enum IssueFilter: String, CaseIterable, Identifiable {
    case assigned, mine, mentioned

    var id: String { rawValue }

    var title: String {
        switch self {
        case .assigned: return t("Назначены мне", "Assigned to me")
        case .mine: return t("Мои", "Created by me")
        case .mentioned: return t("Упоминания", "Mentioned")
        }
    }

    var icon: String {
        switch self {
        case .assigned: return "person.crop.circle.badge.checkmark"
        case .mine: return "person"
        case .mentioned: return "at"
        }
    }

    var query: String {
        switch self {
        case .assigned: return Demo.query("assignee:@me")
        case .mine: return Demo.query("author:@me")
        case .mentioned: return Demo.query("mentions:@me")
        }
    }
}

enum SidebarSection: Hashable {
    case notifications
    case prs(PRFilter)
    case issues(IssueFilter)
    case repos
    case projects
    case claude
}

@Observable
@MainActor
final class AppModel {
    enum Phase { case checking, loggedOut, loggedIn }

    var phase: Phase = .checking
    var api: GitHubAPI?
    var user: RESTUser?
    var scopes: [String] = []
    var section: SidebarSection? = .prs(.reviewRequested)
    var detailRoot: Route?
    var detailPath: [Route] = []
    var refreshTick = 0
    var showOpenLink = false
    var clipboardRoute: Route?
    var reviewRequests: [SearchItem] = []
    var myPRs: [SearchItem] = []
    var reviewRequestCount = 0
    var myPRCount = 0
    var unreadCount = 0
    var lastBadgeRefresh: Date?
    let claude = ClaudeWorkspace()

    private var lastClipboard: String?
    private var pollTask: Task<Void, Never>?
    private var prModels: [ItemRef: PRDetailModel] = [:]
    private var prOrder: [ItemRef] = []

    var hasProjectScope: Bool {
        scopes.isEmpty || scopes.contains("read:project") || scopes.contains("project")
    }

    init() {
        URLCache.shared = URLCache(memoryCapacity: 40 << 20, diskCapacity: 200 << 20)
        Task { await restore() }
    }

    private func restore() async {
        let token = ProcessInfo.processInfo.environment["SHIPYARD_TOKEN"] ?? Keychain.load()
        guard let token else {
            phase = .loggedOut
            return
        }
        do {
            try await login(token: token, persist: false)
        } catch let e as APIError where e.status == 401 {
            Keychain.delete()
            phase = .loggedOut
        } catch {
            api = GitHubAPI(token: token)
            phase = .loggedIn
            startPolling()
        }
    }

    func login(token: String, persist: Bool = true) async throws {
        let api = GitHubAPI(token: token)
        let (user, scopes) = try await api.currentUser()
        if persist { Keychain.save(token) }
        self.api = api
        self.user = Demo.isOn ? Demo.user : user
        self.scopes = scopes
        prModels = [:]
        prOrder = []
        phase = .loggedIn
        startPolling()
    }

    func logout() {
        pollTask?.cancel()
        Keychain.delete()
        api = nil
        user = nil
        scopes = []
        detailRoot = nil
        detailPath = []
        prModels = [:]
        prOrder = []
        reviewRequests = []
        myPRs = []
        reviewRequestCount = 0
        unreadCount = 0
        phase = .loggedOut
    }

    func refresh() {
        refreshTick += 1
        Task { await refreshBadges() }
    }

    func select(_ route: Route?) {
        detailRoot = route
        detailPath = []
    }

    func open(_ route: Route, push: Bool = false) {
        if section == .claude || section == nil {
            section = .prs(.mine)
            detailPath = []
            detailRoot = route
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        if push, detailRoot != nil {
            detailPath.append(route)
        } else {
            select(route)
        }
        NSApp.activate(ignoringOtherApps: true)
    }

    func handle(url: URL) {
        guard let scheme = url.scheme, scheme.hasPrefix("shipyard") || scheme.hasPrefix("ghlite") else { return }
        if url.host == "section" {
            let parts = url.pathComponents.filter { $0 != "/" }
            switch (parts.first, parts.dropFirst().first) {
            case ("notifications", _): section = .notifications
            case ("repos", _): section = .repos
            case ("projects", _): section = .projects
            case ("claude", _): section = .claude
            case ("prs", let f): section = .prs(f.flatMap(PRFilter.init(rawValue:)) ?? .reviewRequested)
            case ("issues", let f): section = .issues(f.flatMap(IssueFilter.init(rawValue:)) ?? .assigned)
            default: break
            }
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        if url.host == "claude" {
            section = .claude
            let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems ?? []
            func value(_ name: String) -> String? { items.first { $0.name == name }?.value }
            if url.path == "/new", let folder = value("folder"), FileManager.default.fileExists(atPath: folder) {
                claude.newSession(folder: folder, prompt: value("prompt"), name: value("name"), mode: value("mode").flatMap(ClaudeMode.init(rawValue:)))
            }
            if url.path == "/resume", let id = value("id") {
                let kind = value("kind").flatMap(ClaudeTabKind.init(rawValue:))
                Task {
                    if !claude.projects.flatMap(\.sessions).contains(where: { $0.id == id }) { await claude.reloadProjects() }
                    if let s = claude.projects.flatMap(\.sessions).first(where: { $0.id == id }) { claude.resume(s, kind: kind) }
                }
            }
            NSApp.activate(ignoringOtherApps: true)
            return
        }
        let target: String
        if let q = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems?.first(where: { $0.name == "url" })?.value {
            target = q
        } else {
            target = "https://" + url.absoluteString.components(separatedBy: "://").dropFirst().joined(separator: "://")
        }
        if let route = GitHubURL.parse(target) {
            open(route)
            if case .pr(let ref) = route, URL(string: target)?.lastPathComponent == "files" { prModel(ref)?.tab = .files }
        }
    }

    func prModel(_ ref: ItemRef) -> PRDetailModel? {
        guard let api else { return nil }
        if let m = prModels[ref] {
            prOrder.removeAll { $0 == ref }
            prOrder.append(ref)
            return m
        }
        let m = PRDetailModel(ref: ref, api: api)
        prModels[ref] = m
        prOrder.append(ref)
        if prOrder.count > 15 {
            let old = prOrder.removeFirst()
            prModels[old] = nil
        }
        return m
    }

    func checkClipboard() {
        guard let s = NSPasteboard.general.string(forType: .string), s != lastClipboard else { return }
        lastClipboard = s
        guard s.count < 300, s.contains("github.com"), let route = GitHubURL.parse(s, allowShort: false) else { return }
        switch route {
        case .pr, .issue:
            if route != detailRoot { clipboardRoute = route }
        default:
            break
        }
    }

    func startPolling() {
        pollTask?.cancel()
        pollTask = Task { [weak self] in
            while !Task.isCancelled {
                await self?.refreshBadges()
                let interval = max(UserDefaults.standard.double(forKey: "pollInterval"), 60)
                try? await Task.sleep(nanoseconds: UInt64(interval * 1_000_000_000))
            }
        }
    }

    func refreshBadges() async {
        guard let api else { return }
        if Demo.isOn {
            reviewRequestCount = 3
            myPRCount = 7
            unreadCount = 12
            return
        }
        if let b = try? await api.badges() {
            reviewRequests = b.rr.nodes
            reviewRequestCount = b.rr.issueCount
            myPRs = b.mine.nodes
            myPRCount = b.mine.issueCount
        }
        if let n = try? await api.unreadNotificationCount() {
            unreadCount = n
        }
        lastBadgeRefresh = Date()
    }
}
