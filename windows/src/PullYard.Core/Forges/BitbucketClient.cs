using System.Globalization;
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

    private sealed class AccountRef
    {
        public string? Uuid { get; set; }
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

    private sealed class HtmlLinks
    {
        public Link? Html { get; set; }
    }

    private sealed class Branch
    {
        public string Name { get; set; } = "";
    }

    private sealed class Repo
    {
        public string FullName { get; set; } = "";
        public string? Description { get; set; }
        public bool? IsPrivate { get; set; }
        public DateTimeOffset? UpdatedOn { get; set; }
        public Branch? Mainbranch { get; set; }
        public HtmlLinks? Links { get; set; }
    }

    private sealed class RepoName
    {
        public string FullName { get; set; } = "";
    }

    private sealed class Endpoint
    {
        public Branch? Branch { get; set; }
        public RepoName? Repository { get; set; }
    }

    private sealed class Participant
    {
        public AccountRef? User { get; set; }
        public string? Role { get; set; }
        public bool? Approved { get; set; }
        public string? State { get; set; }
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
        public HtmlLinks? Links { get; set; }
        public int? CommentCount { get; set; }
        public List<Participant>? Participants { get; set; }
        public List<AccountRef>? Reviewers { get; set; }
    }

    private readonly ForgeHttp http;
    private readonly Func<DateTimeOffset> now;
    private readonly Dictionary<string, (DateTimeOffset At, List<SearchItem> Items)> cache = new();
    private readonly Dictionary<ForgeState, (DateTimeOffset At, List<PullRequest> Pulls)> reviewCache = new();
    private (DateTimeOffset At, List<Repo> Repos)? repoCache;
    private bool participantFilterWorks = true;
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

    public Task<ForgeCapabilities> Capabilities(CancellationToken ct = default) =>
        Task.FromResult(new ForgeCapabilities(new[] { ForgeFilter.ReviewRequested, ForgeFilter.Mine, ForgeFilter.Reviewed }, Array.Empty<ForgeIssueFilter>(), ForgeInbox.None, true));

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

    public async Task<List<ForgeRepo>> Repositories(CancellationToken ct = default) =>
        (await MemberRepos(ct).ConfigureAwait(false))
        .Select(r => new ForgeRepo(r.FullName, string.IsNullOrEmpty(r.Description) ? null : r.Description, r.Links?.Html?.Href ?? $"https://bitbucket.org/{r.FullName}", r.IsPrivate ?? true, r.UpdatedOn, r.Mainbranch?.Name, null))
        .ToList();

    private static List<(string, string)> StateQuery(ForgeState state)
    {
        var states = state switch
        {
            ForgeState.Open => new[] { "OPEN" },
            ForgeState.Closed => new[] { "MERGED", "DECLINED", "SUPERSEDED" },
            _ => new[] { "OPEN", "MERGED", "DECLINED", "SUPERSEDED" }
        };
        return states.Select(s => ("state", s)).Concat(new[] { ("pagelen", "50"), ("sort", "-updated_on"), ("fields", "+values.participants,+values.reviewers") }).ToList();
    }

    private async Task<List<SearchItem>> Fetch(ForgeFilter filter, ForgeState state, CancellationToken ct)
    {
        var me = await UserId(ct).ConfigureAwait(false);
        switch (filter)
        {
            case ForgeFilter.Mine:
                var items = new List<SearchItem>();
                foreach (var ws in await WorkspaceSlugs(ct).ConfigureAwait(false))
                {
                    var page = await http.Get<Page<PullRequest>>($"/workspaces/{Uri.EscapeDataString(ws)}/pullrequests/{Uri.EscapeDataString(me)}", StateQuery(state), ct).ConfigureAwait(false);
                    items.AddRange((page.Values ?? new()).Select(Item));
                }
                return items.NewestFirst();
            case ForgeFilter.ReviewRequested:
                return (await ReviewPulls(state, me, ct).ConfigureAwait(false)).Where(p => AwaitsReview(p, me)).Select(Item).NewestFirst();
            case ForgeFilter.Reviewed:
                return (await ReviewPulls(state, me, ct).ConfigureAwait(false)).Where(p => Reviewed(p, me)).Select(Item).NewestFirst();
            default:
                return new();
        }
    }

    private async Task<List<PullRequest>> ReviewPulls(ForgeState state, string me, CancellationToken ct)
    {
        if (reviewCache.TryGetValue(state, out var hit) && now() - hit.At < CacheTtl) return hit.Pulls;
        var pulls = new List<PullRequest>();
        ApiException? lastError = null;
        foreach (var repo in (await MemberRepos(ct).ConfigureAwait(false)).Take(MaxRepos))
        {
            try
            {
                pulls.AddRange(await RepoPulls(repo.FullName, state, me, ct).ConfigureAwait(false));
            }
            catch (ApiException e)
            {
                lastError = e;
            }
        }
        if (pulls.Count == 0 && lastError is { Status: 401 }) throw lastError;
        reviewCache[state] = (now(), pulls);
        return pulls;
    }

    private async Task<List<PullRequest>> RepoPulls(string repo, ForgeState state, string me, CancellationToken ct)
    {
        var reviewer = $"reviewers.uuid=\"{me}\"";
        if (participantFilterWorks)
        {
            try
            {
                var page = await http.Get<Page<PullRequest>>($"/repositories/{repo}/pullrequests", StateQuery(state).Append(("q", $"{reviewer} OR participants.user.uuid=\"{me}\"")), ct).ConfigureAwait(false);
                return page.Values ?? new();
            }
            catch (ApiException e) when (e.Status == 400)
            {
                participantFilterWorks = false;
            }
        }
        var fallback = await http.Get<Page<PullRequest>>($"/repositories/{repo}/pullrequests", StateQuery(state).Append(("q", reviewer)), ct).ConfigureAwait(false);
        return fallback.Values ?? new();
    }

    private static bool Same(string? a, string b) => string.Equals(a, b, StringComparison.OrdinalIgnoreCase);

    private static Participant? Mine(PullRequest pr, string me) => pr.Participants?.FirstOrDefault(p => Same(p.User?.Uuid, me));

    private static bool AwaitsReview(PullRequest pr, string me)
    {
        var mine = Mine(pr, me);
        var isReviewer = pr.Reviewers?.Any(r => Same(r.Uuid, me)) == true || mine?.Role == "REVIEWER";
        return isReviewer && mine?.Approved != true && mine?.State is null;
    }

    private static bool Reviewed(PullRequest pr, string me)
    {
        if (Same(pr.Author?.Uuid, me) || Mine(pr, me) is not { } mine) return false;
        return mine.Approved == true || mine.State is "approved" or "changes_requested";
    }

    private async Task<List<Repo>> MemberRepos(CancellationToken ct)
    {
        if (repoCache is { } known && now() - known.At < CacheTtl) return known.Repos;
        var repos = new List<Repo>();
        foreach (var ws in await WorkspaceSlugs(ct).ConfigureAwait(false))
        {
            var page = await http.Get<Page<Repo>>($"/repositories/{Uri.EscapeDataString(ws)}", new[] { ("role", "member"), ("sort", "-updated_on"), ("pagelen", "100") }, ct).ConfigureAwait(false);
            repos.AddRange(page.Values ?? new());
        }
        repos = repos.OrderByDescending(r => r.UpdatedOn ?? DateTimeOffset.MinValue).ToList();
        repoCache = (now(), repos);
        return repos;
    }

    private async Task<string> UserId(CancellationToken ct) => uuid ?? (await CurrentUser(ct).ConfigureAwait(false)).Id ?? "";

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
        var participants = pr.Participants ?? new();
        string? review = null;
        if (participants.Any(p => p.State == "changes_requested")) review = "CHANGES_REQUESTED";
        else if (participants.Any(p => p.Approved == true)) review = "APPROVED";
        var repo = pr.Destination?.Repository?.FullName ?? "?";
        return ForgeItems.Item(pr.Id, pr.Title, pr.Links?.Html?.Href ?? $"https://bitbucket.org/{repo}/pull-requests/{pr.Id.ToString(CultureInfo.InvariantCulture)}", state, pr.Draft ?? false, pr.CreatedOn, pr.UpdatedOn,
            pr.Destination?.Branch?.Name, pr.Author?.Nickname ?? pr.Author?.DisplayName, pr.Author?.Links?.Avatar?.Href, repo, pr.CommentCount, Array.Empty<LabelInfo>(), review);
    }
}
