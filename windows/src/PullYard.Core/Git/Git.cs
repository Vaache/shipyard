using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.Core.Git;

public sealed record GitCommitLine(string Id, string Short, string Subject);

public static class GitCli
{
    private static string? binary;

    public static string Binary
    {
        get
        {
            if (binary is not null) return binary;
            var candidates = new[]
            {
                Proc.FindOnPath("git"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Git", "cmd", "git.exe"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "Git", "cmd", "git.exe")
            };
            binary = candidates.FirstOrDefault(c => c is not null && File.Exists(c)) ?? "git";
            return binary;
        }
    }

    public static Task<Proc.Result> Run(IEnumerable<string> args, string dir, CancellationToken ct = default) =>
        Proc.Run(Binary, new[] { "-C", dir, "-c", "core.quotepath=off", "--no-pager" }.Concat(args), env: new Dictionary<string, string?>
        {
            ["GIT_OPTIONAL_LOCKS"] = "0",
            ["LC_ALL"] = "C",
            ["GIT_TERMINAL_PROMPT"] = "0"
        }, timeout: TimeSpan.FromSeconds(30), ct: ct);

    public static async Task<RepoRef?> RemoteRepo(string dir)
    {
        var url = (await Run(new[] { "remote", "get-url", "origin" }, dir).ConfigureAwait(false)).Out.Trim();
        return ParseRemote(url);
    }

    public static RepoRef? ParseRemote(string url)
    {
        var i = url.IndexOf("github.com", StringComparison.OrdinalIgnoreCase);
        if (i < 0) return null;
        var tail = url[(i + "github.com".Length)..].Trim(':', '/');
        if (tail.EndsWith(".git", StringComparison.OrdinalIgnoreCase)) tail = tail[..^4];
        return RepoRef.Parse(tail);
    }

    public static List<PRFile> ParseUnifiedDiff(string text)
    {
        var files = new List<PRFile>();
        string? path = null, oldPath = null;
        var status = "modified";
        var patch = new List<string>();
        int adds = 0, dels = 0;
        var binaryFile = false;

        void Flush()
        {
            if (path is null) return;
            var body = string.Join("\n", patch);
            files.Add(new PRFile
            {
                Filename = path,
                Status = status,
                Additions = adds,
                Deletions = dels,
                Patch = binaryFile || body.Length == 0 ? null : body,
                PreviousFilename = oldPath != path ? oldPath : null
            });
        }

        foreach (var raw in text.Split('\n'))
        {
            var line = raw.TrimEnd('\r');
            if (line.StartsWith("diff --git ", StringComparison.Ordinal))
            {
                Flush();
                path = null;
                oldPath = null;
                status = "modified";
                patch = new List<string>();
                adds = 0;
                dels = 0;
                binaryFile = false;
                var parts = line["diff --git ".Length..].Split(" b/");
                if (parts.Length >= 2)
                {
                    oldPath = parts[0].Length > 2 ? parts[0][2..] : parts[0];
                    path = string.Join(" b/", parts.Skip(1));
                }
                continue;
            }
            if (patch.Count == 0)
            {
                if (line.StartsWith("new file")) { status = "added"; continue; }
                if (line.StartsWith("deleted file")) { status = "removed"; continue; }
                if (line.StartsWith("rename from ")) { status = "renamed"; oldPath = line["rename from ".Length..]; continue; }
                if (line.StartsWith("rename to ")) { path = line["rename to ".Length..]; continue; }
                if (line.StartsWith("Binary files")) { binaryFile = true; continue; }
                if (line.StartsWith("index ") || line.StartsWith("--- ") || line.StartsWith("+++ ") || line.StartsWith("similarity") || line.StartsWith("old mode") || line.StartsWith("new mode")) continue;
            }
            if (path is null) continue;
            if (line.StartsWith("@@") || patch.Count > 0)
            {
                patch.Add(line);
                if (line.StartsWith('+')) adds++;
                else if (line.StartsWith('-')) dels++;
            }
        }
        Flush();
        return files;
    }
}
