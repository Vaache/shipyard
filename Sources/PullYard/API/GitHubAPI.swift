import Foundation

struct APIError: LocalizedError {
    let message: String
    var status: Int = 0
    var schemaError = false
    var errorDescription: String? { message }
}

private struct GraphQLError: Decodable {
    let message: String
    let type: String?
}

private struct GraphQLResponse<T: Decodable>: Decodable {
    let data: T?
    let errors: [GraphQLError]?
}

private struct RESTMessage: Decodable {
    let message: String?
}

private final class NoRedirect: NSObject, URLSessionTaskDelegate {
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest) async -> URLRequest? {
        nil
    }
}

private struct PolledEntry {
    var value: Any
    var lastModified: String?
    var fetchedAt: Date
    var pollInterval: TimeInterval
}

final class GitHubAPI {
    let token: String
    var onRateLimit: ((Date) -> Void)?
    private let session: URLSession
    private let gqlDecoder: JSONDecoder
    private let restDecoder: JSONDecoder
    private let lock = NSLock()
    private var polled: [String: PolledEntry] = [:]

    static let retryDelay: UInt64 = 1_000_000_000
    static let retryStatuses: Set<Int> = [502, 503, 504]
    static let retryErrors: Set<URLError.Code> = [.timedOut, .networkConnectionLost]

    init(token: String) {
        self.token = token
        let config = URLSessionConfiguration.default
        config.urlCache = nil
        config.timeoutIntervalForRequest = 10
        config.timeoutIntervalForResource = 120
        session = URLSession(configuration: config)
        gqlDecoder = JSONDecoder()
        gqlDecoder.dateDecodingStrategy = .custom(GitHubAPI.decodeDate)
        restDecoder = JSONDecoder()
        restDecoder.dateDecodingStrategy = .custom(GitHubAPI.decodeDate)
        restDecoder.keyDecodingStrategy = .convertFromSnakeCase
    }

    private static let isoPlain: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    private static let isoFractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static func decodeDate(_ decoder: Decoder) throws -> Date {
        let c = try decoder.singleValueContainer()
        let s = try c.decode(String.self)
        if let d = isoPlain.date(from: s) ?? isoFractional.date(from: s) { return d }
        throw DecodingError.dataCorruptedError(in: c, debugDescription: "Bad date \(s)")
    }

    private func request(_ url: URL, method: String) -> URLRequest {
        var req = URLRequest(url: url)
        req.httpMethod = method
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.setValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")
        req.setValue("PullYard-macOS", forHTTPHeaderField: "User-Agent")
        return req
    }

    private func perform(_ req: URLRequest, _ kind: RequestKind, _ name: @autoclosure () -> String, retry: Bool, delegate: URLSessionTaskDelegate? = nil) async throws -> (Data, URLResponse, Int?) {
        var attempt = 1
        while true {
            let canRetry = retry && attempt == 1
            do {
                let (data, response, id) = try await Diagnostics.measure(kind, name(), url: req.url) { try await session.data(for: req, delegate: delegate) }
                if let http = response as? HTTPURLResponse {
                    noteRateLimit(http, data)
                    if canRetry, Self.retryStatuses.contains(http.statusCode) {
                        attempt += 1
                        try await Task.sleep(nanoseconds: Self.retryDelay)
                        continue
                    }
                }
                return (data, response, id)
            } catch let e as URLError where canRetry && Self.retryErrors.contains(e.code) {
                attempt += 1
                try await Task.sleep(nanoseconds: Self.retryDelay)
            }
        }
    }

    private func noteRateLimit(_ http: HTTPURLResponse, _ data: Data) {
        guard http.statusCode == 403 || http.statusCode == 429 else { return }
        if let until = Self.rateLimitEnd(http, data) { onRateLimit?(until) }
    }

    static func rateLimitEnd(_ http: HTTPURLResponse, _ data: Data?) -> Date? {
        if let retryAfter = http.value(forHTTPHeaderField: "Retry-After").flatMap(TimeInterval.init) {
            return Date().addingTimeInterval(max(retryAfter, 1))
        }
        let reset = http.value(forHTTPHeaderField: "X-RateLimit-Reset").flatMap(TimeInterval.init).map { Date(timeIntervalSince1970: $0) }
        if http.value(forHTTPHeaderField: "X-RateLimit-Remaining") == "0", let reset, reset > Date() {
            return reset
        }
        let message = data.flatMap { try? JSONDecoder().decode(RESTMessage.self, from: $0) }?.message?.lowercased() ?? ""
        if message.contains("rate limit") {
            return Date().addingTimeInterval(60)
        }
        return nil
    }

    private func check(_ response: URLResponse, _ data: Data) throws {
        guard let http = response as? HTTPURLResponse else { return }
        guard !(200..<300).contains(http.statusCode), http.statusCode != 304 else { return }
        let msg = (try? JSONDecoder().decode(RESTMessage.self, from: data))?.message ?? HTTPURLResponse.localizedString(forStatusCode: http.statusCode)
        if http.statusCode == 401 {
            throw APIError(message: "Token is invalid or revoked (401)", status: 401)
        }
        throw APIError(message: "GitHub \(http.statusCode): \(msg)", status: http.statusCode)
    }

