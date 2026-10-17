import AppKit
import SwiftUI

enum PRFilter: String, CaseIterable, Identifiable {
    case reviewRequested, mine, assigned, mentioned, reviewed

    var id: String { rawValue }

    var title: String {
        switch self {
        case .reviewRequested: return "Review requests"
        case .mine: return "Created by me"
        case .assigned: return "Assigned to me"
        case .mentioned: return "Mentioned"
        case .reviewed: return "Reviewed by me"
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
        case .assigned: return "Assigned to me"
        case .mine: return "Created by me"
        case .mentioned: return "Mentioned"
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

struct CodeOpenRequest {
    let repo: RepoRef
    let ref: String
    let path: String
    let blame: Bool
}

enum SidebarSection: Hashable {
    case notifications
    case prs(PRFilter)
    case myPRsByRepo
    case issues(IssueFilter)
    case repos
    case projects
    case claude
}

@Observable
@MainActor
final class AppModel {
    enum Phase { case checking, loggedOut, locked, loggedIn }

    var phase: Phase = .checking
    var api: GitHubAPI?
    var forge: ForgeClient?
    var account: GitAccount?
    var forgeSelection: SearchItem?
    var forgeCaps = ForgeCapabilities(pullFilters: [.reviewRequested, .mine])
    var showAddAccount = false
    var lockedAccount: GitAccount?
    var unlockRequest: GitAccount?
    var user: RESTUser?
    var scopes: [String] = []
    var section: SidebarSection? = .prs(.reviewRequested)
    var detailRoot: Route?
    var detailPath: [Route] = []
    var refreshTick = 0
    var liveTick = 0
    var showOpenLink = false
    var clipboardRoute: Route?
    var toast: String?
    var pendingCode: CodeOpenRequest?
    var reviewRequests: [SearchItem] = []
    var myPRs: [SearchItem] = []
    var reviewRequestCount = 0
    var myPRCount = 0
    var unreadCount = 0
    var lastBadgeRefresh: Date?
    var rateLimitedUntil: Date?
    private var lastLiveRefresh = Date()
    private var liveStale = false
    private var resumeTask: Task<Void, Never>?
    let claude = ClaudeWorkspace()

    private var lastClipboard: String?
    private var toastTask: Task<Void, Never>?
    @ObservationIgnored private var canonicalItems: [ItemRef: ItemRef] = [:]
    private var pollTask: Task<Void, Never>?
    private var prModels: [ItemRef: PRDetailModel] = [:]
    private var prOrder: [ItemRef] = []

    var hasProjectScope: Bool {
        scopes.isEmpty || scopes.contains("read:project") || scopes.contains("project")
    }

    init() {
        URLCache.shared = URLCache(memoryCapacity: 40 << 20, diskCapacity: 200 << 20)
        Diagnostics.shared.start()
        Task { await restore() }
    }

    private func restore() async {
        if let token = ProcessInfo.processInfo.environment["PULLYARD_TOKEN"] {
            await resumeGitHub(token: token, account: nil)
            return
        }
        let accounts = GitAccounts.shared
        guard let account = accounts.active, let credential = accounts.credential(for: account) else {
            phase = .loggedOut
            return
        }
        if AccountLock.shared.isProtected(account.id) {
            lockedAccount = account
            ClaudeProfiles.shared.scope = ""
            phase = .locked
            return
        }
        await resume(account, credential: credential)
    }

    func unlocked(_ account: GitAccount) {
        if lockedAccount?.id == account.id {
            lockedAccount = nil
            phase = .checking
            Task {
                if let credential = GitAccounts.shared.credential(for: account) {
                    await resume(account, credential: credential)
                } else {
                    phase = .loggedOut
                }
            }
        } else {
            unlockRequest = nil
            switchAccountReportingErrors(to: account.id)
        }
    }

    func requestSwitch(to id: String) {
        guard let target = GitAccounts.shared.accounts.first(where: { $0.id == id }) else { return }
        if target.id != account?.id, AccountLock.shared.isProtected(target.id) {
            unlockRequest = target
        } else {
            switchAccountReportingErrors(to: id)
        }
    }

    func resetLock(_ account: GitAccount) async -> Bool {
        guard await SystemAuth.deviceOwner("reset the PIN for \(account.subtitle)") else { return false }
        AccountLock.shared.removeLock(account.id)
        if ClaudeProfiles.shared.scope == account.id { claude.closeAllForProfileSwitch() }
        await ClaudeAccountStore.shared.logout(profiles: ClaudeProfiles.shared.owned(by: account.id))
        return true
    }

    private func resume(_ account: GitAccount, credential: GitCredential) async {
        if account.provider.isGitHub {
            await resumeGitHub(token: credential.token, account: account)
        } else if let client = makeForgeClient(account, credential: credential) {
            enterForge(account, client: client)
            await verifyForge(client)
        }
    }

    private func resumeGitHub(token: String, account: GitAccount?) async {
        let host = account?.host ?? GitProvider.github.defaultHost
        let cached = account.flatMap { $0.login.isEmpty ? nil : $0.user } ?? ListCache.account(for: token)
        if !Demo.isOn, let cached {
            let api = makeAPI(token, host: host)
            self.account = account
            self.forge = nil
            self.api = api
            user = cached
            ListCache.activate(account?.cacheKey ?? cached.login)
            if let account { bindClaude(to: account) }
            phase = .loggedIn
            startPolling()
            await verify(api, token: token)
            return
        }
        let api = makeAPI(token, host: host)
        do {
            let (user, scopes) = try await api.currentUser()
            enterGitHub(account.map { refreshed($0, user) }, api: api, user: user, scopes: scopes, token: token)
        } catch let e as APIError where e.status == 401 {
            revoked()
        } catch {
            self.account = account
            self.api = api
            if let account { bindClaude(to: account) }
            phase = .loggedIn
            startPolling()
        }
    }

    func refreshTokenFromGH() async {
        let provider = account?.provider ?? .github
        guard provider.isGitHub else { return }
        let host = account?.host ?? GitProvider.github.defaultHost
        if let t = try? await GHCLI.token(host: host) {
            try? await addAccount(provider, host: host, credential: GitCredential(token: t))
        }
    }

    func addAccount(_ provider: GitProvider, host rawHost: String, credential: GitCredential, email: String? = nil) async throws {
        let host = provider.hostEditable ? GitProvider.normalizeHost(rawHost) : provider.defaultHost
        guard !host.isEmpty else { throw APIError(message: "Enter the server address") }
        let key = "account-\(UUID().uuidString.lowercased())"
        var draft = GitAccount(id: UUID().uuidString.lowercased(), provider: provider, host: host, login: "", email: email?.trimmingCharacters(in: .whitespacesAndNewlines), keychainKey: key)
        if provider.isGitHub {
            let api = makeAPI(credential.token, host: host)
            let (user, scopes) = try await api.currentUser()
            draft = refreshed(draft, user)
            let saved = try GitAccounts.shared.upsert(draft, credential: credential)
            await leaveCurrent(for: saved)
            enterGitHub(saved, api: api, user: user, scopes: scopes, token: credential.token)
        } else {
            guard let probe = makeForgeClient(draft, credential: credential) else { return }
            let user = try await probe.currentUser()
            draft.login = user.login
            draft.name = user.name
            draft.avatarUrl = user.avatarUrl
            draft.userID = user.id
            let saved = try GitAccounts.shared.upsert(draft, credential: credential)
            await leaveCurrent(for: saved)
            if let client = makeForgeClient(saved, credential: credential) { enterForge(saved, client: client) }
        }
    }

    func switchAccount(to id: String) async throws {
        let accounts = GitAccounts.shared
        guard id != account?.id || phase != .loggedIn, let target = accounts.accounts.first(where: { $0.id == id }) else { return }
        guard let credential = accounts.credential(for: target) else {
            throw APIError(message: "Sign-in for \(target.subtitle) has expired. Sign in again to use this account.")
        }
        await leaveCurrent(for: target)
        await resume(target, credential: credential)
    }

    func switchAccountReportingErrors(to id: String) {
        Task {
            do { try await switchAccount(to: id) } catch { showToast(error.localizedDescription) }
        }
    }

    private func leaveCurrent(for next: GitAccount) async {
        if phase == .loggedIn, account?.id != next.id { resetSession(forgetCache: false) }
        GitAccounts.shared.setActive(next.id)
    }

    private func bindClaude(to account: GitAccount) {
        let profiles = ClaudeProfiles.shared
        if profiles.owned(by: account.id).isEmpty, !profiles.claimUnowned(for: account.id) {
            _ = try? profiles.add(label: "\(account.provider.shortTitle) @\(account.login)", shareSettings: false, owner: account.id)
        }
        profiles.scope = account.id
        guard let target = profiles.preferred(for: account.id), target.id != profiles.activeID else { return }
        switchClaudeProfile(to: target.id)
    }

    private func unbindClaude(from account: GitAccount) async {
        let profiles = ClaudeProfiles.shared
        let owned = profiles.owned(by: account.id)
        claude.closeAllForProfileSwitch()
        await ClaudeAccountStore.shared.logout(profiles: owned)
        for p in owned {
            if p.isDefault { profiles.release(p.id) } else { profiles.remove(p.id) }
        }
        profiles.scope = nil
    }

    private func refreshed(_ account: GitAccount, _ user: RESTUser) -> GitAccount {
        var a = account
        a.login = user.login
        a.name = user.name
        a.avatarUrl = user.avatarUrl
        return a
    }

    private func enterGitHub(_ account: GitAccount?, api: GitHubAPI, user: RESTUser, scopes: [String], token: String) {
        if self.account?.id != account?.id || self.user?.login != user.login { SearchListModel.resetCache() }
        if let account {
            GitAccounts.shared.update(account)
            bindClaude(to: account)
        }
        self.account = account
        self.forge = nil
        self.api = api
        self.user = Demo.isOn ? Demo.user : user
        self.scopes = scopes
        if !Demo.isOn {
            if account?.provider ?? .github == .github {
                ListCache.remember(user, token: token)
            } else if let account {
                ListCache.activate(account.cacheKey)
            }
        }
        prModels = [:]
        prOrder = []
        phase = .loggedIn
        startPolling()
    }

    private func enterForge(_ account: GitAccount, client: ForgeClient) {
        if self.account?.id != account.id { ForgeListModel.resetCache() }
        bindClaude(to: account)
        self.account = account
        forge = client
        api = nil
        user = account.user
        scopes = []
        ListCache.activate(account.cacheKey)
        forgeCaps = ForgeCapabilities(pullFilters: [.reviewRequested, .mine])
        if !forgeSupports(section) { section = .prs(.reviewRequested) }
        Task {
            let caps = await client.capabilities()
            guard forge === client else { return }
            forgeCaps = caps
            if !forgeSupports(section) { section = .prs(caps.pullFilters.first ?? .mine) }
            await refreshBadges()
        }
        detailRoot = nil
        detailPath = []
        forgeSelection = nil
        phase = .loggedIn
        startPolling()
    }

    private func verify(_ api: GitHubAPI, token: String) async {
        do {
            let (user, scopes) = try await api.currentUser()
            guard self.api === api else { return }
            self.user = user
            self.scopes = scopes
            if let account {
                let updated = refreshed(account, user)
                self.account = updated
                GitAccounts.shared.update(updated)
                if updated.provider == .github { ListCache.remember(user, token: token) }
            } else {
                ListCache.remember(user, token: token)
            }
        } catch let e as APIError where e.status == 401 {
            guard self.api === api else { return }
            revoked()
        } catch {}
    }

    private func verifyForge(_ client: ForgeClient) async {
        do {
            let u = try await client.currentUser()
            guard forge === client, var account else { return }
            account.login = u.login
            account.name = u.name
            account.avatarUrl = u.avatarUrl
            account.userID = u.id ?? account.userID
            self.account = account
            user = account.user
            GitAccounts.shared.update(account)
        } catch let e as APIError where e.status == 401 {
            guard forge === client else { return }
            revoked()
        } catch {}
    }

    var provider: GitProvider { account?.provider ?? .github }

    var cacheAccount: String? {
        Demo.isOn ? nil : (account?.cacheKey ?? user?.login)
    }

    private func makeAPI(_ token: String, host: String = GitProvider.github.defaultHost) -> GitHubAPI {
        let api = GitHubAPI(token: token, host: host)
        api.onRateLimit = { [weak self] until in
            Task { @MainActor in self?.pauseLive(until: until) }
        }
        return api
    }

    var liveRefreshPaused: Bool {
        rateLimitedUntil.map { $0 > Date() } ?? false
    }

    func pauseLive(until: Date) {
        guard until > Date(), until > (rateLimitedUntil ?? .distantPast) else { return }
        rateLimitedUntil = until
        resumeTask?.cancel()
        resumeTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(max(until.timeIntervalSinceNow, 0) * 1_000_000_000))
            guard !Task.isCancelled else { return }
            self?.rateLimitedUntil = nil
        }
    }

    var frontRoute: Route? {
        section == .claude ? nil : (detailPath.last ?? detailRoot)
    }

    private var mainWindowVisible: Bool {
        let windows = NSApp.windows.filter { $0.identifier?.rawValue.hasPrefix("main") == true }
        guard !windows.isEmpty else { return true }
        return windows.contains { $0.isVisible && $0.occlusionState.contains(.visible) }
    }

    func windowVisibilityChanged() {
        guard phase == .loggedIn, liveStale, !liveRefreshPaused, mainWindowVisible else { return }
        liveStale = false
        lastLiveRefresh = Date()
        Diagnostics.shared.beginTick()
        liveTick += 1
    }

    func signOut() async {
        let current = account
        if let current { await unbindClaude(from: current) }
        resetSession(forgetCache: true)
        if let current {
            AccountLock.shared.removeLock(current.id)
            GitAccounts.shared.remove(current.id)
        } else {
            Keychain.delete()
        }
    }

    private func revoked() {
        if let account { GitAccounts.shared.forgetCredential(account.id) } else { Keychain.delete() }
        ClaudeProfiles.shared.scope = nil
        resetSession(forgetCache: false)
    }

    private func resetSession(forgetCache: Bool) {
        pollTask?.cancel()
        resumeTask?.cancel()
        rateLimitedUntil = nil
        if forgetCache { ListCache.forget(account?.cacheKey ?? user?.login ?? ListCache.rememberedLogin) }
        SearchListModel.resetCache()
        ForgeListModel.resetCache()
        api = nil
        forge = nil
        account = nil
        user = nil
        scopes = []
        detailRoot = nil
        detailPath = []
        forgeSelection = nil
        prModels = [:]
        prOrder = []
        reviewRequests = []
        myPRs = []
        reviewRequestCount = 0
        myPRCount = 0
        unreadCount = 0
        RowAppearTracker.resetAll()
        phase = .loggedOut
    }

    func refresh() {
        lastLiveRefresh = Date()
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
        guard let scheme = url.scheme, scheme.hasPrefix("pullyard") || scheme.hasPrefix("shipyard") || scheme.hasPrefix("ghlite") else { return }
        guard phase == .loggedIn else { return }
        if url.host == "section" {
            let parts = url.pathComponents.filter { $0 != "/" }
            switch (parts.first, parts.dropFirst().first) {
            case ("notifications", _): section = .notifications
            case ("repos", _): section = .repos
            case ("projects", _): section = .projects
            case ("claude", _): section = .claude
            case ("prs", "byProject"): section = .myPRsByRepo
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
                let prompt = value("prompt")
                NSApp.activate(ignoringOtherApps: true)
                let alert = NSAlert()
                alert.messageText = "Start a Claude session from a link?"
                alert.informativeText = "Folder: \(folder)" + (prompt.map { "\n\nPrompt:\n\($0.prefix(600))" } ?? "")
                alert.addButton(withTitle: "Start Session")
                alert.addButton(withTitle: "Cancel")
                if alert.runModal() == .alertFirstButtonReturn {
                    claude.newSession(folder: folder, prompt: prompt, name: value("name"))
                }
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
        if api != nil, let route = GitHubURL.parse(target) {
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

    var frontItem: ItemRef? {
        guard let ref = ShortLink.ref(route: frontRoute) else { return nil }
        return canonicalItems[ref] ?? ref
    }

    func noteCanonical(_ ref: ItemRef, url: String) {
        guard let canonical = ShortLink.ref(url: url), canonical.number == ref.number else { return }
        canonicalItems[ref] = canonical
    }

    func copyShortLink(_ ref: ItemRef) {
        let link = ShortLink.make(ref)
        copyToPasteboard(link)
        showToast("Copied \(link)")
    }

    func showToast(_ text: String) {
        toastTask?.cancel()
        withAnimation(Motion.quick) { toast = text }
        toastTask = Task { [weak self] in
            try? await Task.sleep(nanoseconds: 1_800_000_000)
            guard !Task.isCancelled else { return }
            withAnimation(Motion.fade) { self?.toast = nil }
        }
    }

    func openCode(_ repo: RepoRef, ref: String, path: String, blame: Bool) {
        pendingCode = CodeOpenRequest(repo: repo, ref: ref, path: path, blame: blame)
        open(.repo(repo), push: true)
    }

    func checkClipboard() {
        guard let s = NSPasteboard.general.string(forType: .string), s != lastClipboard else { return }
        lastClipboard = s
        guard api != nil, account?.provider ?? .github == .github, s.count < 300, s.contains("github.com"), let route = GitHubURL.parse(s, allowShort: false) else { return }
        switch route {
        case .pr, .issue:
            if route != detailRoot { clipboardRoute = route }
        default:
            break
        }
    }

    static var refreshInterval: TimeInterval {
        max(UserDefaults.standard.double(forKey: "pollInterval"), 15)
    }

    func startPolling() {
        pollTask?.cancel()
        lastLiveRefresh = Date()
        Task { await refreshBadges() }
        pollTask = Task { [weak self] in
            var cycle = 0
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: UInt64(Self.refreshInterval * 1_000_000_000))
                guard !Task.isCancelled, let self else { return }
                cycle += 1
                let due = Date().timeIntervalSince(self.lastLiveRefresh) >= Self.refreshInterval * 0.8
                if due, !self.liveRefreshPaused, NSApp.isActive || cycle % 4 == 0 { self.tickLive() }
            }
        }
    }

    func didBecomeActive() {
        guard phase == .loggedIn, !liveRefreshPaused, Date().timeIntervalSince(lastLiveRefresh) > 10 else { return }
        tickLive()
    }

    private func tickLive() {
        Diagnostics.shared.beginTick()
        lastLiveRefresh = Date()
        if mainWindowVisible {
            liveStale = false
            liveTick += 1
        } else {
            liveStale = true
        }
        Diagnostics.liveTask { await self.refreshBadges(live: true) }
    }

    func refreshBadges(live: Bool = false) async {
        if let forge {
            await refreshForgeBadges(forge)
            return
        }
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
        if let n = try? await api.unreadNotificationCount(live: live) {
            unreadCount = n
        }
        lastBadgeRefresh = Date()
    }

    func forgeSupports(_ section: SidebarSection?) -> Bool {
        switch section {
        case .prs(let f)?: return forgeCaps.pullFilters.contains(f)
        case .myPRsByRepo?: return forgeCaps.pullFilters.contains(.mine)
        case .issues(let f)?: return forgeCaps.issueFilters.contains(f)
        case .notifications?: return forgeCaps.hasInbox
        case .repos?: return forgeCaps.repositories
        case .claude?: return true
        default: return false
        }
    }

    private func refreshForgeBadges(_ forge: ForgeClient) async {
        let supported = forgeCaps.pullFilters
        if supported.contains(.reviewRequested), let rr = try? await forge.pullRequests(.reviewRequested, state: .open), self.forge === forge {
            reviewRequests = Array(rr.prefix(10))
            reviewRequestCount = rr.count
        }
        if supported.contains(.mine), let mine = try? await forge.pullRequests(.mine, state: .open), self.forge === forge {
            myPRs = Array(mine.prefix(10))
            myPRCount = mine.count
        }
        if forgeCaps.hasInbox, let inbox = try? await forge.notifications(), self.forge === forge {
            unreadCount = inbox.filter(\.unread).count
        } else if !forgeCaps.hasInbox {
            unreadCount = 0
        }
        lastBadgeRefresh = Date()
    }
}
