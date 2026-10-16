using System.Text;
using System.Text.RegularExpressions;
using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.Core.Html;

public static class CodeHtml
{
    private static readonly HashSet<string> Keywords = new()
    {
        "func", "let", "var", "if", "else", "for", "while", "return", "class", "struct", "enum", "protocol", "extension", "import", "guard", "switch", "case", "default",
        "break", "continue", "in", "is", "as", "try", "catch", "throw", "throws", "async", "await", "static", "private", "public", "internal", "final", "override", "init",
        "self", "super", "nil", "true", "false", "null", "undefined", "new", "this", "const", "function", "export", "from", "interface", "type", "implements", "extends",
        "abstract", "void", "int", "double", "bool", "String", "late", "required", "factory", "get", "set", "with", "yield", "def", "elif", "pass", "lambda", "None", "True",
        "False", "and", "or", "not", "package", "fun", "val", "object", "when", "data", "sealed", "go", "chan", "defer", "map", "range", "fn", "mut", "pub", "impl", "trait",
        "use", "mod", "match", "where", "some", "any", "do", "repeat", "typealias", "mixin", "dynamic", "of", "delete", "typeof", "instanceof", "unsafe", "fileprivate",
        "open", "inout", "echo", "then", "fi", "esac", "done", "local", "namespace", "using", "readonly", "virtual", "sealed", "record", "partial", "params", "out", "ref",
        "foreach", "string", "object", "decimal", "long", "short", "byte", "char", "float", "unsigned", "signed", "template", "typename", "auto"
    };

    private static readonly HashSet<string> HashComment = new() { "py", "sh", "bash", "zsh", "rb", "yaml", "yml", "toml", "mk", "makefile", "dockerfile", "r", "pl", "conf", "cfg", "ini", "gitignore", "env", "ps1", "psm1" };
    private static readonly HashSet<string> SlashComment = new() { "swift", "dart", "js", "jsx", "ts", "tsx", "kt", "kts", "java", "go", "rs", "c", "h", "cc", "cpp", "hpp", "m", "mm", "cs", "scala", "groovy", "gradle", "php", "css", "scss", "less", "json5", "proto", "fs", "xaml" };
    private static readonly HashSet<string> MarkupComment = new() { "html", "xml", "vue", "svelte", "xaml", "csproj", "props", "targets", "resw" };

    private static readonly Regex StringRx = new("\"(?:\\\\.|[^\"\\\\\\n])*\"|'(?:\\\\.|[^'\\\\\\n])*'|`(?:\\\\.|[^`\\\\])*`", RegexOptions.Compiled);
    private static readonly Regex NumberRx = new(@"\b(?:0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\b", RegexOptions.Compiled);
    private static readonly Regex WordRx = new(@"\b[A-Za-z_][A-Za-z0-9_]*\b", RegexOptions.Compiled);
    private static readonly Regex SlashCommentRx = new(@"//[^\n]*|/\*[\s\S]*?\*/", RegexOptions.Compiled);
    private static readonly Regex HashCommentRx = new(@"(?m)(?:^|\s)#[^\n]*", RegexOptions.Compiled);
    private static readonly Regex MarkupCommentRx = new(@"<!--[\s\S]*?-->", RegexOptions.Compiled);

    private enum Tok : byte { None, Keyword, Number, String, Comment }

    public static string Highlight(string text, string ext)
    {
        var e = ext.ToLowerInvariant();
        var code = SlashComment.Contains(e) || HashComment.Contains(e) || MarkupComment.Contains(e) || e is "lua" or "sql";
        if (!code || text.Length > 400_000) return ConversationHtml.Esc(text);
        var marks = new Tok[text.Length];
        void Paint(Regex rx, Tok tok, Func<Match, bool>? filter = null)
        {
            foreach (Match m in rx.Matches(text))
            {
                if (filter is not null && !filter(m)) continue;
                for (var i = m.Index; i < m.Index + m.Length; i++) marks[i] = tok;
            }
        }
        Paint(WordRx, Tok.Keyword, m => Keywords.Contains(m.Value));
        Paint(NumberRx, Tok.Number);
        Paint(StringRx, Tok.String);
        var commentRx = SlashComment.Contains(e) ? SlashCommentRx : HashComment.Contains(e) ? HashCommentRx : MarkupComment.Contains(e) ? MarkupCommentRx : null;
        if (commentRx is not null) Paint(commentRx, Tok.Comment);
        var sb = new StringBuilder(text.Length * 2);
        var current = Tok.None;
        for (var i = 0; i < text.Length; i++)
        {
            var t = text[i] == '\n' ? Tok.None : marks[i];
            if (t != current)
            {
                if (current != Tok.None) sb.Append("</span>");
                if (t != Tok.None) sb.Append(t switch { Tok.Keyword => "<span class=\"k\">", Tok.Number => "<span class=\"n\">", Tok.String => "<span class=\"s\">", _ => "<span class=\"cm\">" });
                current = t;
            }
            var ch = text[i];
            switch (ch)
            {
                case '&': sb.Append("&amp;"); break;
                case '<': sb.Append("&lt;"); break;
                case '>': sb.Append("&gt;"); break;
                case '"': sb.Append("&quot;"); break;
                case '\r': break;
                default: sb.Append(ch); break;
            }
        }
        if (current != Tok.None) sb.Append("</span>");
        return sb.ToString();
    }

