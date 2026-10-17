import Foundation

enum GitProvider: String, Codable, CaseIterable, Identifiable {
    case github, githubEnterprise, gitlab, bitbucket, gitea

    var id: String { rawValue }

    var title: String {
        switch self {
        case .github: return "GitHub"
        case .githubEnterprise: return "GitHub Enterprise"
        case .gitlab: return "GitLab"
        case .bitbucket: return "Bitbucket"
        case .gitea: return "Gitea / Forgejo"
        }
    }

    var shortTitle: String {
        switch self {
        case .githubEnterprise: return "GHE"
        case .gitea: return "Gitea"
        default: return title
        }
    }

    var icon: String {
        switch self {
        case .github: return "chevron.left.forwardslash.chevron.right"
        case .githubEnterprise: return "building.2.fill"
        case .gitlab: return "flame.fill"
        case .bitbucket: return "bucket.fill"
        case .gitea: return "cup.and.saucer.fill"
        }
    }

    var defaultHost: String {
        switch self {
        case .github: return "github.com"
        case .githubEnterprise: return ""
        case .gitlab: return "gitlab.com"
        case .bitbucket: return "bitbucket.org"
        case .gitea: return "codeberg.org"
        }
    }

    var hostEditable: Bool {
        switch self {
        case .github, .bitbucket: return false
        default: return true
        }
    }

    var hostPlaceholder: String {
        switch self {
        case .githubEnterprise: return "github.example.com"
        case .gitlab: return "gitlab.com or gitlab.example.com"
        case .gitea: return "codeberg.org or git.example.com"
        default: return defaultHost
        }
    }

    var isGitHub: Bool { self == .github || self == .githubEnterprise }

    var requestsTitle: String { self == .gitlab ? "Merge requests" : "Pull requests" }

    var requestTitle: String { self == .gitlab ? "merge request" : "pull request" }

    var requestShort: String { self == .gitlab ? "MR" : "PR" }

    var repositoriesTitle: String { self == .gitlab ? "Projects" : "Repositories" }

    var numberPrefix: String { self == .gitlab ? "!" : "#" }

    var tokenPlaceholder: String {
        switch self {
        case .github, .githubEnterprise: return "ghp_… or github_pat_…"
        case .gitlab: return "glpat-…"
        case .bitbucket: return "Atlassian API token"
        case .gitea: return "Access token"
        }
    }

    var tokenHint: String {
        switch self {
        case .github, .githubEnterprise: return "Required scopes: repo, read:org, read:project, project, notifications, workflow."
        case .gitlab: return "Required scope: api (or read_api for read-only)."
        case .bitbucket: return "Use your Atlassian account email and an API token with Bitbucket read scopes for account, repositories and pull requests."
        case .gitea: return "Required permissions: read for user, repository and issue."
        }
    }

    func tokenCreateURL(host: String) -> String? {
        switch self {
        case .github: return "https://github.com/settings/tokens/new?scopes=repo,read:org,read:project,project,notifications,workflow&description=PullYard"
        case .githubEnterprise: return host.isEmpty ? nil : "https://\(host)/settings/tokens/new?scopes=repo,read:org,read:project,project,notifications,workflow&description=PullYard"
        case .gitlab: return host.isEmpty ? nil : "https://\(host)/-/user_settings/personal_access_tokens?name=PullYard&scopes=api"
        case .bitbucket: return "https://id.atlassian.com/manage-profile/security/api-tokens"
        case .gitea: return host.isEmpty ? nil : "https://\(host)/user/settings/applications"
        }
    }

    static func normalizeHost(_ raw: String) -> String {
        var s = raw.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        for prefix in ["https://", "http://"] where s.hasPrefix(prefix) { s.removeFirst(prefix.count) }
        while s.hasSuffix("/") { s.removeLast() }
        return s
    }
}

struct GitAccount: Codable, Identifiable, Hashable {
    var id: String
    var provider: GitProvider
    var host: String
    var login: String
    var name: String?
    var avatarUrl: String?
    var email: String?
    var userID: String?
    var keychainKey: String

    var user: RESTUser { RESTUser(login: login, name: name, avatarUrl: avatarUrl) }

