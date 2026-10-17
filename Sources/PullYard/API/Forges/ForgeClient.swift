import Foundation

struct ForgeUser {
    let login: String
    let name: String?
    let avatarUrl: String?
    let id: String?
}

@MainActor
protocol ForgeClient: AnyObject {
    var provider: GitProvider { get }
    var host: String { get }
    func currentUser() async throws -> ForgeUser
    func pullRequests(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem]
}

@MainActor
func makeForgeClient(_ account: GitAccount, credential: GitCredential) -> ForgeClient? {
    switch account.provider {
    case .gitlab: return GitLabClient(account: account, credential: credential)
    case .bitbucket: return BitbucketClient(account: account, credential: credential)
    case .gitea: return GiteaClient(account: account, credential: credential)
    case .github, .githubEnterprise: return nil
    }
}

final class ForgeHTTP {
    let base: String
    let service: String
    var authorization: String
    var refresh: (() async throws -> String)?
    private let session: URLSession
    let decoder: JSONDecoder
    nonisolated(unsafe) static var protocolClasses: [AnyClass] = []

    init(base: String, service: String, authorization: String) {
        self.base = base
        self.service = service
        self.authorization = authorization
        let config = URLSessionConfiguration.default
        if !Self.protocolClasses.isEmpty { config.protocolClasses = Self.protocolClasses }
        config.urlCache = nil
        config.timeoutIntervalForRequest = 40
        session = URLSession(configuration: config)
        decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .custom { d in
            let c = try d.singleValueContainer()
            let s = try c.decode(String.self)
            guard let date = ForgeHTTP.parseDate(s) else { throw DecodingError.dataCorruptedError(in: c, debugDescription: "Bad date \(s)") }
            return date
        }
    }

    private static let iso: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    static func parseDate(_ s: String) -> Date? {
        var t = s
        if let r = t.range(of: #"\.\d+"#, options: .regularExpression) { t.removeSubrange(r) }
        return iso.date(from: t)
    }

    func url(_ path: String, _ query: [URLQueryItem]) throws -> URL {
        guard var c = URLComponents(string: path.hasPrefix("http") ? path : base + path) else { throw APIError(message: "Invalid address: \(base)\(path)") }
        if !query.isEmpty { c.queryItems = (c.queryItems ?? []) + query }
        c.percentEncodedQuery = c.percentEncodedQuery?.replacingOccurrences(of: "+", with: "%2B")
        guard let u = c.url else { throw APIError(message: "Invalid address: \(base)\(path)") }
        return u
    }

    func get<T: Decodable>(_ path: String, _ query: [URLQueryItem] = []) async throws -> T {
        let target = try url(path, query)
        var (data, status) = try await send(target)
        if status == 401, let refresh {
            authorization = try await refresh()
            (data, status) = try await send(target)
        }
        guard (200..<300).contains(status) else {
            if status == 401 { throw APIError(message: "Token is invalid or revoked (401)", status: 401) }
            throw APIError(message: "\(service) \(status): \(Self.message(data) ?? HTTPURLResponse.localizedString(forStatusCode: status))", status: status)
        }
        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            throw APIError(message: "Failed to parse response from \(service): \(error)")
        }
    }

    private func send(_ url: URL) async throws -> (Data, Int) {
        var req = URLRequest(url: url)
        req.setValue(authorization, forHTTPHeaderField: "Authorization")
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.setValue("PullYard-macOS", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await session.data(for: req)
        return (data, (response as? HTTPURLResponse)?.statusCode ?? 0)
    }

    private static func message(_ data: Data) -> String? {
        guard let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        if let m = obj["message"] as? String { return m }
        if let e = obj["error"] as? [String: Any], let m = e["message"] as? String { return m }
        if let e = obj["error_description"] as? String { return e }
        if let e = obj["error"] as? String { return e }
        return nil
    }
}

extension SearchItem {
    static func forge(number: Int, title: String, url: String, state: String, draft: Bool, created: Date, updated: Date, base: String?, author: String?, avatar: String?, repo: String, comments: Int?, labels: [LabelInfo], review: String? = nil) -> SearchItem {
        SearchItem(
            typename: "PullRequest",
            number: number,
            title: title,
            url: url,
            prState: state,
            issueState: nil,
            stateReason: nil,
            isDraft: draft,
            createdAt: created,
            updatedAt: updated,
            baseRefName: base,
            author: author.map { Actor(login: $0, avatarUrl: avatar) },
            repository: RepoName(nameWithOwner: repo, defaultBranchRef: nil),
            reviewDecision: review,
            additions: nil,
            deletions: nil,
            comments: comments.map { Count(totalCount: $0) },
            labels: labels.isEmpty ? nil : Nodes(nodes: labels),
            commits: nil
        )
    }
}

extension StateFilter {
    func matches(_ item: SearchItem) -> Bool {
        switch self {
        case .open: return item.prState == "OPEN"
        case .closed: return item.prState != "OPEN"
        case .all: return true
        }
    }
}

extension Array where Element == SearchItem {
    func newestFirst(limit: Int = 100) -> [SearchItem] {
        var seen = Set<String>()
        return filter { seen.insert($0.id).inserted }.sorted { $0.updatedAt > $1.updatedAt }.prefix(limit).map { $0 }
    }
}
