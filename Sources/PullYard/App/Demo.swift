import Foundation

enum Demo {
    private static let env = ProcessInfo.processInfo.environment

    static let repo: String? = env["PULLYARD_DEMO_REPO"].flatMap { $0.isEmpty ? nil : $0 }
    static let folder: String? = env["PULLYARD_DEMO_FOLDER"].flatMap { $0.isEmpty ? nil : $0 }
    static var isOn: Bool { repo != nil || folder != nil }

    static let user = RESTUser(login: "arivera", name: "Alex Rivera", avatarUrl: nil)
    static let email = "alex@example.com"

    static func query(_ base: String) -> String {
        guard let repo else { return base }
        return "repo:\(repo)"
    }
}
