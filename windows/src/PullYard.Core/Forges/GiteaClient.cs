using System.Globalization;
using PullYard.Core.Api;
using PullYard.Core.Auth;

namespace PullYard.Core.Forges;

public sealed class GiteaClient : IForgeClient
{
    private sealed class User
    {
        public long Id { get; set; }
        public string Login { get; set; } = "";
        public string? FullName { get; set; }
        public string? AvatarUrl { get; set; }
    }

    private sealed class Pull
    {
        public bool? Merged { get; set; }
        public bool? Draft { get; set; }
    }

    private sealed class Repo
    {
        public string FullName { get; set; } = "";
        public string? HtmlUrl { get; set; }
    }

    private sealed class Issue
    {
        public int Number { get; set; }
        public string Title { get; set; } = "";
        public string HtmlUrl { get; set; } = "";
        public string State { get; set; } = "";
        public DateTimeOffset CreatedAt { get; set; }
        public DateTimeOffset UpdatedAt { get; set; }
        public User? User { get; set; }
        public Repo? Repository { get; set; }
        public int? Comments { get; set; }
        public List<ForgeLabel>? Labels { get; set; }
        public Pull? PullRequest { get; set; }
    }

    private sealed class Subject
    {
        public string Title { get; set; } = "";
        public string? Type { get; set; }
        public string? Url { get; set; }
        public string? HtmlUrl { get; set; }
        public string? LatestCommentHtmlUrl { get; set; }
        public string? State { get; set; }
    }

    private sealed class Thread
    {
        public long Id { get; set; }
        public bool Unread { get; set; }
        public bool? Pinned { get; set; }
        public DateTimeOffset UpdatedAt { get; set; }
        public Subject Subject { get; set; } = new();
        public Repo? Repository { get; set; }
    }

    private sealed class Repository
    {
        public string FullName { get; set; } = "";
        public string? Description { get; set; }
        public string HtmlUrl { get; set; } = "";
        public bool? Private { get; set; }
        public bool? Archived { get; set; }
        public DateTimeOffset? UpdatedAt { get; set; }
        public string? DefaultBranch { get; set; }
        public int? StarsCount { get; set; }
    }

    private readonly ForgeHttp http;

    public GitProvider Provider => GitProvider.Gitea;
    public string Host { get; }

    public GiteaClient(GitAccount account, GitCredential credential, HttpClient? client = null)
    {
        Host = account.Host;
        http = new ForgeHttp($"https://{account.Host}/api/v1", account.Host, "token " + credential.Token, client);
    }

    public async Task<ForgeUser> CurrentUser(CancellationToken ct = default)
    {
        var u = await http.Get<User>("/user", ct: ct).ConfigureAwait(false);
        return new ForgeUser(u.Login, string.IsNullOrEmpty(u.FullName) ? null : u.FullName, u.AvatarUrl, u.Id.ToString(CultureInfo.InvariantCulture));
    }

    public Task<ForgeCapabilities> Capabilities(CancellationToken ct = default) =>
        Task.FromResult(new ForgeCapabilities(Enum.GetValues<ForgeFilter>(), Enum.GetValues<ForgeIssueFilter>(), ForgeInbox.Notifications, true));

    public async Task<List<SearchItem>> PullRequests(ForgeFilter filter, ForgeState state, CancellationToken ct = default)
    {
        var flag = filter switch
        {
            ForgeFilter.ReviewRequested => "review_requested",
            ForgeFilter.Mine => "created",
            ForgeFilter.Assigned => "assigned",
            ForgeFilter.Mentioned => "mentioned",
            _ => "reviewed"
        };
        var issues = await Search("pulls", flag, state, ct).ConfigureAwait(false);
        return issues.Select(Item).NewestFirst();
    }

    public async Task<List<SearchItem>> Issues(ForgeIssueFilter filter, ForgeState state, CancellationToken ct = default)
    {
        var flag = filter switch
        {
            ForgeIssueFilter.Assigned => "assigned",
            ForgeIssueFilter.Mine => "created",
            _ => "mentioned"
        };
        var issues = await Search("issues", flag, state, ct).ConfigureAwait(false);
        return issues.Select(IssueItem).NewestFirst();
    }

