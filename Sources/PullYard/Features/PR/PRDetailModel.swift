import Foundation
import SwiftUI

enum PRTab: String, CaseIterable, Identifiable {
    case conversation, commits, checks, files
    var id: String { rawValue }
}

enum FlatDiffRow: Identifiable {
    case header(DiffFile)
    case row(DiffFile, DiffRow)

    var id: String {
        switch self {
        case .header(let f): return "h:" + f.id
        case .row(let f, let r): return "r:" + f.id + ":" + r.id
        }
    }
}

enum DiffRow: Identifiable {
    case line(DiffLine)
    case thread(ReviewThread)
    case pending(PendingComment)
    case composer(DiffAnchor)
    case note(String)

    var id: String {
        switch self {
        case .line(let l): return "l\(l.id)"
        case .thread(let t): return "t\(t.id)"
        case .pending(let p): return "p\(p.id)"
        case .composer(let a): return "c\(a.side)\(a.line)"
        case .note(let s): return "n\(s)"
        }
    }
}

@Observable
@MainActor
final class PRDetailModel {
    let ref: ItemRef
    let api: GitHubAPI

    var repo: PRRepository?
    var pr: PullRequestDetail?
    var viewerLogin = ""
    var notPR = false
    var loading = false
    var lastUpdated: Date?
    var refreshError: String?
    var error: String?
    var actionError: String?
    var busy = false
    var tab: PRTab = .conversation {
        didSet {
            visitedTabs.insert(tab)
            ensureFiles()
        }
    }
    var visitedTabs: Set<PRTab> = [.conversation]
    let commitRows = RowAppearTracker()
    let checkRows = RowAppearTracker()
    let fileRows = RowAppearTracker()
    var stack: [StackPR] = []
    @ObservationIgnored private var activeLoad: Task<Void, Never>?
    @ObservationIgnored private var queuedLoad: Task<Void, Never>?
    @ObservationIgnored private var reloadFiles = false
    @ObservationIgnored private var stackRequest: Bool?
    @ObservationIgnored private var stackKey: String?
    private static var stackCache: [String: (stack: [StackPR], at: Date)] = [:]
    private static let stackTTL: TimeInterval = 600
    @ObservationIgnored private(set) var filesOid: String?
    @ObservationIgnored private var filesStale = false
    @ObservationIgnored private var filesStaleMark = 0
    @ObservationIgnored private var filesPending = false
    @ObservationIgnored var inspectorFilesVisible = false {
        didSet { ensureFiles() }
    }
    @ObservationIgnored private var pulse: PRPulse?
    @ObservationIgnored private var needsFull = false
    @ObservationIgnored private var fullReason: String?
    @ObservationIgnored private var lastFullFetch: Date?
    private static let fullFetchInterval: TimeInterval = 300
    @ObservationIgnored private var mergeRecheck: Task<Void, Never>?
    @ObservationIgnored private var mergeRechecks = 0
    var html = ""
    var scrollToBottomToken = 0
    var conflicts: ConflictState?
    var showResolveConflicts = false
    @ObservationIgnored private var conflictsKey: String?
    @ObservationIgnored private var conflictScan: Task<Void, Never>?

    var files: [DiffFile] = []
    var filesLoaded = false
    var filesLoading = false
    var filesError: String?
    var viewedPaths: Set<String> = []
    var collapsed: Set<String> = []
    var scrollTarget: String?
    var pending: [PendingComment] = []
    var composer: DiffAnchor?
    var replyThread: ReviewThread?

    @ObservationIgnored private var flatCacheKey = ""
    @ObservationIgnored private var flatCache: [FlatDiffRow] = []
    private var threadsVersion = 0
    private var filesVersion = 0
    private var threadIndex: [String: [ReviewThread]] = [:]
    private var outdatedByPath: [String: [ReviewThread]] = [:]

    init(ref: ItemRef, api: GitHubAPI) {
        self.ref = ref
        self.api = api
    }

    var canWrite: Bool { repo?.canWrite ?? false }

