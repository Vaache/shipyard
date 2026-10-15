using System.Text;
using Shipyard.Core.Api;
using Shipyard.Core.Util;

namespace Shipyard.Core.Html;

public static class ConversationHtml
{
    public static bool Brand { get; set; } = true;

    public static string Esc(string? s)
    {
        if (string.IsNullOrEmpty(s)) return "";
        var sb = new StringBuilder(s.Length + 16);
        foreach (var ch in s)
        {
            switch (ch)
            {
                case '&': sb.Append("&amp;"); break;
                case '<': sb.Append("&lt;"); break;
                case '>': sb.Append("&gt;"); break;
                case '"': sb.Append("&quot;"); break;
                case '\'': sb.Append("&#39;"); break;
                default: sb.Append(ch); break;
            }
        }
        return sb.ToString();
    }

    private static string Avatar(string? url, int size = 32)
    {
        if (url is null) return "<div class=\"avatar ph\"></div>";
        var sep = url.Contains('?') ? "&" : "?";
        return $"<img class=\"avatar\" src=\"{Esc(url)}{sep}s={size * 2}\" width=\"{size}\" height=\"{size}\">";
    }

    private static string Time(DateTimeOffset? d) => d is null ? "" : $"<span title=\"{Esc(Fmt.Full(d))}\">{Esc(Fmt.Rel(d))}</span>";

    private static string Body(string? html)
    {
        var h = (html ?? "").Trim();
        return h.Length == 0 ? "<div class=\"md\"><p class=\"empty\">No description provided.</p></div>" : $"<div class=\"md\">{h}</div>";
    }

    private static string CommentBox(Actor? author, DateTimeOffset? date, string? html, string note, bool highlight, string? link = null)
    {
        var login = Esc(author?.Login ?? "ghost");
        var linkHtml = link is null ? "" : $" · <a href=\"{Esc(link)}\">link</a>";
        return $"<div class=\"item\">{Avatar(author?.AvatarUrl)}<div class=\"box{(highlight ? " mine" : "")}\">\n<div class=\"head\"><b>{login}</b><span>{note}</span><span>·</span>{Time(date)}{linkHtml}</div>\n{Body(html)}</div></div>";
    }

    private static string Event(string icon, string cls, Actor? actor, string text, DateTimeOffset? date)
    {
        var who = actor is null ? "" : $"{Avatar(actor.AvatarUrl, 18)}<b>{Esc(actor.Login)}</b>";
        return $"<div class=\"event\"><span class=\"ic {cls}\">{icon}</span>{who}<span>{text}</span><span class=\"t\">{Time(date)}</span></div>";
    }

    private static string Label(LabelInfo l) =>
        $"<span class=\"label\" style=\"background:#{Esc(l.Color)}38;border-color:#{Esc(l.Color)}\">{Esc(l.Name)}</span>";

    private static string Hunk(string? diffHunk)
    {
        if (diffHunk is null) return "";
        var lines = diffHunk.Replace("\r\n", "\n").Split('\n').ToList();
        if (lines.Count > 1 && lines[0].StartsWith("@@")) lines.RemoveAt(0);
        var tail = lines.Skip(Math.Max(0, lines.Count - 6));
        var rows = string.Concat(tail.Select(l =>
        {
            var cls = l.StartsWith('+') ? "a" : l.StartsWith('-') ? "d" : "";
            return $"<div class=\"{cls}\">{Esc(l.Length == 0 ? " " : l)}</div>";
        }));
        return $"<div class=\"hunk\">{rows}</div>";
    }

    public static string Thread(ReviewThread t)
    {
        var line = t.Line ?? t.OriginalLine;
        var tags = "";
        if (t.IsOutdated) tags += "<span class=\"pill\">outdated</span>";
        if (t.IsResolved) tags += "<span class=\"pill ok\">resolved</span>";
        var comments = string.Concat(t.Comments.Items.Select(c =>
            $"<div class=\"tc\">{Avatar(c.Author?.AvatarUrl, 20)}<div class=\"tcb\"><div class=\"meta\"><b>{Esc(c.Author?.Login ?? "ghost")}</b> · {Time(c.CreatedAt)}</div><div class=\"md\">{c.BodyHTML}</div></div></div>"));
        var actions = "";
        if (t.ViewerCanReply) actions += $"<button onclick=\"post('reply','{Esc(t.Id)}')\">Reply</button>";
        if (t.IsResolved && t.ViewerCanUnresolve) actions += $"<button onclick=\"post('unresolve','{Esc(t.Id)}')\">Unresolve</button>";
        if (!t.IsResolved && t.ViewerCanResolve) actions += $"<button onclick=\"post('resolve','{Esc(t.Id)}')\">Resolve</button>";
        return ThreadBox(t, line, tags, comments, actions, includeFileLink: true);
    }