    public async Task<List<ForgeNotification>> Notifications(CancellationToken ct = default)
    {
        var threads = await http.Get<List<Thread>>("/notifications", new[] { ("limit", "50") }, ct).ConfigureAwait(false);
        return threads.Select(t => new ForgeNotification(
                t.Id.ToString(CultureInfo.InvariantCulture),
                t.Subject.Title,
                t.Repository?.FullName ?? "",
                Kind(t.Subject.Type),
                Reason(t),
                WebUrl(t),
                t.UpdatedAt,
                null,
                t.Unread))
            .OrderByDescending(n => n.UpdatedAt)
            .ToList();
    }

    public Task MarkDone(ForgeNotification notification, CancellationToken ct = default) =>
        http.Post($"/notifications/threads/{Uri.EscapeDataString(notification.Id)}", new[] { ("to-status", "read") }, "PATCH", ct);

    public async Task<List<ForgeRepo>> Repositories(CancellationToken ct = default)
    {
        const int limit = 50;
        var all = new List<Repository>();
        for (var page = 1; page <= 3; page++)
        {
            var batch = await http.Get<List<Repository>>("/user/repos", new[] { ("order_by", "recentupdate"), ("limit", limit.ToString(CultureInfo.InvariantCulture)), ("page", page.ToString(CultureInfo.InvariantCulture)) }, ct).ConfigureAwait(false);
            all.AddRange(batch);
            if (batch.Count < limit) break;
        }
        var seen = new HashSet<string>();
        return all
            .Where(r => r.Archived != true && seen.Add(r.FullName))
            .OrderByDescending(r => r.UpdatedAt ?? DateTimeOffset.MinValue)
            .Select(r => new ForgeRepo(r.FullName, string.IsNullOrEmpty(r.Description) ? null : r.Description, r.HtmlUrl, r.Private ?? false, r.UpdatedAt, r.DefaultBranch, r.StarsCount))
            .ToList();
    }

    private Task<List<Issue>> Search(string type, string flag, ForgeState state, CancellationToken ct)
    {
        var s = state switch
        {
            ForgeState.Open => "open",
            ForgeState.Closed => "closed",
            _ => "all"
        };
        return http.Get<List<Issue>>("/repos/issues/search", new[] { ("type", type), ("state", s), (flag, "true"), ("sort", "recentupdate"), ("limit", "50") }, ct);
    }

    private string WebUrl(Thread t)
    {
        foreach (var candidate in new[] { t.Subject.LatestCommentHtmlUrl, t.Subject.HtmlUrl })
            if (!string.IsNullOrEmpty(candidate)) return candidate;
        if (t.Subject.Url is { } api && api.IndexOf("/api/v1/repos/", StringComparison.Ordinal) is var cut and >= 0)
            return (api[..cut] + "/" + api[(cut + "/api/v1/repos/".Length)..]).Replace("/git/commits/", "/commit/");
        return t.Repository?.HtmlUrl ?? $"https://{Host}/notifications";
    }

    private static string Kind(string? type) => type?.ToLowerInvariant() switch
    {
        "pull" => "PR",
        "issue" => "Issue",
        "commit" => "Commit",
        "repository" => "Repository",
        _ => type ?? "Notification"
    };

    private static string Reason(Thread t)
    {
        if (t.Pinned == true) return "Pinned";
        string subject;
        switch (t.Subject.Type?.ToLowerInvariant())
        {
            case "pull":
                subject = "Pull request";
                break;
            case "issue":
                subject = "Issue";
                break;
            case "commit":
                return "Commit";
            case "repository":
                return "Repository";
            default:
                return t.Subject.Type ?? "Notification";
        }
        return t.Subject.State?.ToLowerInvariant() switch
        {
            "merged" => subject + " merged",
            "closed" => subject + " closed",
            _ => subject
        };
    }

    private static SearchItem Item(Issue i)
    {
        var state = i.State == "open" ? "OPEN" : i.PullRequest?.Merged == true ? "MERGED" : "CLOSED";
        return ForgeItems.Item(i.Number, i.Title, i.HtmlUrl, state, i.PullRequest?.Draft ?? false, i.CreatedAt, i.UpdatedAt, null,
            i.User?.Login, i.User?.AvatarUrl, i.Repository?.FullName ?? "?", i.Comments, (i.Labels ?? new()).Select(l => l.Info));
    }

    private static SearchItem IssueItem(Issue i) =>
        ForgeItems.Issue(i.Number, i.Title, i.HtmlUrl, i.State == "open", i.CreatedAt, i.UpdatedAt, i.User?.Login, i.User?.AvatarUrl,
            i.Repository?.FullName ?? "?", i.Comments, (i.Labels ?? new()).Select(l => l.Info));
}
