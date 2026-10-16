using System.Collections.Concurrent;
using System.Text;
using System.Text.Json;
using PullYard.Core.Util;

namespace PullYard.Core.Api;

public sealed partial class GitHubApi
{
    public async Task<(RestUser User, List<string> Scopes)> CurrentUser(CancellationToken ct = default)
    {
        var (data, response) = await RestRaw("GET", "/user", ct: ct).ConfigureAwait(false);
        using (response)
        {
            var user = JsonSerializer.Deserialize<RestUser>(data, Json.Rest) ?? new RestUser();
            var scopes = response.Headers.TryGetValues("X-OAuth-Scopes", out var values)
                ? string.Join(",", values).Split(',').Select(s => s.Trim()).Where(s => s.Length > 0).ToList()
                : new List<string>();
            return (user, scopes);
        }
    }

    public async Task<SearchConnection> Search(string query, string? after = null, CancellationToken ct = default)
    {
        var r = await GraphQL<SearchResponse>(Q.Search, V(("q", query), ("after", after)), ct: ct).ConfigureAwait(false);
        return r.Search;
    }

    private sealed class RepoPRsResponse
    {
        public sealed class Repo
        {
            public Nodes<SearchItem> PullRequests { get; set; } = new();
        }

        public Repo? Repository { get; set; }
    }

    public async Task<List<SearchItem>> RepoPullRequests(RepoRef repo, string? headRefName = null, string[]? states = null, int first = 30, CancellationToken ct = default)
    {
        var r = await GraphQL<RepoPRsResponse>(Q.RepoPullRequests, V(("o", repo.Owner), ("n", repo.Name), ("b", headRefName), ("states", states ?? new[] { "OPEN" }), ("first", first)), ct: ct).ConfigureAwait(false);
        return r.Repository?.PullRequests.Items ?? new();
    }

    private sealed class PRItemResponse
    {
        public sealed class Repo
        {
            public SearchItem? PullRequest { get; set; }
        }

        public Repo? Repository { get; set; }
    }

    public async Task<SearchItem?> PullRequestItem(ItemRef r, CancellationToken ct = default)
    {
        var res = await GraphQL<PRItemResponse>(Q.PrItem, V(("o", r.Repo.Owner), ("n", r.Repo.Name), ("num", r.Number)), ct: ct).ConfigureAwait(false);
        return res.Repository?.PullRequest;
    }

    private sealed class StackResponse
    {
        public sealed class Entry
        {
            public int Position { get; set; }
            public StackPR PullRequest { get; set; } = new();
        }

        public sealed class StackInfo
        {
            public Nodes<Entry> Entries { get; set; } = new();
        }

        public sealed class PR
        {
            public int Number { get; set; }
            public string Title { get; set; } = "";
            public string State { get; set; } = "";
            public bool IsDraft { get; set; }
            public string HeadRefName { get; set; } = "";
            public string BaseRefName { get; set; } = "";
            public StackInfo? Stack { get; set; }
        }

        public sealed class Repo
        {
            public BranchName? DefaultBranchRef { get; set; }
            public PR? PullRequest { get; set; }
        }

        public Repo? Repository { get; set; }
    }

    private sealed class NeighborsResponse
    {
        public sealed class Repo
        {
            public Nodes<StackPR> Parents { get; set; } = new();
            public Nodes<StackPR> Children { get; set; } = new();
        }

        public Repo? Repository { get; set; }
    }

    public static readonly TimeSpan StackTtl = TimeSpan.FromMinutes(10);

    private sealed record CachedStack(DateTimeOffset At, List<StackPR> Stack);

    private readonly ConcurrentDictionary<string, CachedStack> stackCache = new();
    private volatile bool stackFieldUnsupported;

    private static string StackKey(ItemRef r, string? head, string? @base) => $"{r.Repo.Full}#{r.Number}|{head}|{@base}";