    var reviewers: [(login: String, avatar: String?, state: String)] {
        guard let pr else { return [] }
        var result: [(String, String?, String)] = pr.latestOpinionatedReviews.nodes.compactMap { r in
            guard let a = r.author else { return nil }
            return (a.login, a.avatarUrl, r.state)
        }
        let done = Set(result.map(\.0))
        for req in pr.reviewRequests.nodes {
            guard let r = req.requestedReviewer, !done.contains(r.display) else { continue }
            result.append((r.display, r.avatarUrl, "PENDING"))
        }
        return result
    }

    var lastCommitDate: Date? { pr?.lastCommit.nodes.first?.commit.committedDate }

    var staleReviews: [(login: String, commits: Int?)] {
        guard let pr, pr.isOpen else { return [] }
        let pending = Set(pr.reviewRequests.nodes.compactMap { $0.requestedReviewer?.display })
        let oids = pr.allCommits.nodes.map(\.commit.oid)
        return pr.latestOpinionatedReviews.nodes.compactMap { r in
            guard let login = r.author?.login, login != viewerLogin, !login.hasSuffix("[bot]"),
                  !pending.contains(login), let oid = r.commit?.oid, oid != pr.headRefOid else { return nil }
            let after = oids.lastIndex(of: oid).map { oids.count - 1 - $0 }
            return (login, after.flatMap { $0 > 0 ? $0 : nil })
        }
    }

    var reviewersToReRequest: [String] { staleReviews.map(\.login) }

    func load(full: Bool = false, live: Bool = false, file: String = #fileID, line: Int = #line) async {
        if full {
            reloadFiles = true
            stackRequest = true
        }
        if !live {
            needsFull = true
            fullReason = "\(full ? "refresh" : "load") \(file.split(separator: "/").last ?? ""):\(line)"
        }
        if let queuedLoad {
            await queuedLoad.value
            return
        }
        if let activeLoad {
            let next = Task { [weak self] () -> Void in
                await activeLoad.value
                await self?.startLoad()
            }
            queuedLoad = next
            await next.value
            return
        }
        await startLoad()
    }

    private func startLoad() async {
        queuedLoad = nil
        let task = Task { [weak self] () -> Void in await self?.fetch() }
        activeLoad = task
        await task.value
        if activeLoad == task { activeLoad = nil }
    }

    private func fetch() async {
        loading = true
        defer { loading = false }
        let recent = lastFullFetch.map { Date().timeIntervalSince($0) < Self.fullFetchInterval } ?? false
        let forced = needsFull ? fullReason : nil
        needsFull = false
        fullReason = nil
        let reason: String
        if pr == nil {
            reason = "first"
        } else if let forced {
            reason = forced
        } else if !recent {
            reason = "stale"
        } else if pulse == nil {
            reason = "no baseline"
        } else {
            switch await pulseCheck() {
            case .same: return
            case .changed: reason = "changed"
            case .failed: reason = "pulse error"
            }
        }
        do {
            let r = try await Diagnostics.$note.withValue("full: \(reason)") { try await api.pullRequest(ref) }
            viewerLogin = r.viewer.login
            guard let repo = r.repository, let pr = repo.pullRequest else {
                notPR = true
                return
            }
            self.repo = repo
            pulse = r.pulse
            lastFullFetch = Date()
            commitRows.seed(pr.allCommits.nodes.map(\.commit.oid))
            checkRows.seed(pr.checks.map(\.id))
            self.pr = pr
            updateConflicts(pr)
            renderHTML()
            viewedPaths = Set(pr.files?.nodes.filter { $0.viewerViewedState == "VIEWED" }.map(\.path) ?? [])
            indexThreads(pr.reviewThreads.nodes)
            error = nil
            refreshError = nil
            lastUpdated = Date()
            if let force = stackRequest {
                stackRequest = nil
                Task(priority: .utility) { await self.loadStack(force: force) }
            } else if stackKey != stackCacheKey(pr) {
                Task(priority: .utility) { await self.loadStack() }
            }
            scheduleMergeRecheck(pr)
            if reloadFiles, filesLoaded { markFilesStale() }
            reloadFiles = false
            ensureFiles()
        } catch {
            if isCancellation(error) { return }
            if pr == nil { self.error = error.localizedDescription } else { refreshError = error.localizedDescription }
        }
    }

