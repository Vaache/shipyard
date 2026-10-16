using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.ViewModels;

public sealed partial class SearchListViewModel : ObservableObject
{
    private sealed record Snapshot(List<SearchItem> Items, int Total, string? Cursor, bool HasMore, DateTimeOffset Updated);

    private static readonly Dictionary<string, Snapshot> Cache = new();

    [ObservableProperty] private List<SearchItem> items = new();
    [ObservableProperty] private int total;
    [ObservableProperty] private bool loading;
    [ObservableProperty] private bool refreshing;
    [ObservableProperty] private DateTimeOffset? lastUpdated;
    [ObservableProperty] private string? error;

    private string? cursor;
    private bool hasMore;
    private bool loadingMore;
    private string currentQuery = "";
    private string? currentKey;
    private int generation;
    private bool fetched;

    public HashSet<string> Fresh { get; } = new();

    private static string? Account => Demo.IsOn ? null : AppViewModel.Shared.Account;

    private static string CacheKey(string? account, string query) => $"{account}\n{query}";

    public static void ClearCache() => Cache.Clear();

    public async Task Load(GitHubApi? api, string query, bool silent = false, bool persist = true)
    {
        if (api is null) return;
        var account = persist ? Account : null;
        var key = CacheKey(Account, query);
        var switched = false;
        if (key != currentKey)
        {
            switched = true;
            currentKey = key;
            currentQuery = query;
            fetched = false;
            Error = null;
            if (Cache.TryGetValue(key, out var snap)) Apply(snap, false);
            else
            {
                Items = new();
                Total = 0;
                LastUpdated = null;
                if (account is not null) ShowStored(account, query, key).Forget();
            }
        }
        var gen = ++generation;
        if (switched ? Items.Count == 0 : !silent) Loading = true;
        Refreshing = true;
        try
        {
            var r = await api.Search(query);
            if (gen != generation) return;
            var merged = r.Nodes.ToList();
            var nextCursor = r.PageInfo?.EndCursor;
            var more = r.PageInfo?.HasNextPage ?? false;
            if (!switched && fetched && Items.Count > r.Nodes.Count)
            {
                var fresh = r.Nodes.Select(n => n.Id).ToHashSet();
                merged.AddRange(Items.Skip(r.Nodes.Count).Where(i => !fresh.Contains(i.Id)));
                nextCursor = cursor;
                more = hasMore;
            }
            var snapshot = new Snapshot(merged, r.IssueCount, nextCursor, more, DateTimeOffset.Now);
            Cache[key] = snapshot;
            fetched = true;
            Apply(snapshot, !switched);
            Error = null;
            if (account is not null && account == Account) ListSnapshotStore.Shared.Save(account, query, new ListSnapshot(r.Nodes, r.IssueCount, snapshot.Updated)).Forget();
        }
        catch (Exception e)
        {
            if (gen != generation) return;
            if (e is not OperationCanceledException) Error = e.Message;
        }
        finally
        {
            if (gen == generation)
            {
                Loading = false;
                Refreshing = false;
            }
        }
    }

    private async Task ShowStored(string? account, string query, string key)
    {
        var stored = await ListSnapshotStore.Shared.LoadAsync(account, query);
        if (stored is null || fetched || key != currentKey || Items.Count > 0) return;
        Apply(new Snapshot(stored.Items, stored.Total, null, false, stored.Updated), false);
        Loading = false;
    }

    private void Apply(Snapshot snap, bool markFresh)
    {
        Fresh.Clear();
        if (markFresh)
        {
            var known = Items.Select(i => i.Id).ToHashSet();
            foreach (var i in snap.Items.Where(i => !known.Contains(i.Id))) Fresh.Add(i.Id);
        }
        Items = snap.Items;
        Total = snap.Total;
        cursor = snap.Cursor;
        hasMore = snap.HasMore;
        LastUpdated = snap.Updated;
    }

    public bool HasMore => hasMore;

    public async Task LoadMore(GitHubApi? api)
    {
        if (api is null || !hasMore || loadingMore || cursor is null) return;
        var query = currentQuery;
        loadingMore = true;
        try
        {
            var r = await api.Search(query, cursor);
            if (query != currentQuery) return;
            var known = Items.Select(i => i.Id).ToHashSet();
            var added = r.Nodes.Where(n => !known.Contains(n.Id)).ToList();
            Items = Items.Concat(added).ToList();
            cursor = r.PageInfo?.EndCursor;
            hasMore = r.PageInfo?.HasNextPage ?? false;
            if (currentKey is { } key) Cache[key] = new Snapshot(Items, Total, cursor, hasMore, LastUpdated ?? DateTimeOffset.Now);
        }
        catch
        {
        }
        finally
        {
            loadingMore = false;
        }
    }

    public async Task LoadAll(GitHubApi? api, int limit = 400)
    {
        while (hasMore && Items.Count < limit)
        {
            var before = Items.Count;
            await LoadMore(api);
            if (Items.Count == before) return;
        }
    }

    public List<RepoGroup> Groups
    {
        get
        {
            var order = new List<string>();
            var byRepo = new Dictionary<string, List<SearchItem>>();
            foreach (var item in Items)
            {
                var key = item.Repository.NameWithOwner;
                if (!byRepo.TryGetValue(key, out var l))
                {
                    byRepo[key] = l = new();
                    order.Add(key);
                }
                l.Add(item);
            }
            return order.Select(k => new RepoGroup(k, byRepo[k])).ToList();
        }
    }
}

