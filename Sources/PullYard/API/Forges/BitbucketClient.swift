import Foundation

@MainActor
final class BitbucketClient: ForgeClient {
    let provider = GitProvider.bitbucket
    let host: String
    private let http: ForgeHTTP
    private var uuid: String?
    private var workspaces: [String]?
    private var cache: [String: (at: Date, items: [SearchItem])] = [:]
    private var reviewCache: [StateFilter: (at: Date, pulls: [PullRequest])] = [:]
    private var repoCache: (at: Date, repos: [Repo])?
    private var participantFilterWorks = true

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

    private struct Account: Decodable { let uuid: String? }

    private struct Page<T: Decodable>: Decodable {
        let values: [T]
    }

    private struct WorkspaceRef: Decodable {
        struct Inner: Decodable { let slug: String }
        let slug: String?
        let workspace: Inner?
        var name: String? { workspace?.slug ?? slug }
    }

    private struct Link: Decodable { let href: String }
    private struct Links: Decodable { let html: Link? }
    private struct Branch: Decodable { let name: String }

    private struct Repo: Decodable {
        let fullName: String
        let description: String?
        let isPrivate: Bool?
        let updatedOn: Date?
        let mainbranch: Branch?
        let links: Links?
    }

    private struct PullRequest: Decodable {
        struct RepoName: Decodable { let fullName: String }
        struct Endpoint: Decodable {
            let branch: Branch?
            let repository: RepoName?
        }
        struct Participant: Decodable {
            let user: Account?
            let role: String?
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
        let reviewers: [Account]?
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
        ForgeCapabilities(pullFilters: [.reviewRequested, .mine, .reviewed], repositories: true)
    }

    func pullRequests(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem] {
        let key = "\(filter.rawValue)|\(state.rawValue)"
        if let hit = cache[key], Date().timeIntervalSince(hit.at) < Self.cacheTTL { return hit.items }
        let items = try await fetch(filter, state: state)
        cache[key] = (Date(), items)
        return items
    }

    func repositories() async throws -> [ForgeRepo] {
        try await memberRepos().map {
            ForgeRepo(
                fullName: $0.fullName,
                description: $0.description,
                url: $0.links?.html?.href ?? "https://bitbucket.org/\($0.fullName)",
                isPrivate: $0.isPrivate ?? true,
                updatedAt: $0.updatedOn,
                defaultBranch: $0.mainbranch?.name,
                stars: nil
            )
        }
    }

    private func stateQuery(_ state: StateFilter) -> [URLQueryItem] {
        let states: [URLQueryItem]
        switch state {
        case .open: states = [URLQueryItem(name: "state", value: "OPEN")]
        case .closed: states = ["MERGED", "DECLINED", "SUPERSEDED"].map { URLQueryItem(name: "state", value: $0) }
        case .all: states = ["OPEN", "MERGED", "DECLINED", "SUPERSEDED"].map { URLQueryItem(name: "state", value: $0) }
        }
        return states + [URLQueryItem(name: "pagelen", value: "50"), URLQueryItem(name: "sort", value: "-updated_on"), URLQueryItem(name: "fields", value: "+values.participants,+values.reviewers")]
    }

    private func fetch(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem] {
        let me = try await userID()
        switch filter {
        case .mine:
            var items: [SearchItem] = []
            for ws in try await workspaceSlugs() {
                let page: Page<PullRequest> = try await http.get("/workspaces/\(Self.segment(ws))/pullrequests/\(Self.segment(me))", stateQuery(state))
                items += page.values.map(item)
            }
            return items.newestFirst()
        case .reviewRequested:
            return try await reviewPulls(state, me: me).filter { awaitsReview($0, from: me) }.map(item).newestFirst()
        case .reviewed:
            return try await reviewPulls(state, me: me).filter { reviewed($0, by: me) }.map(item).newestFirst()
        default:
            return []
        }
    }

    private func reviewPulls(_ state: StateFilter, me: String) async throws -> [PullRequest] {
        if let hit = reviewCache[state], Date().timeIntervalSince(hit.at) < Self.cacheTTL { return hit.pulls }
        var pulls: [PullRequest] = []
        var lastError: Error?
        for repo in try await memberRepos().prefix(Self.maxRepos) {
            do {
                pulls += try await repoPulls(repo.fullName, state: state, me: me)
            } catch {
                lastError = error
            }
        }
        if pulls.isEmpty, let lastError, (lastError as? APIError)?.status == 401 { throw lastError }
        reviewCache[state] = (Date(), pulls)
        return pulls
    }

    private func repoPulls(_ repo: String, state: StateFilter, me: String) async throws -> [PullRequest] {
        let reviewer = "reviewers.uuid=\"\(me)\""
        if participantFilterWorks {
            do {
                let page: Page<PullRequest> = try await http.get("/repositories/\(repo)/pullrequests", stateQuery(state) + [URLQueryItem(name: "q", value: "\(reviewer) OR participants.user.uuid=\"\(me)\"")])
                return page.values
            } catch let e as APIError where e.status == 400 {
                participantFilterWorks = false
            }
        }
        let page: Page<PullRequest> = try await http.get("/repositories/\(repo)/pullrequests", stateQuery(state) + [URLQueryItem(name: "q", value: reviewer)])
        return page.values
    }

    private func myParticipation(_ pr: PullRequest, _ me: String) -> PullRequest.Participant? {
        pr.participants?.first { Self.same($0.user?.uuid, me) }
    }

    private func awaitsReview(_ pr: PullRequest, from me: String) -> Bool {
        let mine = myParticipation(pr, me)
        let isReviewer = pr.reviewers?.contains { Self.same($0.uuid, me) } == true || mine?.role == "REVIEWER"
        return isReviewer && mine?.approved != true && mine?.state == nil
    }

    private func reviewed(_ pr: PullRequest, by me: String) -> Bool {
        guard !Self.same(pr.author?.uuid, me), let mine = myParticipation(pr, me) else { return false }
        return mine.approved == true || mine.state == "approved" || mine.state == "changes_requested"
    }

    private static func same(_ a: String?, _ b: String) -> Bool {
        a?.lowercased() == b.lowercased()
    }

    private func memberRepos() async throws -> [Repo] {
        if let repoCache, Date().timeIntervalSince(repoCache.at) < Self.cacheTTL { return repoCache.repos }
        var repos: [Repo] = []
        for ws in try await workspaceSlugs() {
            let page: Page<Repo> = try await http.get("/repositories/\(Self.segment(ws))", [
                URLQueryItem(name: "role", value: "member"),
                URLQueryItem(name: "sort", value: "-updated_on"),
                URLQueryItem(name: "pagelen", value: "100")
            ])
            repos += page.values
        }
        repos.sort { ($0.updatedOn ?? .distantPast) > ($1.updatedOn ?? .distantPast) }
        repoCache = (Date(), repos)
        return repos
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
