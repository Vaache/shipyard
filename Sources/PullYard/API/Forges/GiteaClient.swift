import Foundation

@MainActor
final class GiteaClient: ForgeClient {
    let provider = GitProvider.gitea
    let host: String
    private let http: ForgeHTTP

    private struct User: Decodable {
        let id: Int
        let login: String
        let fullName: String?
        let avatarUrl: String?
    }

    private struct Issue: Decodable {
        struct Pull: Decodable {
            let merged: Bool?
            let draft: Bool?
        }
        struct Repo: Decodable { let fullName: String }
        struct Label: Decodable {
            let name: String
            let color: String?
        }

        let number: Int
        let title: String
        let htmlUrl: String
        let state: String
        let createdAt: Date
        let updatedAt: Date
        let user: User?
        let repository: Repo?
        let comments: Int?
        let labels: [Label]?
        let pullRequest: Pull?
    }

    private struct Thread: Decodable {
        struct Subject: Decodable {
            let title: String
            let type: String?
            let url: String?
            let htmlUrl: String?
            let latestCommentHtmlUrl: String?
            let state: String?
        }
        struct Repo: Decodable {
            let fullName: String
            let htmlUrl: String?
        }

        let id: Int
        let unread: Bool
        let pinned: Bool?
        let updatedAt: Date
        let subject: Subject
        let repository: Repo?
    }

    private struct Repository: Decodable {
        let fullName: String
        let description: String?
        let htmlUrl: String
        let `private`: Bool?
        let archived: Bool?
        let updatedAt: Date?
        let defaultBranch: String?
        let starsCount: Int?
    }

    init(account: GitAccount, credential: GitCredential) {
        host = account.host
        http = ForgeHTTP(base: "https://\(account.host)/api/v1", service: account.host, authorization: "token \(credential.token)")
    }

    func currentUser() async throws -> ForgeUser {
        let u: User = try await http.get("/user")
        return ForgeUser(login: u.login, name: u.fullName?.isEmpty == false ? u.fullName : nil, avatarUrl: u.avatarUrl, id: String(u.id))
    }

    func capabilities() async -> ForgeCapabilities {
        ForgeCapabilities(pullFilters: PRFilter.allCases, issueFilters: IssueFilter.allCases, inboxTitle: "Notifications", repositories: true)
    }

    func pullRequests(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem] {
        let flag: String
        switch filter {
        case .reviewRequested: flag = "review_requested"
        case .mine: flag = "created"
        case .assigned: flag = "assigned"
        case .mentioned: flag = "mentioned"
        case .reviewed: flag = "reviewed"
        }
        let issues: [Issue] = try await search(type: "pulls", flag: flag, state: state)
        return issues.map(item).newestFirst()
    }

    func issues(_ filter: IssueFilter, state: StateFilter) async throws -> [SearchItem] {
        let flag: String
        switch filter {
        case .assigned: flag = "assigned"
        case .mine: flag = "created"
        case .mentioned: flag = "mentioned"
        }
        let issues: [Issue] = try await search(type: "issues", flag: flag, state: state)
        return issues.map(issueItem).newestFirst()
    }

    func notifications() async throws -> [ForgeNotification] {
        let threads: [Thread] = try await http.get("/notifications", [URLQueryItem(name: "limit", value: "50")])
        return threads.map { t in
            ForgeNotification(
                id: String(t.id),
                title: t.subject.title,
                repo: t.repository?.fullName ?? "",
                kind: Self.kind(t.subject.type),
                reason: Self.reason(t),
                url: webURL(t),
                updatedAt: t.updatedAt,
                author: nil,
                unread: t.unread
            )
        }
        .sorted { $0.updatedAt > $1.updatedAt }
    }

    func markDone(_ notification: ForgeNotification) async throws {
        try await http.post("/notifications/threads/\(notification.id)", [URLQueryItem(name: "to-status", value: "read")], method: "PATCH")
    }

