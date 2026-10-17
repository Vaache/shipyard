import Foundation

@MainActor
final class GitLabClient: ForgeClient {
    let provider = GitProvider.gitlab
    let host: String
    private let http: ForgeHTTP
    private var login: String?
    private var approvalFilterWorks: Bool?

    private struct User: Decodable {
        let id: Int
        let username: String
        let name: String?
        let avatarUrl: String?
    }

    private struct Author: Decodable {
        let username: String
        let avatarUrl: String?
    }

    private struct References: Decodable { let full: String? }

    private struct MergeRequest: Decodable {
        let iid: Int
        let title: String
        let webUrl: String
        let state: String
        let draft: Bool?
        let workInProgress: Bool?
        let createdAt: Date
        let updatedAt: Date
        let targetBranch: String?
        let author: Author?
        let references: References?
        let userNotesCount: Int?
        let labels: [FlexibleLabel]?
    }

    private struct Issue: Decodable {
        let iid: Int
        let title: String
        let webUrl: String
        let state: String
        let createdAt: Date
        let updatedAt: Date
        let author: Author?
        let references: References?
        let userNotesCount: Int?
        let labels: [FlexibleLabel]?
    }

    private struct Todo: Decodable {
        struct Project: Decodable { let pathWithNamespace: String? }
        struct Target: Decodable {
            let iid: Int?
            let title: String?
            let webUrl: String?
        }

        let id: Int
        let actionName: String
        let targetType: String
        let targetUrl: String?
        let body: String?
        let state: String
        let createdAt: Date
        let author: Author?
        let project: Project?
        let target: Target?
    }

    private struct TodoTarget<T: Decodable>: Decodable {
        let target: T?

        enum CodingKeys: String, CodingKey { case target }

        init(from decoder: Decoder) throws {
            target = try? decoder.container(keyedBy: CodingKeys.self).decode(T.self, forKey: .target)
        }
    }

    private struct Project: Decodable {
        let pathWithNamespace: String
        let description: String?
        let webUrl: String
        let visibility: String?
        let lastActivityAt: Date?
        let defaultBranch: String?
        let starCount: Int?
    }

    init(account: GitAccount, credential: GitCredential) {
        host = account.host
        login = account.login.isEmpty ? nil : account.login
        http = ForgeHTTP(base: "https://\(account.host)/api/v4", service: "GitLab", authorization: "Bearer \(credential.token)")
        if let refreshToken = credential.refreshToken, let clientID = credential.clientID {
            let endpoint = DeviceFlow.gitlab(host: account.host)
            var latest = refreshToken
            http.refresh = {
                let fresh = try await DeviceFlow.refresh(clientID: clientID, refreshToken: latest, endpoint: endpoint)
                latest = fresh.refreshToken ?? latest
                var stored = fresh.credential
                stored.clientID = clientID
                try? await MainActor.run { try GitAccounts.shared.store(stored, for: account) }
                return "Bearer \(fresh.accessToken)"
            }
        }
    }

    func currentUser() async throws -> ForgeUser {
        let u: User = try await http.get("/user")
        login = u.username
        return ForgeUser(login: u.username, name: u.name, avatarUrl: u.avatarUrl, id: String(u.id))
    }

    func capabilities() async -> ForgeCapabilities {
        var filters: [PRFilter] = [.reviewRequested, .mine, .assigned, .mentioned]
        if await approvalFilterAvailable() { filters.append(.reviewed) }
        return ForgeCapabilities(pullFilters: filters, issueFilters: IssueFilter.allCases, inboxTitle: "To-Do List", repositories: true)
    }

    private var listQuery: [URLQueryItem] {
        [
            URLQueryItem(name: "order_by", value: "updated_at"),
            URLQueryItem(name: "sort", value: "desc"),
            URLQueryItem(name: "per_page", value: "50"),
            URLQueryItem(name: "with_labels_details", value: "true")
        ]
    }

    private func states(_ state: StateFilter, closed: [String]) -> [String] {
        switch state {
        case .open: return ["opened"]
        case .closed: return closed
        case .all: return ["all"]
        }
    }

    func pullRequests(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem] {
        let me = try await username()
        if filter == .mentioned {
            let todos: [TodoTarget<MergeRequest>] = try await mentionTodos(type: "MergeRequest")
            return todos.compactMap(\.target).map(item).filter(state.matches).newestFirst()
        }
        var query = listQuery
        switch filter {
        case .reviewRequested:
            query += [URLQueryItem(name: "scope", value: "all"), URLQueryItem(name: "reviewer_username", value: me)]
        case .mine:
            query.append(URLQueryItem(name: "scope", value: "created_by_me"))
        case .assigned:
            query.append(URLQueryItem(name: "scope", value: "assigned_to_me"))
        case .reviewed:
            guard await approvalFilterAvailable() else { return [] }
            query += [URLQueryItem(name: "scope", value: "all"), URLQueryItem(name: "approved_by_usernames[]", value: me)]
        case .mentioned:
            return []
        }
        var items: [SearchItem] = []
        for s in states(state, closed: ["merged", "closed"]) {
            let page: [MergeRequest] = try await http.get("/merge_requests", query + [URLQueryItem(name: "state", value: s)])
            items += page.map(item)
        }
        return items.filter { filter != .reviewed || $0.author?.login.lowercased() != me.lowercased() }.newestFirst()
    }