    private enum PulseResult { case same, changed, failed }

    private func pulseCheck() async -> PulseResult {
        guard let known = pulse else { return .changed }
        do {
            guard let current = try await api.pullRequestPulse(ref) else { return .changed }
            guard current == known else { return .changed }
            refreshError = nil
            lastUpdated = Date()
            if let pr { scheduleMergeRecheck(pr) }
            ensureFiles()
            return .same
        } catch {
            return isCancellation(error) ? .same : .failed
        }
    }

    private func renderHTML() {
        guard let pr else { return }
        let newHTML = ConversationHTML.render(pr: pr, conflicts: conflicts, repo: ref.repo)
        if newHTML != html { html = newHTML }
    }

    private func updateConflicts(_ pr: PullRequestDetail, force: Bool = false) {
        guard MergeConflicts.applies(pr) else {
            if pr.isOpen, pr.mergeable == "UNKNOWN", conflicts != nil { return }
            conflictScan?.cancel()
            conflictsKey = nil
            conflicts = nil
            return
        }
        let key = "\(pr.baseTipOid ?? pr.baseRefName)|\(pr.headRefOid)"
        guard force || key != conflictsKey else { return }
        conflictsKey = key
        if conflicts == nil || force { conflicts = .checking }
        conflictScan?.cancel()
        let repo = ref.repo
        conflictScan = Task { [weak self] in
            guard let folder = await LocalRepoStore.detect(repo) else {
                self?.finishScan(.noClone, key: key)
                return
            }
            let result = await Task.detached(priority: .utility) {
                MergeConflicts.scan(folder: folder, repo: repo, number: pr.number, base: pr.baseRefName, baseOid: pr.baseTipOid, headOid: pr.headRefOid)
            }.value
            self?.finishScan(result, key: key)
        }
    }

    private func finishScan(_ result: ConflictState, key: String) {
        guard !Task.isCancelled, key == conflictsKey else { return }
        conflicts = result
        renderHTML()
    }

    func rescanConflicts() {
        guard let pr else { return }
        updateConflicts(pr, force: true)
        renderHTML()
    }

    func chooseLocalClone() {
        guard LocalRepoStore.choose(ref.repo) != nil else { return }
        rescanConflicts()
    }

    private func scheduleMergeRecheck(_ pr: PullRequestDetail) {
        let unknown = pr.isOpen && (pr.mergeStateStatus == "UNKNOWN" || pr.mergeable == "UNKNOWN")
        guard unknown else {
            mergeRechecks = 0
            return
        }
        guard mergeRechecks < 5 else { return }
        mergeRechecks += 1
        let delay = UInt64(mergeRechecks * 2) * 1_000_000_000
        mergeRecheck?.cancel()
        mergeRecheck = Task { [weak self] in
            try? await Task.sleep(nanoseconds: delay)
            guard !Task.isCancelled else { return }
            await self?.load(live: true)
        }
    }

    private var stackRepoPrefix: String {
        "\(viewerLogin)|\(ref.repo.full)|"
    }

    private func stackCacheKey(_ pr: PullRequestDetail) -> String {
        stackRepoPrefix + "\(pr.number)|\(pr.headRepository?.nameWithOwner ?? "")|\(pr.headRefName)|\(pr.baseRefName)"
    }

    private func invalidateStack() {
        let prefix = stackRepoPrefix
        Self.stackCache = Self.stackCache.filter { !$0.key.hasPrefix(prefix) }
        stackRequest = true
    }

    func loadStack(force: Bool = false) async {
        guard let pr else { return }
        let key = stackCacheKey(pr)
        let now = Date()
        if !force, let hit = Self.stackCache[key], now.timeIntervalSince(hit.at) < Self.stackTTL {
            stackKey = key
            if hit.stack != stack { stack = hit.stack }
            return
        }
        let current = StackPR(number: pr.number, title: pr.title, state: pr.state, isDraft: pr.isDraft, headRefName: pr.headRefName, baseRefName: pr.baseRefName)
        guard let s = try? await api.pullRequestStack(ref, current: current) else { return }
        Self.stackCache = Self.stackCache.filter { now.timeIntervalSince($0.value.at) < Self.stackTTL }
        Self.stackCache[key] = (s, Date())
        stackKey = key
        if s != stack { stack = s }
    }

