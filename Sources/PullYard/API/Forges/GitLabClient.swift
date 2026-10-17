import Foundation

@MainActor
final class GitLabClient: ForgeClient {
    let provider = GitProvider.gitlab
    let host: String
    private let http: ForgeHTTP
    private var login: String?

    private struct User: Decodable {
        let id: Int
        let username: String
        let name: String?
        let avatarUrl: String?
    }

    private struct MergeRequest: Decodable {
        struct Author: Decodable {
            let username: String
            let avatarUrl: String?
        }
        struct References: Decodable { let full: String? }
        struct Label: Decodable {
            let name: String
            let color: String?
        }

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
        let labels: [Label]?
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

    func pullRequests(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem] {
        let me = try await username()
        var base = [
            URLQueryItem(name: "order_by", value: "updated_at"),
            URLQueryItem(name: "sort", value: "desc"),
            URLQueryItem(name: "per_page", value: "50"),
            URLQueryItem(name: "with_labels_details", value: "true")
        ]
        switch filter {
        case .reviewRequested:
            base += [URLQueryItem(name: "scope", value: "all"), URLQueryItem(name: "reviewer_username", value: me)]
        case .mine:
            base.append(URLQueryItem(name: "scope", value: "created_by_me"))
        case .assigned:
            base.append(URLQueryItem(name: "scope", value: "assigned_to_me"))
        case .mentioned, .reviewed:
            return []
        }
        let states: [String]
        switch state {
        case .open: states = ["opened"]
        case .closed: states = ["merged", "closed"]
        case .all: states = ["all"]
        }
        var items: [SearchItem] = []
        for s in states {
            let page: [MergeRequest] = try await http.get("/merge_requests", base + [URLQueryItem(name: "state", value: s)])
            items += page.map(item)
        }
        return items.newestFirst()
    }

    private func username() async throws -> String {
        if let login { return login }
        return try await currentUser().login
    }

    private func item(_ mr: MergeRequest) -> SearchItem {
        let state: String
        switch mr.state {
        case "merged": state = "MERGED"
        case "opened", "locked": state = "OPEN"
        default: state = "CLOSED"
        }
        let repo = mr.references?.full?.components(separatedBy: "!").first ?? Self.project(from: mr.webUrl)
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
            repo: repo,
            comments: mr.userNotesCount,
            labels: (mr.labels ?? []).map { LabelInfo(name: $0.name, color: ($0.color ?? "888888").replacingOccurrences(of: "#", with: "")) }
        )
    }

    private static func project(from url: String) -> String {
        guard let path = URL(string: url)?.path else { return "?" }
        return path.components(separatedBy: "/-/").first.map { String($0.drop { $0 == "/" }) } ?? "?"
    }
}
