import AppKit
import Foundation

enum LocalRepoStore {
    private static let key = "localRepoPaths"

    static func saved(_ repo: RepoRef) -> String? {
        let map = UserDefaults.standard.dictionary(forKey: key) as? [String: String] ?? [:]
        guard let path = map[repo.full.lowercased()], FileManager.default.fileExists(atPath: path + "/.git") else { return nil }
        return path
    }

    static func save(_ repo: RepoRef, path: String) {
        var map = UserDefaults.standard.dictionary(forKey: key) as? [String: String] ?? [:]
        map[repo.full.lowercased()] = path
        UserDefaults.standard.set(map, forKey: key)
    }

    static func remoteMatches(_ path: String, repo: RepoRef) -> Bool {
        guard let config = try? String(contentsOfFile: path + "/.git/config", encoding: .utf8) else { return false }
        let target = repo.full.lowercased()
        for line in config.lowercased().split(separator: "\n") where line.contains("url") && line.contains("github.com") {
            var s = String(line.split(separator: "=", maxSplits: 1).last ?? "").trimmingCharacters(in: .whitespaces)
            if s.hasSuffix(".git") { s.removeLast(4) }
            if s.hasSuffix("/" + target) || s.hasSuffix(":" + target) { return true }
        }
        return false
    }

    static func detect(_ repo: RepoRef) async -> String? {
        if let s = saved(repo) { return s }
        let found = await Task.detached(priority: .userInitiated) { () -> String? in
            let home = FileManager.default.homeDirectoryForCurrentUser.path
            let roots = ["Documents", "Developer", "Projects", "projects", "Code", "code", "src", "dev", "work", "GitHub", "git", "repos", "Desktop", ""].map { $0.isEmpty ? home : home + "/" + $0 }
            var seen = Set<String>()
            for root in roots {
                if let hit = scan(root, repo: repo, depth: root == home ? 1 : 3, seen: &seen) { return hit }
            }
            return nil
        }.value
        if let found { save(repo, path: found) }
        return found
    }

    private static func scan(_ dir: String, repo: RepoRef, depth: Int, seen: inout Set<String>) -> String? {
        guard depth >= 0, seen.insert(dir).inserted else { return nil }
        let fm = FileManager.default
        if fm.fileExists(atPath: dir + "/.git"), remoteMatches(dir, repo: repo) { return dir }
        guard depth > 0, let items = try? fm.contentsOfDirectory(atPath: dir) else { return nil }
        let skip: Set<String> = ["node_modules", "Library", "Pods", "build", "DerivedData", "Applications", "Music", "Movies", "Pictures"]
        let preferred = items.sorted { a, b in
            let ma = a.lowercased() == repo.name.lowercased(), mb = b.lowercased() == repo.name.lowercased()
            return ma && !mb
        }
        for item in preferred where !item.hasPrefix(".") && !skip.contains(item) {
            var isDir: ObjCBool = false
            let path = dir + "/" + item
            guard fm.fileExists(atPath: path, isDirectory: &isDir), isDir.boolValue else { continue }
            if let hit = scan(path, repo: repo, depth: depth - 1, seen: &seen) { return hit }
        }
        return nil
    }

    @MainActor
    static func choose(_ repo: RepoRef) -> String? {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose the local folder of \(repo.full)"
        panel.prompt = "Choose"
        guard panel.runModal() == .OK, let path = panel.url?.path else { return nil }
        save(repo, path: path)
        return path
    }
}
