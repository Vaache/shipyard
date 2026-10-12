import Foundation

struct APIError: LocalizedError {
    let message: String
    var status: Int = 0
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

final class GitHubAPI {
    let token: String
    private let session: URLSession
    private let gqlDecoder: JSONDecoder
    private let restDecoder: JSONDecoder

    init(token: String) {
        self.token = token
        let config = URLSessionConfiguration.default
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        config.timeoutIntervalForRequest = 40
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
        req.setValue("GitHubLite-macOS", forHTTPHeaderField: "User-Agent")
        return req
    }

    private func check(_ response: URLResponse, _ data: Data) throws {
        guard let http = response as? HTTPURLResponse else { return }
        guard !(200..<300).contains(http.statusCode) else { return }
        let msg = (try? JSONDecoder().decode(RESTMessage.self, from: data))?.message ?? HTTPURLResponse.localizedString(forStatusCode: http.statusCode)
        if http.statusCode == 401 {
            throw APIError(message: t("Токен недействителен или отозван (401)", "Token is invalid or revoked (401)"), status: 401)
        }
        throw APIError(message: "GitHub \(http.statusCode): \(msg)", status: http.statusCode)
    }

    func graphQL<T: Decodable>(_ query: String, _ variables: [String: Any?] = [:], strict: Bool = false) async throws -> T {
        var req = request(URL(string: "https://api.github.com/graphql")!, method: "POST")
        let vars = variables.compactMapValues { $0 }
        req.httpBody = try JSONSerialization.data(withJSONObject: ["query": query, "variables": vars])
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        let (data, response) = try await session.data(for: req)
        try check(response, data)
        let decoded: GraphQLResponse<T>
        do {
            decoded = try gqlDecoder.decode(GraphQLResponse<T>.self, from: data)
        } catch {
            if let errs = try? gqlDecoder.decode(GraphQLResponse<EmptyData>.self, from: data).errors, let first = errs.first {
                throw APIError(message: GitHubAPI.describe(first))
            }
            throw APIError(message: t("Ошибка разбора ответа: \(error)", "Failed to parse response: \(error)"))
        }
        if let errors = decoded.errors, let first = errors.first, strict || decoded.data == nil {
            throw APIError(message: GitHubAPI.describe(first))
        }
        guard let value = decoded.data else { throw APIError(message: t("Пустой ответ GitHub", "Empty response from GitHub")) }
        return value
    }

    private static func describe(_ e: GraphQLError) -> String {
        if e.type == "INSUFFICIENT_SCOPES" {
            return t("Недостаточно прав токена: \(e.message)", "Insufficient token scopes: \(e.message)")
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
            throw APIError(message: t("Ошибка разбора ответа: \(error)", "Failed to parse response: \(error)"))
        }
    }

    func send(_ method: String, _ path: String, body: [String: Any]? = nil) async throws {
        _ = try await restRaw(method, path, body: body)
    }

    @discardableResult
    func restRaw(_ method: String, _ path: String, body: [String: Any]? = nil) async throws -> (Data, HTTPURLResponse) {
        var req = request(url(path), method: method)
        if let body {
            req.httpBody = try JSONSerialization.data(withJSONObject: body)
            req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        let (data, response) = try await session.data(for: req)
        try check(response, data)
        return (data, response as! HTTPURLResponse)
    }

    func redirectTarget(_ path: String) async throws -> URL {
        let req = request(url(path), method: "GET")
        let (data, response) = try await session.data(for: req, delegate: NoRedirect())
        guard let http = response as? HTTPURLResponse else { throw APIError(message: t("Нет ответа", "No response")) }
        if (300..<400).contains(http.statusCode), let loc = http.value(forHTTPHeaderField: "Location"), let u = URL(string: loc) {
            return u
        }
        try check(response, data)
        throw APIError(message: t("Лог недоступен", "Log unavailable"))
    }

    func plain(_ url: URL) async throws -> String {
        let (data, response) = try await URLSession.shared.data(from: url)
        try check(response, data)
        return String(decoding: data, as: UTF8.self)
    }
}

struct EmptyData: Decodable {}
