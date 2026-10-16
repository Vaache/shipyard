using System.Text;
using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.Core.Html;

public sealed class DiffRenderOptions
{
    public bool Commentable { get; init; }
    public bool ShowViewed { get; init; }
    public bool ShowOpen { get; init; }
    public bool ShowBlame { get; init; }
    public string BlameLabel { get; init; } = "Blame";
    public string BlameTitle { get; init; } = "Blame this file at the pull request head";
    public IReadOnlySet<string> Viewed { get; init; } = new HashSet<string>();
    public IReadOnlySet<string> Collapsed { get; init; } = new HashSet<string>();
    public IReadOnlyDictionary<string, List<ReviewThread>> Threads { get; init; } = new Dictionary<string, List<ReviewThread>>();
    public IReadOnlyDictionary<string, List<ReviewThread>> Outdated { get; init; } = new Dictionary<string, List<ReviewThread>>();
    public IReadOnlyList<PendingComment> Pending { get; init; } = Array.Empty<PendingComment>();
    public string Filter { get; init; } = "";
}

public static class DiffHtml
{
    public static string Esc(string? s) => ConversationHtml.Esc(s);

    public static string StatusColor(string status) => status switch
    {
        "added" => "var(--green)",
        "removed" => "var(--red)",
        "renamed" => "var(--purple)",
        _ => "var(--amber)"
    };

    public static string Render(IReadOnlyList<DiffFile> files, DiffRenderOptions o)
    {
        var sb = new StringBuilder();
        sb.Append("<div class=\"diff\">");
        var visible = files.Where(f => o.Filter.Length == 0 || f.Id.Contains(o.Filter, StringComparison.OrdinalIgnoreCase)).ToList();
        if (visible.Count == 0) sb.Append("<p class=\"empty\">No files</p>");
        foreach (var f in visible)
        {
            var collapsed = o.Collapsed.Contains(f.Id);
            sb.Append($"<section class=\"file{(collapsed ? " collapsed" : "")}\" data-path=\"{Esc(f.Id)}\">");
            sb.Append(Header(f, o, collapsed));
            sb.Append("<div class=\"fbody\">");
            if (!collapsed) sb.Append(Body(f, o));
            sb.Append("</div></section>");
        }
        sb.Append("</div>");
        return sb.ToString();
    }

    private static string Header(DiffFile f, DiffRenderOptions o, bool collapsed)
    {
        var path = Esc(f.Id);
        var name = f.PreviousFilename is { } prev ? $"{Esc(prev)} → {path}" : path;
        var sb = new StringBuilder();
        sb.Append("<div class=\"fh\">");
        sb.Append($"<button class=\"chev\" onclick=\"toggleFile(this)\" title=\"Collapse or expand\">{(collapsed ? "▸" : "▾")}</button>");
        sb.Append($"<span class=\"st\" style=\"color:{StatusColor(f.Status)}\">{f.StatusLetter}</span>");
        sb.Append($"<span class=\"fp\" title=\"{path}\">{name}</span>");
        sb.Append($"<span class=\"stat\"><span class=\"plus\">+{f.Additions}</span> <span class=\"minus\">−{f.Deletions}</span></span>");
        sb.Append("<span class=\"sp\"></span>");
        if (o.ShowViewed)
        {
            var on = o.Viewed.Contains(f.Id) ? " checked" : "";
            sb.Append($"<label class=\"viewed\"><input type=\"checkbox\"{on} onchange=\"postFile(this,'viewed')\">Viewed</label>");
        }
        if (o.ShowOpen) sb.Append($"<button class=\"ghost\" onclick=\"postFile(this,'open')\">Open</button>");
        if (o.ShowBlame && f.Status != "removed") sb.Append($"<button class=\"ghost\" onclick=\"postFile(this,'blame')\" title=\"{Esc(o.BlameTitle)}\">{Esc(o.BlameLabel)}</button>");
        sb.Append($"<button class=\"ghost\" onclick=\"postFile(this,'copy')\" title=\"Copy path\">⧉</button>");
        sb.Append("</div>");
        return sb.ToString();
    }

    public static string Body(DiffFile f, DiffRenderOptions o)
    {
        if (!f.HasPatch) return "<div class=\"note\">Diff unavailable (binary or too large)</div>";
        var sb = new StringBuilder(f.Lines.Count * 120);
        sb.Append("<table class=\"dt\"><tbody>");
        var path = Esc(f.Id);
        var filePending = o.Pending.Where(p => p.Path == f.Id).ToList();
        foreach (var line in f.Lines)
        {
            sb.Append(LineRow(line, path, o.Commentable));
            if (!line.Commentable) continue;
            var keys = new List<string>(2);
            if (line.NewLine is { } n && line.Kind != DiffLineKind.Del) keys.Add($"{f.Id}|RIGHT|{n}");
            if (line.OldLine is { } old && line.Kind != DiffLineKind.Add) keys.Add($"{f.Id}|LEFT|{old}");
            foreach (var k in keys)
                if (o.Threads.TryGetValue(k, out var threads))
                    foreach (var t in threads) sb.Append($"<tr class=\"ins\"><td colspan=\"3\">{ConversationHtml.InlineThread(t)}</td></tr>");
            if (line.AnchorLine is { } anchor)
                foreach (var p in filePending.Where(p => p.Line == anchor && p.Side == line.AnchorSide))
                    sb.Append($"<tr class=\"ins\"><td colspan=\"3\"><div class=\"pending\"><span class=\"tag\">pending</span><div class=\"pbody\">{Esc(p.Body)}</div><button class=\"ghost\" onclick=\"post('deletePending','{p.Id}')\" title=\"Delete pending comment\">✕</button></div></td></tr>");
        }
        sb.Append("</tbody></table>");
        if (o.Outdated.TryGetValue(f.Id, out var outdated) && outdated.Count > 0)
        {
            sb.Append("<div class=\"note\">Outdated comments</div>");
            foreach (var t in outdated) sb.Append(ConversationHtml.InlineThread(t));
        }
        return sb.ToString();
    }

