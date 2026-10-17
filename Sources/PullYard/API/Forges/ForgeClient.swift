import Foundation

struct ForgeUser {
    let login: String
    let name: String?
    let avatarUrl: String?
    let id: String?
}

struct ForgeCapabilities: Equatable {
    var pullFilters: [PRFilter]
    var issueFilters: [IssueFilter] = []
    var inboxTitle: String?
    var repositories = false

    var hasInbox: Bool { inboxTitle != nil }
}

struct ForgeNotification: Identifiable, Equatable {
    let id: String
    let title: String
    let repo: String
    let kind: String
    let reason: String
    let url: String
    let updatedAt: Date
    let author: Actor?
    var unread: Bool
}

struct ForgeRepo: Identifiable, Equatable {
    var id: String { fullName }
    let fullName: String
    let description: String?
    let url: String
    let isPrivate: Bool
    let updatedAt: Date?
    let defaultBranch: String?
    let stars: Int?
}

@MainActor
protocol ForgeClient: AnyObject {
    var provider: GitProvider { get }
    var host: String { get }
    func currentUser() async throws -> ForgeUser
    func capabilities() async -> ForgeCapabilities
    func pullRequests(_ filter: PRFilter, state: StateFilter) async throws -> [SearchItem]
    func issues(_ filter: IssueFilter, state: StateFilter) async throws -> [SearchItem]
    func notifications() async throws -> [ForgeNotification]
    func markDone(_ notification: ForgeNotification) async throws
    func repositories() async throws -> [ForgeRepo]
    func reviewBackend() -> PRBackend?
}

extension ForgeClient {
    func issues(_ filter: IssueFilter, state: StateFilter) async throws -> [SearchItem] { [] }
    func notifications() async throws -> [ForgeNotification] { [] }
    func markDone(_ notification: ForgeNotification) async throws {}
    func repositories() async throws -> [ForgeRepo] { [] }
    func reviewBackend() -> PRBackend? { nil }
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
        try await call("GET", path, query)
    }

    func post(_ path: String, _ query: [URLQueryItem] = [], method: String = "POST", json: [String: Any]? = nil) async throws {
        let _: Empty = try await call(method, path, query, json: json, allowEmpty: true)
    }

    func send<T: Decodable>(_ method: String, _ path: String, _ query: [URLQueryItem] = [], json: [String: Any]? = nil) async throws -> T {
        try await call(method, path, query, json: json)
    }

    private struct Empty: Decodable {}

    private func call<T: Decodable>(_ method: String, _ path: String, _ query: [URLQueryItem], json: [String: Any]? = nil, allowEmpty: Bool = false) async throws -> T {
        let target = try url(path, query)
        let body = try json.map { try JSONSerialization.data(withJSONObject: $0, options: [.sortedKeys]) }
        var (data, status) = try await send(target, method: method, body: body)
        if status == 401, let refresh {
            authorization = try await refresh()
            (data, status) = try await send(target, method: method, body: body)
        }
        guard (200..<300).contains(status) else {
            if status == 401 { throw APIError(message: "Token is invalid or revoked (401)", status: 401) }
            throw APIError(message: "\(service) \(status): \(Self.message(data) ?? HTTPURLResponse.localizedString(forStatusCode: status))", status: status)
        }
        if allowEmpty, let empty = Empty() as? T { return empty }
        do {
            return try decoder.decode(T.self, from: data)
        } catch {
            throw APIError(message: "Failed to parse response from \(service): \(error)")
        }
    }

    private func send(_ url: URL, method: String = "GET", body: Data? = nil) async throws -> (Data, Int) {
        var req = URLRequest(url: url)
        req.httpMethod = method
        if let body {
            req.httpBody = body
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        req.setValue(authorization, forHTTPHeaderField: "Authorization")
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.setValue("PullYard-macOS", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await session.data(for: req)
        return (data, (response as? HTTPURLResponse)?.statusCode ?? 0)
    }

    private static func message(_ data: Data) -> String? {
        guard let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }
        if let m = obj["message"] as? String { return m }
        if let m = obj["message"] as? [String], !m.isEmpty { return m.joined(separator: "; ") }
        if let m = obj["message"] as? [String: Any], !m.isEmpty {
            return m.keys.sorted().map { key in
                let value = (m[key] as? [String])?.joined(separator: ", ") ?? (m[key].map { "\($0)" } ?? "")
                return key == "base" ? value : "\(key) \(value)"
            }.joined(separator: "; ")
        }
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

extension SearchItem {
    static func forgeIssue(number: Int, title: String, url: String, open: Bool, created: Date, updated: Date, author: String?, avatar: String?, repo: String, comments: Int?, labels: [LabelInfo], completed: Bool = true) -> SearchItem {
        SearchItem(
            typename: "Issue",
            number: number,
            title: title,
            url: url,
            prState: nil,
            issueState: open ? "OPEN" : "CLOSED",
            stateReason: open ? nil : (completed ? "COMPLETED" : "NOT_PLANNED"),
            isDraft: nil,
            createdAt: created,
            updatedAt: updated,
            baseRefName: nil,
            author: author.map { Actor(login: $0, avatarUrl: avatar) },
            repository: RepoName(nameWithOwner: repo, defaultBranchRef: nil),
            reviewDecision: nil,
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
        let open = item.isPR ? item.prState == "OPEN" : item.issueState == "OPEN"
        switch self {
        case .open: return open
        case .closed: return !open
        case .all: return true
        }
    }
}

struct FlexibleLabel: Decodable {
    let name: String
    let color: String?

    init(from decoder: Decoder) throws {
        if let single = try? decoder.singleValueContainer(), let name = try? single.decode(String.self) {
            self.name = name
            color = nil
            return
        }
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        color = try c.decodeIfPresent(String.self, forKey: .color)
    }

    enum CodingKeys: String, CodingKey { case name, color }

    var info: LabelInfo { LabelInfo(name: name, color: (color ?? "888888").replacingOccurrences(of: "#", with: "")) }
}

extension Array where Element == SearchItem {
    func newestFirst(limit: Int = 100) -> [SearchItem] {
        var seen = Set<String>()
        return filter { seen.insert($0.id).inserted }.sorted { $0.updatedAt > $1.updatedAt }.prefix(limit).map { $0 }
    }
}
