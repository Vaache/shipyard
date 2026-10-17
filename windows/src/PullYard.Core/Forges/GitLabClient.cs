using PullYard.Core.Api;
using PullYard.Core.Auth;

namespace PullYard.Core.Forges;

public sealed class GitLabClient : IForgeClient
{
    private sealed class User
    {
        public long Id { get; set; }
        public string Username { get; set; } = "";
        public string? Name { get; set; }
        public string? AvatarUrl { get; set; }
    }

    private sealed class Author
    {
        public string Username { get; set; } = "";
        public string? AvatarUrl { get; set; }
    }

    private sealed class References
    {
        public string? Full { get; set; }
    }

    private sealed class Label
    {
        public string Name { get; set; } = "";
        public string? Color { get; set; }
    }

    private sealed class MergeRequest
    {
        public int Iid { get; set; }
        public string Title { get; set; } = "";
        public string WebUrl { get; set; } = "";
        public string State { get; set; } = "";
        public bool? Draft { get; set; }
        public bool? WorkInProgress { get; set; }
        public DateTimeOffset CreatedAt { get; set; }
        public DateTimeOffset UpdatedAt { get; set; }
        public string? TargetBranch { get; set; }
        public Author? Author { get; set; }
        public References? References { get; set; }
        public int? UserNotesCount { get; set; }
        public List<Label>? Labels { get; set; }
    }

    private readonly ForgeHttp http;
    private string? login;

    public GitProvider Provider => GitProvider.GitLab;
    public string Host { get; }

    public GitLabClient(GitAccount account, GitCredential credential, Action<GitCredential>? persist = null, HttpClient? client = null)
    {
        Host = account.Host;
        login = account.Login.Length == 0 ? null : account.Login;
        http = new ForgeHttp($"https://{account.Host}/api/v4", "GitLab", "Bearer " + credential.Token, client);
        if (credential.RefreshToken is { } refreshToken && credential.ClientId is { } clientId)
        {
            var endpoint = DeviceFlow.GitLab(account.Host);
            var latest = refreshToken;
            http.Refresh = async ct =>
            {
                var fresh = await DeviceFlow.Refresh(clientId, latest, endpoint, client, ct).ConfigureAwait(false);
                latest = fresh.RefreshToken ?? latest;
                persist?.Invoke(fresh.Credential(clientId));
                return "Bearer " + fresh.AccessToken;
            };
        }
    }

    public async Task<ForgeUser> CurrentUser(CancellationToken ct = default)
    {
        var u = await http.Get<User>("/user", ct: ct).ConfigureAwait(false);
        login = u.Username;
        return new ForgeUser(u.Username, u.Name, u.AvatarUrl, u.Id.ToString());
    }

    public async Task<List<SearchItem>> PullRequests(ForgeFilter filter, ForgeState state, CancellationToken ct = default)
    {
        if (filter is not (ForgeFilter.ReviewRequested or ForgeFilter.Mine or ForgeFilter.Assigned)) return new();
        var query = new List<(string, string)>
        {
            ("order_by", "updated_at"),
            ("sort", "desc"),
            ("per_page", "50"),
            ("with_labels_details", "true")
        };
        switch (filter)
        {
            case ForgeFilter.ReviewRequested:
                var me = login ?? (await CurrentUser(ct).ConfigureAwait(false)).Login;
                query.Add(("scope", "all"));
                query.Add(("reviewer_username", me));
                break;
            case ForgeFilter.Mine:
                query.Add(("scope", "created_by_me"));
                break;
            default:
                query.Add(("scope", "assigned_to_me"));
                break;
        }
        var states = state switch
        {
            ForgeState.Open => new[] { "opened" },
            ForgeState.Closed => new[] { "merged", "closed" },
            _ => new[] { "all" }
        };
        var items = new List<SearchItem>();
        foreach (var s in states)
        {
            var page = await http.Get<List<MergeRequest>>("/merge_requests", query.Append(("state", s)), ct).ConfigureAwait(false);
            items.AddRange(page.Select(Item));
        }
        return items.NewestFirst();
    }

    private static SearchItem Item(MergeRequest mr)
    {
        var state = mr.State switch
        {
            "merged" => "MERGED",
            "opened" or "locked" => "OPEN",
            _ => "CLOSED"
        };
        var repo = mr.References?.Full is { } full ? full.Split('!')[0] : Project(mr.WebUrl);
        return ForgeItems.Item(mr.Iid, mr.Title, mr.WebUrl, state, mr.Draft ?? mr.WorkInProgress ?? false, mr.CreatedAt, mr.UpdatedAt, mr.TargetBranch,
            mr.Author?.Username, mr.Author?.AvatarUrl, repo, mr.UserNotesCount, (mr.Labels ?? new()).Select(l => ForgeItems.Label(l.Name, l.Color)));
    }

    public static string Project(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u)) return "?";
        var path = u.AbsolutePath;
        var cut = path.IndexOf("/-/", StringComparison.Ordinal);
        return (cut >= 0 ? path[..cut] : path).Trim('/');
    }
}