    func graphQL<T: Decodable>(_ query: String, _ variables: [String: Any?] = [:], strict: Bool = false) async throws -> T {
        var req = request(URL(string: "https://api.github.com/graphql")!, method: "POST")
        let vars = variables.compactMapValues { $0 }
        req.httpBody = try JSONSerialization.data(withJSONObject: ["query": query, "variables": vars])
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let isRead = !query.trimmingCharacters(in: .whitespacesAndNewlines).hasPrefix("mutation")
        let (data, response, recordID) = try await perform(req, .graphql, Diagnostics.graphQLName(query), retry: isRead)
        try check(response, data)
        let decoded: GraphQLResponse<T>
        do {
            decoded = try gqlDecoder.decode(GraphQLResponse<T>.self, from: data)
        } catch {
            if let errs = try? gqlDecoder.decode(GraphQLResponse<EmptyData>.self, from: data).errors, let first = errs.first {
                if let recordID { Diagnostics.shared.annotate(recordID, error: "graphql error") }
                throw APIError(message: GitHubAPI.describe(first), schemaError: first.type == nil)
            }
            if let recordID { Diagnostics.shared.annotate(recordID, error: "parse error") }
            throw APIError(message: "Failed to parse response: \(error)")
        }
        if let errors = decoded.errors, errors.contains(where: { $0.type == "RATE_LIMITED" }), let http = response as? HTTPURLResponse {
            onRateLimit?(Self.rateLimitEnd(http, nil) ?? Date().addingTimeInterval(60))
        }
        if let errors = decoded.errors, let first = errors.first, strict || decoded.data == nil {
            if let recordID { Diagnostics.shared.annotate(recordID, error: "graphql error") }
            throw APIError(message: GitHubAPI.describe(first), schemaError: first.type == nil)
        }
        if let recordID, decoded.errors?.isEmpty == false { Diagnostics.shared.annotate(recordID, note: "partial") }
        guard let value = decoded.data else { throw APIError(message: "Empty response from GitHub") }
        return value
    }

    private static func describe(_ e: GraphQLError) -> String {
        if e.type == "INSUFFICIENT_SCOPES" {
            return "Insufficient token scopes: \(e.message)"
        }
        return e.message
    }

    private func url(_ path: String) -> URL {
        if path.hasPrefix("http") { return URL(string: path)! }
        return URL(string: "https://api.github.com" + path)!
    }

    func rest<T: Decodable>(_ method: String = "GET", _ path: String, body: [String: Any]? = nil) async throws -> T {
        let (data, _) = try await restRaw(method, path, body: body)
        do {
            return try restDecoder.decode(T.self, from: data)
        } catch {
            throw APIError(message: "Failed to parse response: \(error)")
        }
    }

    func send(_ method: String, _ path: String, body: [String: Any]? = nil) async throws {
        _ = try await restRaw(method, path, body: body)
    }

    @discardableResult
    func restRaw(_ method: String, _ path: String, body: [String: Any]? = nil, headers: [String: String] = [:]) async throws -> (Data, HTTPURLResponse) {
        var req = request(url(path), method: method)
        if let body {
            req.httpBody = try JSONSerialization.data(withJSONObject: body)
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        for (name, value) in headers { req.setValue(value, forHTTPHeaderField: name) }
        let (data, response, _) = try await perform(req, .rest, Diagnostics.restName(method, req.url), retry: method == "GET")
        try check(response, data)
        return (data, response as! HTTPURLResponse)
    }

    func restPolled<T: Decodable>(_ path: String, live: Bool) async throws -> T {
        let cached = lock.withLock { polled[path] }
        if live, let cached, let value = cached.value as? T, Date() < cached.fetchedAt.addingTimeInterval(cached.pollInterval) {
            return value
        }
        let conditional = cached?.lastModified.map { ["If-Modified-Since": $0] } ?? [:]
        let (data, http) = try await restRaw("GET", path, headers: conditional)
        let interval = http.value(forHTTPHeaderField: "X-Poll-Interval").flatMap(TimeInterval.init) ?? 0
        if http.statusCode == 304, let cached, let value = cached.value as? T {
            lock.withLock {
                polled[path]?.fetchedAt = Date()
                polled[path]?.pollInterval = interval
            }
            return value
        }
        let value: T
        do {
            value = try restDecoder.decode(T.self, from: data)
        } catch {
            throw APIError(message: "Failed to parse response: \(error)")
        }
        let entry = PolledEntry(value: value, lastModified: http.value(forHTTPHeaderField: "Last-Modified"), fetchedAt: Date(), pollInterval: interval)
        lock.withLock { polled[path] = entry }
        return value
    }

    func forgetPolled(prefix: String) {
        lock.withLock { polled = polled.filter { !$0.key.hasPrefix(prefix) } }
    }

    func redirectTarget(_ path: String) async throws -> URL {
        let req = request(url(path), method: "GET")
        let (data, response, _) = try await perform(req, .rest, Diagnostics.restName("GET", req.url), retry: true, delegate: NoRedirect())
        guard let http = response as? HTTPURLResponse else { throw APIError(message: "No response") }
        if (300..<400).contains(http.statusCode), let loc = http.value(forHTTPHeaderField: "Location"), let u = URL(string: loc) {
            return u
        }
        try check(response, data)
        throw APIError(message: "Log unavailable")
    }

    func plain(_ url: URL) async throws -> String {
        let (data, response) = try await Diagnostics.track(.rest, "GET {job-log}", url: url, authenticated: false) { try await URLSession.shared.data(from: url) }
        try check(response, data)
        return String(decoding: data, as: UTF8.self)
    }
}

struct EmptyData: Decodable {}
