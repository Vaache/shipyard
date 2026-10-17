using PullYard.Core.Api;
using PullYard.Core.Auth;

namespace PullYard.Core.Forges;

public sealed class BitbucketClient : IForgeClient
{
    public const int MaxRepos = 20;
    public static readonly TimeSpan CacheTtl = TimeSpan.FromSeconds(180);

    private sealed class Link
    {
        public string? Href { get; set; }
    }

    private sealed class UserLinks
    {
        public Link? Avatar { get; set; }
    }

    private sealed class User
    {
        public string Uuid { get; set; } = "";
        public string? DisplayName { get; set; }
        public string? Nickname { get; set; }
        public string? Username { get; set; }
        public UserLinks? Links { get; set; }
    }

    private sealed class Page<T> where T : class
    {
        public List<T>? Values { get; set; }
    }

    private sealed class WorkspaceSlug
    {
        public string? Slug { get; set; }
    }

    private sealed class WorkspaceRef
    {
        public string? Slug { get; set; }
        public WorkspaceSlug? Workspace { get; set; }
        public string? Name => Workspace?.Slug ?? Slug;
    }

    private sealed class Repo
    {
        public string FullName { get; set; } = "";
    }

    private sealed class Branch
    {
        public string Name { get; set; } = "";
    }

    private sealed class Endpoint
    {
        public Branch? Branch { get; set; }
        public Repo? Repository { get; set; }
    }

    private sealed class Participant
    {
        public bool? Approved { get; set; }
        public string? State { get; set; }
    }

    private sealed class PrLinks
    {
        public Link? Html { get; set; }
    }

    private sealed class PullRequest
    {
        public int Id { get; set; }
        public string Title { get; set; } = "";
        public string State { get; set; } = "";
        public bool? Draft { get; set; }
        public DateTimeOffset CreatedOn { get; set; }
        public DateTimeOffset UpdatedOn { get; set; }
        public User? Author { get; set; }
        public Endpoint? Destination { get; set; }
        public PrLinks? Links { get; set; }
        public int? CommentCount { get; set; }
        public List<Participant>? Participants { get; set; }
    }

    private readonly ForgeHttp http;
    private readonly Dictionary<string, (DateTimeOffset At, List<SearchItem> Items)> cache = new();
    private readonly Func<DateTimeOffset> now;
    private string? uuid;
    private List<string>? workspaces;

    public GitProvider Provider => GitProvider.Bitbucket;
    public string Host { get; }

    public BitbucketClient(GitAccount account, GitCredential credential, HttpClient? client = null, Func<DateTimeOffset>? now = null)
    {
        Host = account.Host;
        uuid = account.UserId;
        this.now = now ?? (() => DateTimeOffset.Now);
        var auth = string.IsNullOrWhiteSpace(account.Email) ? "Bearer " + credential.Token : ForgeHttp.Basic(account.Email.Trim(), credential.Token);
        http = new ForgeHttp("https://api.bitbucket.org/2.0", "Bitbucket", auth, client);
    }

    public async Task<ForgeUser> CurrentUser(CancellationToken ct = default)
    {
        var u = await http.Get<User>("/user", ct: ct).ConfigureAwait(false);
        uuid = u.Uuid;
        return new ForgeUser(u.Nickname ?? u.Username ?? u.DisplayName ?? u.Uuid, u.DisplayName, u.Links?.Avatar?.Href, u.Uuid);
    }

    public async Task<List<SearchItem>> PullRequests(ForgeFilter filter, ForgeState state, CancellationToken ct = default)
    {
        var key = $"{filter}|{state}";
        lock (cache)
        {
            if (cache.TryGetValue(key, out var hit) && now() - hit.At < CacheTtl) return hit.Items;
        }
        var items = await Fetch(filter, state, ct).ConfigureAwait(false);
        lock (cache) cache[key] = (now(), items);
        return items;
    }