    public static string Render(string text, string ext, bool wrap, IReadOnlyList<BlameRange>? blame = null, DateTimeOffset? now = null)
    {
        var highlighted = Highlight(text, ext);
        var lines = highlighted.Split('\n');
        var count = lines.Length;
        if (count > 1 && lines[^1].Length == 0) count--;
        var sb = new StringBuilder(highlighted.Length + count * 40 + (blame?.Count ?? 0) * 260);
        sb.Append($"<table class=\"code{(wrap ? " wrap" : "")}{(blame is not null ? " blame" : "")}\"><tbody>");
        var at = 0;
        var covered = 0;
        var clock = now ?? DateTimeOffset.Now;
        for (var i = 0; i < count; i++)
        {
            sb.Append("<tr>");
            if (blame is not null)
            {
                var line = i + 1;
                while (at < blame.Count && blame[at].End < line) at++;
                if (at < blame.Count && blame[at].Start == line)
                {
                    var r = blame[at];
                    var span = Math.Min(r.End, count) - line + 1;
                    covered = line + span - 1;
                    sb.Append(BlameCell(r, span, clock));
                }
                else if (line > covered)
                {
                    sb.Append("<td class=\"bl\"></td>");
                }
            }
            sb.Append($"<td class=\"ln\">{i + 1}</td><td class=\"src\">{(lines[i].Length == 0 ? " " : lines[i])}</td></tr>");
        }
        sb.Append("</tbody></table>");
        return sb.ToString();
    }

    private static string BlameCell(BlameRange r, int span, DateTimeOffset now)
    {
        var when = r.Date is { } d ? Fmt.Rel(d, now) : "";
        var tip = $"{r.ShortSha} · {r.Message}\n{r.Author}{(r.Date is { } full ? " · " + Fmt.Full(full) : "")}";
        var rows = span > 1 ? $" rowspan=\"{span}\"" : "";
        var href = r.Url.Length > 0 ? r.Url : "";
        return $"<td class=\"bl h{r.Heat}\"{rows}><a class=\"ba\" href=\"{ConversationHtml.Esc(href)}\" title=\"{ConversationHtml.Esc(tip)}\"><span class=\"who\">{ConversationHtml.Esc(r.Author)}</span><span class=\"when\">{ConversationHtml.Esc(when)}</span></a></td>";
    }

    public const string Css = """
    body.code-body{padding:0}
    table.code{border-collapse:collapse;font:12px/1.55 "Cascadia Mono","Cascadia Code",Consolas,monospace;min-width:100%}
    table.code td{padding:0;vertical-align:top}
    table.code td.ln{color:var(--gutter);text-align:right;padding:0 10px 0 12px;user-select:none;border-right:1px solid var(--border);position:sticky;left:0;background:var(--code)}
    table.code td.src{white-space:pre;padding-left:10px;color:var(--fg)}
    table.code.wrap td.src{white-space:pre-wrap;word-break:break-all}
    table.code td.bl{position:sticky;left:0;z-index:1;width:200px;min-width:200px;max-width:200px;padding:0;background:var(--code);background-image:linear-gradient(var(--tint,transparent),var(--tint,transparent));border-right:1px solid var(--border);box-shadow:inset 3px 0 0 var(--heat,transparent),inset 0 1px 0 var(--border);font:11px/18.6px "Segoe UI Variable Text","Segoe UI",sans-serif}
    table.code.blame td.ln{left:201px}
    table.code tr:first-child td.bl{box-shadow:inset 3px 0 0 var(--heat,transparent)}
    a.ba{position:sticky;top:0;display:flex;gap:8px;padding:0 8px 0 10px;color:var(--muted);text-decoration:none}
    a.ba:hover{color:var(--fg);text-decoration:none}a.ba:hover .who{text-decoration:underline}
    a.ba .who{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
    a.ba .when{flex:none;color:var(--gutter)}
    td.bl.h0{--heat:color-mix(in srgb,var(--link) 8%,transparent)}td.bl.h1{--heat:color-mix(in srgb,var(--link) 16%,transparent)}
    td.bl.h2{--heat:color-mix(in srgb,var(--link) 24%,transparent)}td.bl.h3{--heat:color-mix(in srgb,var(--link) 34%,transparent)}
    td.bl.h4{--heat:color-mix(in srgb,var(--link) 44%,transparent)}td.bl.h5{--heat:color-mix(in srgb,var(--link) 54%,transparent)}
    td.bl.h6{--heat:color-mix(in srgb,var(--link) 64%,transparent)}td.bl.h7{--heat:color-mix(in srgb,var(--link) 76%,transparent);--tint:color-mix(in srgb,var(--link) 3%,transparent)}
    td.bl.h8{--heat:color-mix(in srgb,var(--link) 88%,transparent);--tint:color-mix(in srgb,var(--link) 5%,transparent)}td.bl.h9{--heat:var(--link);--tint:color-mix(in srgb,var(--link) 8%,transparent)}
    td.bl.h8 a.ba,td.bl.h9 a.ba{color:var(--fg)}
    .k{color:var(--k)}.n{color:var(--purple)}.s{color:var(--green)}.cm{color:var(--gutter);font-style:italic}
    :root{--k:#d8513f}@media (prefers-color-scheme: dark){:root{--k:#ee6f5f}}
    """;
}