    var cacheKey: String {
        provider == .github ? login : "\(login)-\(provider.rawValue)-\(host)"
    }

    var subtitle: String {
        provider == .github ? "@\(login)" : "@\(login) · \(host)"
    }

    var identity: String { "\(provider.rawValue)|\(host)|\(login.lowercased())" }

    func same(as other: GitAccount) -> Bool {
        identity == other.identity
    }
}

struct GitCredential: Codable {
    var token: String
    var refreshToken: String?
    var expiresAt: Date?
    var clientID: String?

    static func decode(_ raw: String) -> GitCredential {
        if raw.hasPrefix("{"), let data = raw.data(using: .utf8), let c = try? JSONDecoder().decode(GitCredential.self, from: data) {
            return c
        }
        return GitCredential(token: raw)
    }

    var encoded: String {
        guard refreshToken != nil || expiresAt != nil || clientID != nil,
              let data = try? JSONEncoder().encode(self), let s = String(data: data, encoding: .utf8) else { return token }
        return s
    }
}

@Observable
@MainActor
final class GitAccounts {
    static let shared = GitAccounts()
    private static let listKey = "git.accounts"
    private static let activeKey = "git.activeAccount"

    private(set) var accounts: [GitAccount]
    private(set) var activeID: String?

    private init() {
        accounts = UserDefaults.standard.data(forKey: Self.listKey).flatMap { try? JSONDecoder().decode([GitAccount].self, from: $0) } ?? []
        activeID = UserDefaults.standard.string(forKey: Self.activeKey)
        if accounts.isEmpty, Keychain.load() != nil {
            let login = ListCache.rememberedLogin ?? ""
            accounts = [GitAccount(id: UUID().uuidString.lowercased(), provider: .github, host: GitProvider.github.defaultHost, login: login, keychainKey: Keychain.defaultAccount)]
            activeID = accounts[0].id
            save()
        }
        if activeID.map({ id in !accounts.contains { $0.id == id } }) ?? true { activeID = accounts.first?.id }
    }

    var active: GitAccount? { accounts.first { $0.id == activeID } }

    func credential(for account: GitAccount) -> GitCredential? {
        Keychain.load(account: account.keychainKey).map(GitCredential.decode)
    }

    func store(_ credential: GitCredential, for account: GitAccount) throws {
        try Keychain.save(credential.encoded, account: account.keychainKey)
    }

    @discardableResult
    func upsert(_ candidate: GitAccount, credential: GitCredential) throws -> GitAccount {
        var account = candidate
        if let existing = accounts.first(where: { $0.same(as: candidate) }) {
            account.id = existing.id
            account.keychainKey = existing.keychainKey
        } else if account.provider == .github, let legacy = accounts.firstIndex(where: { $0.provider == .github && $0.login.isEmpty }) {
            account.id = accounts[legacy].id
            account.keychainKey = accounts[legacy].keychainKey
        }
        try store(credential, for: account)
        if let i = accounts.firstIndex(where: { $0.id == account.id }) {
            accounts[i] = account
        } else {
            accounts.append(account)
        }
        save()
        return account
    }

    func update(_ account: GitAccount) {
        guard let i = accounts.firstIndex(where: { $0.id == account.id }), accounts[i] != account else { return }
        accounts[i] = account
        save()
    }

    func setActive(_ id: String?) {
        activeID = id
        save()
    }

    func hasCredential(_ account: GitAccount) -> Bool {
        Keychain.load(account: account.keychainKey) != nil
    }

    func forgetCredential(_ id: String) {
        guard let account = accounts.first(where: { $0.id == id }) else { return }
        Keychain.delete(account: account.keychainKey)
    }

    func remove(_ id: String) {
        guard let account = accounts.first(where: { $0.id == id }) else { return }
        Keychain.delete(account: account.keychainKey)
        accounts.removeAll { $0.id == id }
        if activeID == id { activeID = nil }
        save()
    }

    private func save() {
        if let data = try? JSONEncoder().encode(accounts) { UserDefaults.standard.set(data, forKey: Self.listKey) }
        UserDefaults.standard.set(activeID, forKey: Self.activeKey)
    }
}