    private async Task<List<SearchItem>> Fetch(ForgeFilter filter, ForgeState state, CancellationToken ct)
    {
        if (filter is not (ForgeFilter.Mine or ForgeFilter.ReviewRequested)) return new();
        var me = uuid ?? (await CurrentUser(ct).ConfigureAwait(false)).Id ?? "";
        var spaces = await WorkspaceSlugs(ct).ConfigureAwait(false);
        var states = state switch
        {
            ForgeState.Open => new[] { "OPEN" },
            ForgeState.Closed => new[] { "MERGED", "DECLINED", "SUPERSEDED" },
            _ => new[] { "OPEN", "MERGED", "DECLINED", "SUPERSEDED" }
        };
        var common = states.Select(s => ("state", s)).Concat(new[] { ("pagelen", "50"), ("sort", "-updated_on"), ("fields", "+values.participants") }).ToList();
        var items = new List<SearchItem>();
        if (filter == ForgeFilter.Mine)
        {
            foreach (var ws in spaces)
            {
                var page = await http.Get<Page<PullRequest>>($"/workspaces/{Uri.EscapeDataString(ws)}/pullrequests/{Uri.EscapeDataString(me)}", common, ct).ConfigureAwait(false);
                items.AddRange((page.Values ?? new()).Select(Item));
            }
            return items.NewestFirst();
        }
        var repos = new List<string>();
        foreach (var ws in spaces)
        {
            if (repos.Count >= MaxRepos) break;
            var page = await http.Get<Page<Repo>>($"/repositories/{Uri.EscapeDataString(ws)}", new[] { ("role", "member"), ("sort", "-updated_on"), ("pagelen", MaxRepos.ToString()) }, ct).ConfigureAwait(false);
            repos.AddRange((page.Values ?? new()).Select(r => r.FullName));
        }
        Exception? lastError = null;
        foreach (var repo in repos.Take(MaxRepos))
        {
            try
            {
                var page = await http.Get<Page<PullRequest>>($"/repositories/{repo}/pullrequests", common.Append(("q", $"reviewers.uuid=\"{me}\"")), ct).ConfigureAwait(false);
                items.AddRange((page.Values ?? new()).Select(Item));
            }
            catch (ApiException e)
            {
                lastError = e;
            }
        }
        if (items.Count == 0 && lastError is ApiException { Status: 401 }) throw lastError;
        return items.NewestFirst();
    }

    private async Task<List<string>> WorkspaceSlugs(CancellationToken ct)
    {
        if (workspaces is { } known) return known;
        var query = new[] { ("pagelen", "100") };
        Page<WorkspaceRef> page;
        try
        {
            page = await http.Get<Page<WorkspaceRef>>("/user/workspaces", query, ct).ConfigureAwait(false);
        }
        catch (ApiException e) when (e.Status is 404 or 410)
        {
            page = await http.Get<Page<WorkspaceRef>>("/workspaces", query, ct).ConfigureAwait(false);
        }
        var slugs = (page.Values ?? new()).Select(w => w.Name).OfType<string>().ToList();
        workspaces = slugs;
        return slugs;
    }

    private static SearchItem Item(PullRequest pr)
    {
        var state = pr.State switch
        {
            "OPEN" => "OPEN",
            "MERGED" => "MERGED",
            _ => "CLOSED"
        };
        string? review = null;
        if ((pr.Participants ?? new()).Any(p => p.State == "changes_requested")) review = "CHANGES_REQUESTED";
        else if ((pr.Participants ?? new()).Any(p => p.Approved == true)) review = "APPROVED";
        var repo = pr.Destination?.Repository?.FullName ?? "?";
        return ForgeItems.Item(pr.Id, pr.Title, pr.Links?.Html?.Href ?? $"https://bitbucket.org/{repo}/pull-requests/{pr.Id}", state, pr.Draft ?? false, pr.CreatedOn, pr.UpdatedOn,
            pr.Destination?.Branch?.Name, pr.Author?.Nickname ?? pr.Author?.DisplayName, pr.Author?.Links?.Avatar?.Href, repo, pr.CommentCount, Array.Empty<LabelInfo>(), review);
    }
}