    func repositories() async throws -> [ForgeRepo] {
        let limit = 50
        var all: [Repository] = []
        for page in 1...3 {
            let batch: [Repository] = try await http.get("/user/repos", [
                URLQueryItem(name: "order_by", value: "recentupdate"),
                URLQueryItem(name: "limit", value: String(limit)),
                URLQueryItem(name: "page", value: String(page))
            ])
            all += batch
            if batch.count < limit { break }
        }
        var seen = Set<String>()
        return all
            .filter { $0.archived != true && seen.insert($0.fullName).inserted }
            .sorted { ($0.updatedAt ?? .distantPast) > ($1.updatedAt ?? .distantPast) }
            .map { ForgeRepo(fullName: $0.fullName, description: $0.description?.isEmpty == false ? $0.description : nil, url: $0.htmlUrl, isPrivate: $0.private ?? false, updatedAt: $0.updatedAt, defaultBranch: $0.defaultBranch, stars: $0.starsCount) }
    }

    private func search(type: String, flag: String, state: StateFilter) async throws -> [Issue] {
        let s: String
        switch state {
        case .open: s = "open"
        case .closed: s = "closed"
        case .all: s = "all"
        }
        return try await http.get("/repos/issues/search", [
            URLQueryItem(name: "type", value: type),
            URLQueryItem(name: "state", value: s),
            URLQueryItem(name: flag, value: "true"),
            URLQueryItem(name: "sort", value: "recentupdate"),
            URLQueryItem(name: "limit", value: "50")
        ])
    }

    private func webURL(_ t: Thread) -> String {
        for candidate in [t.subject.latestCommentHtmlUrl, t.subject.htmlUrl] {
            if let candidate, !candidate.isEmpty { return candidate }
        }
        if let api = t.subject.url, let range = api.range(of: "/api/v1/repos/") {
            return (api[..<range.lowerBound] + "/" + api[range.upperBound...]).replacingOccurrences(of: "/git/commits/", with: "/commit/")
        }
        return t.repository?.htmlUrl ?? "https://\(host)/notifications"
    }

    private static func kind(_ type: String?) -> String {
        switch type?.lowercased() {
        case "pull": return "PR"
        case "issue": return "Issue"
        case "commit": return "Commit"
        case "repository": return "Repository"
        default: return type ?? "Notification"
        }
    }

    private static func reason(_ t: Thread) -> String {
        if t.pinned == true { return "Pinned" }
        let subject: String
        switch t.subject.type?.lowercased() {
        case "pull": subject = "Pull request"
        case "issue": subject = "Issue"
        case "commit": return "Commit"
        case "repository": return "Repository"
        default: return t.subject.type ?? "Notification"
        }
        switch t.subject.state?.lowercased() {
        case "merged": return "\(subject) merged"
        case "closed": return "\(subject) closed"
        default: return subject
        }
    }

    private func item(_ i: Issue) -> SearchItem {
        let state = i.state == "open" ? "OPEN" : (i.pullRequest?.merged == true ? "MERGED" : "CLOSED")
        return .forge(
            number: i.number,
            title: i.title,
            url: i.htmlUrl,
            state: state,
            draft: i.pullRequest?.draft ?? false,
            created: i.createdAt,
            updated: i.updatedAt,
            base: nil,
            author: i.user?.login,
            avatar: i.user?.avatarUrl,
            repo: i.repository?.fullName ?? "?",
            comments: i.comments,
            labels: labels(i)
        )
    }

    private func issueItem(_ i: Issue) -> SearchItem {
        .forgeIssue(
            number: i.number,
            title: i.title,
            url: i.htmlUrl,
            open: i.state == "open",
            created: i.createdAt,
            updated: i.updatedAt,
            author: i.user?.login,
            avatar: i.user?.avatarUrl,
            repo: i.repository?.fullName ?? "?",
            comments: i.comments,
            labels: labels(i)
        )
    }

    private func labels(_ i: Issue) -> [LabelInfo] {
        (i.labels ?? []).map { LabelInfo(name: $0.name, color: ($0.color ?? "888888").replacingOccurrences(of: "#", with: "")) }
    }
}
