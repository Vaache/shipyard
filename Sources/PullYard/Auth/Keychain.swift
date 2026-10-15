import Foundation
import Security

enum Keychain {
    private static let service = "dev.vaache.shipyard"
    private static let legacyService = "dev.vaache.githublite"
    private static let legacyCheckedKey = "legacyKeychainChecked"
    private static let account = "github-token"

    private static func base(_ service: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    static func save(_ token: String) throws {
        let data = Data(token.utf8)
        let update = [kSecValueData as String: data, kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlock] as [String: Any]
        var status = SecItemUpdate(base(service) as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var query = base(service)
            query[kSecValueData as String] = data
            query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
            status = SecItemAdd(query as CFDictionary, nil)
        }
        guard status == errSecSuccess else {
            let reason = SecCopyErrorMessageString(status, nil) as String? ?? "OSStatus \(status)"
            throw APIError(message: "Could not save token to Keychain: \(reason)")
        }
    }

    static func load() -> String? {
        if let token = read(service) { return token }
        guard !UserDefaults.standard.bool(forKey: legacyCheckedKey) else { return nil }
        UserDefaults.standard.set(true, forKey: legacyCheckedKey)
        guard let token = read(legacyService) else { return nil }
        try? save(token)
        return token
    }

    static func delete() {
        SecItemDelete(base(service) as CFDictionary)
    }

    private static func read(_ service: String) -> String? {
        var query = base(service)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        guard SecItemCopyMatching(query as CFDictionary, &result) == errSecSuccess, let data = result as? Data else { return nil }
        return String(data: data, encoding: .utf8)
    }
}
