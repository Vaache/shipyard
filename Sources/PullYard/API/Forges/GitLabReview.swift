import Foundation

enum GitLabMR {
    struct User: Decodable {
        let id: Int?
        let username: String
        let name: String?
        let avatarUrl: String?

        var actor: Actor { Actor(login: username, avatarUrl: avatarUrl) }
    }

    struct DiffRefs: Decodable {
        let baseSha: String?
        let headSha: String?
        let startSha: String?
    }

    struct Pipeline: Decodable {
        let id: Int
        let projectId: Int?
        let status: String
        let webUrl: String?
    }

    struct Permissions: Decodable {
        let canMerge: Bool?
    }

    struct References: Decodable {
        let full: String?
    }

    struct Detail: Decodable {
        let id: Int
        let iid: Int
        let projectId: Int?
        let sourceProjectId: Int?
        let targetProjectId: Int?
        let title: String
        let description: String?
        let descriptionHtml: String?
        let state: String
        let draft: Bool?
        let workInProgress: Bool?
        let webUrl: String
        let createdAt: Date
        let updatedAt: Date
        let author: User?
        let assignees: [User]?
        let reviewers: [User]?
        let sourceBranch: String
        let targetBranch: String
        let sha: String?
        let diffRefs: DiffRefs?
        let detailedMergeStatus: String?
        let mergeStatus: String?
        let hasConflicts: Bool?
        let blockingDiscussionsResolved: Bool?
        let labels: [FlexibleLabel]?
        let headPipeline: Pipeline?
        let changesCount: String?
        let userNotesCount: Int?
        let forceRemoveSourceBranch: Bool?
        let squash: Bool?
        let squashOnMerge: Bool?
        let mergeWhenPipelineSucceeds: Bool?
        let user: Permissions?
        let references: References?

        var isDraft: Bool { draft ?? workInProgress ?? false }
    }

    struct Approvals: Decodable {
        struct Approver: Decodable { let user: User }
        let approved: Bool?
        let approvalsRequired: Int?
        let approvalsLeft: Int?
        let approvedBy: [Approver]?
        let userHasApproved: Bool?
        let userCanApprove: Bool?
    }

    struct Position: Decodable {
        let baseSha: String?
        let startSha: String?
        let headSha: String?
        let oldPath: String?
        let newPath: String?
        let oldLine: Int?
        let newLine: Int?
        let positionType: String?
    }

    struct Note: Decodable {
        let id: Int
        let type: String?
        let body: String
        let author: User?
        let createdAt: Date
        let system: Bool
        let resolvable: Bool?
        let resolved: Bool?
        let position: Position?
    }

    struct Discussion: Decodable {
        let id: String
        let individualNote: Bool
        let notes: [Note]
    }

    struct Commit: Decodable {
        struct Stats: Decodable {
            let additions: Int
            let deletions: Int
        }
        let id: String
        let shortId: String
        let title: String
        let message: String?
        let authorName: String?
        let authoredDate: Date?
        let committedDate: Date?
        let webUrl: String?
        let stats: Stats?
    }

    struct Job: Decodable {
        let id: Int
        let name: String
        let stage: String?
        let status: String
        let webUrl: String?
        let startedAt: Date?
        let finishedAt: Date?
        let allowFailure: Bool?
    }

    struct Diff: Decodable {
        let oldPath: String
        let newPath: String
        let diff: String?
        let newFile: Bool
        let renamedFile: Bool
        let deletedFile: Bool
        let tooLarge: Bool?
        let collapsed: Bool?
    }

    struct Changes: Decodable {
        let changes: [Diff]
    }

    struct Project: Decodable {
        let id: Int
        let pathWithNamespace: String
        let webUrl: String?
        let mergeMethod: String?
        let squashOption: String?
        let removeSourceBranchAfterMerge: Bool?
    }
}

extension GitLabClient {
    static func projectPath(_ project: String) -> String {
        "/projects/" + (project.addingPercentEncoding(withAllowedCharacters: .alphanumerics.union(CharacterSet(charactersIn: "-._~"))) ?? project)
    }

    private func mrPath(_ project: String, _ iid: Int) -> String {
        "\(Self.projectPath(project))/merge_requests/\(iid)"
    }

    private func pages<T: Decodable>(_ path: String, _ query: [URLQueryItem] = [], limit: Int = 10) async throws -> [T] {
        var all: [T] = []
        for page in 1...limit {
            let batch: [T] = try await http.get(path, query + [URLQueryItem(name: "per_page", value: "100"), URLQueryItem(name: "page", value: String(page))])
            all += batch
            if batch.count < 100 { break }
        }
        return all
    }

