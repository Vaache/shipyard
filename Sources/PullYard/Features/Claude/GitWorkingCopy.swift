import CoreServices
import Foundation

struct GitCommitLine: Identifiable, Hashable {
    let id: String
    let short: String
    let subject: String
}

enum Git {
    static func run(_ args: [String], in dir: String, env extra: [String: String] = [:], timeout: TimeInterval? = nil) -> (out: String, ok: Bool, status: Int32) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        var safeArgs = args
        if let sub = args.first, sub == "diff" || sub == "show" {
            safeArgs.insert(contentsOf: ["--no-ext-diff", "--no-textconv"], at: 1)
        }
        p.arguments = ["-C", dir, "-c", "core.quotepath=off", "-c", "core.fsmonitor=false", "--no-pager"] + safeArgs
        var env = ProcessInfo.processInfo.environment.filter { !$0.key.hasPrefix("GIT_") }
        env["GIT_OPTIONAL_LOCKS"] = "0"
        env["GIT_TERMINAL_PROMPT"] = "0"
        env["GIT_PAGER"] = "cat"
        env["LC_ALL"] = "C"
        env.merge(extra) { _, new in new }
        p.environment = env
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        do { try p.run() } catch { return ("", false, -1) }
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + (timeout ?? 30)) { [weak p] in
            if p?.isRunning == true { p?.terminate() }
        }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return (String(decoding: data, as: UTF8.self), p.terminationStatus == 0, p.terminationStatus)
    }

    static func remoteRepo(_ dir: String) -> RepoRef? {
        let url = run(["remote", "get-url", "origin"], in: dir).out.trimmingCharacters(in: .whitespacesAndNewlines)
        guard url.contains("github.com") else { return nil }
        var tail = url.components(separatedBy: "github.com").last ?? ""
        tail = tail.trimmingCharacters(in: CharacterSet(charactersIn: ":/"))
        if tail.hasSuffix(".git") { tail.removeLast(4) }
        return RepoRef(full: tail)
    }

    static func parseUnifiedDiff(_ text: String) -> [PRFile] {
        var files: [PRFile] = []
        var path: String?
        var oldPath: String?
        var status = "modified"
        var patch: [Substring] = []
        var adds = 0, dels = 0
        var binary = false

        func flush() {
            guard let p = path else { return }
            let body = patch.joined(separator: "\n")
            files.append(PRFile(filename: p, status: status, additions: adds, deletions: dels, patch: binary || body.isEmpty ? nil : body, previousFilename: oldPath != p ? oldPath : nil))
        }

        for line in text.split(separator: "\n", omittingEmptySubsequences: false) {
            if line.hasPrefix("diff --git ") {
                flush()
                path = nil; oldPath = nil; status = "modified"; patch = []; adds = 0; dels = 0; binary = false
                let parts = line.dropFirst("diff --git ".count).components(separatedBy: " b/")
                if parts.count >= 2 {
                    oldPath = String(parts[0].dropFirst(2))
                    path = parts.dropFirst().joined(separator: " b/")
                }
                continue
            }
            if patch.isEmpty {
                if line.hasPrefix("new file") { status = "added"; continue }
                if line.hasPrefix("deleted file") { status = "removed"; continue }
                if line.hasPrefix("rename from ") { status = "renamed"; oldPath = String(line.dropFirst("rename from ".count)); continue }
                if line.hasPrefix("rename to ") { path = String(line.dropFirst("rename to ".count)); continue }
                if line.hasPrefix("Binary files") { binary = true; continue }
                if line.hasPrefix("index ") || line.hasPrefix("--- ") || line.hasPrefix("+++ ") || line.hasPrefix("similarity") || line.hasPrefix("old mode") || line.hasPrefix("new mode") { continue }
            }
            guard path != nil else { continue }
            if line.hasPrefix("@@") || !patch.isEmpty {
                patch.append(line)
                if line.hasPrefix("+") { adds += 1 } else if line.hasPrefix("-") { dels += 1 }
            }
        }
        flush()
        return files
    }
}

@Observable
@MainActor
final class GitWorkingCopy {
    let path: String
    var isRepo = false
    var branch: String?
    var repo: RepoRef?
    var changes: [DiffFile] = []
    var unpushed: [GitCommitLine] = []
    var ahead = 0
    var loading = false
    var lastRefresh: Date?

