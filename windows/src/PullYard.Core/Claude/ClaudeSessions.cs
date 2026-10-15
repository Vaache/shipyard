using System.Collections.Concurrent;
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using PullYard.Core.Util;

namespace PullYard.Core.Claude;

public sealed class ClaudeSessionInfo
{
    public string Id { get; init; } = "";
    public string File { get; init; } = "";
    public string Cwd { get; init; } = "";
    public string Title { get; init; } = "";
    public string? Branch { get; init; }
    public DateTimeOffset Modified { get; init; }
    public string? PrUrl { get; init; }
    public int? PrNumber { get; init; }
    public string? Model { get; init; }
    public int? LinesAdded { get; init; }
    public int? LinesRemoved { get; init; }

    public string? WorktreeName
    {
        get
        {
            var norm = Cwd.Replace('\\', '/');
            var i = norm.IndexOf("/.claude/worktrees/", StringComparison.OrdinalIgnoreCase);
            return i < 0 ? null : norm[(i + "/.claude/worktrees/".Length)..].Split('/')[0];
        }
    }

    public bool HasDiff => (LinesAdded ?? 0) + (LinesRemoved ?? 0) > 0;
}

public sealed class ClaudeProject
{
    public string Id { get; init; } = "";
    public string Path { get; init; } = "";
    public List<ClaudeSessionInfo> Sessions { get; init; } = new();

    public string Name => System.IO.Path.GetFileName(Path.TrimEnd('\\', '/')) is { Length: > 0 } n ? n : Path;
    public bool Exists => Directory.Exists(Path);
    public DateTimeOffset LastActivity => Sessions.FirstOrDefault()?.Modified ?? DateTimeOffset.MinValue;
}

public static class ClaudeSessionParser
{
    private static readonly ConcurrentDictionary<string, (DateTime, ClaudeSessionInfo?)> Cache = new();

    public static string ProjectRoot(string cwd)
    {
        foreach (var marker in new[] { "\\.claude\\worktrees\\", "/.claude/worktrees/" })
        {
            var i = cwd.IndexOf(marker, StringComparison.OrdinalIgnoreCase);
            if (i >= 0) return cwd[..i];
        }
        return cwd;
    }

    public static List<ClaudeProject> LoadAll(string root)
    {
        if (!Directory.Exists(root)) return new();
        var byProject = new Dictionary<string, List<ClaudeSessionInfo>>(StringComparer.OrdinalIgnoreCase);
        string[] dirs;
        try
        {
            dirs = Directory.GetDirectories(root);
        }
        catch
        {
            return new();
        }
        foreach (var dir in dirs)
        {
            string[] files;
            try
            {
                files = Directory.GetFiles(dir, "*.jsonl");
            }
            catch
            {
                continue;
            }
            foreach (var file in files)
            {
                if (Parse(file) is not { } info) continue;
                var key = ProjectRoot(info.Cwd);
                if (!byProject.TryGetValue(key, out var list)) byProject[key] = list = new();
                list.Add(info);
            }
        }
        return byProject
            .Select(kv => new ClaudeProject { Id = kv.Key, Path = kv.Key, Sessions = kv.Value.OrderByDescending(s => s.Modified).ToList() })
            .OrderByDescending(p => p.LastActivity)
            .ToList();
    }

    public static ClaudeSessionInfo? Parse(string file)
    {
        DateTime mtime;
        try
        {
            mtime = System.IO.File.GetLastWriteTimeUtc(file);
        }
        catch
        {
            return null;
        }
        if (Cache.TryGetValue(file, out var hit) && hit.Item1 == mtime) return hit.Item2;
        var info = ParseUncached(file, mtime);
        Cache[file] = (mtime, info);
        return info;
    }

    private static List<JsonObject> Lines(byte[] data, bool dropFirst, bool dropLast)
    {
        var parts = Encoding.UTF8.GetString(data).Split('\n', StringSplitOptions.RemoveEmptyEntries).ToList();
        if (dropFirst && parts.Count > 0) parts.RemoveAt(0);
        if (dropLast && parts.Count > 0) parts.RemoveAt(parts.Count - 1);
        var result = new List<JsonObject>(parts.Count);
        foreach (var p in parts)
        {
            try
            {
                if (JsonNode.Parse(p) is JsonObject o) result.Add(o);
            }
            catch
            {
            }
        }
        return result;
    }