    func issues(_ filter: IssueFilter, state: StateFilter) async throws -> [SearchItem] {
        if filter == .mentioned {
            let todos: [TodoTarget<Issue>] = try await mentionTodos(type: "Issue")
            return todos.compactMap(\.target).map(issueItem).filter(state.matches).newestFirst()
        }
        let scope = filter == .assigned ? "assigned_to_me" : "created_by_me"
        var items: [SearchItem] = []
        for s in states(state, closed: ["closed"]) {
            let page: [Issue] = try await http.get("/issues", listQuery + [URLQueryItem(name: "scope", value: scope), URLQueryItem(name: "state", value: s)])
            items += page.map(issueItem)
        }
        return items.newestFirst()
    }

    func notifications() async throws -> [ForgeNotification] {
        let todos: [Todo] = try await http.get("/todos", [URLQueryItem(name: "state", value: "pending"), URLQueryItem(name: "per_page", value: "50")])
        return todos.map { t in
            ForgeNotification(
                id: String(t.id),
                title: t.target?.title ?? t.body ?? t.targetType,
                repo: t.project?.pathWithNamespace ?? "",
                kind: t.targetType == "MergeRequest" ? "MR" : t.targetType,
                reason: Self.reason(t.actionName),
                url: t.targetUrl ?? t.target?.webUrl ?? "https://\(host)/dashboard/todos",
                updatedAt: t.createdAt,
                author: t.author.map { Actor(login: $0.username, avatarUrl: $0.avatarUrl) },
                unread: t.state == "pending"
            )
        }
    }

    func markDone(_ notification: ForgeNotification) async throws {
        try await http.post("/todos/\(notification.id)/mark_as_done")
    }

    func repositories() async throws -> [ForgeRepo] {
        let projects: [Project] = try await http.get("/projects", [
            URLQueryItem(name: "membership", value: "true"),
            URLQueryItem(name: "order_by", value: "last_activity_at"),
            URLQueryItem(name: "per_page", value: "100"),
            URLQueryItem(name: "archived", value: "false")
        ])
        return projects.map {
            ForgeRepo(fullName: $0.pathWithNamespace, description: $0.description, url: $0.webUrl, isPrivate: $0.visibility != "public", updatedAt: $0.lastActivityAt, defaultBranch: $0.defaultBranch, stars: $0.starCount)
        }
    }

    private func mentionTodos<T: Decodable>(type: String) async throws -> [T] {
        var all: [T] = []
        for action in ["mentioned", "directly_addressed"] {
            for state in ["pending", "done"] {
                let page: [T] = try await http.get("/todos", [
                    URLQueryItem(name: "type", value: type),
                    URLQueryItem(name: "action", value: action),
                    URLQueryItem(name: "state", value: state),
                    URLQueryItem(name: "per_page", value: "50")
                ])
                all += page
            }
        }
        return all
    }

    private func approvalFilterAvailable() async -> Bool {
        if let approvalFilterWorks { return approvalFilterWorks }
        let probe = "pullyard-probe-\(UUID().uuidString.prefix(8).lowercased())"
        let page: [MergeRequest]? = try? await http.get("/merge_requests", [
            URLQueryItem(name: "scope", value: "all"),
            URLQueryItem(name: "state", value: "all"),
            URLQueryItem(name: "per_page", value: "1"),
            URLQueryItem(name: "approved_by_usernames[]", value: probe)
        ])
        guard let page else { return false }
        approvalFilterWorks = page.isEmpty
        return page.isEmpty
    }

    private func username() async throws -> String {
        if let login { return login }
        return try await currentUser().login
    }

    private static func reason(_ action: String) -> String {
        switch action {
        case "assigned": return "Assigned"
        case "mentioned": return "Mentioned"
        case "directly_addressed": return "Addressed you"
        case "build_failed": return "Pipeline failed"
        case "marked": return "Added to do"
        case "approval_required": return "Approval required"
        case "unmergeable": return "Can't merge"
        case "merge_train_removed": return "Removed from merge train"
        case "review_requested": return "Review requested"
        case "review_submitted": return "Review submitted"
        case "member_access_requested": return "Access requested"
        default: return action.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }

    private func item(_ mr: MergeRequest) -> SearchItem {
        let state: String
        switch mr.state {
        case "merged": state = "MERGED"
        case "opened", "locked": state = "OPEN"
        default: state = "CLOSED"
        }
        return .forge(
            number: mr.iid,
            title: mr.title,
            url: mr.webUrl,
            state: state,
            draft: mr.draft ?? mr.workInProgress ?? false,
            created: mr.createdAt,
            updated: mr.updatedAt,
            base: mr.targetBranch,
            author: mr.author?.username,
            avatar: mr.author?.avatarUrl,
            repo: Self.project(mr.references?.full, separator: "!", url: mr.webUrl),
            comments: mr.userNotesCount,
            labels: (mr.labels ?? []).map(\.info)
        )
    }

    private func issueItem(_ i: Issue) -> SearchItem {
        .forgeIssue(
            number: i.iid,
            title: i.title,
            url: i.webUrl,
            open: i.state == "opened",
            created: i.createdAt,
            updated: i.updatedAt,
            author: i.author?.username,
            avatar: i.author?.avatarUrl,
            repo: Self.project(i.references?.full, separator: "#", url: i.webUrl),
            comments: i.userNotesCount,
            labels: (i.labels ?? []).map(\.info)
        )
    }

    private static func project(_ reference: String?, separator: String, url: String) -> String {
        if let reference, let range = reference.range(of: separator, options: .backwards) {
            return String(reference[..<range.lowerBound])
        }
        guard let path = URL(string: url)?.path else { return "?" }
        return path.components(separatedBy: "/-/").first.map { String($0.drop { $0 == "/" }) } ?? "?"
    }
}
