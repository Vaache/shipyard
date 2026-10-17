import Foundation

@MainActor
final class BitbucketClient: ForgeClient {
    let provider = GitProvider.bitbucket
    let host: String
    private let http: ForgeHTTP
    private var uuid: String?
    private var workspaces: [String]?
    private var cache: [String: (at: Date, items: [SearchItem])] = [:]

    static let maxRepos = 20
    static let cacheTTL: TimeInterval = 180

    private struct Avatar: Decodable { let href: String? }
    private struct UserLinks: Decodable { let avatar: Avatar? }
    private struct User: Decodable {
        let uuid: String
        let displayName: String?
        let nickname: String?
        let username: String?
        let links: UserLinks?
    }

    private struct Page<T: Decodable>: Decodable {
        let values: [T]
    }

    private struct WorkspaceRef: Decodable {
        struct Inner: Decodable { let slug: String }
        let slug: String?
        let workspace: Inner?
        var name: String? { workspace?.slug ?? slug }
    }

    private struct Repo: Decodable { let fullName: String }

    private struct PullRequest: Decodable {
        struct Link: Decodable { let href: String }
        struct Links: Decodable { let html: Link? }
        struct Branch: Decodable { let name: String }
        struct RepoName: Decodable { let fullName: String }
        struct Endpoint: Decodable {
            let branch: Branch?
            let repository: RepoName?
        }
        struct Participant: Decodable {
            let approved: Bool?
            let state: String?
        }

        let id: Int
        let title: String
        let state: String
        let draft: Bool?
        let createdOn: Date
        let updatedOn: Date
        let author: User?
        let destination: Endpoint?
        let links: Links?
        let commentCount: Int?
        let participants: [Participant]?
    }

    init(account: GitAccount, credential: GitCredential) {
        host = account.host
        uuid = account.userID
        let auth: String
        if let email = account.email, !email.isEmpty {
            auth = "Basic " + Data("\(email):\(credential.token)".utf8).base64EncodedString()
        } else {
            auth = "Bearer \(credential.token)"
        }
        http = ForgeHTTP(base: "https://api.bitbucket.org/2.0", service: "Bitbucket", authorization: auth)
    }

    func currentUser() async throws -> ForgeUser {
        let u: User = try await http.get("/user")
        uuid = u.uuid
        return ForgeUser(login: u.nickname ?? u.username ?? u.displayName ?? u.uuid, name: u.displayName, avatarUrl: u.links?.avatar?.href, id: u.uuid)
    }

    func capabilities() async -> ForgeCapabilities {
        ForgeCapabilities(pullFilters: [.reviewRequested, .mine])
    }

    func pullRequests(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem] {
        let key = "\(filter.rawValue)|\(state.rawValue)"
        if let hit = cache[key], Date().timeIntervalSince(hit.at) < Self.cacheTTL { return hit.items }
        let items = try await fetch(filter, state: state)
        cache[key] = (Date(), items)
        return items
    }

    private func fetch(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem] {
        let me = try await userID()
        let spaces = try await workspaceSlugs()
        let states: [URLQueryItem]
        switch state {
        case .open: states = [URLQueryItem(name: "state", value: "OPEN")]
        case .closed: states = ["MERGED", "DECLINED", "SUPERSEDED"].map { URLQueryItem(name: "state", value: $0) }
        case .all: states = ["OPEN", "MERGED", "DECLINED", "SUPERSEDED"].map { URLQueryItem(name: "state", value: $0) }
        }
        let common = states + [URLQueryItem(name: "pagelen", value: "50"), URLQueryItem(name: "sort", value: "-updated_on"), URLQueryItem(name: "fields", value: "+values.participants")]
        switch filter {
        case .mine:
            var items: [SearchItem] = []
            for ws in spaces {
                let page: Page<PullRequest> = try await http.get("/workspaces/\(Self.segment(ws))/pullrequests/\(Self.segment(me))", common)
                items += page.values.map(item)
            }
            return items.newestFirst()
        case .reviewRequested:
            var repos: [String] = []
            for ws in spaces where repos.count < Self.maxRepos {
                let page: Page<Repo> = try await http.get("/repositories/\(Self.segment(ws))", [
                    URLQueryItem(name: "role", value: "member"),
                    URLQueryItem(name: "sort", value: "-updated_on"),
                    URLQueryItem(name: "pagelen", value: "\(Self.maxRepos)")
                ])
                repos += page.values.map(\.fullName)
            }
            var items: [SearchItem] = []
            var lastError: Error?
            for repo in repos.prefix(Self.maxRepos) {
                do {
                    let page: Page<PullRequest> = try await http.get("/repositories/\(repo)/pullrequests", common + [URLQueryItem(name: "q", value: "reviewers.uuid=\"\(me)\"")])
                    items += page.values.map(item)
                } catch {
                    lastError = error
                }
            }
            if items.isEmpty, let lastError, (lastError as? APIError)?.status == 401 { throw lastError }
            return items.newestFirst()
        default:
            return []
        }
    }

    private func userID() async throws -> String {
        if let uuid { return uuid }
        return try await currentUser().id ?? ""
    }

    private func workspaceSlugs() async throws -> [String] {
        if let workspaces { return workspaces }
        let page: Page<WorkspaceRef>
        do {
            page = try await http.get("/user/workspaces", [URLQueryItem(name: "pagelen", value: "100")])
        } catch let e as APIError where e.status == 404 || e.status == 410 {
            page = try await http.get("/workspaces", [URLQueryItem(name: "pagelen", value: "100")])
        }
        let slugs = page.values.compactMap(\.name)
        workspaces = slugs
        return slugs
    }

    private static func segment(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(CharacterSet(charactersIn: "/"))) ?? s
    }

    private func item(_ pr: PullRequest) -> SearchItem {
        let state: String
        switch pr.state {
        case "OPEN": state = "OPEN"
        case "MERGED": state = "MERGED"
        default: state = "CLOSED"
        }
        let participants = pr.participants ?? []
        let review: String?
        if participants.contains(where: { $0.state == "changes_requested" }) {
            review = "CHANGES_REQUESTED"
        } else if participants.contains(where: { $0.approved == true }) {
            review = "APPROVED"
        } else {
            review = nil
        }
        let repo = pr.destination?.repository?.fullName ?? "?"
        return .forge(
            number: pr.id,
            title: pr.title,
            url: pr.links?.html?.href ?? "https://bitbucket.org/\(repo)/pull-requests/\(pr.id)",
            state: state,
            draft: pr.draft ?? false,
            created: pr.createdOn,
            updated: pr.updatedOn,
            base: pr.destination?.branch?.name,
            author: pr.author?.nickname ?? pr.author?.displayName,
            avatar: pr.author?.links?.avatar?.href,
            repo: repo,
            comments: pr.commentCount,
            labels: [],
            review: review
        )
    }
}
