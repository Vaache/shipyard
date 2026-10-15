namespace Shipyard.Core.Api;

public static class GitHubUrl
{
    private static readonly HashSet<string> Reserved = new(StringComparer.OrdinalIgnoreCase)
    {
        "orgs", "settings", "notifications", "users", "marketplace", "features", "pulls", "issues", "search", "login", "logout", "apps",
        "sponsors", "topics", "explore", "new", "codespaces", "organizations", "enterprise", "about", "pricing", "collections", "trending"
    };

    public static Route? Parse(string? raw, bool allowShort = true)
    {
        var s = (raw ?? "").Trim();
        if (s.Length == 0 || s.Contains(' ')) return null;
        if (allowShort && !s.Contains("://") && s.IndexOf('#') is var hash and >= 0)
        {
            var repoPart = s[..hash];
            if (int.TryParse(s[(hash + 1)..], out var num) && RepoRef.Parse(repoPart) is { } repo && !repo.Name.Contains('/'))
                return Route.Pr(new ItemRef(repo, num));
            return null;
        }
        if (!Uri.TryCreate(s.StartsWith("http", StringComparison.OrdinalIgnoreCase) ? s : "https://" + s, UriKind.Absolute, out var url)) return null;
        var host = url.Host.ToLowerInvariant();
        var parts = url.AbsolutePath.Split('/', StringSplitOptions.RemoveEmptyEntries).Select(Uri.UnescapeDataString).ToArray();
        if (host is "github.com" or "www.github.com")
        {
            if (parts.Length < 2 || Reserved.Contains(parts[0])) return null;
            var repo = new RepoRef(parts[0], parts[1]);
            if (parts.Length >= 4)
            {
                if (int.TryParse(parts[3], out var n))
                {
                    if (parts[2] == "pull") return Route.Pr(new ItemRef(repo, n));
                    if (parts[2] == "issues") return Route.Issue(new ItemRef(repo, n));
                }
                if (parts[2] == "commit") return Route.Commit(repo, parts[3]);
                return null;
            }
            if (parts.Length == 2) return Route.ForRepo(repo);
            if (parts.Length == 3 && parts[2] is "pulls" or "issues") return Route.ForRepo(repo);
            return null;
        }
        if (host == "api.github.com" && parts.Length >= 5 && parts[0] == "repos" && int.TryParse(parts[4], out var number))
        {
            var repo = new RepoRef(parts[1], parts[2]);
            if (parts[3] == "pulls") return Route.Pr(new ItemRef(repo, number));
            if (parts[3] == "issues") return Route.Issue(new ItemRef(repo, number));
        }
        return null;
    }

    public static bool IsFilesLink(string url) =>
        Uri.TryCreate(url, UriKind.Absolute, out var u) && u.AbsolutePath.TrimEnd('/').EndsWith("/files", StringComparison.Ordinal);
}

public enum VisualState { Open, Draft, Merged, Closed, IssueOpen, Completed, NotPlanned }

public static class VisualStates
{
    public static VisualState Pr(string state, bool draft) => state switch
    {
        "MERGED" => VisualState.Merged,
        "CLOSED" => VisualState.Closed,
        _ => draft ? VisualState.Draft : VisualState.Open
    };

    public static VisualState Issue(string state, string? reason)
    {
        if (state != "CLOSED") return VisualState.IssueOpen;
        return reason == "NOT_PLANNED" ? VisualState.NotPlanned : VisualState.Completed;
    }

    public static string Title(this VisualState s) => s switch
    {
        VisualState.Open or VisualState.IssueOpen => "Open",
        VisualState.Draft => "Draft",
        VisualState.Merged => "Merged",
        VisualState.Closed => "Closed",
        VisualState.Completed => "Completed",
        VisualState.NotPlanned => "Not planned",
        _ => ""
    };

    public static string ColorKey(this VisualState s) => s switch
    {
        VisualState.Open or VisualState.IssueOpen => "green",
        VisualState.Draft or VisualState.NotPlanned => "slate",
        VisualState.Merged or VisualState.Completed => "violet",
        VisualState.Closed => "red",
        _ => "slate"
    };

    public static string Glyph(this VisualState s) => s switch
    {
        VisualState.Open or VisualState.Draft => "",
        VisualState.Merged => "",
        VisualState.Closed => "",
        VisualState.IssueOpen => "",
        VisualState.Completed => "",
        VisualState.NotPlanned => "",
        _ => ""
    };
}