    func mergeRequest(_ project: String, iid: Int, html: Bool = true) async throws -> GitLabMR.Detail {
        try await http.get(mrPath(project, iid), html ? [URLQueryItem(name: "render_html", value: "true")] : [])
    }

    func approvals(_ project: String, iid: Int) async throws -> GitLabMR.Approvals {
        try await http.get(mrPath(project, iid) + "/approvals")
    }

    func discussions(_ project: String, iid: Int) async throws -> [GitLabMR.Discussion] {
        try await pages(mrPath(project, iid) + "/discussions")
    }

    func commits(_ project: String, iid: Int) async throws -> [GitLabMR.Commit] {
        try await pages(mrPath(project, iid) + "/commits", limit: 3)
    }

    func jobs(_ project: String, pipeline: Int) async throws -> [GitLabMR.Job] {
        try await pages("\(Self.projectPath(project))/pipelines/\(pipeline)/jobs", limit: 3)
    }

    func project(_ project: String) async throws -> GitLabMR.Project {
        try await http.get(Self.projectPath(project))
    }

    func diffs(_ project: String, iid: Int) async throws -> [GitLabMR.Diff] {
        do {
            return try await pages(mrPath(project, iid) + "/diffs", limit: 30)
        } catch let e as APIError where e.status == 404 {
            let c: GitLabMR.Changes = try await http.get(mrPath(project, iid) + "/changes")
            return c.changes
        }
    }

    func addNote(_ project: String, iid: Int, body: String) async throws {
        try await http.post(mrPath(project, iid) + "/notes", json: ["body": body])
    }

    func addDiscussion(_ project: String, iid: Int, body: String, position: [String: Any]?) async throws {
        var json: [String: Any] = ["body": body]
        if let position { json["position"] = position }
        try await http.post(mrPath(project, iid) + "/discussions", json: json)
    }

    func reply(_ project: String, iid: Int, discussion: String, body: String) async throws {
        try await http.post(mrPath(project, iid) + "/discussions/\(discussion)/notes", json: ["body": body])
    }

    func resolve(_ project: String, iid: Int, discussion: String, resolved: Bool) async throws {
        try await http.post(mrPath(project, iid) + "/discussions/\(discussion)", method: "PUT", json: ["resolved": resolved])
    }

    func approve(_ project: String, iid: Int, sha: String?) async throws {
        try await http.post(mrPath(project, iid) + "/approve", json: sha.map { ["sha": $0] } ?? [:])
    }

    func unapprove(_ project: String, iid: Int) async throws {
        try await http.post(mrPath(project, iid) + "/unapprove")
    }

    func merge(_ project: String, iid: Int, squash: Bool, removeBranch: Bool, message: String?, sha: String?, auto: Bool) async throws {
        var json: [String: Any] = ["squash": squash, "should_remove_source_branch": removeBranch]
        if let sha { json["sha"] = sha }
        if let message { json[squash ? "squash_commit_message" : "merge_commit_message"] = message }
        if auto {
            json["auto_merge"] = true
            json["merge_when_pipeline_succeeds"] = true
        }
        try await http.post(mrPath(project, iid) + "/merge", method: "PUT", json: json)
    }

    func cancelAutoMerge(_ project: String, iid: Int) async throws {
        try await http.post(mrPath(project, iid) + "/cancel_merge_when_pipeline_succeeds")
    }

    func rebase(_ project: String, iid: Int) async throws {
        try await http.post(mrPath(project, iid) + "/rebase", method: "PUT")
    }

    func update(_ project: String, iid: Int, fields: [String: Any]) async throws {
        try await http.post(mrPath(project, iid), method: "PUT", json: fields)
    }

    func commit(_ project: String, sha: String) async throws -> CommitDetailREST {
        async let detail: GitLabMR.Commit = http.get("\(Self.projectPath(project))/repository/commits/\(sha)")
        async let diff: [GitLabMR.Diff] = pages("\(Self.projectPath(project))/repository/commits/\(sha)/diff", limit: 10)
        let c = try await detail
        let files = try await diff
        return CommitDetailREST(
            sha: c.id,
            htmlUrl: c.webUrl ?? "https://\(host)/\(project)/-/commit/\(c.id)",
            commit: .init(message: c.message ?? c.title, author: .init(name: c.authorName, date: c.authoredDate ?? c.committedDate)),
            author: nil,
            stats: c.stats.map { .init(additions: $0.additions, deletions: $0.deletions) },
            files: files.map(GitLabReviewMapper.file)
        )
    }
}
