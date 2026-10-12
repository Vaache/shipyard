import Foundation
import SwiftUI

enum PRTab: String, CaseIterable, Identifiable {
    case conversation, commits, checks, files
    var id: String { rawValue }
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
    var error: String?
    var actionError: String?
    var busy = false
    var tab: PRTab = .conversation {
        didSet { visitedTabs.insert(tab) }
    }
    var visitedTabs: Set<PRTab> = [.conversation]
    var html = ""
    var scrollToBottomToken = 0

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

    func load() async {
        guard !loading else { return }
        loading = true
        defer { loading = false }
        do {
            let r = try await api.pullRequest(ref)
            viewerLogin = r.viewer.login
            guard let repo = r.repository, let pr = repo.pullRequest else {
                notPR = true
                return
            }
            self.repo = repo
            self.pr = pr
            let newHTML = ConversationHTML.render(pr: pr)
            if newHTML != html { html = newHTML }
            viewedPaths = Set(pr.files?.nodes.filter { $0.viewerViewedState == "VIEWED" }.map(\.path) ?? [])
            indexThreads(pr.reviewThreads.nodes)
            error = nil
            if filesLoaded {
                await loadFiles(force: true)
            } else if !filesLoading {
                Task(priority: .utility) { await self.loadFiles() }
            }
        } catch {
            if pr == nil { self.error = error.localizedDescription } else { actionError = error.localizedDescription }
        }
    }

    func loadFiles(force: Bool = false) async {
        guard force || (!filesLoaded && !filesLoading) else { return }
        filesLoading = true
        defer { filesLoading = false }
        do {
            let raw = try await api.pullRequestFiles(ref)
            let parsed = await Task.detached(priority: .userInitiated) { raw.map(DiffFile.init) }.value
            if !filesLoaded {
                collapsed = Set(parsed.filter { viewedPaths.contains($0.id) || $0.lines.count > 1500 }.map(\.id))
            }
            files = parsed
            filesLoaded = true
            filesError = nil
        } catch {
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
    }

    func threadCount(_ path: String) -> Int {
        (pr?.reviewThreads.nodes ?? []).filter { $0.path == path }.count
    }

    func rows(for file: DiffFile) -> [DiffRow] {
        guard file.hasPatch else { return [.note(t("Diff недоступен (бинарный или слишком большой файл)", "Diff unavailable (binary or too large)"))] }
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
            rows.append(.note(t("Устаревшие комментарии", "Outdated comments")))
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
