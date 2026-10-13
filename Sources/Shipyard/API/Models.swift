import Foundation

struct RepoRef: Hashable, Codable {
    let owner: String
    let name: String
    var full: String { "\(owner)/\(name)" }

    init(owner: String, name: String) {
        self.owner = owner
        self.name = name
    }

    init?(full: String) {
        let parts = full.split(separator: "/", maxSplits: 1).map(String.init)
        guard parts.count == 2, !parts[0].isEmpty, !parts[1].isEmpty else { return nil }
        self.init(owner: parts[0], name: parts[1])
    }
}

struct ItemRef: Hashable, Codable {
    let repo: RepoRef
    let number: Int
    var title: String { "\(repo.full)#\(number)" }
}

struct ProjectRef: Hashable {
    let id: String
    let title: String
    let url: String
}

enum Route: Hashable {
    case pr(ItemRef)
    case issue(ItemRef)
    case repo(RepoRef)
    case project(ProjectRef)
    case commit(RepoRef, String)
}

struct Nodes<T: Decodable>: Decodable {
    var nodes: [T]
    var totalCount: Int?

    enum CodingKeys: String, CodingKey { case nodes, totalCount }

    init(nodes: [T] = [], totalCount: Int? = nil) {
        self.nodes = nodes
        self.totalCount = totalCount
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        nodes = (try c.decodeIfPresent([T?].self, forKey: .nodes) ?? []).compactMap { $0 }
        totalCount = try c.decodeIfPresent(Int.self, forKey: .totalCount)
    }
}

struct PageInfo: Decodable {
    let hasNextPage: Bool
    let endCursor: String?
}

struct Count: Decodable {
    let totalCount: Int
}

struct IDOnly: Decodable {
    let id: String
}

struct Login: Decodable {
    let login: String
}

struct Actor: Decodable, Hashable {
    let login: String
    let avatarUrl: String?
}

struct LabelInfo: Decodable, Hashable {
    let name: String
    let color: String
}

struct RepoName: Decodable {
    struct Branch: Decodable { let name: String }
    let nameWithOwner: String
    var defaultBranchRef: Branch?
}

struct StackPR: Decodable, Identifiable, Hashable {
    let number: Int
    let title: String
    let state: String
    let isDraft: Bool
    let headRefName: String
    let baseRefName: String
    var id: Int { number }
    var visualState: VisualState { .pr(state: state, draft: isDraft) }
}

struct Viewer: Decodable {
    let login: String
}

struct RESTUser: Decodable {
    let login: String
    let name: String?
    let avatarUrl: String?
}

struct RollupState: Decodable {
    let state: String
}

struct RollupCommit: Decodable {
    let statusCheckRollup: RollupState?
}

struct RollupCommitNode: Decodable {
    let commit: RollupCommit
}

struct SearchItem: Decodable, Identifiable {
    let typename: String
    let number: Int
    let title: String
    let url: String
    let prState: String?
    let issueState: String?
    let stateReason: String?
    let isDraft: Bool?
    let createdAt: Date
    let updatedAt: Date
    let baseRefName: String?
    let author: Actor?
    let repository: RepoName
    let reviewDecision: String?
    let additions: Int?
    let deletions: Int?
    let comments: Count
    let labels: Nodes<LabelInfo>?
    let commits: Nodes<RollupCommitNode>?

    enum CodingKeys: String, CodingKey {
        case typename = "__typename", number, title, url, prState, issueState, stateReason, isDraft, createdAt, updatedAt, baseRefName, author, repository, reviewDecision, additions, deletions, comments, labels, commits
    }

    var id: String { url }
    var isPR: Bool { typename == "PullRequest" }
    var repo: RepoRef { RepoRef(full: repository.nameWithOwner) ?? RepoRef(owner: "?", name: "?") }
    var ref: ItemRef { ItemRef(repo: repo, number: number) }
    var route: Route { isPR ? .pr(ref) : .issue(ref) }
    var ciState: String? { commits?.nodes.first?.commit.statusCheckRollup?.state }
    var stackedOn: String? {
        guard isPR, let base = baseRefName, let main = repository.defaultBranchRef?.name, base != main else { return nil }
        return base
    }
    var visualState: VisualState {
        isPR ? .pr(state: prState ?? "OPEN", draft: isDraft ?? false) : .issue(state: issueState ?? "OPEN", reason: stateReason)
    }
}

