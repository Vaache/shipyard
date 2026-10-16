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
    private static let checkedFile = "checked.json"
    private static let queue = DispatchQueue(label: "app.pullyard.list-cache", qos: .utility)
    private static var active: String?
    private static var written: [URL: (value: Data, at: Date)] = [:]
    private static var checked: [String: Date] = [:]

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

    private static var record: AccountRecord? {
        UserDefaults.standard.data(forKey: accountKey).flatMap { try? JSONDecoder().decode(AccountRecord.self, from: $0) }
    }

    static var rememberedLogin: String? {
        record?.user.login
    }

    static func account(for token: String) -> RESTUser? {
        guard let record, record.fingerprint == fingerprint(token) else { return nil }
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
        let dir = folder(login)
        queue.async {
            guard active != name else { return }
            active = name
            written = [:]
            checked = dir.flatMap { try? Data(contentsOf: $0.appendingPathComponent(checkedFile)) }
                .flatMap { try? JSONDecoder().decode([String: Date].self, from: $0) } ?? [:]
            prune()
        }
    }

    static func forget(_ login: String?) {
        UserDefaults.standard.removeObject(forKey: accountKey)
        let dir = login.flatMap(folder)
        queue.async {
            active = nil
            written = [:]
            checked = [:]
            if let dir { try? FileManager.default.removeItem(at: dir) }
        }
    }

    static func load<T: Codable>(_ type: T.Type, account: String?, key: String) async -> ListSnapshot<T>? {
        guard let account, let url = file(account, key) else { return nil }
        let name = folderName(account)
        return await withCheckedContinuation { continuation in
            queue.async {
                guard active == name, let data = try? Data(contentsOf: url),
                      let snap = try? JSONDecoder().decode(ListSnapshot<T>.self, from: data) else {
                    continuation.resume(returning: nil)
                    return
                }
                let at = max(snap.saved, checked[url.lastPathComponent] ?? .distantPast)
                continuation.resume(returning: Date().timeIntervalSince(at) < maxAge ? ListSnapshot(saved: at, value: snap.value) : nil)
            }
        }
    }

    static func save<T: Codable>(_ value: T, account: String?, key: String) {
        guard let account, let url = file(account, key) else { return }
        let name = folderName(account)
        let snap = ListSnapshot(saved: Date(), value: value)
        queue.async {
            guard active == name, let body = try? JSONEncoder().encode(snap.value) else { return }
            try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            if let last = written[url], last.value == body, snap.saved.timeIntervalSince(last.at) < rewriteAfter {
                markChecked(url, snap.saved)
                return
            }
            guard let data = try? JSONEncoder().encode(snap), (try? data.write(to: url, options: .atomic)) != nil else { return }
            written[url] = (body, snap.saved)
            markChecked(url, snap.saved)
        }
    }

    static func remove(account: String?, key: String) {
        guard let account, let url = file(account, key) else { return }
        let name = folderName(account)
        queue.async {
            guard active == name else { return }
            written[url] = nil
            try? FileManager.default.removeItem(at: url)
            markChecked(url, nil)
        }
    }

    private static func markChecked(_ url: URL, _ date: Date?) {
        checked[url.lastPathComponent] = date
        guard let data = try? JSONEncoder().encode(checked) else { return }
        try? data.write(to: url.deletingLastPathComponent().appendingPathComponent(checkedFile), options: .atomic)
    }

    private static func modified(_ url: URL) -> Date {
        (try? url.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
    }

    private static func prune() {
        let fm = FileManager.default
        let now = Date()
        let folders = (try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
        for dir in folders where dir.lastPathComponent != active && now.timeIntervalSince(modified(dir)) > maxAge {
            try? fm.removeItem(at: dir)
        }
        guard let name = active else { return }
        let dir = root.appendingPathComponent(name, isDirectory: true)
        let urls = (try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.contentModificationDateKey])) ?? []
        let dated = urls.filter { $0.lastPathComponent != checkedFile }
            .map { ($0, max(modified($0), checked[$0.lastPathComponent] ?? .distantPast)) }
            .sorted { $0.1 > $1.1 }
        var removed = false
        for (i, entry) in dated.enumerated() where i >= maxFiles || now.timeIntervalSince(entry.1) > maxAge {
            try? fm.removeItem(at: entry.0)
            checked[entry.0.lastPathComponent] = nil
            removed = true
        }
        let present = Set(dated.map { $0.0.lastPathComponent })
        if removed || checked.keys.contains(where: { !present.contains($0) }) {
            checked = checked.filter { present.contains($0.key) && fm.fileExists(atPath: dir.appendingPathComponent($0.key).path) }
            if let data = try? JSONEncoder().encode(checked) {
                try? data.write(to: dir.appendingPathComponent(checkedFile), options: .atomic)
            }
        }
    }
}
