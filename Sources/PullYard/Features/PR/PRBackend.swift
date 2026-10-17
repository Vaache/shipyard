import Foundation

enum PRPulseToken: Equatable {
    case github(PRPulse)
    case fingerprint(String)
}

struct PRLoad {
    let viewer: String
    let repository: PRRepository?
    let pulse: PRPulseToken?
}

enum PRAction {
    case close, reopen, readyForReview, convertToDraft, updateBranch, disableAutoMerge
}

struct PRFeatures {
    var provider: GitProvider
    var stack = false
    var localConflicts = false
    var viewedFiles = false
    var requestReviewers = false
    var editing = false
    var draftToggle = false
    var updateBranch = false
    var rerunChecks = false
    var jobLogs = false
    var requestChanges = false
    var revokeApproval = false
    var claude = false
    var blame = false
    var deleteBranch = false

    static let github = PRFeatures(provider: .github, stack: true, localConflicts: true, viewedFiles: true, requestReviewers: true, editing: true, draftToggle: true, updateBranch: true, rerunChecks: true, jobLogs: true, requestChanges: true, claude: true, blame: true, deleteBranch: true)

    static let gitlab = PRFeatures(provider: .gitlab, editing: true, draftToggle: true, updateBranch: true, revokeApproval: true)

    var checksTitle: String { provider == .gitlab ? "Pipeline" : "Checks" }
    var updateBranchTitle: String { provider == .gitlab ? "Rebase" : "Update branch" }
    var serviceTitle: String { provider == .githubEnterprise ? "GitHub Enterprise" : provider.title }
}

struct PRBackendUnsupported: LocalizedError {
    let provider: GitProvider
    var errorDescription: String? { "Not available for \(provider.title) yet" }
}

@MainActor
protocol PRBackend: AnyObject {
    var features: PRFeatures { get }
    var github: GitHubAPI? { get }
    func load(_ ref: ItemRef) async throws -> PRLoad
    func pulse(_ ref: ItemRef) async throws -> PRPulseToken?
    func files(_ ref: ItemRef, pr: PullRequestDetail) async throws -> [PRFile]
    func comment(_ ref: ItemRef, pr: PullRequestDetail, body: String) async throws
    func submitReview(_ ref: ItemRef, pr: PullRequestDetail, event: String, body: String, comments: [PendingComment], files: [DiffFile]) async throws
    func reply(_ ref: ItemRef, thread: String, body: String) async throws
    func setResolved(_ ref: ItemRef, thread: String, resolved: Bool) async throws
    func merge(_ ref: ItemRef, pr: PullRequestDetail, method: String, title: String?, body: String?, deleteBranch: Bool) async throws
    func enableAutoMerge(_ ref: ItemRef, pr: PullRequestDetail, method: String) async throws
    func perform(_ action: PRAction, ref: ItemRef, pr: PullRequestDetail) async throws
    func edit(_ ref: ItemRef, pr: PullRequestDetail, title: String, body: String) async throws
    func revokeApproval(_ ref: ItemRef, pr: PullRequestDetail) async throws
    func requestReviewers(_ ref: ItemRef, logins: [String]) async throws
    func deleteBranch(_ ref: ItemRef, pr: PullRequestDetail) async throws
    func setViewed(_ ref: ItemRef, pr: PullRequestDetail, path: String, viewed: Bool) async throws
    func rerunFailed(_ ref: ItemRef, runs: Set<Int>) async throws
    func stack(_ ref: ItemRef, current: StackPR) async throws -> [StackPR]
}

extension PRBackend {
    var github: GitHubAPI? { nil }
    func revokeApproval(_ ref: ItemRef, pr: PullRequestDetail) async throws { throw PRBackendUnsupported(provider: features.provider) }
    func requestReviewers(_ ref: ItemRef, logins: [String]) async throws { throw PRBackendUnsupported(provider: features.provider) }
    func deleteBranch(_ ref: ItemRef, pr: PullRequestDetail) async throws { throw PRBackendUnsupported(provider: features.provider) }
    func setViewed(_ ref: ItemRef, pr: PullRequestDetail, path: String, viewed: Bool) async throws { throw PRBackendUnsupported(provider: features.provider) }
    func rerunFailed(_ ref: ItemRef, runs: Set<Int>) async throws { throw PRBackendUnsupported(provider: features.provider) }
    func stack(_ ref: ItemRef, current: StackPR) async throws -> [StackPR] { [] }
}