struct SearchConnection: Decodable {
    let issueCount: Int
    let pageInfo: PageInfo?
    let nodes: [SearchItem]

    enum CodingKeys: String, CodingKey { case issueCount, pageInfo, nodes }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        issueCount = try c.decode(Int.self, forKey: .issueCount)
        pageInfo = try c.decodeIfPresent(PageInfo.self, forKey: .pageInfo)
        nodes = (try c.decodeIfPresent([SearchItem?].self, forKey: .nodes) ?? []).compactMap { $0 }
    }
}

struct SearchResponse: Decodable {
    let search: SearchConnection
}

struct BadgeResponse: Decodable {
    let rr: SearchConnection
    let mine: SearchConnection
}

struct Reviewer: Decodable, Hashable {
    let typename: String?
    let login: String?
    let avatarUrl: String?
    let name: String?

    enum CodingKeys: String, CodingKey { case typename = "__typename", login, avatarUrl, name }

    var display: String { login ?? name ?? "?" }
}

struct ReviewRequest: Decodable {
    let requestedReviewer: Reviewer?
}

struct LatestReview: Decodable {
    let state: String
    let author: Actor?
}

struct WorkflowInfo: Decodable {
    let name: String
}

struct WorkflowRun: Decodable {
    let databaseId: Int?
    let workflow: WorkflowInfo?
}

struct CheckSuite: Decodable {
    let workflowRun: WorkflowRun?
}

enum CheckOutcome: Int {
    case failure = 0, pending, neutral, skipped, success
}

struct CheckContext: Decodable, Identifiable {
    let typename: String
    let databaseId: Int?
    let name: String?
    let status: String?
    let conclusion: String?
    let detailsUrl: String?
    let startedAt: Date?
    let completedAt: Date?
    let checkSuite: CheckSuite?
    let context: String?
    let state: String?
    let targetUrl: String?
    let description: String?

    enum CodingKeys: String, CodingKey {
        case typename = "__typename", databaseId, name, status, conclusion, detailsUrl, startedAt, completedAt, checkSuite, context, state, targetUrl, description
    }

    var id: String { "\(typename)-\(databaseId.map(String.init) ?? context ?? name ?? UUID().uuidString)" }
    var title: String { name ?? context ?? "?" }
    var workflowName: String? { checkSuite?.workflowRun?.workflow?.name }
    var runId: Int? { checkSuite?.workflowRun?.databaseId }
    var link: String? { detailsUrl ?? targetUrl }
    var isActionsJob: Bool { typename == "CheckRun" && runId != nil && databaseId != nil }

    var outcome: CheckOutcome {
        if typename == "StatusContext" {
            switch state {
            case "SUCCESS": return .success
            case "FAILURE", "ERROR": return .failure
            default: return .pending
            }
        }
        guard status == "COMPLETED" else { return .pending }
        switch conclusion {
        case "SUCCESS": return .success
        case "FAILURE", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE": return .failure
        case "SKIPPED": return .skipped
        default: return .neutral
        }
    }

    var duration: String? {
        guard let s = startedAt, let e = completedAt else { return nil }
        let secs = Int(e.timeIntervalSince(s))
        return secs >= 60 ? t("\(secs / 60)м \(secs % 60)с", "\(secs / 60)m \(secs % 60)s") : t("\(secs)с", "\(secs)s")
    }
}

struct StatusRollup: Decodable {
    let state: String
    let contexts: Nodes<CheckContext>?
}

struct LastCommit: Decodable {
    let oid: String
    let statusCheckRollup: StatusRollup?
}

struct LastCommitNode: Decodable {
    let commit: LastCommit
}

