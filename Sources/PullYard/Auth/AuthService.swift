import Foundation

enum CLITool {
    static func find(_ name: String) -> String? {
        ["/opt/homebrew/bin/\(name)", "/usr/local/bin/\(name)", "/usr/bin/\(name)"].first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    static func output(_ bin: String, _ arguments: [String], emptyMessage: String) async throws -> String {
        try await Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: bin)
            p.arguments = arguments
            let out = Pipe(), err = Pipe()
            p.standardOutput = out
            p.standardError = err
            try p.run()
            DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 30) { [weak p] in
                if p?.isRunning == true { p?.terminate() }
            }
            p.waitUntilExit()
            let token = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            guard p.terminationStatus == 0, !token.isEmpty else {
                let e = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                throw APIError(message: e.isEmpty ? emptyMessage : e)
            }
            return token
        }.value
    }
}

enum GHCLI {
    static var binary: String? { CLITool.find("gh") }

    static func token(host: String = "github.com") async throws -> String {
        guard let bin = binary else { throw APIError(message: "GitHub CLI (gh) not found") }
        return try await CLITool.output(bin, ["auth", "token", "--hostname", host], emptyMessage: "gh returned no token. Run gh auth login --hostname \(host)")
    }
}

enum GLabCLI {
    static var binary: String? { CLITool.find("glab") }

    static func token(host: String = "gitlab.com") async throws -> String {
        guard let bin = binary else { throw APIError(message: "GitLab CLI (glab) not found") }
        return try await CLITool.output(bin, ["config", "get", "token", "--host", host], emptyMessage: "glab returned no token. Run glab auth login --hostname \(host)")
    }
}

struct DeviceCode: Decodable {
    let deviceCode: String
    let userCode: String
    let verificationUri: String
    let expiresIn: Int
    let interval: Int
}

struct DeviceToken {
    let accessToken: String
    let refreshToken: String?
    let expiresIn: Int?

    var credential: GitCredential {
        GitCredential(token: accessToken, refreshToken: refreshToken, expiresAt: expiresIn.map { Date().addingTimeInterval(TimeInterval($0)) })
    }
}

enum DeviceFlow {
    struct Endpoint {
        let codeURL: String
        let tokenURL: String
        let scopes: String
        let service: String
    }

    static let scopes = "repo read:org read:project project notifications workflow"

    static func github(host: String = "github.com") -> Endpoint {
        Endpoint(codeURL: "https://\(host)/login/device/code", tokenURL: "https://\(host)/login/oauth/access_token", scopes: scopes, service: host == "github.com" ? "GitHub" : host)
    }

    static func gitlab(host: String) -> Endpoint {
        Endpoint(codeURL: "https://\(host)/oauth/authorize_device", tokenURL: "https://\(host)/oauth/token", scopes: "api", service: host)
    }

    private static func post(_ url: String, _ params: [String: String], service: String) async throws -> [String: Any] {
        guard let target = URL(string: url) else { throw APIError(message: "Invalid address: \(url)") }
        var req = URLRequest(url: target)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var c = URLComponents()
        c.queryItems = params.map { URLQueryItem(name: $0.key, value: $0.value) }
        req.httpBody = c.percentEncodedQuery?.data(using: .utf8)
        let (data, _) = try await URLSession.shared.data(for: req)
        guard let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw APIError(message: "Invalid response from \(service)")
        }
        return obj
    }

    static func start(clientID: String, endpoint: Endpoint = github()) async throws -> DeviceCode {
        let obj = try await post(endpoint.codeURL, ["client_id": clientID, "scope": endpoint.scopes], service: endpoint.service)
        if let e = obj["error_description"] as? String ?? obj["error"] as? String { throw APIError(message: e) }
        guard let dc = obj["device_code"] as? String, let uc = obj["user_code"] as? String, let uri = obj["verification_uri"] as? String else {
            throw APIError(message: "Device flow is not available for this Client ID")
        }
        return DeviceCode(deviceCode: dc, userCode: uc, verificationUri: uri, expiresIn: obj["expires_in"] as? Int ?? 900, interval: obj["interval"] as? Int ?? 5)
    }

    static func poll(clientID: String, code: DeviceCode, endpoint: Endpoint = github()) async throws -> DeviceToken {
        var interval = max(code.interval, 5)
        let deadline = Date().addingTimeInterval(TimeInterval(code.expiresIn))
        while Date() < deadline {
            try await Task.sleep(nanoseconds: UInt64(interval) * 1_000_000_000)
            let obj = try await post(endpoint.tokenURL, [
                "client_id": clientID,
                "device_code": code.deviceCode,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
            ], service: endpoint.service)
            if let token = obj["access_token"] as? String {
                return DeviceToken(accessToken: token, refreshToken: obj["refresh_token"] as? String, expiresIn: obj["expires_in"] as? Int)
            }
            switch obj["error"] as? String {
            case "authorization_pending": continue
            case "slow_down": interval += 5
            case let e?: throw APIError(message: obj["error_description"] as? String ?? e)
            default: throw APIError(message: "Unexpected response from \(endpoint.service)")
            }
        }
        throw APIError(message: "Timed out")
    }

    static func refresh(clientID: String, refreshToken: String, endpoint: Endpoint) async throws -> DeviceToken {
        let obj = try await post(endpoint.tokenURL, [
            "client_id": clientID,
            "refresh_token": refreshToken,
            "grant_type": "refresh_token"
        ], service: endpoint.service)
        guard let token = obj["access_token"] as? String else {
            throw APIError(message: obj["error_description"] as? String ?? "Sign-in expired. Sign in again", status: 401)
        }
        return DeviceToken(accessToken: token, refreshToken: obj["refresh_token"] as? String ?? refreshToken, expiresIn: obj["expires_in"] as? Int)
    }
}
