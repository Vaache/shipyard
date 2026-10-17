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

    init(account: GitAccount, credential: GitCredential) {
        host = account.host
        http = ForgeHTTP(base: "https://\(account.host)/api/v1", service: account.host, authorization: "token \(credential.token)")
    }

    func currentUser() async throws -> ForgeUser {
        let u: User = try await http.get("/user")
        return ForgeUser(login: u.login, name: u.fullName?.isEmpty == false ? u.fullName : nil, avatarUrl: u.avatarUrl, id: String(u.id))
    }

    func capabilities() async -> ForgeCapabilities {
        ForgeCapabilities(pullFilters: PRFilter.allCases)
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
        let s: String
        switch state {
        case .open: s = "open"
        case .closed: s = "closed"
        case .all: s = "all"
        }
        let issues: [Issue] = try await http.get("/repos/issues/search", [
            URLQueryItem(name: "type", value: "pulls"),
            URLQueryItem(name: "state", value: s),
            URLQueryItem(name: flag, value: "true"),
            URLQueryItem(name: "limit", value: "50")
        ])
        return issues.map(item).newestFirst()
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
            labels: (i.labels ?? []).map { LabelInfo(name: $0.name, color: ($0.color ?? "888888").replacingOccurrences(of: "#", with: "")) }
        )
    }
}