    public static string InlineThread(ReviewThread t)
    {
        var line = t.Line ?? t.OriginalLine;
        var tags = "";
        if (t.IsOutdated) tags += "<span class=\"pill\">outdated</span>";
        if (t.IsResolved) tags += "<span class=\"pill ok\">resolved</span>";
        var comments = string.Concat(t.Comments.Items.Select(c =>
            $"<div class=\"tc\">{Avatar(c.Author?.AvatarUrl, 20)}<div class=\"tcb\"><div class=\"meta\"><b>{Esc(c.Author?.Login ?? "ghost")}</b> · {Time(c.CreatedAt)}</div><div class=\"md\">{c.BodyHTML}</div></div></div>"));
        var actions = "";
        if (t.ViewerCanReply) actions += $"<button onclick=\"post('reply','{Esc(t.Id)}')\">Reply</button>";
        if (t.IsResolved && t.ViewerCanUnresolve) actions += $"<button onclick=\"post('unresolve','{Esc(t.Id)}')\">Unresolve</button>";
        if (!t.IsResolved && t.ViewerCanResolve) actions += $"<button onclick=\"post('resolve','{Esc(t.Id)}')\">Resolve</button>";
        var summary = $"{Fmt.Plural(t.Comments.Items.Count, "comment", "comments")}{tags}";
        return $"<details class=\"thread inline\"{(t.IsResolved ? "" : " open")}><summary class=\"th\">{summary}</summary>\n{comments}<div class=\"actions\">{actions}</div></details>";
    }

    private static string ThreadBox(ReviewThread t, int? line, string tags, string comments, string actions, bool includeFileLink)
    {
        if (includeFileLink) actions += $"<button class=\"ghost\" onclick=\"post('file','{Esc(t.Path)}')\">View in files</button>";
        var ln = line is { } l ? $"<span class=\"ln\">:{l}</span>" : "";
        return $"<details class=\"thread\"{(t.IsResolved ? "" : " open")}><summary class=\"th\">{Esc(t.Path)}{ln}{tags}</summary>\n{Hunk(t.Comments.Items.FirstOrDefault()?.DiffHunk)}{comments}<div class=\"actions\">{actions}</div></details>";
    }

    public static string Render(PullRequestDetail pr)
    {
        var authorLogin = pr.Author?.Login;
        var sb = new StringBuilder();
        sb.Append(CommentBox(pr.Author, pr.CreatedAt, pr.BodyHTML, "description", true));
        var byReview = new Dictionary<string, List<ReviewThread>>();
        foreach (var t in pr.ReviewThreads.Items)
        {
            if (t.Comments.Items.FirstOrDefault()?.PullRequestReview?.Id is { } rid)
            {
                if (!byReview.TryGetValue(rid, out var list)) byReview[rid] = list = new();
                list.Add(t);
            }
        }
        var rendered = new HashSet<string>();
        var total = pr.TimelineItems.TotalCount ?? 0;
        if (total > pr.TimelineItems.Items.Count)
            sb.Append($"<div class=\"event\"><span class=\"ic\">…</span><span>Showing the latest {pr.TimelineItems.Items.Count} of {total} events — <a href=\"{Esc(pr.Url)}\">full history</a></span></div>");
        foreach (var item in pr.TimelineItems.Items)
        {
            if (item.Typename == "PullRequestReview")
            {
                var threads = byReview.TryGetValue(item.Id ?? "", out var l) ? l : new List<ReviewThread>();
                foreach (var t in threads) rendered.Add(t.Id);
                sb.Append(Review(item, threads, item.Author?.Login == authorLogin));
            }
            else
            {
                sb.Append(Common(item, authorLogin, pr.BaseRefName));
            }
        }
        var rest = pr.ReviewThreads.Items.Where(t => !rendered.Contains(t.Id)).ToList();
        if (rest.Count > 0)
            sb.Append("<h3 class=\"sec\">Review threads</h3>").Append(string.Concat(rest.Select(Thread)));
        return Wrap(sb.ToString());
    }

    public static string Render(IssueDetail issue)
    {
        var sb = new StringBuilder();
        sb.Append(CommentBox(issue.Author, issue.CreatedAt, issue.BodyHTML, "description", true));
        foreach (var item in issue.TimelineItems.Items) sb.Append(Common(item, issue.Author?.Login, ""));
        return Wrap(sb.ToString());
    }