    public List<StackPR>? CachedPullRequestStack(ItemRef r, string? head, string? @base, bool live)
    {
        if (!stackCache.TryGetValue(StackKey(r, head, @base), out var hit)) return null;
        return live || DateTimeOffset.Now - hit.At < StackTtl ? hit.Stack : null;
    }

    public async Task<List<StackPR>> PullRequestStack(ItemRef r, string? head = null, string? @base = null, bool live = false, bool force = false, CancellationToken ct = default)
    {
        if (!force && CachedPullRequestStack(r, head, @base, live) is { } cached) return cached;
        var stack = await LookupStack(r, ct).ConfigureAwait(false);
        stackCache[StackKey(r, head, @base)] = new CachedStack(DateTimeOffset.Now, stack);
        return stack;
    }

    private async Task<List<StackPR>> LookupStack(ItemRef r, CancellationToken ct)
    {
        var plainQuery = Q.PrStack.Replace(" stack { entries(first: 30) { nodes { position pullRequest { ...StackPR } } } }", "");
        StackResponse res;
        if (stackFieldUnsupported)
        {
            res = await GraphQL<StackResponse>(plainQuery, V(("o", r.Repo.Owner), ("n", r.Repo.Name), ("num", r.Number)), ct: ct).ConfigureAwait(false);
        }
        else
        {
            try
            {
                res = await GraphQL<StackResponse>(Q.PrStack, V(("o", r.Repo.Owner), ("n", r.Repo.Name), ("num", r.Number)), ct: ct).ConfigureAwait(false);
            }
            catch (ApiException e) when (e.SchemaError)
            {
                stackFieldUnsupported = true;
                res = await GraphQL<StackResponse>(plainQuery, V(("o", r.Repo.Owner), ("n", r.Repo.Name), ("num", r.Number)), ct: ct).ConfigureAwait(false);
            }
        }
        if (res.Repository?.PullRequest is not { } pr) return new();
        if (pr.Stack?.Entries.Items is { Count: > 1 } entries) return entries.OrderBy(e => e.Position).Select(e => e.PullRequest).ToList();
        var main = res.Repository.DefaultBranchRef?.Name;
        var current = new StackPR { Number = pr.Number, Title = pr.Title, State = pr.State, IsDraft = pr.IsDraft, HeadRefName = pr.HeadRefName, BaseRefName = pr.BaseRefName };
        var below = new List<StackPR>();
        var above = new List<StackPR>();
        var seen = new HashSet<int> { current.Number };
        var node = current;
        for (var i = 0; i < 8 && node.BaseRefName != main; i++)
        {
            var (parents, _) = await StackNeighbors(r.Repo, node, ct).ConfigureAwait(false);
            var parent = parents.FirstOrDefault(p => p.State == "OPEN") ?? parents.FirstOrDefault();
            if (parent is null || !seen.Add(parent.Number)) break;
            below.Insert(0, parent);
            node = parent;
        }
        node = current;
        for (var i = 0; i < 8; i++)
        {
            var (_, children) = await StackNeighbors(r.Repo, node, ct).ConfigureAwait(false);
            var child = children.FirstOrDefault(p => p.State == "OPEN") ?? children.FirstOrDefault();
            if (child is null || !seen.Add(child.Number)) break;
            above.Add(child);
            node = child;
        }
        var chain = below.Append(current).Concat(above).ToList();
        return chain.Count > 1 ? chain : new();
    }

    private async Task<(List<StackPR> Parents, List<StackPR> Children)> StackNeighbors(RepoRef repo, StackPR pr, CancellationToken ct)
    {
        var r = await GraphQL<NeighborsResponse>(Q.PrStackNeighbors, V(("o", repo.Owner), ("n", repo.Name), ("head", pr.HeadRefName), ("base", pr.BaseRefName)), ct: ct).ConfigureAwait(false);
        return (r.Repository?.Parents.Items ?? new(), r.Repository?.Children.Items ?? new());
    }

    public Task<BadgeResponse> Badges(CancellationToken ct = default) => GraphQL<BadgeResponse>(Q.Badges, ct: ct);

