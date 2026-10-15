using Shipyard.Core.Api;
using Shipyard.Core.Util;

namespace Shipyard.Core.Git;

public static class LocalRepoStore
{
    private static Dictionary<string, string> Map() => Settings.Shared.GetObject<Dictionary<string, string>>(Keys.LocalRepoPaths) ?? new();

    public static string? Saved(RepoRef repo)
    {
        if (!Map().TryGetValue(repo.Full.ToLowerInvariant(), out var path)) return null;
        return Directory.Exists(Path.Combine(path, ".git")) || File.Exists(Path.Combine(path, ".git")) ? path : null;
    }

    public static void Save(RepoRef repo, string path)
    {
        var map = Map();
        map[repo.Full.ToLowerInvariant()] = path;
        Settings.Shared.Set(Keys.LocalRepoPaths, map);
    }

    public static bool RemoteMatches(string path, RepoRef repo)
    {
        string config;
        try
        {
            var gitDir = Path.Combine(path, ".git");
            if (File.Exists(gitDir))
            {
                var pointer = File.ReadAllText(gitDir).Trim();
                if (pointer.StartsWith("gitdir:", StringComparison.OrdinalIgnoreCase))
                {
                    var target = pointer["gitdir:".Length..].Trim();
                    var full = Path.IsPathRooted(target) ? target : Path.GetFullPath(Path.Combine(path, target));
                    var common = Path.Combine(full, "commondir");
                    if (File.Exists(common)) full = Path.GetFullPath(Path.Combine(full, File.ReadAllText(common).Trim()));
                    gitDir = full;
                }
            }
            config = File.ReadAllText(Path.Combine(gitDir, "config"));
        }
        catch
        {
            return false;
        }
        var wanted = repo.Full.ToLowerInvariant();
        foreach (var raw in config.ToLowerInvariant().Split('\n'))
        {
            if (!raw.Contains("url") || !raw.Contains("github.com")) continue;
            var eq = raw.IndexOf('=');
            var s = (eq >= 0 ? raw[(eq + 1)..] : raw).Trim();
            if (s.EndsWith(".git")) s = s[..^4];
            if (s.EndsWith("/" + wanted) || s.EndsWith(":" + wanted)) return true;
        }
        return false;
    }

    public static async Task<string?> Detect(RepoRef repo)
    {
        if (Saved(repo) is { } s) return s;
        var found = await Task.Run(() =>
        {
            var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
            var docs = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments);
            var roots = new List<(string, int)>();
            foreach (var r in new[] { "source", "source\\repos", "repos", "Projects", "projects", "Code", "code", "src", "dev", "Developer", "work", "GitHub", "git", "Desktop" })
                roots.Add((Path.Combine(home, r), 3));
            if (!string.IsNullOrEmpty(docs))
            {
                roots.Add((Path.Combine(docs, "GitHub"), 2));
                roots.Add((docs, 3));
            }
            foreach (var drive in new[] { "C:\\dev", "C:\\src", "C:\\code", "C:\\repos", "D:\\dev", "D:\\src", "D:\\code", "D:\\repos" })
                roots.Add((drive, 2));
            roots.Add((home, 1));
            var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
            foreach (var (root, depth) in roots)
                if (Scan(root, repo, depth, seen) is { } hit) return hit;
            return null;
        });
        if (found is not null) Save(repo, found);
        return found;
    }

    private static readonly HashSet<string> Skip = new(StringComparer.OrdinalIgnoreCase)
    {
        "node_modules", "AppData", "Library", "Pods", "build", "bin", "obj", "DerivedData", "Music", "Videos", "Pictures", "OneDrive", "$Recycle.Bin", "packages"
    };

    private static string? Scan(string dir, RepoRef repo, int depth, HashSet<string> seen)
    {
        if (depth < 0 || !seen.Add(dir) || !Directory.Exists(dir)) return null;
        var git = Path.Combine(dir, ".git");
        if ((Directory.Exists(git) || File.Exists(git)) && RemoteMatches(dir, repo)) return dir;
        if (depth == 0) return null;
        string[] items;
        try
        {
            items = Directory.GetDirectories(dir);
        }
        catch
        {
            return null;
        }
        var ordered = items.OrderBy(p => string.Equals(Path.GetFileName(p), repo.Name, StringComparison.OrdinalIgnoreCase) ? 0 : 1);
        foreach (var path in ordered)
        {
            var name = Path.GetFileName(path);
            if (name.StartsWith('.') || Skip.Contains(name)) continue;
            if (Scan(path, repo, depth - 1, seen) is { } hit) return hit;
        }
        return null;
    }
}
