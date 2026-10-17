using System.Globalization;
using System.Text.Json;
using System.Text.Json.Nodes;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Util;

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
        public List<ForgeLabel>? Labels { get; set; }
    }

    private sealed class Issue
    {
        public int Iid { get; set; }
        public string Title { get; set; } = "";
        public string WebUrl { get; set; } = "";
        public string State { get; set; } = "";
        public DateTimeOffset CreatedAt { get; set; }
        public DateTimeOffset UpdatedAt { get; set; }
        public Author? Author { get; set; }
        public References? References { get; set; }
        public int? UserNotesCount { get; set; }
        public List<ForgeLabel>? Labels { get; set; }
    }

    private sealed class TodoProject
    {
        public string? PathWithNamespace { get; set; }
    }

    private sealed class TodoTarget
    {
        public int? Iid { get; set; }
        public string? Title { get; set; }
        public string? WebUrl { get; set; }
    }

    private sealed class Todo
    {
        public long Id { get; set; }
        public string ActionName { get; set; } = "";
        public string TargetType { get; set; } = "";
        public string? TargetUrl { get; set; }
        public string? Body { get; set; }
        public string State { get; set; } = "";
        public DateTimeOffset CreatedAt { get; set; }
        public Author? Author { get; set; }
        public TodoProject? Project { get; set; }
        public TodoTarget? Target { get; set; }
    }

    private sealed class ProjectDto
    {
        public string PathWithNamespace { get; set; } = "";
        public string? Description { get; set; }
        public string WebUrl { get; set; } = "";
        public string? Visibility { get; set; }
        public DateTimeOffset? LastActivityAt { get; set; }
        public string? DefaultBranch { get; set; }
        public int? StarCount { get; set; }
    }

    private readonly ForgeHttp http;
    private string? login;
    private bool? approvalFilterWorks;

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
        return new ForgeUser(u.Username, u.Name, u.AvatarUrl, u.Id.ToString(CultureInfo.InvariantCulture));
    }

    public async Task<ForgeCapabilities> Capabilities(CancellationToken ct = default)
    {
        var filters = new List<ForgeFilter> { ForgeFilter.ReviewRequested, ForgeFilter.Mine, ForgeFilter.Assigned, ForgeFilter.Mentioned };
        if (await ApprovalFilterAvailable(ct).ConfigureAwait(false)) filters.Add(ForgeFilter.Reviewed);
        return new ForgeCapabilities(filters, Enum.GetValues<ForgeIssueFilter>(), ForgeInbox.ToDoList, true);
    }

    private static List<(string, string)> ListQuery() => new()
    {
        ("order_by", "updated_at"),
        ("sort", "desc"),
        ("per_page", "50"),
        ("with_labels_details", "true")
    };

    private static string[] States(ForgeState state, params string[] closed) => state switch
    {
        ForgeState.Open => new[] { "opened" },
        ForgeState.Closed => closed,
        _ => new[] { "all" }
    };

    public async Task<List<SearchItem>> PullRequests(ForgeFilter filter, ForgeState state, CancellationToken ct = default)
    {
        var me = await Username(ct).ConfigureAwait(false);
        if (filter == ForgeFilter.Mentioned)
        {
            var targets = await MentionTargets<MergeRequest>("MergeRequest", ct).ConfigureAwait(false);
            return targets.Where(t => t.WebUrl.Length > 0).Select(Item).Where(i => state.Matches(i)).NewestFirst();
        }
        var query = ListQuery();
        switch (filter)
        {
            case ForgeFilter.ReviewRequested:
                query.Add(("scope", "all"));
                query.Add(("reviewer_username", me));
                break;
            case ForgeFilter.Mine:
                query.Add(("scope", "created_by_me"));
                break;
            case ForgeFilter.Assigned:
                query.Add(("scope", "assigned_to_me"));
                break;
            default:
                if (!await ApprovalFilterAvailable(ct).ConfigureAwait(false)) return new();
                query.Add(("scope", "all"));
                query.Add(("approved_by_usernames[]", me));
                break;
        }
        var items = new List<SearchItem>();
        foreach (var s in States(state, "merged", "closed"))
        {
            var page = await http.Get<List<MergeRequest>>("/merge_requests", query.Append(("state", s)), ct).ConfigureAwait(false);
            items.AddRange(page.Select(Item));
        }
        return items.Where(i => filter != ForgeFilter.Reviewed || !string.Equals(i.Author?.Login, me, StringComparison.OrdinalIgnoreCase)).NewestFirst();
    }

    public async Task<List<SearchItem>> Issues(ForgeIssueFilter filter, ForgeState state, CancellationToken ct = default)
    {
        if (filter == ForgeIssueFilter.Mentioned)
        {
            var targets = await MentionTargets<Issue>("Issue", ct).ConfigureAwait(false);
            return targets.Where(t => t.WebUrl.Length > 0).Select(IssueItem).Where(i => state.Matches(i)).NewestFirst();
        }
        var scope = filter == ForgeIssueFilter.Assigned ? "assigned_to_me" : "created_by_me";
        var items = new List<SearchItem>();
        foreach (var s in States(state, "closed"))
        {
            var page = await http.Get<List<Issue>>("/issues", ListQuery().Append(("scope", scope)).Append(("state", s)), ct).ConfigureAwait(false);
            items.AddRange(page.Select(IssueItem));
        }
        return items.NewestFirst();
    }

    public async Task<List<ForgeNotification>> Notifications(CancellationToken ct = default)
    {
        var todos = await http.Get<List<Todo>>("/todos", new[] { ("state", "pending"), ("per_page", "50") }, ct).ConfigureAwait(false);
        return todos.Select(t => new ForgeNotification(
            t.Id.ToString(CultureInfo.InvariantCulture),
            t.Target?.Title ?? t.Body ?? t.TargetType,
            t.Project?.PathWithNamespace ?? "",
            t.TargetType == "MergeRequest" ? "MR" : t.TargetType,
            Reason(t.ActionName),
            t.TargetUrl ?? t.Target?.WebUrl ?? $"https://{Host}/dashboard/todos",
            t.CreatedAt,
            t.Author is { } a ? new Actor { Login = a.Username, AvatarUrl = a.AvatarUrl } : null,
            t.State == "pending")).ToList();
    }

    public Task MarkDone(ForgeNotification notification, CancellationToken ct = default) =>
        http.Post($"/todos/{Uri.EscapeDataString(notification.Id)}/mark_as_done", ct: ct);

    public async Task<List<ForgeRepo>> Repositories(CancellationToken ct = default)
    {
        var projects = await http.Get<List<ProjectDto>>("/projects", new[] { ("membership", "true"), ("order_by", "last_activity_at"), ("per_page", "100"), ("archived", "false") }, ct).ConfigureAwait(false);
        return projects.Select(p => new ForgeRepo(p.PathWithNamespace, p.Description, p.WebUrl, p.Visibility != "public", p.LastActivityAt, p.DefaultBranch, p.StarCount)).ToList();
    }

    private async Task<List<T>> MentionTargets<T>(string type, CancellationToken ct) where T : class
    {
        var all = new List<T>();
        foreach (var action in new[] { "mentioned", "directly_addressed" })
        {
            foreach (var state in new[] { "pending", "done" })
            {
                var page = await http.Get<JsonArray>("/todos", new[] { ("type", type), ("action", action), ("state", state), ("per_page", "50") }, ct).ConfigureAwait(false);
                foreach (var todo in page.OfType<JsonObject>())
                {
                    if (todo["target"] is not JsonObject target) continue;
                    try
                    {
                        if (target.Deserialize<T>(Json.Rest) is { } value) all.Add(value);
                    }
                    catch (JsonException)
                    {
                    }
                }
            }
        }
        return all;
    }

    private async Task<bool> ApprovalFilterAvailable(CancellationToken ct)
    {
        if (approvalFilterWorks is { } known) return known;
        var probe = "pullyard-probe-" + Guid.NewGuid().ToString("N")[..8];
        try
        {
            var page = await http.Get<List<MergeRequest>>("/merge_requests", new[] { ("scope", "all"), ("state", "all"), ("per_page", "1"), ("approved_by_usernames[]", probe) }, ct).ConfigureAwait(false);
            approvalFilterWorks = page.Count == 0;
            return page.Count == 0;
        }
        catch (Exception e) when (e is ApiException or HttpRequestException)
        {
            return false;
        }
    }

    private async Task<string> Username(CancellationToken ct) => login ?? (await CurrentUser(ct).ConfigureAwait(false)).Login;

    public static string Reason(string action) => action switch
    {
        "assigned" => "Assigned",
        "mentioned" => "Mentioned",
        "directly_addressed" => "Addressed you",
        "build_failed" => "Pipeline failed",
        "marked" => "Added to do",
        "approval_required" => "Approval required",
        "unmergeable" => "Can't merge",
        "merge_train_removed" => "Removed from merge train",
        "review_requested" => "Review requested",
        "review_submitted" => "Review submitted",
        "member_access_requested" => "Access requested",
        _ => CultureInfo.InvariantCulture.TextInfo.ToTitleCase(action.Replace('_', ' '))
    };

    private static SearchItem Item(MergeRequest mr)
    {
        var state = mr.State switch
        {
            "merged" => "MERGED",
            "opened" or "locked" => "OPEN",
            _ => "CLOSED"
        };
        return ForgeItems.Item(mr.Iid, mr.Title, mr.WebUrl, state, mr.Draft ?? mr.WorkInProgress ?? false, mr.CreatedAt, mr.UpdatedAt, mr.TargetBranch,
            mr.Author?.Username, mr.Author?.AvatarUrl, ProjectPath(mr.References?.Full, '!', mr.WebUrl), mr.UserNotesCount, (mr.Labels ?? new()).Select(l => l.Info));
    }

    private static SearchItem IssueItem(Issue i) =>
        ForgeItems.Issue(i.Iid, i.Title, i.WebUrl, i.State == "opened", i.CreatedAt, i.UpdatedAt, i.Author?.Username, i.Author?.AvatarUrl,
            ProjectPath(i.References?.Full, '#', i.WebUrl), i.UserNotesCount, (i.Labels ?? new()).Select(l => l.Info));

    public static string ProjectPath(string? reference, char separator, string url)
    {
        if (reference is not null && reference.LastIndexOf(separator) is var cut and > 0) return reference[..cut];
        return ProjectPath(url);
    }

    public static string ProjectPath(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var u)) return "?";
        var path = u.AbsolutePath;
        var cut = path.IndexOf("/-/", StringComparison.Ordinal);
        return (cut >= 0 ? path[..cut] : path).Trim('/');
    }
}
