import Foundation

enum ConversationHTML {
    static func esc(_ s: String) -> String {
        var r = ""
        r.reserveCapacity(s.count)
        for ch in s {
            switch ch {
            case "&": r += "&amp;"
            case "<": r += "&lt;"
            case ">": r += "&gt;"
            case "\"": r += "&quot;"
            case "'": r += "&#39;"
            default: r.append(ch)
            }
        }
        return r
    }

    private static func avatar(_ url: String?, size: Int = 32) -> String {
        guard let url else { return "<div class=\"avatar ph\"></div>" }
        let sep = url.contains("?") ? "&" : "?"
        return "<img class=\"avatar\" src=\"\(esc(url))\(sep)s=\(size * 2)\" width=\"\(size)\" height=\"\(size)\">"
    }

    private static func time(_ d: Date?) -> String {
        guard let d else { return "" }
        return "<span title=\"\(esc(Fmt.full.string(from: d)))\">\(esc(Fmt.rel(d)))</span>"
    }

    private static func body(_ html: String?) -> String {
        let h = (html ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
        return h.isEmpty ? "<div class=\"md\"><p class=\"empty\">\("No description provided.")</p></div>" : "<div class=\"md\">\(h)</div>"
    }

    private static func commentBox(author: Actor?, date: Date?, html: String?, note: String, highlight: Bool, link: String? = nil) -> String {
        let login = esc(author?.login ?? "ghost")
        let linkHTML = link.map { " · <a href=\"\(esc($0))\">\("link")</a>" } ?? ""
        return """
        <div class="item">\(avatar(author?.avatarUrl))<div class="box\(highlight ? " mine" : "")">
        <div class="head"><b>\(login)</b><span>\(note)</span><span>·</span>\(time(date))\(linkHTML)</div>
        \(body(html))</div></div>
        """
    }

    private static func event(_ icon: String, _ cls: String, actor: Actor?, text: String, date: Date?) -> String {
        let who = actor.map { "\(avatar($0.avatarUrl, size: 18))<b>\(esc($0.login))</b>" } ?? ""
        return "<div class=\"event\"><span class=\"ic \(cls)\">\(icon)</span>\(who)<span>\(text)</span><span class=\"t\">\(time(date))</span></div>"
    }

    private static func label(_ l: LabelInfo) -> String {
        "<span class=\"label\" style=\"background:#\(esc(l.color))38;border-color:#\(esc(l.color))\">\(esc(l.name))</span>"
    }

    private static func hunk(_ diffHunk: String?) -> String {
        guard let diffHunk else { return "" }
        var lines = diffHunk.components(separatedBy: "\n")
        if lines.first?.hasPrefix("@@") == true, lines.count > 1 { lines.removeFirst() }
        let tail = lines.suffix(6)
        let rows = tail.map { l -> String in
            let cls = l.hasPrefix("+") ? "a" : l.hasPrefix("-") ? "d" : ""
            return "<div class=\"\(cls)\">\(esc(l.isEmpty ? " " : l))</div>"
        }.joined()
        return "<div class=\"hunk\">\(rows)</div>"
    }

    static func thread(_ t: ReviewThread) -> String {
        let line = t.line ?? t.originalLine
        var tags = ""
        if t.isOutdated { tags += "<span class=\"pill\">\("outdated")</span>" }
        if t.isResolved { tags += "<span class=\"pill ok\">\("resolved")</span>" }
        let comments = t.comments.nodes.map { c in
            """
            <div class="tc">\(avatar(c.author?.avatarUrl, size: 20))<div class="tcb"><div class="meta"><b>\(esc(c.author?.login ?? "ghost"))</b> · \(time(c.createdAt))</div><div class="md">\(c.bodyHTML)</div></div></div>
            """
        }.joined()
        var actions = ""
        if t.viewerCanReply { actions += "<button onclick=\"post('reply','\(esc(t.id))')\">\("Reply")</button>" }
        if t.isResolved, t.viewerCanUnresolve { actions += "<button onclick=\"post('unresolve','\(esc(t.id))')\">\("Unresolve")</button>" }
        if !t.isResolved, t.viewerCanResolve { actions += "<button onclick=\"post('resolve','\(esc(t.id))')\">\("Resolve")</button>" }
        actions += "<button class=\"ghost\" onclick=\"post('file','\(esc(t.path))')\">\("View in files")</button>"
        return """
        <details class="thread"\(t.isResolved ? "" : " open")><summary class="th">\(esc(t.path))\(line.map { "<span class=\"ln\">:\($0)</span>" } ?? "")\(tags)</summary>
        \(hunk(t.comments.nodes.first?.diffHunk))\(comments)<div class="actions">\(actions)</div></details>
        """
    }

    static func render(pr: PullRequestDetail) -> String {
        let authorLogin = pr.author?.login
        var out = commentBox(author: pr.author, date: pr.createdAt, html: pr.bodyHTML, note: "description", highlight: true)
        var byReview: [String: [ReviewThread]] = [:]
        for t in pr.reviewThreads.nodes {
            if let rid = t.comments.nodes.first?.pullRequestReview?.id { byReview[rid, default: []].append(t) }
        }
        var rendered = Set<String>()
        if (pr.timelineItems.totalCount ?? 0) > pr.timelineItems.nodes.count {
            out += "<div class=\"event\"><span class=\"ic\">…</span><span>\("Showing the latest \(pr.timelineItems.nodes.count) of \(pr.timelineItems.totalCount ?? 0) events") — <a href=\"\(esc(pr.url))\">\("full history")</a></span></div>"
        }
        for item in pr.timelineItems.nodes {
            if item.typename == "PullRequestReview" {
                let threads = byReview[item.id ?? ""] ?? []
                threads.forEach { rendered.insert($0.id) }
                out += review(item, threads: threads, highlight: item.author?.login == authorLogin)
            } else {
                out += common(item, authorLogin: authorLogin, base: pr.baseRefName)
            }
        }
        let rest = pr.reviewThreads.nodes.filter { !rendered.contains($0.id) }
        if !rest.isEmpty {
            out += "<h3 class=\"sec\">\("Review threads")</h3>" + rest.map(thread).joined()
        }
        return page(out)
    }

    static func render(issue: IssueDetail) -> String {
        var out = commentBox(author: issue.author, date: issue.createdAt, html: issue.bodyHTML, note: "description", highlight: true)
        for item in issue.timelineItems.nodes {
            out += common(item, authorLogin: issue.author?.login, base: "")
        }
        return page(out)
    }

    private static func review(_ item: TimelineItem, threads: [ReviewThread], highlight: Bool) -> String {
        let hasBody = !(item.bodyHTML ?? "").trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let (icon, cls, text): (String, String, String) = {
            switch item.state {
            case "APPROVED": return ("✓", "ok", "approved")
            case "CHANGES_REQUESTED": return ("±", "bad", "requested changes")
            case "DISMISSED": return ("✕", "", "review dismissed")
            case "PENDING": return ("…", "", "pending review")
            default: return ("💬", "", "reviewed")
            }
        }()
        if !hasBody && threads.isEmpty {
            if item.state == "COMMENTED" { return "" }
            return event(icon, cls, actor: item.author, text: text, date: item.createdAt)
        }
        let login = esc(item.author?.login ?? "ghost")
        let bodyHTML = hasBody ? "<div class=\"md\">\(item.bodyHTML ?? "")</div>" : ""
        return """
        <div class="item">\(avatar(item.author?.avatarUrl))<div class="box\(highlight ? " mine" : "")">
        <div class="head"><b>\(login)</b><span class="rv \(cls)">\(icon) \(text)</span><span>·</span>\(time(item.createdAt))</div>
        \(bodyHTML)\(threads.map(thread).joined())</div></div>
        """
    }

    private static func common(_ item: TimelineItem, authorLogin: String?, base: String) -> String {
        switch item.typename {
        case "IssueComment":
            return commentBox(author: item.author, date: item.createdAt, html: item.bodyHTML, note: "commented", highlight: item.author?.login == authorLogin)
        case "PullRequestCommit":
            guard let c = item.commit else { return "" }
            let who = c.author?.user?.login ?? c.author?.name ?? ""
            let actor = Actor(login: who, avatarUrl: c.author?.avatarUrl)
            return event("●", "", actor: actor, text: "commit" + " <code>\(esc(c.abbreviatedOid ?? ""))</code> \(esc(c.messageHeadline ?? ""))", date: c.committedDate)
        case "MergedEvent":
            return event("⇢", "merged", actor: item.actor, text: "merged" + "\(item.commit?.abbreviatedOid.map { " (<code>\(esc($0))</code>)" } ?? "") " + "into" + " <code>\(esc(item.mergeRefName ?? base))</code>", date: item.createdAt)
        case "ClosedEvent":
            return event("✕", "bad", actor: item.actor, text: "closed", date: item.createdAt)
        case "ReopenedEvent":
            return event("↺", "ok", actor: item.actor, text: "reopened", date: item.createdAt)
        case "HeadRefForcePushedEvent":
            return event("⤒", "", actor: item.actor, text: "force-pushed the branch", date: item.createdAt)
        case "HeadRefDeletedEvent":
            return event("⌫", "", actor: item.actor, text: "deleted the branch", date: item.createdAt)
        case "ReadyForReviewEvent":
            return event("◉", "ok", actor: item.actor, text: "marked as ready for review", date: item.createdAt)
        case "ConvertToDraftEvent":
            return event("◌", "", actor: item.actor, text: "converted to draft", date: item.createdAt)
        case "ReviewRequestedEvent":
            return event("👁", "", actor: item.actor, text: "requested review from" + " → <b>\(esc(item.requestedReviewer?.display ?? "?"))</b>", date: item.createdAt)
        case "LabeledEvent":
            guard let l = item.label else { return "" }
            return event("🏷", "", actor: item.actor, text: "label" + " \(label(l))", date: item.createdAt)
        case "RenamedTitleEvent":
            return event("✎", "", actor: item.actor, text: "title" + ": <s>\(esc(item.previousTitle ?? ""))</s> → \(esc(item.currentTitle ?? ""))", date: item.createdAt)
        case "CrossReferencedEvent":
            guard let s = item.source, let url = s.url else { return "" }
            let name = "\(s.repository?.nameWithOwner ?? "")#\(s.number ?? 0)"
            return event("↗", "", actor: item.actor, text: "mentioned in" + " <a href=\"\(esc(url))\">\(esc(name)) \(esc(s.title ?? ""))</a>", date: item.createdAt)
        default:
            return ""
        }
    }

    private static func page(_ content: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8"><style>\(css)</style>
        <script>function post(t,id){window.webkit.messageHandlers.app.postMessage({type:t,id:id})}</script>
        </head><body><div class="tl">\(content)</div></body></html>
        """
    }

    static var css: String { (Theme.isBrand ? brandVars : systemVars) + baseCSS }

    private static let systemVars = """
    :root{--fg:#1f2328;--muted:#59636e;--border:#d1d9e0;--sub:#f6f8fa;--card:transparent;--link:#0969da;--green:#1a7f37;--red:#d1242f;--purple:#8250df;--add:#dafbe1;--del:#ffebe9;--mine:#ddf4ff;--code:#fff;color-scheme:light dark}
    @media (prefers-color-scheme: dark){:root{--fg:#e6edf3;--muted:#9198a1;--border:#3d444d;--sub:#151b23;--link:#4493f8;--green:#3fb950;--red:#f85149;--purple:#ab7df8;--add:rgba(46,160,67,.18);--del:rgba(248,81,73,.18);--mine:rgba(56,139,253,.12);--code:transparent}}

    """

    private static let brandVars = """
    :root{--fg:#0f172a;--muted:#52627a;--border:rgba(15,23,42,.12);--sub:#f3f6fb;--card:#ffffff;--link:#0278c2;--green:#16a34a;--red:#dc2626;--purple:#7c3aed;--add:rgba(22,163,74,.11);--del:rgba(220,38,38,.10);--mine:rgba(2,132,199,.07);--code:#f8fafc;color-scheme:light dark}
    @media (prefers-color-scheme: dark){:root{--fg:#e8edf6;--muted:#93a1b8;--border:rgba(148,163,184,.18);--sub:#121b31;--card:#141d35;--link:#38bdf8;--green:#4ade80;--red:#f87171;--purple:#a78bfa;--add:rgba(74,222,128,.10);--del:rgba(248,113,113,.11);--mine:rgba(56,189,248,.08);--code:#0b1222}}

    """

    private static let baseCSS = """
    *{box-sizing:border-box}
    body{font:13px/1.5 -apple-system,BlinkMacSystemFont,"Helvetica Neue",sans-serif;color:var(--fg);background:transparent;margin:0;padding:16px 20px 32px;-webkit-font-smoothing:antialiased}
    a{color:var(--link);text-decoration:none}a:hover{text-decoration:underline}
    .tl{max-width:1000px;margin:0 auto}
    .item{display:flex;gap:12px;margin:0 0 16px;align-items:flex-start}
    .avatar{border-radius:50%;flex:none}.avatar.ph{width:32px;height:32px;background:var(--border)}
    .box{flex:1;min-width:0;border:1px solid var(--border);border-radius:10px;overflow:hidden;background:var(--card)}
    .head{padding:7px 12px;background:var(--sub);border-bottom:1px solid var(--border);color:var(--muted);display:flex;gap:6px;flex-wrap:wrap;align-items:center}
    .box.mine .head{background:var(--mine)}
    .head b{color:var(--fg)}
    .rv.ok{color:var(--green);font-weight:600}.rv.bad{color:var(--red);font-weight:600}
    .md{padding:10px 14px;overflow-x:auto;word-wrap:break-word}
    .md>:first-child{margin-top:0}.md>:last-child{margin-bottom:0}
    .md p,.md ul,.md ol,.md blockquote,.md pre,.md table,.md details{margin:0 0 10px}
    .md img{max-width:100%;border-radius:4px}
    .md pre{background:var(--code);border:1px solid var(--border);padding:10px 12px;border-radius:8px;overflow:auto;font-size:12px;line-height:1.45}
    code,pre,kbd{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px}
    .md :not(pre)>code,.event code{background:rgba(129,139,152,.16);padding:.1em .35em;border-radius:5px}
    .md blockquote{padding:0 1em;color:var(--muted);border-left:.25em solid var(--border)}
    .md table{border-collapse:collapse;display:block;overflow:auto;max-width:100%}.md td,.md th{border:1px solid var(--border);padding:4px 10px}
    .md h1,.md h2{border-bottom:1px solid var(--border);padding-bottom:.25em}.md h1{font-size:1.5em}.md h2{font-size:1.3em}.md h3{font-size:1.12em}
    .md h1,.md h2,.md h3,.md h4{margin:14px 0 8px}
    .md hr{border:0;border-top:1px solid var(--border)}
    .md ul.contains-task-list{list-style:none;padding-left:1.1em}
    .md summary{cursor:pointer}
    .md .user-mention,.md .team-mention{font-weight:600;color:var(--fg)}
    .md .pl-k,.md .pl-s1 .pl-k{color:#cf222e}.md .pl-s,.md .pl-pds{color:#0a3069}.md .pl-c{color:#6e7781}.md .pl-en,.md .pl-e{color:#8250df}.md .pl-c1,.md .pl-v{color:#0550ae}.md .pl-smi{color:inherit}
    @media (prefers-color-scheme: dark){.md .pl-k{color:#ff7b72}.md .pl-s,.md .pl-pds{color:#a5d6ff}.md .pl-c{color:#8b949e}.md .pl-en,.md .pl-e{color:#d2a8ff}.md .pl-c1,.md .pl-v{color:#79c0ff}}
    .md .blob-num,.md .blob-code{font:12px ui-monospace,monospace}
    .empty{color:var(--muted);font-style:italic}
    .event{display:flex;align-items:center;gap:7px;margin:0 0 12px 44px;color:var(--muted);font-size:12px;flex-wrap:wrap}
    .event b{color:var(--fg)}
    .event .ic{width:22px;height:22px;border-radius:50%;display:inline-flex;align-items:center;justify-content:center;background:var(--sub);border:1px solid var(--border);font-size:11px;flex:none;color:var(--fg)}
    .event .ic.ok{background:var(--green);color:#fff;border-color:transparent}.event .ic.bad{background:var(--red);color:#fff;border-color:transparent}.event .ic.merged{background:var(--purple);color:#fff;border-color:transparent}
    .event .t{margin-left:2px}
    .label{display:inline-block;padding:0 7px;border-radius:10px;border:1px solid;font-size:11px;font-weight:500;color:var(--fg)}
    .thread{margin:10px 12px;border:1px solid var(--border);border-radius:9px;overflow:hidden;background:var(--sub)}
    .thread>summary{padding:6px 10px;font:12px ui-monospace,monospace;cursor:pointer;display:flex;gap:8px;align-items:center;list-style:none}
    .thread>summary::-webkit-details-marker{display:none}
    .thread .ln{color:var(--muted);margin-left:-6px}
    .pill{font:11px -apple-system;padding:0 7px;border-radius:10px;border:1px solid var(--border);color:var(--muted)}.pill.ok{color:var(--green);border-color:var(--green)}
    .hunk{font:11.5px/1.5 ui-monospace,monospace;overflow:auto;border-top:1px solid var(--border);border-bottom:1px solid var(--border);background:var(--bg,transparent)}
    .hunk{background:var(--code)}
    .hunk div{padding:0 10px;white-space:pre}.hunk .a{background:var(--add)}.hunk .d{background:var(--del)}
    .tc{display:flex;gap:8px;padding:8px 10px;border-bottom:1px solid var(--border)}
    .tc .avatar{margin-top:2px}.tcb{min-width:0;flex:1}.tc .meta{color:var(--muted);font-size:12px}.tc .meta b{color:var(--fg)}
    .tc .md{padding:2px 0 0}
    .actions{padding:6px 10px;display:flex;gap:6px}
    button{font:12px -apple-system;padding:3px 10px;border-radius:7px;border:1px solid var(--border);background:var(--sub);color:var(--fg);cursor:pointer}
    button:hover{border-color:var(--muted)}button.ghost{background:transparent;color:var(--muted)}
    h3.sec{margin:24px 0 8px 44px;font-size:13px;color:var(--muted)}
    """
}
