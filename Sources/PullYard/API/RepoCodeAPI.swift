import Foundation

struct BlobMeta: Decodable {
    let byteSize: Int?
    let isBinary: Bool?
}

struct TreeEntry: Decodable, Identifiable {
    let name: String
    let path: String
    let type: String
    let object: BlobMeta?

    var id: String { path }
    var isDir: Bool { type == "tree" }
    var isSubmodule: Bool { type == "commit" }
}

struct GitObject: Decodable {
    let typename: String
    let entries: [TreeEntry]?
    let byteSize: Int?
    let isBinary: Bool?
    let isTruncated: Bool?
    let text: String?

    enum CodingKeys: String, CodingKey { case typename = "__typename", entries, byteSize, isBinary, isTruncated, text }
}

struct GitObjectResponse: Decodable {
    struct R: Decodable { let object: GitObject? }
    let repository: R?
}

struct BranchInfo: Decodable, Identifiable, Hashable {
    struct C: Decodable, Hashable { let sha: String }
    let name: String
    let commit: C
    var id: String { name }
}

struct GitTreeREST: Decodable {
    struct Item: Decodable {
        let path: String
        let type: String
    }
    let tree: [Item]
    let truncated: Bool
}

struct BlameCommit: Decodable, Equatable {
    struct User: Decodable, Equatable {
        let login: String
        let avatarUrl: String?
    }
    struct Author: Decodable, Equatable {
        let name: String?
        let user: User?
    }
    let oid: String
    let abbreviatedOid: String
    let messageHeadline: String
    let committedDate: Date
    let url: String
    let author: Author?

    var authorLabel: String {
        author?.user?.login ?? author?.name ?? "unknown"
    }
}

struct BlameRange: Decodable, Equatable {
    let startingLine: Int
    let endingLine: Int
    let age: Int
    let commit: BlameCommit
}

final class ResolvedObject: Decodable {
    let typename: String
    let oid: String
    let target: ResolvedObject?

    enum CodingKeys: String, CodingKey { case typename = "__typename", oid, target }

    var commitOid: String? {
        if typename == "Commit" { return oid }
        return target?.commitOid
    }
}

struct ResolveRefResponse: Decodable {
    struct R: Decodable { let object: ResolvedObject? }
    let repository: R?
}

struct BlameResponse: Decodable {
    struct Ranges: Decodable { let ranges: [BlameRange] }
    struct Target: Decodable { let blame: Ranges? }
    struct R: Decodable { let object: Target? }
    let repository: R?
}

extension GitHubAPI {
    static let blameQuery = """
    query Blame($owner: String!, $name: String!, $ref: String!, $path: String!) { repository(owner: $owner, name: $name) { object(expression: $ref) {
      ... on Commit { blame(path: $path) { ranges { startingLine endingLine age commit { oid abbreviatedOid messageHeadline committedDate url author { name user { login avatarUrl } } } } } }
    } } }
    """

    static let resolveCommitQuery = """
    query ResolveCommit($owner: String!, $name: String!, $ref: String!) { repository(owner: $owner, name: $name) { object(expression: $ref) {
      __typename oid ... on Tag { target { __typename oid ... on Tag { target { __typename oid ... on Tag { target { __typename oid } } } } } }
    } } }
    """

    func resolveCommit(_ repo: RepoRef, ref: String) async throws -> String? {
        let r: ResolveRefResponse = try await graphQL(GitHubAPI.resolveCommitQuery, ["owner": repo.owner, "name": repo.name, "ref": ref])
        return r.repository?.object?.commitOid
    }

    func blame(_ repo: RepoRef, ref: String, path: String) async throws -> [BlameRange] {
        let r: BlameResponse = try await graphQL(GitHubAPI.blameQuery, ["owner": repo.owner, "name": repo.name, "ref": ref, "path": path], strict: true)
        guard let ranges = r.repository?.object?.blame?.ranges else { throw APIError(message: "No blame for this file") }
        return ranges
    }

    static let gitObjectQuery = """
    query GitObject($owner: String!, $name: String!, $expr: String!) { repository(owner: $owner, name: $name) { object(expression: $expr) { __typename
      ... on Tree { entries { name path type object { ... on Blob { byteSize isBinary } } } }
      ... on Blob { byteSize isBinary isTruncated text }
    } } }
    """

    static func encodePath(_ path: String) -> String {
        path.split(separator: "/", omittingEmptySubsequences: false)
            .map { $0.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(CharacterSet(charactersIn: "/?#"))) ?? String($0) }
            .joined(separator: "/")
    }

    static func encodeQuery(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed.subtracting(CharacterSet(charactersIn: "&=+/?#"))) ?? s
    }

    func gitObject(_ repo: RepoRef, ref: String, path: String) async throws -> GitObject? {
        let r: GitObjectResponse = try await graphQL(GitHubAPI.gitObjectQuery, ["owner": repo.owner, "name": repo.name, "expr": "\(ref):\(path)"])
        return r.repository?.object
    }

    func branches(_ repo: RepoRef) async throws -> [BranchInfo] {
        var all: [BranchInfo] = []
        for page in 1...10 {
            let batch: [BranchInfo] = try await rest("GET", "/repos/\(repo.full)/branches?per_page=100&page=\(page)")
            all += batch
            if batch.count < 100 { break }
        }
        return all
    }

    func allPaths(_ repo: RepoRef, sha: String) async throws -> (paths: [String], truncated: Bool) {
        let r: GitTreeREST = try await rest("GET", "/repos/\(repo.full)/git/trees/\(sha)?recursive=1")
        return (r.tree.filter { $0.type == "blob" }.map(\.path), r.truncated)
    }

    func fetch(_ path: String, accept: String) async throws -> Data? {
        var req = URLRequest(url: URL(string: "https://api.github.com" + path)!)
        req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        req.setValue(accept, forHTTPHeaderField: "Accept")
        req.setValue("2022-11-28", forHTTPHeaderField: "X-GitHub-Api-Version")
        req.setValue("PullYard-macOS", forHTTPHeaderField: "User-Agent")
        let (data, response) = try await Diagnostics.track(.rest, Diagnostics.restName("GET", req.url), url: req.url) { try await URLSession.shared.data(for: req) }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        if status == 404 { return nil }
        guard (200..<300).contains(status) else { throw APIError(message: "GitHub \(status)", status: status) }
        return data
    }

    func readmeHTML(_ repo: RepoRef, ref: String, dir: String) async throws -> String? {
        let suffix = dir.isEmpty ? "" : "/" + GitHubAPI.encodePath(dir)
        guard let data = try await fetch("/repos/\(repo.full)/readme\(suffix)?ref=\(GitHubAPI.encodeQuery(ref))", accept: "application/vnd.github.html+json") else { return nil }
        return String(decoding: data, as: UTF8.self)
    }

    func renderedFile(_ repo: RepoRef, ref: String, path: String) async throws -> String? {
        guard let data = try await fetch("/repos/\(repo.full)/contents/\(GitHubAPI.encodePath(path))?ref=\(GitHubAPI.encodeQuery(ref))", accept: "application/vnd.github.html+json") else { return nil }
        return String(decoding: data, as: UTF8.self)
    }

    func rawFile(_ repo: RepoRef, ref: String, path: String) async throws -> Data? {
        try await fetch("/repos/\(repo.full)/contents/\(GitHubAPI.encodePath(path))?ref=\(GitHubAPI.encodeQuery(ref))", accept: "application/vnd.github.raw")
    }
}