struct GitActor: Decodable {
    let name: String?
    let avatarUrl: String?
    let user: Login?
}

struct CommitInfo: Decodable {
    let oid: String
    let abbreviatedOid: String
    let messageHeadline: String
    let committedDate: Date
    let url: String
    let author: GitActor?
    let statusCheckRollup: RollupState?
}

struct CommitInfoNode: Decodable {
    let commit: CommitInfo
}

struct FileViewed: Decodable {
    let path: String
    let viewerViewedState: String
}

struct XRefSource: Decodable {
    let typename: String?
    let number: Int?
    let title: String?
    let url: String?
    let repository: RepoName?

    enum CodingKeys: String, CodingKey { case typename = "__typename", number, title, url, repository }
}

struct TimelineCommit: Decodable {
    let oid: String?
    let abbreviatedOid: String?
    let messageHeadline: String?
    let committedDate: Date?
    let author: GitActor?
}

struct TimelineItem: Decodable {
    let typename: String
    let id: String?
    let author: Actor?
    let actor: Actor?
    let body: String?
    let bodyHTML: String?
    let createdAt: Date?
    let url: String?
    let state: String?
    let label: LabelInfo?
    let source: XRefSource?
    let previousTitle: String?
    let currentTitle: String?
    let commit: TimelineCommit?
    let mergeRefName: String?
    let requestedReviewer: Reviewer?

    enum CodingKeys: String, CodingKey {
        case typename = "__typename", id, author, actor, body, bodyHTML, createdAt, url, state, label, source, previousTitle, currentTitle, commit, mergeRefName, requestedReviewer
    }
}

struct ThreadComment: Decodable, Identifiable {
    let id: String
    let author: Actor?
    let body: String
    let bodyHTML: String
    let createdAt: Date
    let diffHunk: String?
    let pullRequestReview: IDOnly?
}

struct ReviewThread: Decodable, Identifiable {
    let id: String
    let isResolved: Bool
    let isOutdated: Bool
    let path: String
    let line: Int?
    let originalLine: Int?
    let diffSide: String
    let viewerCanResolve: Bool
    let viewerCanUnresolve: Bool
    let viewerCanReply: Bool
    let comments: Nodes<ThreadComment>
}

struct AutoMergeRequest: Decodable {
    let enabledAt: Date?
    let mergeMethod: String?
}

struct HeadRef: Decodable {
    let name: String
}

struct PullRequestDetail: Decodable {
    let id: String
    let number: Int
    let title: String
    let url: String
    let state: String
    let isDraft: Bool
    let merged: Bool
    let mergeable: String
    let mergeStateStatus: String?
    let body: String
    let bodyHTML: String
    let createdAt: Date
    let author: Actor?
    let headRefName: String
    let baseRefName: String
    let headRefOid: String
    let headRepository: RepoName?
    let headRef: HeadRef?
    let additions: Int
    let deletions: Int
    let changedFiles: Int
    let reviewDecision: String?
    let viewerCanUpdate: Bool
    let viewerDidAuthor: Bool
    let autoMergeRequest: AutoMergeRequest?
    let labels: Nodes<LabelInfo>
    let assignees: Nodes<Actor>
    let reviewRequests: Nodes<ReviewRequest>
    let latestOpinionatedReviews: Nodes<LatestReview>
    let lastCommit: Nodes<LastCommitNode>
    let allCommits: Nodes<CommitInfoNode>
    let files: Nodes<FileViewed>?
    let timelineItems: Nodes<TimelineItem>
    let reviewThreads: Nodes<ReviewThread>

    var visualState: VisualState { .pr(state: state, draft: isDraft) }
    var rollup: StatusRollup? { lastCommit.nodes.first?.commit.statusCheckRollup }
    var checks: [CheckContext] { rollup?.contexts?.nodes ?? [] }
    var isOpen: Bool { state == "OPEN" }
}