    private static string? TextOf(JsonNode? message)
    {
        if (message is not JsonObject m) return null;
        if (m["content"] is JsonValue v && v.TryGetValue<string>(out var s)) return s;
        if (m["content"] is JsonArray blocks)
            return blocks.Objects().Where(b => b.Str("type") == "text").Select(b => b.Str("text")).FirstOrDefault(t => t is not null);
        return null;
    }

    private static readonly Regex TagRx = new("<[^>]+>", RegexOptions.Compiled);
    private static readonly Regex SpaceRx = new(@"\s+", RegexOptions.Compiled);

    public static string? CleanPrompt(string raw)
    {
        var s = raw.Trim();
        if (s.StartsWith("<command-name>") || s.StartsWith("<local-command") || s.StartsWith("Caveat:") || s.StartsWith("[Request interrupted")) return null;
        if (s.StartsWith('<')) s = TagRx.Replace(s, " ").Trim();
        s = SpaceRx.Replace(s, " ");
        return s.Length == 0 ? null : (s.Length > 160 ? s[..160] : s);
    }

    private static ClaudeSessionInfo? ParseUncached(string file, DateTime mtime)
    {
        byte[] head;
        byte[] tail = Array.Empty<byte>();
        long size;
        try
        {
            using var fs = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            size = fs.Length;
            var headSize = (int)Math.Min(size, 96 * 1024);
            head = new byte[headSize];
            fs.ReadExactly(head, 0, headSize);
            var tailSize = (int)Math.Min(size, 160 * 1024);
            if (size > headSize)
            {
                fs.Seek(size - tailSize, SeekOrigin.Begin);
                tail = new byte[tailSize];
                fs.ReadExactly(tail, 0, tailSize);
            }
        }
        catch
        {
            return null;
        }
        string? cwd = null, branch = null, firstPrompt = null;
        foreach (var obj in Lines(head, false, size > head.Length))
        {
            cwd ??= obj.Str("cwd");
            if (branch is null && obj.Str("gitBranch") is { Length: > 0 } b && b != "HEAD") branch = b;
            if (firstPrompt is null && obj.Str("type") == "user" && obj.Bool("isMeta") != true && obj.Bool("isSidechain") != true &&
                TextOf(obj["message"]) is { } t && CleanPrompt(t) is { } clean)
                firstPrompt = clean;
            if (cwd is not null && branch is not null && firstPrompt is not null) break;
        }
        if (cwd is null || (firstPrompt is null && size <= 2048)) return null;
        string? title = null, lastPrompt = null, prUrl = null, model = null;
        int? prNumber = null, added = null, removed = null;
        var tailObjects = tail.Length == 0 ? Lines(head, false, false) : Lines(tail, true, false);
        foreach (var obj in tailObjects)
        {
            switch (obj.Str("type"))
            {
                case "custom-title":
                    title = obj.Str("customTitle") ?? title;
                    break;
                case "last-prompt":
                    lastPrompt = obj.Str("lastPrompt") ?? lastPrompt;
                    break;
                case "pr-link":
                    prUrl = obj.Str("prUrl") ?? prUrl;
                    prNumber = obj.Int("prNumber") ?? prNumber;
                    break;
                case "assistant":
                    if (obj.Obj("message").Str("model") is { } m && !m.StartsWith('<')) model = m;
                    if (obj.Str("gitBranch") is { Length: > 0 } gb && gb != "HEAD") branch = gb;
                    break;
                case "cost-state":
                    added = obj.Int("totalLinesAdded") ?? added;
                    removed = obj.Int("totalLinesRemoved") ?? removed;
                    break;
            }
        }
        var display = !string.IsNullOrEmpty(title) ? title : firstPrompt ?? (lastPrompt is not null ? CleanPrompt(lastPrompt) : null) ?? "Untitled";
        return new ClaudeSessionInfo
        {
            Id = System.IO.Path.GetFileNameWithoutExtension(file),
            File = file,
            Cwd = cwd,
            Title = display,
            Branch = branch,
            Modified = new DateTimeOffset(mtime, TimeSpan.Zero),
            PrUrl = prUrl,
            PrNumber = prNumber,
            Model = model,
            LinesAdded = added,
            LinesRemoved = removed
        };
    }
}