    public Task<PRDetailResponse> PullRequest(ItemRef r, CancellationToken ct = default) =>
        GraphQL<PRDetailResponse>(Q.PullRequest, V(("owner", r.Repo.Owner), ("name", r.Repo.Name), ("number", r.Number)), ct: ct);

    public async Task<PullRequestDetail?> PullRequestPulse(ItemRef r, CancellationToken ct = default)
    {
        var res = await GraphQL<PRDetailResponse>(Q.PullRequestPulse, V(("owner", r.Repo.Owner), ("name", r.Repo.Name), ("number", r.Number)), ct: ct).ConfigureAwait(false);
        return res.Repository?.PullRequest;
    }

    public Task<IssueDetailResponse> Issue(ItemRef r, CancellationToken ct = default) =>
        GraphQL<IssueDetailResponse>(Q.Issue, V(("owner", r.Repo.Owner), ("name", r.Repo.Name), ("number", r.Number)), ct: ct);

    public async Task<List<PRFile>> PullRequestFiles(ItemRef r, CancellationToken ct = default)
    {
        var all = new List<PRFile>();
        for (var page = 1; page <= 30; page++)
        {
            var batch = await Rest<List<PRFile>>("GET", $"/repos/{r.Repo.Full}/pulls/{r.Number}/files?per_page=100&page={page}", ct: ct).ConfigureAwait(false);
            all.AddRange(batch);
            if (batch.Count < 100) break;
        }
        return all;
    }

    public Task<CommitDetailRest> Commit(RepoRef repo, string sha, CancellationToken ct = default) =>
        Rest<CommitDetailRest>("GET", $"/repos/{repo.Full}/commits/{sha}", ct: ct);

    public async Task<RepoConnection> Repos(string? after, CancellationToken ct = default)
    {
        var r = await GraphQL<ReposResponse>(Q.Repos, V(("after", after)), ct: ct).ConfigureAwait(false);
        return r.Viewer.Repositories;
    }

    public async Task<RepoInfo?> RepoInfo(RepoRef repo, CancellationToken ct = default)
    {
        var r = await GraphQL<RepoInfoResponse>(Q.RepoInfo, V(("owner", repo.Owner), ("name", repo.Name)), ct: ct).ConfigureAwait(false);
        return r.Repository;
    }

    public async Task<List<ProjectSummary>> Projects(CancellationToken ct = default)
    {
        var r = await GraphQL<ProjectsResponse>(Q.Projects, ct: ct).ConfigureAwait(false);
        var seen = new HashSet<string>();
        return r.Viewer.ProjectsV2.Items.Concat(r.Viewer.Organizations.Items.SelectMany(o => o.ProjectsV2.Items)).Where(p => seen.Add(p.Id)).ToList();
    }

    public async Task<(ProjectDetail Detail, List<ProjectItem> Items)> Project(string id, CancellationToken ct = default)
    {
        string? after = null;
        var items = new List<ProjectItem>();
        ProjectDetail? detail = null;
        for (var i = 0; i < 10; i++)
        {
            var r = await GraphQL<ProjectDetailResponse>(Q.Project, V(("id", id), ("after", after)), ct: ct).ConfigureAwait(false);
            if (r.Node is not { } node) throw new ApiException("Project not found");
            detail ??= node;
            items.AddRange(node.Items.Nodes);
            if (!node.Items.PageInfo.HasNextPage) break;
            after = node.Items.PageInfo.EndCursor;
        }
        return (detail!, items);
    }

    public async Task MoveProjectItem(string project, string item, string field, string? option, CancellationToken ct = default)
    {
        if (option is not null)
            await GraphQL<EmptyData>(Q.MoveProjectItem, V(("p", project), ("i", item), ("f", field), ("o", option)), strict: true, ct: ct).ConfigureAwait(false);
        else
            await GraphQL<EmptyData>(Q.ClearProjectItem, V(("p", project), ("i", item), ("f", field)), strict: true, ct: ct).ConfigureAwait(false);
    }

