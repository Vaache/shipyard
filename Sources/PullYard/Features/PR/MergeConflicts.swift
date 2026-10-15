import Foundation

enum ConflictState: Equatable {
    case checking
    case files([String])
    case noClone
    case unavailable(String)

    var files: [String] {
        if case .files(let list) = self { return list }
        return []
    }
}

enum MergeConflicts {
    static func applies(_ pr: PullRequestDetail) -> Bool {
        pr.isOpen && (pr.mergeable == "CONFLICTING" || pr.mergeStateStatus == "DIRTY")
    }

    static func remote(for repo: RepoRef, in dir: String) -> String {
        let target = repo.full.lowercased()
        for line in Git.run(["remote", "-v"], in: dir).out.split(separator: "\n") {
            let parts = line.split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init)
            guard parts.count >= 2 else { continue }
            var url = parts[1].lowercased()
            if url.hasSuffix(".git") { url.removeLast(4) }
            if url.contains("github.com"), url.hasSuffix("/" + target) || url.hasSuffix(":" + target) { return parts[0] }
        }
        return "origin"
    }

    static func scan(folder: String, repo: RepoRef, number: Int, base: String, baseOid: String?, headOid: String) -> ConflictState {
        guard Git.run(["rev-parse", "--git-dir"], in: folder).ok else {
            return .unavailable("\(folder) is not a git repository.")
        }
        let remote = remote(for: repo, in: folder)
        let baseRev = baseOid ?? "refs/remotes/\(remote)/\(base)"
        func has(_ rev: String) -> Bool { Git.run(["cat-file", "-e", rev + "^{commit}"], in: folder).ok }
        if baseOid == nil || !has(baseRev) || !has(headOid) {
            let env = ["GIT_TERMINAL_PROMPT": "0", "GCM_INTERACTIVE": "never"]
            let refspecs = ["+refs/heads/\(base):refs/remotes/\(remote)/\(base)", "refs/pull/\(number)/head"]
            _ = Git.run(["fetch", "--no-tags", "--no-write-fetch-head", "--quiet", remote] + refspecs, in: folder, env: env, timeout: 120)
            guard has(baseRev), has(headOid) else {
                return .unavailable("Couldn't fetch the latest \(base) and PR commits into \(abbreviate(folder)).")
            }
        }
        let r = Git.run(["merge-tree", "--write-tree", "--name-only", "--no-messages", baseRev, headOid], in: folder)
        switch r.status {
        case 0:
            return .files([])
        case 1:
            let lines = r.out.split(separator: "\n").dropFirst().map(String.init).filter { !$0.isEmpty }
            var seen = Set<String>()
            return .files(lines.filter { seen.insert($0).inserted })
        default:
            return .unavailable("git merge-tree failed — conflict detection needs git 2.38 or newer.")
        }
    }

    static func abbreviate(_ path: String) -> String {
        path.replacingOccurrences(of: FileManager.default.homeDirectoryForCurrentUser.path, with: "~")
    }
}

enum ConflictPrompt {
    static func build(pr: PullRequestDetail, repo: RepoRef, files: [String], push: Bool) -> String {
        var out = """
        Resolve the merge conflicts on pull request #\(pr.number) "\(pr.title)" in \(repo.full).
        PR: \(pr.url)
        Branch: \(pr.headRefName) → \(pr.baseRefName)

        GitHub reports that \(pr.headRefName) can't be merged into \(pr.baseRefName) because of conflicts.

        Steps:
        1. Make sure the working tree is on the PR branch: run `gh pr checkout \(pr.number)` and `git pull`. If there are uncommitted local changes, stop and ask me first.
        2. Fetch the latest \(pr.baseRefName) from the remote that points to \(repo.full) (usually `origin`) and merge it into the PR branch, e.g. `git fetch origin \(pr.baseRefName)` and `git merge origin/\(pr.baseRefName)`. Merge — do not rebase or force-push.
        3. Resolve every conflict. For each file, read what both sides changed and keep the intent of both; don't just take one side. If a conflict can't be resolved safely, stop and explain why.
        4. Check that no conflict markers are left (`git diff --check`), then build the project and run the tests related to the conflicting code if the project has an obvious way to do so.
        5. Commit the merge with a clear message.

        """
        out += push ? "6. Push the branch (`git push`).\n" : "6. Do not push — I will review the merge first.\n"
        out += "7. Finish with a short list: each conflicting file → how it was resolved.\n"
        if files.isEmpty {
            out += "\n## Conflicting files\nThe exact list isn't known yet — the merge in step 2 will show it.\n"
        } else {
            out += "\n## Conflicting files (\(files.count))\n" + files.map { "- `\($0)`" }.joined(separator: "\n") + "\n"
        }
        return out
    }
}
