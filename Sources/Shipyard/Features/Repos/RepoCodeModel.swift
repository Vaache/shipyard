import AppKit
import SwiftUI

enum FileContent {
    case text(String, truncated: Bool)
    case markdown(html: String, source: String)
    case image(NSImage)
    case binary(Int)
}

@Observable
@MainActor
final class RepoCodeModel {
    let repo: RepoRef
    let api: GitHubAPI

    var branches: [BranchInfo] = []
    var ref = ""
    var dir = ""
    var entries: [TreeEntry] = []
    var loadingDir = false
    var dirError: String?
    var readme: String?
    var selectedFile: String?
    var file: FileContent?
    var fileSize: Int?
    var loadingFile = false
    var fileError: String?
    var allPaths: [String]?
    var pathsTruncated = false
    var loadingPaths = false
    @ObservationIgnored private var dirCache: [String: (entries: [TreeEntry], readme: String?)] = [:]

    private static let imageExts: Set<String> = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "icns", "tiff", "heic", "svg"]
    private static let markdownExts: Set<String> = ["md", "markdown", "mdx"]

    init(repo: RepoRef, api: GitHubAPI) {
        self.repo = repo
        self.api = api
    }

    var crumbs: [(name: String, path: String)] {
        var result: [(String, String)] = [(repo.name, "")]
        var acc = ""
        for part in dir.split(separator: "/") {
            acc = acc.isEmpty ? String(part) : acc + "/" + part
            result.append((String(part), acc))
        }
        return result
    }

    func start(defaultBranch: String?) async {
        guard ref.isEmpty else { return }
        ref = defaultBranch ?? "HEAD"
        async let b: Void = loadBranches()
        await openDir("")
        await b
    }

    private func loadBranches() async {
        if let list = try? await api.branches(repo) {
            branches = list.sorted { a, b in
                if a.name == ref { return true }
                if b.name == ref { return false }
                return a.name.localizedStandardCompare(b.name) == .orderedAscending
            }
        }
    }

    func switchRef(_ name: String) async {
        guard name != ref else { return }
        ref = name
        allPaths = nil
        selectedFile = nil
        file = nil
        let keepDir = dir
        await openDir(keepDir)
        if dirError != nil, !keepDir.isEmpty { await openDir("") }
    }

    func openDir(_ path: String) async {
        let key = "\(ref)|\(path)"
        let switching = path != dir
        dir = path
        if let cached = dirCache[key] {
            entries = cached.entries
            readme = cached.readme
            dirError = nil
        } else if switching {
            entries = []
            readme = nil
            dirError = nil
        }
        loadingDir = true
        defer { if path == dir { loadingDir = false } }
        do {
            let obj = try await api.gitObject(repo, ref: ref, path: path)
            guard path == dir, key == "\(ref)|\(dir)" else { return }
            guard let list = obj?.entries else {
                entries = []
                readme = nil
                dirError = "Folder not found on this branch"
                return
            }
            let sorted = list.sorted { a, b in
                if a.isDir != b.isDir { return a.isDir }
                return a.name.localizedStandardCompare(b.name) == .orderedAscending
            }
            entries = sorted
            dirError = nil
            var html: String?
            if sorted.contains(where: { !$0.isDir && $0.name.lowercased().hasPrefix("readme") }) {
                html = try? await api.readmeHTML(repo, ref: ref, dir: path)
                guard path == dir, key == "\(ref)|\(dir)" else { return }
            }
            dirCache[key] = (sorted, html)
            readme = html
        } catch {
            if isCancellation(error) { return }
            guard path == dir else { return }
            dirError = error.localizedDescription
        }
    }

    func select(_ entry: TreeEntry) async {
        if entry.isDir {
            selectedFile = nil
            file = nil
            await openDir(entry.path)
        } else if !entry.isSubmodule {
            await openFile(entry.path)
        }
    }

    func reveal(_ path: String) async {
        let parent = (path as NSString).deletingLastPathComponent
        if parent != dir { await openDir(parent) }
        await openFile(path)
    }

    func openFile(_ path: String) async {
        selectedFile = path
        file = nil
        fileError = nil
        loadingFile = true
        defer { loadingFile = false }
        let ext = (path as NSString).pathExtension.lowercased()
        do {
            if Self.imageExts.contains(ext) {
                guard let data = try await api.rawFile(repo, ref: ref, path: path) else { throw APIError(message: "File not found") }
                guard selectedFile == path else { return }
                fileSize = data.count
                if let img = NSImage(data: data) { file = .image(img) } else { file = .binary(data.count) }
                return
            }
            guard let obj = try await api.gitObject(repo, ref: ref, path: path) else { throw APIError(message: "File not found") }
            guard selectedFile == path else { return }
            fileSize = obj.byteSize
            if obj.isBinary == true {
                file = .binary(obj.byteSize ?? 0)
            } else if Self.markdownExts.contains(ext), let html = try? await api.renderedFile(repo, ref: ref, path: path) {
                file = .markdown(html: html, source: obj.text ?? "")
            } else {
                file = .text(obj.text ?? "", truncated: obj.isTruncated ?? false)
            }
        } catch {
            if isCancellation(error) { return }
            fileError = error.localizedDescription
        }
    }

    func loadAllPaths() async {
        guard allPaths == nil, !loadingPaths else { return }
        loadingPaths = true
        defer { loadingPaths = false }
        var sha = branches.first { $0.name == ref }?.commit.sha
        if sha == nil {
            await loadBranches()
            sha = branches.first { $0.name == ref }?.commit.sha
        }
        if let r = try? await api.allPaths(repo, sha: sha ?? ref) {
            allPaths = r.paths
            pathsTruncated = r.truncated
        }
    }

    var webURL: String {
        let base = "https://github.com/\(repo.full)"
        if let f = selectedFile { return "\(base)/blob/\(ref)/\(f)" }
        return dir.isEmpty ? "\(base)/tree/\(ref)" : "\(base)/tree/\(ref)/\(dir)"
    }

    static func fuzzyMatch(_ query: String, in paths: [String], limit: Int = 200) -> [String] {
        let q = query.lowercased().filter { !$0.isWhitespace }
        guard !q.isEmpty else { return Array(paths.prefix(limit)) }
        var scored: [(String, Int)] = []
        for p in paths {
            let lower = p.lowercased()
            var score = 0
            var qi = q.startIndex
            var last: String.Index?
            for i in lower.indices where qi < q.endIndex {
                if lower[i] == q[qi] {
                    if let l = last, lower.index(after: l) == i { score += 3 } else { score += 1 }
                    last = i
                    qi = q.index(after: qi)
                }
            }
            guard qi == q.endIndex else { continue }
            let name = (lower as NSString).lastPathComponent
            if name.contains(q) { score += 20 }
            if name.hasPrefix(q) { score += 10 }
            score -= lower.count / 20
            scored.append((p, score))
        }
        return scored.sorted { $0.1 > $1.1 }.prefix(limit).map(\.0)
    }
}