    @ObservationIgnored private var stream: FSEventStreamRef?
    @ObservationIgnored private var pending: DispatchWorkItem?

    init(path: String) {
        self.path = path
    }

    deinit {
        if let stream {
            FSEventStreamStop(stream)
            FSEventStreamInvalidate(stream)
            FSEventStreamRelease(stream)
        }
    }

    func refresh() async {
        loading = true
        defer { loading = false }
        let dir = path
        let result = await Task.detached(priority: .utility) { () -> (Bool, String?, RepoRef?, [DiffFile], [GitCommitLine]) in
            guard Git.run(["rev-parse", "--is-inside-work-tree"], in: dir).ok else { return (false, nil, nil, [], []) }
            let branch = Git.run(["rev-parse", "--abbrev-ref", "HEAD"], in: dir).out.trimmingCharacters(in: .whitespacesAndNewlines)
            let repo = Git.remoteRepo(dir)
            var files = Git.parseUnifiedDiff(Git.run(["diff", "HEAD", "--no-color", "-M"], in: dir).out)
            if files.isEmpty {
                files = Git.parseUnifiedDiff(Git.run(["diff", "--no-color", "-M"], in: dir).out)
            }
            let untracked = Git.run(["ls-files", "--others", "--exclude-standard"], in: dir).out.split(separator: "\n").prefix(200)
            for name in untracked {
                let full = dir + "/" + name
                let attrs = try? FileManager.default.attributesOfItem(atPath: full)
                let size = (attrs?[.size] as? Int) ?? 0
                var patch: String?
                var adds = 0
                if size < 300_000, let text = try? String(contentsOfFile: full, encoding: .utf8) {
                    let lines = text.split(separator: "\n", omittingEmptySubsequences: false)
                    adds = lines.count
                    patch = "@@ -0,0 +1,\(lines.count) @@\n" + lines.map { "+" + $0 }.joined(separator: "\n")
                }
                files.append(PRFile(filename: String(name), status: "added", additions: adds, deletions: 0, patch: patch, previousFilename: nil))
            }
            var commits: [GitCommitLine] = []
            let log = Git.run(["log", "--format=%H%x09%h%x09%s", "-n", "30", "@{upstream}..HEAD"], in: dir)
            if log.ok {
                commits = log.out.split(separator: "\n").compactMap { line in
                    let parts = line.split(separator: "\t", maxSplits: 2).map(String.init)
                    return parts.count == 3 ? GitCommitLine(id: parts[0], short: parts[1], subject: parts[2]) : nil
                }
            }
            return (true, branch.isEmpty ? nil : branch, repo, files.map(DiffFile.init), commits)
        }.value
        isRepo = result.0
        branch = result.1
        repo = result.2
        changes = result.3
        unpushed = result.4
        ahead = result.4.count
        lastRefresh = Date()
    }

    func startWatching() {
        guard stream == nil else { return }
        let callback: FSEventStreamCallback = { _, info, count, paths, _, _ in
            guard let info else { return }
            let me = Unmanaged<GitWorkingCopy>.fromOpaque(info).takeUnretainedValue()
            let list = Unmanaged<CFArray>.fromOpaque(paths).takeUnretainedValue() as? [String] ?? []
            let relevant = list.contains { p in
                !(p.contains("/.git/objects") || p.contains("/.git/logs") || p.contains("/node_modules/") || p.contains("/.dart_tool/") || p.contains("/build/"))
            }
            guard relevant, count > 0 else { return }
            DispatchQueue.main.async { me.scheduleRefresh() }
        }
        var context = FSEventStreamContext(version: 0, info: Unmanaged.passUnretained(self).toOpaque(), retain: nil, release: nil, copyDescription: nil)
        let flags = UInt32(kFSEventStreamCreateFlagUseCFTypes | kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagNoDefer)
        guard let s = FSEventStreamCreate(nil, callback, &context, [path] as CFArray, FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 0.6, flags) else { return }
        FSEventStreamSetDispatchQueue(s, DispatchQueue.global(qos: .utility))
        FSEventStreamStart(s)
        stream = s
    }

    private func scheduleRefresh() {
        pending?.cancel()
        let item = DispatchWorkItem { [weak self] in
            Task { await self?.refresh() }
        }
        pending = item
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.8, execute: item)
    }
}