    private var filesVisible: Bool {
        tab == .files || inspectorFilesVisible
    }

    private var filesOutdated: Bool {
        filesStale || (pr != nil && filesOid != pr?.headRefOid)
    }

    private func markFilesStale() {
        filesStale = true
        filesStaleMark += 1
    }

    private func ensureFiles() {
        guard filesVisible, !filesLoading, !filesPending, pr != nil, !filesLoaded || filesOutdated else { return }
        filesPending = true
        Task(priority: .utility) {
            self.filesPending = false
            await self.loadFiles(force: self.filesLoaded)
        }
    }

    func loadFiles(force: Bool = false) async {
        guard force || (!filesLoaded && !filesLoading) else { return }
        filesLoading = true
        defer { filesLoading = false }
        let oid = pr?.headRefOid
        let mark = filesStaleMark
        do {
            let raw = try await api.pullRequestFiles(ref)
            let parsed = await Task.detached(priority: .userInitiated) { raw.map(DiffFile.init) }.value
            if !filesLoaded {
                collapsed = Set(parsed.filter { viewedPaths.contains($0.id) || $0.lines.count > 1500 }.map(\.id))
            }
            fileRows.seed(parsed.map(\.id))
            files = parsed
            filesOid = oid
            if mark == filesStaleMark { filesStale = false }
            filesVersion += 1
            filesLoaded = true
            filesError = nil
            filesLoading = false
            ensureFiles()
        } catch {
            if isCancellation(error) { return }
            filesError = error.localizedDescription
        }
    }

    private func indexThreads(_ threads: [ReviewThread]) {
        var idx: [String: [ReviewThread]] = [:]
        var outdated: [String: [ReviewThread]] = [:]
        for t in threads {
            if let line = t.line {
                idx["\(t.path)|\(t.diffSide)|\(line)", default: []].append(t)
            } else {
                outdated[t.path, default: []].append(t)
            }
        }
        threadIndex = idx
        outdatedByPath = outdated
        threadsVersion += 1
    }

    func flatRows(filter: String) -> [FlatDiffRow] {
        let key = "\(filter)|\(filesVersion)|\(collapsed.sorted().joined(separator: ","))|\(pending.map { $0.id.uuidString }.joined())|\(composer.map { "\($0.path):\($0.side):\($0.line)" } ?? "")|\(threadsVersion)"
        if key == flatCacheKey { return flatCache }
        var out: [FlatDiffRow] = []
        for file in files where filter.isEmpty || file.id.localizedCaseInsensitiveContains(filter) {
            out.append(.header(file))
            if collapsed.contains(file.id) { continue }
            for r in rows(for: file) { out.append(.row(file, r)) }
        }
        flatCacheKey = key
        flatCache = out
        return out
    }

    func threadCount(_ path: String) -> Int {
        (pr?.reviewThreads.nodes ?? []).filter { $0.path == path }.count
    }

    func rows(for file: DiffFile) -> [DiffRow] {
        guard file.hasPatch else { return [.note("Diff unavailable (binary or too large)")] }
        var rows: [DiffRow] = []
        rows.reserveCapacity(file.lines.count + 4)
        let filePending = pending.filter { $0.path == file.id }
        for line in file.lines {
            rows.append(.line(line))
            guard line.commentable else { continue }
            var keys: [String] = []
            if let n = line.newLine, line.kind != .del { keys.append("\(file.id)|RIGHT|\(n)") }
            if let o = line.oldLine, line.kind != .add { keys.append("\(file.id)|LEFT|\(o)") }
            for k in keys { threadIndex[k]?.forEach { rows.append(.thread($0)) } }
            if let anchorLine = line.anchorLine {
                for p in filePending where p.line == anchorLine && p.side == line.anchorSide { rows.append(.pending(p)) }
                if let c = composer, c.path == file.id, c.line == anchorLine, c.side == line.anchorSide { rows.append(.composer(c)) }
            }
        }
        if let outdated = outdatedByPath[file.id], !outdated.isEmpty {
            rows.append(.note("Outdated comments"))
            outdated.forEach { rows.append(.thread($0)) }
        }
        return rows
    }

