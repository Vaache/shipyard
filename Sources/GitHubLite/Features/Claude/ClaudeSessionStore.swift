import Foundation

struct ClaudeSessionInfo: Identifiable, Hashable {
    let id: String
    let file: URL
    let cwd: String
    let title: String
    let branch: String?
    let modified: Date
    let prURL: String?
    let prNumber: Int?
    let model: String?
    let linesAdded: Int?
    let linesRemoved: Int?

    var worktreeName: String? {
        guard let r = cwd.range(of: "/.claude/worktrees/") else { return nil }
        return String(cwd[r.upperBound...]).components(separatedBy: "/").first
    }
}

struct ClaudeProject: Identifiable, Hashable {
    let id: String
    let path: String
    var sessions: [ClaudeSessionInfo]

    var name: String { (path as NSString).lastPathComponent }
    var exists: Bool { FileManager.default.fileExists(atPath: path) }
    var lastActivity: Date { sessions.first?.modified ?? .distantPast }
}

enum ClaudeSessionParser {
    private static var cache: [String: (Date, ClaudeSessionInfo?)] = [:]
    private static let lock = NSLock()

    static var root: URL {
        if let dir = ProcessInfo.processInfo.environment["CLAUDE_CONFIG_DIR"], !dir.isEmpty {
            return URL(fileURLWithPath: dir).appendingPathComponent("projects", isDirectory: true)
        }
        return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude/projects", isDirectory: true)
    }

    static func projectRoot(for cwd: String) -> String {
        if let r = cwd.range(of: "/.claude/worktrees/") { return String(cwd[..<r.lowerBound]) }
        return cwd
    }

    static func loadAll() -> [ClaudeProject] {
        let fm = FileManager.default
        guard let dirs = try? fm.contentsOfDirectory(at: root, includingPropertiesForKeys: nil) else { return [] }
        var byProject: [String: [ClaudeSessionInfo]] = [:]
        for dir in dirs {
            guard let files = try? fm.contentsOfDirectory(at: dir, includingPropertiesForKeys: [.contentModificationDateKey]) else { continue }
            for file in files where file.pathExtension == "jsonl" {
                guard let info = parse(file) else { continue }
                byProject[projectRoot(for: info.cwd), default: []].append(info)
            }
        }
        return byProject.map { path, sessions in
            ClaudeProject(id: path, path: path, sessions: sessions.sorted { $0.modified > $1.modified })
        }
        .sorted { $0.lastActivity > $1.lastActivity }
    }

    static func parse(_ file: URL) -> ClaudeSessionInfo? {
        let mtime = (try? file.resourceValues(forKeys: [.contentModificationDateKey]).contentModificationDate) ?? .distantPast
        lock.lock()
        if let hit = cache[file.path], hit.0 == mtime {
            lock.unlock()
            return hit.1
        }
        lock.unlock()
        let info = parseUncached(file, mtime: mtime)
        lock.lock()
        cache[file.path] = (mtime, info)
        lock.unlock()
        return info
    }

    private static func lines(_ data: Data, dropFirst: Bool, dropLast: Bool) -> [[String: Any]] {
        var parts = data.split(separator: UInt8(ascii: "\n"), omittingEmptySubsequences: true)
        if dropFirst, !parts.isEmpty { parts.removeFirst() }
        if dropLast, !parts.isEmpty { parts.removeLast() }
        return parts.compactMap { try? JSONSerialization.jsonObject(with: Data($0)) as? [String: Any] }
    }

    private static func text(of message: Any?) -> String? {
        guard let m = message as? [String: Any] else { return nil }
        if let s = m["content"] as? String { return s }
        if let blocks = m["content"] as? [[String: Any]] {
            return blocks.compactMap { $0["type"] as? String == "text" ? $0["text"] as? String : nil }.first
        }
        return nil
    }

    private static func cleanPrompt(_ raw: String) -> String? {
        var s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        if s.hasPrefix("<command-name>") || s.hasPrefix("<local-command") || s.hasPrefix("Caveat:") || s.hasPrefix("[Request interrupted") { return nil }
        if s.hasPrefix("<") {
            s = s.replacingOccurrences(of: "<[^>]+>", with: " ", options: .regularExpression).trimmingCharacters(in: .whitespacesAndNewlines)
        }
        s = s.replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
        return s.isEmpty ? nil : String(s.prefix(160))
    }

    private static func parseUncached(_ file: URL, mtime: Date) -> ClaudeSessionInfo? {
        guard let handle = try? FileHandle(forReadingFrom: file) else { return nil }
        defer { try? handle.close() }
        let size = (try? handle.seekToEnd()) ?? 0
        try? handle.seek(toOffset: 0)
        let headSize = min(size, 96 * 1024)
        let head = (try? handle.read(upToCount: Int(headSize))) ?? Data()
        let tailSize = min(size, 160 * 1024)
        var tail = Data()
        if size > headSize {
            try? handle.seek(toOffset: size - tailSize)
            tail = (try? handle.read(upToCount: Int(tailSize))) ?? Data()
        }

        var cwd: String?
        var branch: String?
        var firstPrompt: String?
        for obj in lines(head, dropFirst: false, dropLast: size > headSize) {
            if cwd == nil, let c = obj["cwd"] as? String { cwd = c }
            if branch == nil, let b = obj["gitBranch"] as? String, !b.isEmpty, b != "HEAD" { branch = b }
            if firstPrompt == nil, obj["type"] as? String == "user", obj["isMeta"] as? Bool != true, obj["isSidechain"] as? Bool != true,
               let t = text(of: obj["message"]), let clean = cleanPrompt(t) {
                firstPrompt = clean
            }
            if cwd != nil, branch != nil, firstPrompt != nil { break }
        }
        guard let cwd, firstPrompt != nil || size > 2048 else { return nil }

        var title: String?
        var lastPrompt: String?
        var prURL: String?
        var prNumber: Int?
        var model: String?
        var added: Int?
        var removed: Int?
        let tailObjects = tail.isEmpty ? lines(head, dropFirst: false, dropLast: false) : lines(tail, dropFirst: true, dropLast: false)
        for obj in tailObjects {
            switch obj["type"] as? String {
            case "custom-title": title = obj["customTitle"] as? String ?? title
            case "last-prompt": lastPrompt = obj["lastPrompt"] as? String ?? lastPrompt
            case "pr-link":
                prURL = obj["prUrl"] as? String ?? prURL
                prNumber = obj["prNumber"] as? Int ?? prNumber
            case "assistant":
                if let m = (obj["message"] as? [String: Any])?["model"] as? String, !m.hasPrefix("<") { model = m }
                if let b = obj["gitBranch"] as? String, !b.isEmpty, b != "HEAD" { branch = b }
            case "cost-state":
                added = obj["totalLinesAdded"] as? Int ?? added
                removed = obj["totalLinesRemoved"] as? Int ?? removed
            default:
                break
            }
        }
        let display = title?.isEmpty == false ? title! : (firstPrompt ?? lastPrompt.flatMap(cleanPrompt) ?? t("Без названия", "Untitled"))
        return ClaudeSessionInfo(
            id: file.deletingPathExtension().lastPathComponent,
            file: file,
            cwd: cwd,
            title: display,
            branch: branch,
            modified: mtime,
            prURL: prURL,
            prNumber: prNumber,
            model: model,
            linesAdded: added,
            linesRemoved: removed
        )
    }
}