public sealed record RepoGroup(string Id, List<SearchItem> Items)
{
    public string Owner => Id.Split('/').FirstOrDefault() ?? "";
    public string Name => Id.Split('/').LastOrDefault() ?? Id;
}

public sealed partial class IssueDetailViewModel : ObservableObject
{
    public ItemRef Ref { get; }
    public GitHubApi Api { get; }

    [ObservableProperty] private IssueDetail? issue;
    [ObservableProperty] private bool isPR;
    [ObservableProperty] private string html = "";
    [ObservableProperty] private bool loading;
    [ObservableProperty] private bool busy;
    [ObservableProperty] private DateTimeOffset? lastUpdated;
    [ObservableProperty] private string? refreshError;
    [ObservableProperty] private string? error;
    [ObservableProperty] private string? actionError;
    [ObservableProperty] private int scrollToBottomToken;

    public IssueDetailViewModel(ItemRef r, GitHubApi api)
    {
        Ref = r;
        Api = api;
    }

    public async Task Load()
    {
        Loading = true;
        try
        {
            var r = await Api.Issue(Ref);
            if (r.Repository?.IssueOrPullRequest is not { } node)
            {
                Error = $"Issue {Ref.Title} not found";
                return;
            }
            if (node.Typename == "PullRequest")
            {
                IsPR = true;
                return;
            }
            Issue = node;
            var h = PullYard.Core.Html.ConversationHtml.Render(node);
            if (h != Html) Html = h;
            LastUpdated = DateTimeOffset.Now;
            Error = null;
            RefreshError = null;
        }
        catch (Exception e)
        {
            if (Issue is null) Error = e.Message;
            else RefreshError = e.Message;
        }
        finally
        {
            Loading = false;
        }
    }

    private async Task<bool> Run(Func<Task> op)
    {
        Busy = true;
        try
        {
            await op();
            await Load();
            return true;
        }
        catch (Exception e)
        {
            ActionError = e.Message;
            return false;
        }
        finally
        {
            Busy = false;
        }
    }

    public async Task<bool> Comment(string body)
    {
        if (Issue is not { } issue) return false;
        var ok = await Run(() => Api.AddComment(issue.Id, body));
        if (ok) ScrollToBottomToken++;
        return ok;
    }

    public Task<bool> SetOpen(bool open) =>
        Issue is { } issue ? Run(() => Api.Mutate(open ? Q.ReopenIssue : Q.CloseIssue, GitHubApiVars.Of(("id", issue.Id)))) : Task.FromResult(false);
}

public sealed partial class ProjectBoardViewModel : ObservableObject
{
    public ProjectRef Ref { get; }
    public GitHubApi Api { get; }

    [ObservableProperty] private ProjectDetail? detail;
    [ObservableProperty] private List<ProjectItem> items = new();
    [ObservableProperty] private string? fieldId;
    [ObservableProperty] private bool loading;
    [ObservableProperty] private DateTimeOffset? lastUpdated;
    [ObservableProperty] private string? error;
    [ObservableProperty] private string? actionError;

    public ProjectBoardViewModel(ProjectRef r, GitHubApi api)
    {
        Ref = r;
        Api = api;
    }

    public List<ProjectField> GroupFields => (Detail?.Fields.Items ?? new()).Where(f => f.Options is not null && f.Id is not null).ToList();

    public ProjectField? Field => GroupFields.FirstOrDefault(f => f.Id == FieldId);

    public async Task Load()
    {
        Loading = true;
        try
        {
            var (d, all) = await Api.Project(Ref.Id);
            Detail = d;
            if (FieldId is null || !GroupFields.Any(f => f.Id == FieldId))
                FieldId = (GroupFields.FirstOrDefault(f => string.Equals(f.Name, "status", StringComparison.OrdinalIgnoreCase)) ?? GroupFields.FirstOrDefault())?.Id;
            Items = all.Where(i => !i.IsArchived).ToList();
            LastUpdated = DateTimeOffset.Now;
            Error = null;
        }
        catch (Exception e)
        {
            Error = e.Message;
        }
        finally
        {
            Loading = false;
        }
    }

    public async Task Move(string itemId, string? optionId)
    {
        if (FieldId is not { } fieldId) return;
        var index = Items.FindIndex(i => i.Id == itemId);
        if (index < 0 || Items[index].OptionId(fieldId) == optionId) return;
        var item = Items[index];
        var backup = item.FieldValues;
        var nodes = item.FieldValues.Items.Where(v => v.Field?.Id != fieldId).ToList();
        if (optionId is not null) nodes.Add(new ProjectFieldValue { Typename = "ProjectV2ItemFieldSingleSelectValue", OptionId = optionId, Field = new FieldRef { Id = fieldId } });
        item.FieldValues = new Nodes<ProjectFieldValue> { Items = nodes };
        Items = Items.ToList();
        try
        {
            await Api.MoveProjectItem(Ref.Id, itemId, fieldId, optionId);
        }
        catch (Exception e)
        {
            item.FieldValues = backup;
            Items = Items.ToList();
            ActionError = e.Message;
        }
    }
}

public sealed record ProjectColumn(string Id, string? OptionId, string Name, string Color, List<ProjectItem> Items);