    private static string Review(TimelineItem item, List<ReviewThread> threads, bool highlight)
    {
        var hasBody = !string.IsNullOrWhiteSpace(item.BodyHTML);
        var (icon, cls, text) = item.State switch
        {
            "APPROVED" => ("✓", "ok", "approved"),
            "CHANGES_REQUESTED" => ("±", "bad", "requested changes"),
            "DISMISSED" => ("✕", "", "review dismissed"),
            "PENDING" => ("…", "", "pending review"),
            _ => ("💬", "", "reviewed")
        };
        if (!hasBody && threads.Count == 0)
        {
            if (item.State == "COMMENTED") return "";
            return Event(icon, cls, item.Author, text, item.CreatedAt);
        }
        var login = Esc(item.Author?.Login ?? "ghost");
        var bodyHtml = hasBody ? $"<div class=\"md\">{item.BodyHTML}</div>" : "";
        return $"<div class=\"item\">{Avatar(item.Author?.AvatarUrl)}<div class=\"box{(highlight ? " mine" : "")}\">\n<div class=\"head\"><b>{login}</b><span class=\"rv {cls}\">{icon} {text}</span><span>·</span>{Time(item.CreatedAt)}</div>\n{bodyHtml}{string.Concat(threads.Select(Thread))}</div></div>";
    }

    private static string Common(TimelineItem item, string? authorLogin, string @base)
    {
        switch (item.Typename)
        {
            case "IssueComment":
                return CommentBox(item.Author, item.CreatedAt, item.BodyHTML, "commented", item.Author?.Login == authorLogin);
            case "PullRequestCommit":
            {
                if (item.Commit is not { } c) return "";
                var who = c.Author?.User?.Login ?? c.Author?.Name ?? "";
                var actor = new Actor { Login = who, AvatarUrl = c.Author?.AvatarUrl };
                return Event("●", "", actor, $"commit <code>{Esc(c.AbbreviatedOid)}</code> {Esc(c.MessageHeadline)}", c.CommittedDate);
            }
            case "MergedEvent":
            {
                var sha = item.Commit?.AbbreviatedOid is { } a ? $" (<code>{Esc(a)}</code>)" : "";
                return Event("⇢", "merged", item.Actor, $"merged{sha} into <code>{Esc(item.MergeRefName ?? @base)}</code>", item.CreatedAt);
            }
            case "ClosedEvent":
                return Event("✕", "bad", item.Actor, "closed", item.CreatedAt);
            case "ReopenedEvent":
                return Event("↺", "ok", item.Actor, "reopened", item.CreatedAt);
            case "HeadRefForcePushedEvent":
                return Event("⤒", "", item.Actor, "force-pushed the branch", item.CreatedAt);
            case "HeadRefDeletedEvent":
                return Event("⌫", "", item.Actor, "deleted the branch", item.CreatedAt);
            case "ReadyForReviewEvent":
                return Event("◉", "ok", item.Actor, "marked as ready for review", item.CreatedAt);
            case "ConvertToDraftEvent":
                return Event("◌", "", item.Actor, "converted to draft", item.CreatedAt);
            case "ReviewRequestedEvent":
                return Event("👁", "", item.Actor, $"requested review from → <b>{Esc(item.RequestedReviewer?.Display ?? "?")}</b>", item.CreatedAt);
            case "LabeledEvent":
                return item.Label is { } l ? Event("🏷", "", item.Actor, $"label {Label(l)}", item.CreatedAt) : "";
            case "RenamedTitleEvent":
                return Event("✎", "", item.Actor, $"title: <s>{Esc(item.PreviousTitle)}</s> → {Esc(item.CurrentTitle)}", item.CreatedAt);
            case "CrossReferencedEvent":
            {
                if (item.Source is not { Url: { } url } s) return "";
                var name = $"{s.Repository?.NameWithOwner ?? ""}#{s.Number ?? 0}";
                return Event("↗", "", item.Actor, $"mentioned in <a href=\"{Esc(url)}\">{Esc(name)} {Esc(s.Title)}</a>", item.CreatedAt);
            }
            default:
                return "";
        }
    }

    public static string Wrap(string content) => $"<div class=\"tl\">{content}</div>";

    public static string Css => Brand ? BrandVars + BaseCss + BrandOverrides : SystemVars + BaseCss;

    private const string SystemVars = """
    :root{--fg:#1f2328;--muted:#59636e;--border:#d1d9e0;--sub:#f6f8fa;--card:transparent;--link:#0969da;--green:#1a7f37;--red:#d1242f;--purple:#8250df;--amber:#9a6700;--add:#dafbe1;--del:#ffebe9;--hunkbg:rgba(9,105,218,.08);--mine:#ddf4ff;--code:#fff;--raised:#ffffff;--gutter:#8c959f;color-scheme:light dark}
    @media (prefers-color-scheme: dark){:root{--fg:#e6edf3;--muted:#9198a1;--border:#3d444d;--sub:#151b23;--link:#4493f8;--green:#3fb950;--red:#f85149;--purple:#ab7df8;--amber:#d29922;--add:rgba(46,160,67,.18);--del:rgba(248,81,73,.18);--hunkbg:rgba(56,139,253,.12);--mine:rgba(56,139,253,.12);--code:transparent;--raised:#1c2128;--gutter:#6e7681}}

    """;

