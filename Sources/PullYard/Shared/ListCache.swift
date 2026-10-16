import CryptoKit
import Foundation

struct ListSnapshot<T: Codable>: Codable {
    let saved: Date
    let value: T
}

enum ListCache {
    static let maxAge: TimeInterval = 7 * 24 * 3600
    static let maxFiles = 60
    static let rewriteAfter: TimeInterval = 3600
    private static let accountKey = "listCacheAccount"
    private static let queue = DispatchQueue(label: "app.pullyard.list-cache", qos: .utility)
    private static var active: String?
    private static var written: [URL: (value: Data, at: Date)] = [:]

    private struct AccountRecord: Codable {
        let fingerprint: String
        let user: RESTUser
    }

    private static var root: URL {
        FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("PullYard/lists", isDirectory: true)
    }

    private static func folderName(_ login: String) -> String? {
        let name = String(login.lowercased().unicodeScalars.filter { CharacterSet.alphanumerics.contains($0) || $0 == "-" || $0 == "_" })
        return name.isEmpty ? nil : name
    }

    private static func folder(_ login: String) -> URL? {
        folderName(login).map { root.appendingPathComponent($0, isDirectory: true) }
    }

    private static func file(_ account: String?, _ key: String) -> URL? {
        guard let account, let dir = folder(account) else { return nil }
        let digest = SHA256.hash(data: Data(key.utf8)).prefix(16).map { String(format: "%02x", $0) }.joined()
        return dir.appendingPathComponent(digest + ".json")
    }

    private static func fingerprint(_ token: String) -> String {
        SHA256.hash(data: Data("pullyard-list-cache|\(token)".utf8)).prefix(8).map { String(format: "%02x", $0) }.joined()
    }

    static func account(for token: String) -> RESTUser? {
        guard let data = UserDefaults.standard.data(forKey: accountKey),
              let record = try? JSONDecoder().decode(AccountRecord.self, from: data),
              record.fingerprint == fingerprint(token) else { return nil }
        return record.user
    }

    static func remember(_ user: RESTUser, token: String) {
        if let data = try? JSONEncoder().encode(AccountRecord(fingerprint: fingerprint(token), user: user)) {
            UserDefaults.standard.set(data, forKey: accountKey)
        }
        activate(user.login)
    }

    static func activate(_ login: String) {
        let name = folderName(login)
        queue.async {
            active = name
            prune(login)
        }
    }

    static func forget(_ login: String?) {
        UserDefaults.standard.removeObject(forKey: accountKey)
        let dir = login.flatMap(folder)
        queue.async {
            active = nil
            written = [:]
            if let dir { try? FileManager.default.removeItem(at: dir) }
        }
    }

    static func load<T: Codable>(_ type: T.Type, account: String?, key: String) -> ListSnapshot<T>? {
        guard let url = file(account, key), let data = try? Data(contentsOf: url),
              let snap = try? JSONDecoder().decode(ListSnapshot<T>.self, from: data),
              Date().timeIntervalSince(snap.saved) < maxAge else { return nil }
        return snap
    }

    static func save<T: Codable>(_ value: T, account: String?, key: String) {
        guard let account, let url = file(account, key) else { return }
        let name = folderName(account)
        let snap = ListSnapshot(saved: Date(), value: value)
        queue.async {
            guard active == name, let body = try? JSONEncoder().encode(snap.value) else { return }
            if let last = written[url], last.value == body, snap.saved.timeIntervalSince(last.at) < rewriteAfter { return }
            guard let data = try? JSONEncoder().encode(snap) else { return }
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            guard (try? data.write(to: url, options: .atomic)) != nil else { return }
            written[url] = (body, snap.saved)
        }
    }

    private static func prune(_ login: String) {
        guard let dir = folder(login),
              let urls = try? FileManager.default.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.contentModificationDateKey]) else { return }
        let dated = urls.map { url in
            (url, (try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast)
        }.sorted { $0.1 > $1.1 }
        let now = Date()
        for (i, entry) in dated.enumerated() where i >= maxFiles || now.timeIntervalSince(entry.1) > maxAge {
            try? FileManager.default.removeItem(at: entry.0)
        }
    }
}