struct PRRepository: Decodable {
    let viewerPermission: String?
    let mergeCommitAllowed: Bool
    let squashMergeAllowed: Bool
    let rebaseMergeAllowed: Bool
    let deleteBranchOnMerge: Bool
    let autoMergeAllowed: Bool
    let viewerDefaultMergeMethod: String
    let pullRequest: PullRequestDetail?

    var canWrite: Bool { ["ADMIN", "MAINTAIN", "WRITE"].contains(viewerPermission ?? "") }
}

struct PRDetailResponse: Decodable {
    let viewer: Viewer
    let repository: PRRepository?
}

struct IssueDetail: Decodable {
    let id: String
    let number: Int
    let title: String
    let url: String
    let issueState: String
    let stateReason: String?
    let bodyHTML: String
    let createdAt: Date
    let viewerCanUpdate: Bool
    let author: Actor?
    let labels: Nodes<LabelInfo>
    let assignees: Nodes<Actor>
    let comments: Count
    let timelineItems: Nodes<TimelineItem>

    var visualState: VisualState { .issue(state: issueState, reason: stateReason) }
}

struct IssueOrPR: Decodable {
    let typename: String
    let issue: IssueDetail?

    enum CodingKeys: String, CodingKey { case typename = "__typename" }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        typename = try c.decode(String.self, forKey: .typename)
        issue = typename == "Issue" ? try IssueDetail(from: decoder) : nil
    }
}

struct IssueRepo: Decodable {
    let issueOrPullRequest: IssueOrPR?
}

struct IssueDetailResponse: Decodable {
    let viewer: Viewer
    let repository: IssueRepo?
}

struct PRFile: Decodable {
    let filename: String
    let status: String
    let additions: Int
    let deletions: Int
    let patch: String?
    let previousFilename: String?
}

struct CommitDetailREST: Decodable {
    struct Person: Decodable {
        let name: String?
        let date: Date?
    }
    struct Inner: Decodable {
        let message: String
        let author: Person?
    }
    struct Stats: Decodable {
        let additions: Int
        let deletions: Int
    }
    let sha: String
    let htmlUrl: String
    let commit: Inner
    let author: RESTUser?
    let stats: Stats?
    let files: [PRFile]?
}

struct RepoSummary: Decodable, Identifiable {
    struct Language: Decodable {
        let name: String
        let color: String?
    }
    let nameWithOwner: String
    let description: String?
    let isPrivate: Bool
    let isFork: Bool
    let isArchived: Bool
    let stargazerCount: Int
    let pushedAt: Date?
    let url: String
    let primaryLanguage: Language?
    let pullRequests: Count
    let issues: Count

    var id: String { nameWithOwner }
    var ref: RepoRef? { RepoRef(full: nameWithOwner) }
}

struct RepoConnection: Decodable {
    let totalCount: Int
    let pageInfo: PageInfo
    let nodes: [RepoSummary]
}

struct ReposResponse: Decodable {
    struct V: Decodable { let repositories: RepoConnection }
    let viewer: V
}

struct RepoInfoResponse: Decodable {
    struct R: Decodable {
        let nameWithOwner: String
        let description: String?
        let url: String
        let isPrivate: Bool
        let stargazerCount: Int
        let forkCount: Int
        let defaultBranchRef: HeadRef?
        let pullRequests: Count
        let issues: Count
    }
    let repository: R?
}

struct ProjectOwner: Decodable {
    let login: String?
}

struct ProjectSummary: Decodable, Identifiable {
    let id: String
    let number: Int
    let title: String
    let url: String
    let closed: Bool
    let updatedAt: Date
    let shortDescription: String?
    let items: Count
    let owner: ProjectOwner?

    var ref: ProjectRef { ProjectRef(id: id, title: title, url: url) }
}

struct ProjectsResponse: Decodable {
    struct Org: Decodable {
        let login: String
        let projectsV2: Nodes<ProjectSummary>
    }
    struct V: Decodable {
        let projectsV2: Nodes<ProjectSummary>
        let organizations: Nodes<Org>
    }
    let viewer: V
}