    private const string BrandVars = """
    :root{--fg:#1a2333;--muted:#5f6b7e;--border:rgba(26,35,51,.09);--sub:#f3f1ec;--card:#ffffff;--link:#1f7fb8;--green:#2e8a57;--red:#c2412f;--purple:#6b55d6;--amber:#b7791f;--add:rgba(46,138,87,.11);--del:rgba(194,65,47,.10);--hunkbg:rgba(31,127,184,.08);--mine:rgba(31,127,184,.07);--code:#f7f5f0;--raised:#ffffff;--gutter:#9aa2b0;color-scheme:light dark}
    @media (prefers-color-scheme: dark){:root{--fg:#e6ebf2;--muted:#8693a8;--border:rgba(160,174,196,.10);--sub:#111a2c;--card:#152036;--link:#5bb8ec;--green:#5dbe8a;--red:#e8746a;--purple:#9d8cf0;--amber:#e8b44c;--add:rgba(93,190,138,.10);--del:rgba(232,116,106,.11);--hunkbg:rgba(91,184,236,.08);--mine:rgba(91,184,236,.07);--code:#0a101c;--raised:#152036;--gutter:#4a5670}}

    """;

    private const string BrandOverrides = """
    .box,.thread,.hunk,.event .ic,.pill,button,.md pre{border:0}
    .head{border-bottom:0}
    .pill{background:var(--sub)}
    button:hover{background:var(--border)}
    .tc{border-bottom:1px solid var(--border)}

    """;

    private const string BaseCss = """
    *{box-sizing:border-box}
    body{font:13px/1.5 "Segoe UI Variable Text","Segoe UI",system-ui,sans-serif;color:var(--fg);background:transparent;margin:0;padding:16px 20px 32px}
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
    code,pre,kbd{font-family:"Cascadia Mono","Cascadia Code",Consolas,monospace;font-size:12px}
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
    .md .blob-num,.md .blob-code{font:12px "Cascadia Mono",Consolas,monospace}
    .empty{color:var(--muted);font-style:italic}
    .event{display:flex;align-items:center;gap:7px;margin:0 0 12px 44px;color:var(--muted);font-size:12px;flex-wrap:wrap}
    .event b{color:var(--fg)}
    .event .ic{width:22px;height:22px;border-radius:50%;display:inline-flex;align-items:center;justify-content:center;background:var(--sub);border:1px solid var(--border);font-size:11px;flex:none;color:var(--fg)}
    .event .ic.ok{background:var(--green);color:#fff;border-color:transparent}.event .ic.bad{background:var(--red);color:#fff;border-color:transparent}.event .ic.merged{background:var(--purple);color:#fff;border-color:transparent}
    .event .t{margin-left:2px}
    .label{display:inline-block;padding:0 7px;border-radius:10px;border:1px solid;font-size:11px;font-weight:500;color:var(--fg)}
    .thread{margin:10px 12px;border:1px solid var(--border);border-radius:9px;overflow:hidden;background:var(--sub)}
    .thread>summary{padding:6px 10px;font:12px "Cascadia Mono",Consolas,monospace;cursor:pointer;display:flex;gap:8px;align-items:center;list-style:none}
    .thread>summary::-webkit-details-marker{display:none}
    .thread .ln{color:var(--muted);margin-left:-6px}
    .pill{font:11px "Segoe UI",sans-serif;padding:0 7px;border-radius:10px;border:1px solid var(--border);color:var(--muted)}.pill.ok{color:var(--green);border-color:var(--green)}
    .hunk{font:11.5px/1.5 "Cascadia Mono",Consolas,monospace;overflow:auto;border-top:1px solid var(--border);border-bottom:1px solid var(--border);background:var(--code)}
    .hunk div{padding:0 10px;white-space:pre}.hunk .a{background:var(--add)}.hunk .d{background:var(--del)}
    .tc{display:flex;gap:8px;padding:8px 10px;border-bottom:1px solid var(--border)}
    .tc .avatar{margin-top:2px}.tcb{min-width:0;flex:1}.tc .meta{color:var(--muted);font-size:12px}.tc .meta b{color:var(--fg)}
    .tc .md{padding:2px 0 0}
    .actions{padding:6px 10px;display:flex;gap:6px;flex-wrap:wrap}
    button{font:12px "Segoe UI",sans-serif;padding:3px 10px;border-radius:7px;border:1px solid var(--border);background:var(--sub);color:var(--fg);cursor:pointer}
    button:hover{border-color:var(--muted)}button.ghost{background:transparent;color:var(--muted)}
    button.primary{background:var(--link);color:#fff;border-color:transparent}
    h3.sec{margin:24px 0 8px 44px;font-size:13px;color:var(--muted)}

    """;
}