    private static string NotificationsPath(bool all) => $"/notifications?all={(all ? "true" : "false")}&per_page=50";

    public async Task<List<NotificationThread>> Notifications(bool all, CancellationToken ct = default)
    {
        if (Demo.IsOn) return new();
        var (data, _) = await Conditional(NotificationsPath(all), false, ct).ConfigureAwait(false);
        return DecodeNotifications(data);
    }

    public async Task<List<NotificationThread>?> NotificationsIfChanged(bool all, bool live, bool needData = false, CancellationToken ct = default)
    {
        if (Demo.IsOn) return new();
        var (data, changed) = await Conditional(NotificationsPath(all), live, ct).ConfigureAwait(false);
        return changed || needData ? DecodeNotifications(data) : null;
    }

    public async Task<int?> UnreadNotificationCount(bool live = false, CancellationToken ct = default)
    {
        var (data, changed) = await Conditional("/notifications?per_page=50", live, ct).ConfigureAwait(false);
        return changed ? DecodeNotifications(data).Count : null;
    }

    private static List<NotificationThread> DecodeNotifications(byte[] data)
    {
        try
        {
            return JsonSerializer.Deserialize<List<NotificationThread>>(data, Json.Rest) ?? new();
        }
        catch (JsonException e)
        {
            throw new ApiException($"Failed to parse response: {e.Message}");
        }
    }

    public Task MarkNotificationRead(string id) => ChangeNotifications(() => Send("PATCH", $"/notifications/threads/{id}"));

    public Task MarkNotificationDone(string id) => ChangeNotifications(() => Send("DELETE", $"/notifications/threads/{id}"));

    public Task MarkAllNotificationsRead() => ChangeNotifications(() => Send("PUT", "/notifications", new Dictionary<string, object> { ["read"] = true }));

    private async Task ChangeNotifications(Func<Task> change)
    {
        try
        {
            await change().ConfigureAwait(false);
        }
        finally
        {
            Invalidate("/notifications");
        }
    }

    public async Task Mutate(string query, IDictionary<string, object?> vars, CancellationToken ct = default)
    {
        await GraphQL<EmptyData>(query, vars, strict: true, ct: ct).ConfigureAwait(false);
    }

    public Task AddComment(string subject, string body) => Mutate(Q.AddComment, V(("id", subject), ("b", body)));

    public Task AddReview(string pr, string @event, string? body, IReadOnlyList<PendingComment> comments)
    {
        var threads = comments.Select(c => new Dictionary<string, object> { ["path"] = c.Path, ["line"] = c.Line, ["side"] = c.Side, ["body"] = c.Body }).ToList();
        return Mutate(Q.AddReview, V(("id", pr), ("e", @event), ("b", string.IsNullOrEmpty(body) ? null : body), ("th", threads.Count == 0 ? null : threads)));
    }

    public Task RequestReviewers(ItemRef r, IEnumerable<string> logins)
    {
        var list = logins.ToList();
        var teams = list.Where(l => l.Contains('/')).Select(l => l.Split('/').Last()).ToList();
        var users = list.Where(l => !l.Contains('/')).ToList();
        var body = new Dictionary<string, List<string>>();
        if (users.Count > 0) body["reviewers"] = users;
        if (teams.Count > 0) body["team_reviewers"] = teams;
        return Send("POST", $"/repos/{r.Repo.Full}/pulls/{r.Number}/requested_reviewers", body);
    }

    public Task DeleteBranch(string repo, string branch)
    {
        var encoded = string.Join("/", branch.Split('/').Select(Uri.EscapeDataString));
        return Send("DELETE", $"/repos/{repo}/git/refs/heads/{encoded}");
    }

    public Task RerunFailedJobs(RepoRef repo, long runId) => Send("POST", $"/repos/{repo.Full}/actions/runs/{runId}/rerun-failed-jobs");

