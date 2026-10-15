namespace Shipyard.Core.Api;

public enum DiffLineKind { Hunk, Context, Add, Del, Meta }

public sealed class DiffLine
{
    public int Id { get; init; }
    public DiffLineKind Kind { get; init; }
    public string Text { get; init; } = "";
    public int? OldLine { get; init; }
    public int? NewLine { get; init; }

    public string AnchorSide => Kind == DiffLineKind.Del ? "LEFT" : "RIGHT";
    public int? AnchorLine => Kind == DiffLineKind.Del ? OldLine : NewLine;
    public bool Commentable => Kind is DiffLineKind.Add or DiffLineKind.Del or DiffLineKind.Context;
}

public sealed class DiffFile
{
    public string Id { get; }
    public string? PreviousFilename { get; }
    public string Status { get; }
    public int Additions { get; }
    public int Deletions { get; }
    public IReadOnlyList<DiffLine> Lines { get; }
    public bool HasPatch { get; }

    public DiffFile(PRFile f)
    {
        Id = f.Filename;
        PreviousFilename = f.PreviousFilename;
        Status = f.Status;
        Additions = f.Additions;
        Deletions = f.Deletions;
        HasPatch = f.Patch is not null;
        Lines = f.Patch is { } p ? DiffParser.Parse(p) : Array.Empty<DiffLine>();
    }

    public string StatusLetter => Status switch
    {
        "added" => "A",
        "removed" => "D",
        "renamed" => "R",
        "copied" => "C",
        _ => "M"
    };

    public string FileName => Id.Contains('/') ? Id[(Id.LastIndexOf('/') + 1)..] : Id;
    public string Directory => Id.Contains('/') ? Id[..Id.LastIndexOf('/')] : "";
}

public sealed record DiffAnchor(string Path, int Line, string Side);

public static class DiffParser
{
    public static List<DiffLine> Parse(string patch)
    {
        var result = new List<DiffLine>(patch.Length / 40 + 1);
        int old = 0, @new = 0, idx = 0;
        foreach (var rawLine in SplitLines(patch))
        {
            var raw = rawLine;
            if (raw.StartsWith("@@", StringComparison.Ordinal))
            {
                var (o, n) = HunkStart(raw);
                old = o;
                @new = n;
                result.Add(new DiffLine { Id = idx++, Kind = DiffLineKind.Hunk, Text = raw });
                continue;
            }
            var body = raw.Length == 0 ? "" : raw[1..];
            var first = raw.Length == 0 ? '\0' : raw[0];
            switch (first)
            {
                case '+':
                    result.Add(new DiffLine { Id = idx++, Kind = DiffLineKind.Add, Text = body, NewLine = @new });
                    @new++;
                    break;
                case '-':
                    result.Add(new DiffLine { Id = idx++, Kind = DiffLineKind.Del, Text = body, OldLine = old });
                    old++;
                    break;
                case '\\':
                    result.Add(new DiffLine { Id = idx++, Kind = DiffLineKind.Meta, Text = raw });
                    break;
                default:
                    result.Add(new DiffLine { Id = idx++, Kind = DiffLineKind.Context, Text = body, OldLine = old, NewLine = @new });
                    old++;
                    @new++;
                    break;
            }
        }
        return result;
    }

    private static IEnumerable<string> SplitLines(string s)
    {
        var start = 0;
        for (var i = 0; i < s.Length; i++)
        {
            if (s[i] != '\n') continue;
            var end = i > start && s[i - 1] == '\r' ? i - 1 : i;
            yield return s[start..end];
            start = i + 1;
        }
        if (start < s.Length) yield return s[start..].TrimEnd('\r');
    }

    private static (int, int) HunkStart(string header)
    {
        var parts = header.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        static int Start(string? token)
        {
            if (token is null || token.Length < 2) return 1;
            var digits = token[1..].Split(',')[0];
            return int.TryParse(digits, out var v) ? v : 1;
        }
        return (Start(parts.FirstOrDefault(p => p.StartsWith('-'))), Start(parts.FirstOrDefault(p => p.StartsWith('+'))));
    }
}