    private static string LineRow(DiffLine line, string path, bool commentable)
    {
        if (line.Kind == DiffLineKind.Hunk)
            return $"<tr class=\"hk\"><td class=\"g\"></td><td class=\"g\"></td><td class=\"c\">{Esc(line.Text)}</td></tr>";
        var cls = line.Kind switch
        {
            DiffLineKind.Add => "a",
            DiffLineKind.Del => "d",
            DiffLineKind.Meta => "m",
            _ => ""
        };
        var sign = line.Kind switch
        {
            DiffLineKind.Add => "+",
            DiffLineKind.Del => "-",
            DiffLineKind.Context => " ",
            _ => ""
        };
        var oldN = line.OldLine?.ToString() ?? "";
        var newN = line.NewLine?.ToString() ?? "";
        var canComment = commentable && line.Commentable && line.AnchorLine is not null;
        var attr = canComment ? $" data-c=\"{path}|{line.AnchorSide}|{line.AnchorLine}\" title=\"Comment on this line\"" : "";
        var gutterClass = canComment ? "g cm" : "g";
        return $"<tr class=\"{cls}\"><td class=\"{gutterClass}\"{attr}>{oldN}</td><td class=\"{gutterClass}\"{attr}>{newN}</td><td class=\"c\">{Esc(sign + line.Text)}</td></tr>";
    }

    public const string Css = """
    .diff{font-size:12px}
    .file{margin:0 0 12px;border:1px solid var(--border);border-radius:10px;overflow:hidden;background:var(--card);content-visibility:auto;contain-intrinsic-size:auto 400px}
    .fh{position:sticky;top:0;z-index:2;display:flex;align-items:center;gap:8px;padding:6px 10px;background:var(--raised);border-bottom:1px solid var(--border)}
    .file.collapsed .fh{border-bottom:0}
    .fh .chev{border:0;background:transparent;padding:0 4px;color:var(--muted);font-size:12px}
    .fh .st{font:700 11px "Cascadia Mono",Consolas,monospace;width:12px}
    .fh .fp{font:600 12.5px "Cascadia Mono",Consolas,monospace;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;direction:rtl;text-align:left;min-width:0}
    .fh .stat{font-size:12px;white-space:nowrap}.plus{color:var(--green)}.minus{color:var(--red)}
    .fh .sp{flex:1}
    .fh .viewed{display:flex;align-items:center;gap:4px;color:var(--muted);font-size:12px;cursor:pointer;white-space:nowrap}
    .dt{border-collapse:collapse;width:100%;font:12px/1.55 "Cascadia Mono","Cascadia Code",Consolas,monospace;table-layout:fixed}
    .dt td{padding:0;vertical-align:top}
    .dt td.g{width:50px;min-width:50px;text-align:right;padding:0 8px 0 0;color:var(--gutter);user-select:none;white-space:nowrap}
    .dt td.g.cm{cursor:pointer}
    .dt tr:hover td.g.cm{color:var(--link);background:var(--hunkbg)}
    .dt td.c{white-space:pre-wrap;word-break:break-all;padding-left:8px;color:var(--fg)}
    .dt tr.a td{background:var(--add)}.dt tr.d td{background:var(--del)}
    .dt tr.m td.c{color:var(--muted)}
    .dt tr.hk td{background:var(--hunkbg);color:var(--muted);padding-top:3px;padding-bottom:3px}
    .dt tr.ins td{background:transparent;padding:4px 12px 4px 108px;font-family:"Segoe UI",sans-serif;white-space:normal}
    .thread.inline{margin:4px 0;max-width:900px;background:var(--raised)}
    .pending{display:flex;align-items:flex-start;gap:8px;padding:8px 10px;border:1px solid var(--amber);border-radius:8px;max-width:900px;font:13px "Segoe UI",sans-serif}
    .pending .tag{font-size:11px;font-weight:600;padding:0 6px;border-radius:8px;background:rgba(232,180,76,.25)}
    .pending .pbody{flex:1;white-space:pre-wrap}
    .note{padding:8px 12px;color:var(--muted);font:12px "Segoe UI",sans-serif}
    """;
}