    public async Task<string> JobLog(RepoRef repo, long jobId, CancellationToken ct = default)
    {
        var target = await RedirectTarget($"/repos/{repo.Full}/actions/jobs/{jobId}/logs", ct).ConfigureAwait(false);
        return await Plain(target, ct).ConfigureAwait(false);
    }

    public static readonly string GitObjectQuery = @"query GitObject($owner: String!, $name: String!, $expr: String!) { repository(owner: $owner, name: $name) { object(expression: $expr) { __typename
  ... on Tree { entries { name path type object { ... on Blob { byteSize isBinary } } } }
  ... on Blob { byteSize isBinary isTruncated text }
} } }";

    public static string EncodePath(string path) => string.Join("/", path.Split('/').Select(Uri.EscapeDataString));

    public static string EncodeQuery(string s) => Uri.EscapeDataString(s);

    public async Task<GitObject?> GitObject(RepoRef repo, string @ref, string path, CancellationToken ct = default)
    {
        var r = await GraphQL<GitObjectResponse>(GitObjectQuery, V(("owner", repo.Owner), ("name", repo.Name), ("expr", $"{@ref}:{path}")), ct: ct).ConfigureAwait(false);
        return r.Repository?.Object;
    }

    public const string BlameQuery = @"query Blame($owner: String!, $name: String!, $ref: String!, $path: String!) { repository(owner: $owner, name: $name) { object(expression: $ref) { __typename
  ... on Commit { blame(path: $path) { ranges { startingLine endingLine age commit { oid abbreviatedOid messageHeadline committedDate url author { name user { login avatarUrl } } } } } }
} } }";

    public async Task<List<BlameRangeNode>?> BlameRanges(RepoRef repo, string @ref, string path, CancellationToken ct = default)
    {
        var r = await GraphQL<BlameResponse>(BlameQuery, V(("owner", repo.Owner), ("name", repo.Name), ("ref", @ref), ("path", path)), ct: ct).ConfigureAwait(false);
        return r.Repository?.Object?.Blame?.Ranges;
    }

    public async Task<List<BranchInfo>> Branches(RepoRef repo, CancellationToken ct = default)
    {
        var all = new List<BranchInfo>();
        for (var page = 1; page <= 10; page++)
        {
            var batch = await Rest<List<BranchInfo>>("GET", $"/repos/{repo.Full}/branches?per_page=100&page={page}", ct: ct).ConfigureAwait(false);
            all.AddRange(batch);
            if (batch.Count < 100) break;
        }
        return all;
    }

    public async Task<(List<string> Paths, bool Truncated)> AllPaths(RepoRef repo, string sha, CancellationToken ct = default)
    {
        var r = await Rest<GitTreeRest>("GET", $"/repos/{repo.Full}/git/trees/{sha}?recursive=1", ct: ct).ConfigureAwait(false);
        return (r.Tree.Where(t => t.Type == "blob").Select(t => t.Path).ToList(), r.Truncated);
    }

    public async Task<string?> ReadmeHtml(RepoRef repo, string @ref, string dir, CancellationToken ct = default)
    {
        var suffix = dir.Length == 0 ? "" : "/" + EncodePath(dir);
        var data = await Fetch($"/repos/{repo.Full}/readme{suffix}?ref={EncodeQuery(@ref)}", "application/vnd.github.html+json", ct).ConfigureAwait(false);
        return data is null ? null : Encoding.UTF8.GetString(data);
    }

    public async Task<string?> RenderedFile(RepoRef repo, string @ref, string path, CancellationToken ct = default)
    {
        var data = await Fetch($"/repos/{repo.Full}/contents/{EncodePath(path)}?ref={EncodeQuery(@ref)}", "application/vnd.github.html+json", ct).ConfigureAwait(false);
        return data is null ? null : Encoding.UTF8.GetString(data);
    }

    public Task<byte[]?> RawFile(RepoRef repo, string @ref, string path, CancellationToken ct = default) =>
        Fetch($"/repos/{repo.Full}/contents/{EncodePath(path)}?ref={EncodeQuery(@ref)}", "application/vnd.github.raw", ct);
}
