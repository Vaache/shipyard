import Foundation

struct PendingComment: Identifiable, Hashable {
    let id = UUID()
    let path: String
    let line: Int
    let side: String
    let body: String
}

extension GitHubAPI {
    func currentUser() async throws -> (RESTUser, [String]) {
        let (data, response) = try await restRaw("GET", "/user")
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        let user = try decoder.decode(RESTUser.self, from: data)
        let scopes = (response.value(forHTTPHeaderField: "X-OAuth-Scopes") ?? "")
            .split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        return (user, scopes)
    }

    func search(_ query: String, after: String? = nil) async throws -> SearchConnection {
        let r: SearchResponse = try await graphQL(Q.search, ["q": query, "after": after])
        return r.search
    }

    func repoPullRequests(_ repo: RepoRef, headRefName: String? = nil, states: [String], first: Int = 30) async throws -> [SearchItem] {
        struct R: Decodable {
            struct Repo: Decodable {
                struct Conn: Decodable { let nodes: [SearchItem] }
                let pullRequests: Conn
            }
            let repository: Repo?
        }
        let r: R = try await graphQL(Q.repoPullRequests, ["o": repo.owner, "n": repo.name, "b": headRefName, "states": states, "first": first])
        return r.repository?.pullRequests.nodes ?? []
    }

    func badges() async throws -> BadgeResponse {
        try await graphQL(Q.badges)
    }

    func pullRequest(_ ref: ItemRef) async throws -> PRDetailResponse {
        try await graphQL(Q.pullRequest, ["owner": ref.repo.owner, "name": ref.repo.name, "number": ref.number])
    }

    func issue(_ ref: ItemRef) async throws -> IssueDetailResponse {
        try await graphQL(Q.issue, ["owner": ref.repo.owner, "name": ref.repo.name, "number": ref.number])
    }

    func pullRequestFiles(_ ref: ItemRef) async throws -> [PRFile] {
        var all: [PRFile] = []
        var page = 1
        while page <= 30 {
            let batch: [PRFile] = try await rest("GET", "/repos/\(ref.repo.full)/pulls/\(ref.number)/files?per_page=100&page=\(page)")
            all += batch
            if batch.count < 100 { break }
            page += 1
        }
        return all
    }

    func commit(_ repo: RepoRef, sha: String) async throws -> CommitDetailREST {
        try await rest("GET", "/repos/\(repo.full)/commits/\(sha)")
    }

    func repos(after: String?) async throws -> RepoConnection {
        let r: ReposResponse = try await graphQL(Q.repos, ["after": after])
        return r.viewer.repositories
    }

    func repoInfo(_ repo: RepoRef) async throws -> RepoInfoResponse.R? {
        let r: RepoInfoResponse = try await graphQL(Q.repoInfo, ["owner": repo.owner, "name": repo.name])
        return r.repository
    }

    func projects() async throws -> [ProjectSummary] {
        let r: ProjectsResponse = try await graphQL(Q.projects)
        var seen = Set<String>()
        let all = r.viewer.projectsV2.nodes + r.viewer.organizations.nodes.flatMap { $0.projectsV2.nodes }
        return all.filter { seen.insert($0.id).inserted }
    }

    func project(_ id: String) async throws -> (ProjectDetail, [ProjectItem]) {
        var after: String?
        var items: [ProjectItem] = []
        var detail: ProjectDetail?
        for _ in 0..<10 {
            let r: ProjectDetailResponse = try await graphQL(Q.project, ["id": id, "after": after])
            guard let node = r.node else { throw APIError(message: t("Проект не найден", "Project not found")) }
            detail = detail ?? node
            items += node.items.nodes
            guard node.items.pageInfo.hasNextPage else { break }
            after = node.items.pageInfo.endCursor
        }
        return (detail!, items)
    }

    func moveProjectItem(project: String, item: String, field: String, option: String?) async throws {
        if let option {
            let _: EmptyData = try await graphQL(Q.moveProjectItem, ["p": project, "i": item, "f": field, "o": option], strict: true)
        } else {
            let _: EmptyData = try await graphQL(Q.clearProjectItem, ["p": project, "i": item, "f": field], strict: true)
        }
    }

    func notifications(all: Bool) async throws -> [NotificationThread] {
        try await rest("GET", "/notifications?all=\(all)&per_page=50")
    }

    func unreadNotificationCount() async throws -> Int {
        let list: [NotificationThread] = try await rest("GET", "/notifications?per_page=50")
        return list.count
    }

    func markNotificationRead(_ id: String) async throws {
        try await send("PATCH", "/notifications/threads/\(id)")
    }

    func markNotificationDone(_ id: String) async throws {
        try await send("DELETE", "/notifications/threads/\(id)")
    }

    func markAllNotificationsRead() async throws {
        try await send("PUT", "/notifications", body: ["read": true])
    }

    func mutate(_ query: String, _ vars: [String: Any?]) async throws {
        let _: EmptyData = try await graphQL(query, vars, strict: true)
    }

    func addComment(subject: String, body: String) async throws {
        try await mutate(Q.addComment, ["id": subject, "b": body])
    }

    func addReview(pr: String, event: String, body: String?, comments: [PendingComment]) async throws {
        let threads: [[String: Any]] = comments.map { ["path": $0.path, "line": $0.line, "side": $0.side, "body": $0.body] }
        try await mutate(Q.addReview, ["id": pr, "e": event, "b": body?.isEmpty == true ? nil : body, "th": threads.isEmpty ? nil : threads])
    }

    func requestReviewers(_ ref: ItemRef, logins: [String]) async throws {
        try await send("POST", "/repos/\(ref.repo.full)/pulls/\(ref.number)/requested_reviewers", body: ["reviewers": logins])
    }

    func deleteBranch(repo: String, branch: String) async throws {
        let encoded = branch.split(separator: "/").map { $0.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? String($0) }.joined(separator: "/")
        try await send("DELETE", "/repos/\(repo)/git/refs/heads/\(encoded)")
    }

    func rerunFailedJobs(repo: RepoRef, runId: Int) async throws {
        try await send("POST", "/repos/\(repo.full)/actions/runs/\(runId)/rerun-failed-jobs")
    }

    func jobLog(repo: RepoRef, jobId: Int) async throws -> String {
        let target = try await redirectTarget("/repos/\(repo.full)/actions/jobs/\(jobId)/logs")
        return try await plain(target)
    }
}
