import Foundation

struct FixOptions {
    var includeGeneral = true
    var commit = true
    var push = false
    var replyAndResolve = false
}

struct GeneralFeedback: Identifiable {
    let id: String
    let author: String
    let body: String
}

enum FixPrompt {
    static func generalFeedback(_ pr: PullRequestDetail) -> [GeneralFeedback] {
        pr.timelineItems.nodes.compactMap { item in
            guard item.typename == "PullRequestReview", item.state == "CHANGES_REQUESTED",
                  let body = item.body?.trimmingCharacters(in: .whitespacesAndNewlines), !body.isEmpty else { return nil }
            return GeneralFeedback(id: item.id ?? UUID().uuidString, author: item.author?.login ?? "ghost", body: body)
        }
    }

    static func build(pr: PullRequestDetail, repo: RepoRef, threads: [ReviewThread], general: [GeneralFeedback], options: FixOptions) -> String {
        var out = """
        Address the code review feedback on pull request #\(pr.number) "\(pr.title)" in \(repo.full).
        PR: \(pr.url)
        Branch: \(pr.headRefName) → \(pr.baseRefName)

        Steps:
        1. Make sure the working tree is on the PR branch: run `gh pr checkout \(pr.number)` and `git pull`. If there are uncommitted local changes, stop and ask me first.
        2. Go through every item below. Make the change the reviewer asked for. If a comment is unclear, already handled, or you disagree with it, do not change code for it — explain why in your summary.
        3. Keep changes minimal and in the style of the surrounding code. Run the project's analyzer/linter and the tests related to the touched code.

        """
        var step = 4
        if options.commit {
            out += "\(step). Commit the fixes with a clear message.\n"
            step += 1
        }
        if options.push {
            out += "\(step). Push the branch (`git push`).\n"
            step += 1
        } else {
            out += "\(step). Do not push — I will review the changes first.\n"
            step += 1
        }
        if options.replyAndResolve {
            out += """
            \(step). After pushing, for each fixed thread reply with a short note of what changed and resolve it:
               gh api graphql -f query='mutation($id:ID!,$b:String!){addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$id,body:$b}){clientMutationId}}' -f id=THREAD_ID -f b='Fixed: …'
               gh api graphql -f query='mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){clientMutationId}}' -f id=THREAD_ID
               Leave threads you did not fix unresolved.

            """
            step += 1
        }
        out += "\(step). Finish with a short list: each item → fixed / skipped (and why).\n"

        if !threads.isEmpty {
            out += "\n## Review comments in code (\(threads.count))\n"
            for (i, th) in threads.enumerated() {
                let line = th.line ?? th.originalLine
                out += "\n### \(i + 1). `\(th.path)\(line.map { ":\($0)" } ?? "")`"
                if th.isOutdated { out += " (outdated — code may have moved)" }
                out += "\nThread ID: \(th.id)\n"
                if let hunk = th.comments.nodes.first?.diffHunk {
                    let tail = hunk.components(separatedBy: "\n").filter { !$0.hasPrefix("@@") }.suffix(8)
                    out += "```diff\n\(tail.joined(separator: "\n"))\n```\n"
                }
                for c in th.comments.nodes {
                    let body = c.body.trimmingCharacters(in: .whitespacesAndNewlines)
                    out += "**@\(c.author?.login ?? "ghost")**:\n" + body.split(separator: "\n", omittingEmptySubsequences: false).map { "> \($0)" }.joined(separator: "\n") + "\n"
                }
            }
        }
        if options.includeGeneral, !general.isEmpty {
            out += "\n## General review feedback (changes requested)\n"
            for g in general {
                out += "\n**@\(g.author)**:\n" + g.body.split(separator: "\n", omittingEmptySubsequences: false).map { "> \($0)" }.joined(separator: "\n") + "\n"
            }
        }
        return out
    }
}

struct FoundFixPrompt {
    let text: String
    let author: String
    let date: Date?
    let url: String?
}

enum FixPromptExtractor {
    static func extract(_ body: String) -> String? {
        if let start = body.range(of: "--- FIX PROMPT ---", options: .caseInsensitive) {
            var rest = body[start.upperBound...]
            if let end = rest.range(of: "--- END FIX PROMPT ---", options: .caseInsensitive) {
                rest = rest[..<end.lowerBound]
            } else if let fence = rest.range(of: "```") {
                rest = rest[..<fence.lowerBound]
            }
            let text = rest.trimmingCharacters(in: .whitespacesAndNewlines)
            return text.isEmpty ? nil : text
        }
        let lower = body.lowercased()
        for marker in ["fix prompt", "prompt for ai agent"] {
            guard let m = lower.range(of: marker) else { continue }
            let offset = lower.distance(from: lower.startIndex, to: m.upperBound)
            let after = body[body.index(body.startIndex, offsetBy: offset)...]
            guard let open = after.range(of: "```") else { continue }
            let afterOpen = after[open.upperBound...]
            guard let newline = afterOpen.firstIndex(of: "\n") else { continue }
            let content = afterOpen[afterOpen.index(after: newline)...]
            guard let close = content.range(of: "```") else { continue }
            let text = content[..<close.lowerBound].trimmingCharacters(in: .whitespacesAndNewlines)
            if !text.isEmpty { return text }
        }
        return nil
    }

    static func latest(in pr: PullRequestDetail) -> FoundFixPrompt? {
        var best: FoundFixPrompt?
        func consider(_ body: String?, author: String?, date: Date?, url: String?) {
            guard let body, let text = extract(body) else { return }
            if let b = best, let bd = b.date, let d = date, d < bd { return }
            best = FoundFixPrompt(text: text, author: author ?? "ghost", date: date, url: url)
        }
        consider(pr.body, author: pr.author?.login, date: pr.createdAt, url: pr.url)
        for item in pr.timelineItems.nodes where item.typename == "IssueComment" || item.typename == "PullRequestReview" {
            consider(item.body, author: item.author?.login, date: item.createdAt, url: item.url)
        }
        return best
    }

    static func wrap(_ found: FoundFixPrompt, pr: PullRequestDetail, options: FixOptions) -> String {
        var out = """
        Before starting: make sure the working tree is on the PR branch — run `gh pr checkout \(pr.number)` and `git pull`. If there are uncommitted local changes, stop and ask me first.

        \(found.text)

        """
        var extra: [String] = []
        if options.commit { extra.append("Commit the fixes with a clear message.") }
        extra.append(options.push ? "Push the branch when done (`git push`)." : "Do not push — I will review the changes first.")
        if options.replyAndResolve {
            extra.append("After pushing, reply on each review thread you fixed with a short note and resolve it via `gh api graphql` (addPullRequestReviewThreadReply / resolveReviewThread). Leave unfixed threads unresolved.")
        }
        out += "\nAdditional instructions:\n" + extra.map { "- \($0)" }.joined(separator: "\n") + "\n"
        return out
    }
}