    func revealFile(_ path: String) {
        tab = .files
        collapsed.remove(path)
        scrollTarget = path
    }

    @discardableResult
    private func run(reload: Bool = true, _ op: () async throws -> Void) async -> Bool {
        busy = true
        defer { busy = false }
        do {
            try await op()
            if reload { await load() }
            return true
        } catch {
            actionError = error.localizedDescription
            return false
        }
    }

    func comment(_ body: String) async -> Bool {
        guard let pr else { return false }
        let ok = await run { try await api.addComment(subject: pr.id, body: body) }
        if ok { scrollToBottomToken += 1 }
        return ok
    }

    func submitReview(event: String, body: String) async -> Bool {
        guard let pr else { return false }
        let comments = pending
        let ok = await run { try await api.addReview(pr: pr.id, event: event, body: body, comments: comments) }
        if ok {
            pending = []
            scrollToBottomToken += 1
        }
        return ok
    }

    func addInline(_ anchor: DiffAnchor, body: String, immediately: Bool) async -> Bool {
        let c = PendingComment(path: anchor.path, line: anchor.line, side: anchor.side, body: body)
        if immediately {
            guard let pr else { return false }
            let ok = await run { try await api.addReview(pr: pr.id, event: "COMMENT", body: nil, comments: [c]) }
            if ok { composer = nil }
            return ok
        }
        pending.append(c)
        composer = nil
        return true
    }

    func reply(_ threadId: String, body: String) async -> Bool {
        await run { try await api.mutate(Q.replyThread, ["id": threadId, "b": body]) }
    }

    func setResolved(_ threadId: String, _ resolved: Bool) async {
        await run { try await api.mutate(resolved ? Q.resolveThread : Q.unresolveThread, ["id": threadId]) }
    }

    func merge(method: String, title: String?, body: String?, deleteBranch: Bool) async -> Bool {
        guard let pr else { return false }
        invalidateStack()
        return await run {
            try await api.mutate(Q.merge, ["id": pr.id, "m": method, "t": title, "b": body])
            if deleteBranch, let head = pr.headRepository?.nameWithOwner {
                try? await api.deleteBranch(repo: head, branch: pr.headRefName)
            }
        }
    }

    func enableAutoMerge(method: String) async -> Bool {
        guard let pr else { return false }
        return await run { try await api.mutate(Q.enableAutoMerge, ["id": pr.id, "m": method]) }
    }

    func simple(_ mutation: String) async {
        guard let pr else { return }
        invalidateStack()
        await run { try await api.mutate(mutation, ["id": pr.id]) }
    }

    func edit(title: String, body: String) async -> Bool {
        guard let pr else { return false }
        return await run { try await api.mutate(Q.editPR, ["id": pr.id, "t": title, "b": body]) }
    }

    func requestReviewers(_ logins: [String]) async -> Bool {
        await run { try await api.requestReviewers(ref, logins: logins) }
    }

    func deleteBranch() async {
        guard let pr, let head = pr.headRepository?.nameWithOwner else { return }
        await run { try await api.deleteBranch(repo: head, branch: pr.headRefName) }
    }

    func toggleViewed(_ path: String) async {
        guard let pr else { return }
        let viewed = viewedPaths.contains(path)
        if viewed {
            viewedPaths.remove(path)
        } else {
            viewedPaths.insert(path)
            collapsed.insert(path)
        }
        await run(reload: false) {
            try await api.mutate(viewed ? Q.unmarkViewed : Q.markViewed, ["id": pr.id, "p": path])
        }
    }

    func rerunFailed() async {
        let runs = Set((pr?.checks ?? []).filter { $0.outcome == .failure }.compactMap(\.runId))
        await run {
            for id in runs { try await api.rerunFailedJobs(repo: ref.repo, runId: id) }
        }
    }
}
