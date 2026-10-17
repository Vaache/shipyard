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
    }

    private sealed class Label
    {
        public string Name { get; set; } = "";
        public string? Color { get; set; }
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
        public List<Label>? Labels { get; set; }
        public Pull? PullRequest { get; set; }
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
        return new ForgeUser(u.Login, string.IsNullOrEmpty(u.FullName) ? null : u.FullName, u.AvatarUrl, u.Id.ToString());
    }

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
        var s = state switch
        {
            ForgeState.Open => "open",
            ForgeState.Closed => "closed",
            _ => "all"
        };
        var issues = await http.Get<List<Issue>>("/repos/issues/search", new[] { ("type", "pulls"), ("state", s), (flag, "true"), ("limit", "50") }, ct).ConfigureAwait(false);
        return issues.Select(Item).NewestFirst();
    }

    private static SearchItem Item(Issue i)
    {
        var state = i.State == "open" ? "OPEN" : i.PullRequest?.Merged == true ? "MERGED" : "CLOSED";
        return ForgeItems.Item(i.Number, i.Title, i.HtmlUrl, state, i.PullRequest?.Draft ?? false, i.CreatedAt, i.UpdatedAt, null,
            i.User?.Login, i.User?.AvatarUrl, i.Repository?.FullName ?? "?", i.Comments, (i.Labels ?? new()).Select(l => ForgeItems.Label(l.Name, l.Color)));
    }
}