@MainActor
final class GitHubPRBackend: PRBackend {
    let api: GitHubAPI
    let features: PRFeatures

    init(api: GitHubAPI, provider: GitProvider = .github) {
        self.api = api
        var f = PRFeatures.github
        f.provider = provider
        features = f
    }

    var github: GitHubAPI? { api }

    func load(_ ref: ItemRef) async throws -> PRLoad {
        let r = try await api.pullRequest(ref)
        return PRLoad(viewer: r.viewer.login, repository: r.repository, pulse: r.pulse.map(PRPulseToken.github))
    }

    func pulse(_ ref: ItemRef) async throws -> PRPulseToken? {
        try await api.pullRequestPulse(ref).map(PRPulseToken.github)
    }

    func files(_ ref: ItemRef, pr: PullRequestDetail) async throws -> [PRFile] {
        try await api.pullRequestFiles(ref)
    }

    func comment(_ ref: ItemRef, pr: PullRequestDetail, body: String) async throws {
        try await api.addComment(subject: pr.id, body: body)
    }

    func submitReview(_ ref: ItemRef, pr: PullRequestDetail, event: String, body: String, comments: [PendingComment], files: [DiffFile]) async throws {
        try await api.addReview(pr: pr.id, event: event, body: body, comments: comments)
    }

    func reply(_ ref: ItemRef, thread: String, body: String) async throws {
        try await api.mutate(Q.replyThread, ["id": thread, "b": body])
    }

    func setResolved(_ ref: ItemRef, thread: String, resolved: Bool) async throws {
        try await api.mutate(resolved ? Q.resolveThread : Q.unresolveThread, ["id": thread])
    }

    func merge(_ ref: ItemRef, pr: PullRequestDetail, method: String, title: String?, body: String?, deleteBranch: Bool) async throws {
        try await api.mutate(Q.merge, ["id": pr.id, "m": method, "t": title, "b": body])
        if deleteBranch, let head = pr.headRepository?.nameWithOwner {
            try? await api.deleteBranch(repo: head, branch: pr.headRefName)
        }
    }

    func enableAutoMerge(_ ref: ItemRef, pr: PullRequestDetail, method: String) async throws {
        try await api.mutate(Q.enableAutoMerge, ["id": pr.id, "m": method])
    }

    func perform(_ action: PRAction, ref: ItemRef, pr: PullRequestDetail) async throws {
        let mutation: String
        switch action {
        case .close: mutation = Q.closePR
        case .reopen: mutation = Q.reopenPR
        case .readyForReview: mutation = Q.readyForReview
        case .convertToDraft: mutation = Q.convertToDraft
        case .updateBranch: mutation = Q.updateBranch
        case .disableAutoMerge: mutation = Q.disableAutoMerge
        }
        try await api.mutate(mutation, ["id": pr.id])
    }

    func edit(_ ref: ItemRef, pr: PullRequestDetail, title: String, body: String) async throws {
        try await api.mutate(Q.editPR, ["id": pr.id, "t": title, "b": body])
    }

    func requestReviewers(_ ref: ItemRef, logins: [String]) async throws {
        try await api.requestReviewers(ref, logins: logins)
    }

    func deleteBranch(_ ref: ItemRef, pr: PullRequestDetail) async throws {
        guard let head = pr.headRepository?.nameWithOwner else { return }
        try await api.deleteBranch(repo: head, branch: pr.headRefName)
    }

    func setViewed(_ ref: ItemRef, pr: PullRequestDetail, path: String, viewed: Bool) async throws {
        try await api.mutate(viewed ? Q.markViewed : Q.unmarkViewed, ["id": pr.id, "p": path])
    }

    func rerunFailed(_ ref: ItemRef, runs: Set<Int>) async throws {
        for id in runs { try await api.rerunFailedJobs(repo: ref.repo, runId: id) }
    }

    func stack(_ ref: ItemRef, current: StackPR) async throws -> [StackPR] {
        try await api.pullRequestStack(ref, current: current)
    }
}
