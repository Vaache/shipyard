import Foundation

enum GHCLI {
    static var binary: String? {
        ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"].first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    static func token() async throws -> String {
        guard let bin = binary else { throw APIError(message: t("GitHub CLI (gh) не найден", "GitHub CLI (gh) not found")) }
        return try await Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: bin)
            p.arguments = ["auth", "token", "--hostname", "github.com"]
            let out = Pipe(), err = Pipe()
            p.standardOutput = out
            p.standardError = err
            try p.run()
            p.waitUntilExit()
            let token = String(decoding: out.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            guard p.terminationStatus == 0, !token.isEmpty else {
                let e = String(decoding: err.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
                throw APIError(message: e.isEmpty ? t("gh не вернул токен. Выполните gh auth login", "gh returned no token. Run gh auth login") : e)
            }
            return token
        }.value
    }
}

struct DeviceCode: Decodable {
    let deviceCode: String
    let userCode: String
    let verificationUri: String
    let expiresIn: Int
    let interval: Int
}

enum DeviceFlow {
    static let scopes = "repo read:org read:project project notifications workflow"

    private static func post(_ url: String, _ params: [String: String]) async throws -> [String: Any] {
        var req = URLRequest(url: URL(string: url)!)
        req.httpMethod = "POST"
        req.setValue("application/json", forHTTPHeaderField: "Accept")
        req.setValue("application/x-www-form-urlencoded", forHTTPHeaderField: "Content-Type")
        var c = URLComponents()
        c.queryItems = params.map { URLQueryItem(name: $0.key, value: $0.value) }
        req.httpBody = c.percentEncodedQuery?.data(using: .utf8)
        let (data, _) = try await URLSession.shared.data(for: req)
        guard let obj = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            throw APIError(message: t("Неверный ответ GitHub", "Invalid response from GitHub"))
        }
        return obj
    }

    static func start(clientID: String) async throws -> DeviceCode {
        let obj = try await post("https://github.com/login/device/code", ["client_id": clientID, "scope": scopes])
        if let e = obj["error_description"] as? String ?? obj["error"] as? String { throw APIError(message: e) }
        guard let dc = obj["device_code"] as? String, let uc = obj["user_code"] as? String, let uri = obj["verification_uri"] as? String else {
            throw APIError(message: t("Device flow недоступен для этого Client ID", "Device flow is not available for this Client ID"))
        }
        return DeviceCode(deviceCode: dc, userCode: uc, verificationUri: uri, expiresIn: obj["expires_in"] as? Int ?? 900, interval: obj["interval"] as? Int ?? 5)
    }

    static func poll(clientID: String, code: DeviceCode) async throws -> String {
        var interval = max(code.interval, 5)
        let deadline = Date().addingTimeInterval(TimeInterval(code.expiresIn))
        while Date() < deadline {
            try await Task.sleep(nanoseconds: UInt64(interval) * 1_000_000_000)
            let obj = try await post("https://github.com/login/oauth/access_token", [
                "client_id": clientID,
                "device_code": code.deviceCode,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
            ])
            if let token = obj["access_token"] as? String { return token }
            switch obj["error"] as? String {
            case "authorization_pending": continue
            case "slow_down": interval += 5
            case let e?: throw APIError(message: obj["error_description"] as? String ?? e)
            default: throw APIError(message: t("Неизвестный ответ GitHub", "Unexpected response from GitHub"))
            }
        }
        throw APIError(message: t("Время ожидания истекло", "Timed out"))
    }
}