struct ProjectOption: Decodable, Identifiable, Hashable {
    let id: String
    let name: String
    let color: String
}

struct ProjectField: Decodable, Identifiable {
    let typename: String
    let id: String
    let name: String
    let options: [ProjectOption]?

    enum CodingKeys: String, CodingKey { case typename = "__typename", id, name, options }
}

struct FieldRef: Decodable {
    let id: String?
}

struct ProjectFieldValue: Decodable {
    let typename: String
    let optionId: String?
    let field: FieldRef?

    enum CodingKeys: String, CodingKey { case typename = "__typename", optionId, field }
}

struct ProjectContent: Decodable {
    let typename: String
    let number: Int?
    let title: String?
    let url: String?
    let issueState: String?
    let prState: String?
    let isDraft: Bool?
    let repository: RepoName?
    let assignees: Nodes<Actor>?

    enum CodingKeys: String, CodingKey { case typename = "__typename", number, title, url, issueState, prState, isDraft, repository, assignees }

    var route: Route? {
        guard let number, let full = repository?.nameWithOwner, let repo = RepoRef(full: full) else { return nil }
        let ref = ItemRef(repo: repo, number: number)
        return typename == "PullRequest" ? .pr(ref) : .issue(ref)
    }

    var visualState: VisualState? {
        switch typename {
        case "PullRequest": return .pr(state: prState ?? "OPEN", draft: isDraft ?? false)
        case "Issue": return .issue(state: issueState ?? "OPEN", reason: nil)
        default: return nil
        }
    }
}

struct ProjectItem: Decodable, Identifiable {
    let id: String
    let type: String
    let isArchived: Bool
    var fieldValues: Nodes<ProjectFieldValue>
    let content: ProjectContent?

    func optionId(for fieldId: String) -> String? {
        fieldValues.nodes.first { $0.field?.id == fieldId && $0.optionId != nil }?.optionId
    }
}

struct ProjectItemsConnection: Decodable {
    let pageInfo: PageInfo
    let nodes: [ProjectItem]

    enum CodingKeys: String, CodingKey { case pageInfo, nodes }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        pageInfo = try c.decode(PageInfo.self, forKey: .pageInfo)
        nodes = (try c.decodeIfPresent([ProjectItem?].self, forKey: .nodes) ?? []).compactMap { $0 }
    }
}

struct ProjectDetail: Decodable {
    let id: String
    let title: String
    let url: String
    let fields: Nodes<ProjectFieldLoose>
    let items: ProjectItemsConnection
}

struct ProjectFieldLoose: Decodable {
    let field: ProjectField?

    init(from decoder: Decoder) throws {
        field = try? ProjectField(from: decoder)
    }
}

struct ProjectDetailResponse: Decodable {
    let node: ProjectDetail?
}

struct NotificationThread: Decodable, Identifiable {
    struct Subject: Decodable {
        let title: String
        let url: String?
        let type: String
    }
    struct Owner: Decodable {
        let avatarUrl: String?
    }
    struct Repo: Decodable {
        let fullName: String
        let owner: Owner?
    }
    let id: String
    var unread: Bool
    let reason: String
    let updatedAt: Date
    let subject: Subject
    let repository: Repo

    var route: Route? {
        if let u = subject.url, let r = GitHubURL.parse(u, allowShort: false) { return r }
        return nil
    }

    var webURL: String {
        if let u = subject.url, let r = GitHubURL.parse(u, allowShort: false) {
            switch r {
            case .pr(let ref): return "https://github.com/\(ref.repo.full)/pull/\(ref.number)"
            case .issue(let ref): return "https://github.com/\(ref.repo.full)/issues/\(ref.number)"
            default: break
            }
        }
        switch subject.type {
        case "Release": return "https://github.com/\(repository.fullName)/releases"
        case "CheckSuite": return "https://github.com/\(repository.fullName)/actions"
        case "Discussion": return "https://github.com/\(repository.fullName)/discussions"
        default: return "https://github.com/\(repository.fullName)"
        }
    }
}
