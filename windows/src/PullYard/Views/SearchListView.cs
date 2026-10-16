using System.ComponentModel;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Controls;
using PullYard.Core.Api;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public sealed class SearchListView : ListPane
{
    private readonly string kind;
    private readonly string baseQuery;
    private readonly bool grouped;
    private readonly SearchListViewModel vm = new();
    private StateFilter stateFilter = StateFilter.Open;
    private string text = "";
    private readonly HashSet<string> collapsed = new();
    private bool syncing;

    public SearchListView(string title, string kind, string baseQuery, bool grouped) : base(title, "FeedRowTemplate")
    {
        this.kind = kind;
        this.baseQuery = baseQuery;
        this.grouped = grouped;
        FilterRow.Children.Add(StateBar(stateFilter, f =>
        {
            stateFilter = f;
            Reload().Forget();
        }));
        FilterRow.Children.Add(SearchBox(L.T("Search_Placeholder", "Filter: text, repo:org/name, label:bug"), t =>
        {
            text = t.Trim();
            Reload(typing: true).Forget();
        }));
        List.SelectionChanged += OnSelection;
        List.ItemClick += OnItemClick;
        List.ContainerContentChanging += (_, e) =>
        {
            if (!grouped && e.Item is SearchItem item && vm.Items.Count > 0 && ReferenceEquals(item, vm.Items[^1])) vm.LoadMore(Model.Api).Forget();
        };
        List.ContextRequested += OnContext;
        vm.PropertyChanged += OnVm;
        Model.PropertyChanged += OnModel;
        Unloaded += (_, _) => Model.PropertyChanged -= OnModel;
        Loaded += (_, _) =>
        {
            Model.PropertyChanged -= OnModel;
            Model.PropertyChanged += OnModel;
        };
    }

    private string Query => string.Join(" ", new[] { baseQuery, $"is:{kind}", stateFilter.Query(), "archived:false", "sort:updated-desc", text }.Where(s => s.Length > 0));

    protected override void OnAttached() => Reload().Forget();

    protected override void OnLiveTick() => Reload(silent: true).Forget();

    protected override Task OnPullRefresh() => Reload(silent: true);

    private async Task Reload(bool silent = false, bool typing = false)
    {
        await vm.Load(Model.Api, Query, silent || typing);
        if (grouped) await vm.LoadAll(Model.Api);
    }

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(AppViewModel.DetailRoot)) SyncSelection();
        if (e.PropertyName == nameof(AppViewModel.Api)) Reload().Forget();
    }

    private void OnVm(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(SearchListViewModel.Items):
                Render();
                break;
            case nameof(SearchListViewModel.Loading):
            case nameof(SearchListViewModel.Refreshing):
            case nameof(SearchListViewModel.Error):
            case nameof(SearchListViewModel.LastUpdated):
            case nameof(SearchListViewModel.Total):
                UpdateState();
                break;
        }
    }

    private void UpdateState()
    {
        CountText.Text = vm.Total > 0 ? vm.Total.ToString() : "";
        SetLoading(vm.Loading, vm.Items.Count > 0);
        Footer.Update(vm.LastUpdated, vm.Refreshing && !vm.Loading, vm.Items.Count == 0 ? null : vm.Error);
        if (vm.Items.Count == 0)
        {
            if (vm.Loading) ShowOverlay(Ui.Loading());
            else if (vm.Error is { } e) ShowOverlay(Ui.Error(e, () => Reload().Forget()));
            else ShowOverlay(Ui.Empty("", "green", L.T("Common_NothingHere", "Nothing here")));
        }
        else
        {
            ShowOverlay(null);
        }
    }

    private void Render()
    {
        syncing = true;
        if (grouped)
        {
            var rows = new List<object>();
            foreach (var g in vm.Groups)
            {
                var open = !collapsed.Contains(g.Id) || text.Length > 0;
                rows.Add(new GroupHeaderItem { Group = g, Expanded = open });
                if (open) rows.AddRange(g.Items.Select(i => (object)new GroupedSearchItem(i)));
            }
            List.ItemsSource = rows;
        }
        else
        {
            List.ItemsSource = vm.Items;
        }
        syncing = false;
        SyncSelection();
        UpdateState();
    }

    private void SyncSelection()
    {
        syncing = true;
        var route = Model.DetailRoot;
        object? match = null;
        if (route is not null && List.ItemsSource is System.Collections.IEnumerable items)
        {
            foreach (var i in items)
            {
                var r = i switch
                {
                    SearchItem s => s.Route,
                    GroupedSearchItem g => g.Item.Route,
                    _ => null
                };
                if (r == route)
                {
                    match = i;
                    break;
                }
            }
        }
        List.SelectedItem = match;
        syncing = false;
    }

    private void OnSelection(object sender, SelectionChangedEventArgs e)
    {
        if (syncing) return;
        var route = List.SelectedItem switch
        {
            SearchItem s => s.Route,
            GroupedSearchItem g => g.Item.Route,
            _ => null
        };
        if (route is not null && route != Model.DetailRoot) Model.Select(route);
    }

    private void OnItemClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is GroupHeaderItem h)
        {
            if (!collapsed.Remove(h.Group.Id)) collapsed.Add(h.Group.Id);
            Render();
        }
    }

    private void OnContext(UIElement sender, Microsoft.UI.Xaml.Input.ContextRequestedEventArgs args)
    {
        var element = args.OriginalSource as FrameworkElement;
        var item = element?.DataContext switch
        {
            SearchItem s => s,
            GroupedSearchItem g => g.Item,
            _ => null
        };
        if (item is null) return;
        args.Handled = true;
        ItemMenu(item).ShowAt(element!);
    }

    public static MenuFlyout ItemMenu(SearchItem item)
    {
        var menu = Ui.Menu(
            Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(item.Url), ""),
            Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(item.Url), ""));
        if (item.IsPR) menu.Items.Add(Ui.MenuItem(L.T("Menu_CopyCheckout", "Copy “gh pr checkout”"), () => WindowInterop.Copy($"gh pr checkout {item.Number} -R {item.Repository.NameWithOwner}"), ""));
        return menu;
    }
}

public sealed class NotificationsView : ListPane
{
    private List<NotificationThread> items = new();
    private bool loading;
    private string? error;
    private bool showAll;
    private DateTimeOffset? lastUpdated;
    private bool syncing;

    public NotificationsView() : base(L.T("Section_Notifications", "Notifications"), "NotificationRowTemplate")
    {
        var bar = new SelectorBar();
        var unread = new SelectorBarItem { Text = L.T("Notifications_Unread", "Unread") };
        var all = new SelectorBarItem { Text = L.T("State_All", "All") };
        bar.Items.Add(unread);
        bar.Items.Add(all);
        bar.SelectedItem = unread;
        bar.SelectionChanged += (s, _) =>
        {
            showAll = s.SelectedItem == all;
            Load(true).Forget();
        };
        FilterRow.Children.Add(bar);
        Tools.Children.Add(Ui.IconButton("", L.T("Notifications_MarkAllRead", "Mark all as read"), () => MarkAll().Forget()));
        List.SelectionChanged += (_, _) =>
        {
            if (syncing || List.SelectedItem is not NotificationThread n) return;
            if (n.Route is { } r) Model.Select(r);
            else LinkBrowser.Open(n.WebUrl);
            if (n.Unread) MarkRead(n).Forget();
        };
        List.ContextRequested += (s, args) =>
        {
            if ((args.OriginalSource as FrameworkElement)?.DataContext is not NotificationThread n) return;
            args.Handled = true;
            var menu = Ui.Menu(
                n.Unread ? Ui.MenuItem(L.T("Notifications_MarkRead", "Mark as read"), () => MarkRead(n).Forget()) : null,
                Ui.MenuItem(L.T("Notifications_Done", "Done (remove from inbox)"), () => MarkDone(n).Forget()),
                Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(n.WebUrl)));
            menu.ShowAt((FrameworkElement)args.OriginalSource);
        };
    }

    protected override void OnAttached() => Load(true).Forget();

    protected override void OnLiveTick() => Load(false, live: true).Forget();

    protected override Task OnPullRefresh() => Load(false);

    private async Task Load(bool reset, bool live = false)
    {
        if (Model.Api is not { } api) return;
        loading = true;
        Update();
        var changed = true;
        try
        {
            var mode = showAll;
            var fetched = reset ? await api.Notifications(mode) : await api.NotificationsIfChanged(mode, live);
            if (mode != showAll) return;
            lastUpdated = DateTimeOffset.Now;
            error = null;
            if (fetched is null)
            {
                changed = false;
                return;
            }
            items = fetched;
            if (!showAll) Model.UnreadCount = items.Count;
        }
        catch (Exception e)
        {
            error = e.Message;
        }
        finally
        {
            loading = false;
            if (changed) Render();
            else Update();
        }
    }

    private void Render()
    {
        syncing = true;
        List.ItemsSource = items.ToList();
        syncing = false;
        Update();
    }

    private void Update()
    {
        SetLoading(loading, items.Count > 0);
        Footer.Update(lastUpdated, loading, items.Count == 0 ? null : error);
        if (items.Count == 0)
        {
            if (loading) ShowOverlay(Ui.Loading());
            else if (error is { } e) ShowOverlay(Ui.Error(e, () => Load(true).Forget()));
            else ShowOverlay(Ui.Empty(showAll ? "" : "", showAll ? "slate" : "green", showAll ? L.T("Notifications_None", "No notifications") : L.T("Notifications_CaughtUp", "All caught up")));
        }
        else
        {
            ShowOverlay(null);
        }
    }

    private async Task MarkRead(NotificationThread n)
    {
        if (Model.Api is not { } api) return;
        try
        {
            await api.MarkNotificationRead(n.Id);
        }
        catch
        {
        }
        n.Unread = false;
        Model.UnreadCount = Math.Max(0, Model.UnreadCount - 1);
        Render();
    }

    private async Task MarkDone(NotificationThread n)
    {
        if (Model.Api is not { } api) return;
        try
        {
            await api.MarkNotificationDone(n.Id);
        }
        catch
        {
        }
        items.RemoveAll(i => i.Id == n.Id);
        if (n.Unread) Model.UnreadCount = Math.Max(0, Model.UnreadCount - 1);
        Render();
    }

    private async Task MarkAll()
    {
        if (Model.Api is not { } api) return;
        try
        {
            await api.MarkAllNotificationsRead();
        }
        catch
        {
        }
        foreach (var i in items) i.Unread = false;
        Model.UnreadCount = 0;
        Render();
    }
}

public sealed class ReposView : ListPane
{
    private List<RepoSummary> repos = new();
    private string? cursor;
    private bool hasMore;
    private bool loading;
    private bool refreshing;
    private string? error;
    private string text = "";
    private bool hideArchived = true;
    private DateTimeOffset? lastUpdated;
    private bool syncing;

    public ReposView() : base(L.T("Section_Repositories", "Repositories"), "RepoRowTemplate")
    {
        FilterRow.Children.Add(SearchBox(L.T("Repos_Filter", "Filter repositories"), t =>
        {
            text = t.Trim();
            Render();
            if (text.Length > 0 && hasMore) LoadAll().Forget();
        }, 200));
        Tools.Children.Add(Toggle("", L.T("Repos_HideArchived", "Hide archived"), hideArchived, on =>
        {
            hideArchived = on;
            Render();
        }));
        List.SelectionChanged += (_, _) =>
        {
            if (syncing || List.SelectedItem is not RepoSummary r || r.Ref is not { } rr) return;
            Model.Select(Route.ForRepo(rr));
        };
        List.ContainerContentChanging += (_, e) =>
        {
            if (e.Item is RepoSummary r && repos.Count > 0 && ReferenceEquals(r, repos[^1])) LoadMore().Forget();
        };
        List.ContextRequested += (s, args) =>
        {
            if ((args.OriginalSource as FrameworkElement)?.DataContext is not RepoSummary r) return;
            args.Handled = true;
            Ui.Menu(
                Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(r.Url)),
                Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(r.Url)),
                Ui.MenuItem(L.T("Repos_CopyClone", "Copy “gh repo clone”"), () => WindowInterop.Copy($"gh repo clone {r.NameWithOwner}"))).ShowAt((FrameworkElement)args.OriginalSource);
        };
    }

    protected override void OnAttached()
    {
        if (repos.Count == 0) Load().Forget();
    }

    protected override Task OnPullRefresh() => Load();

    private IEnumerable<RepoSummary> Visible => repos.Where(r => (!hideArchived || !r.IsArchived) &&
        (text.Length == 0 || r.NameWithOwner.Contains(text, StringComparison.OrdinalIgnoreCase) || (r.Description ?? "").Contains(text, StringComparison.OrdinalIgnoreCase)));

    private async Task Load()
    {
        if (Model.Api is not { } api) return;
        loading = true;
        refreshing = true;
        Update();
        try
        {
            var r = await api.Repos(null);
            var fresh = r.Nodes.Select(n => n.Id).ToHashSet();
            var keep = repos.Count > r.Nodes.Count ? repos.Skip(r.Nodes.Count).Where(n => !fresh.Contains(n.Id)).ToList() : new List<RepoSummary>();
            repos = r.Nodes.Concat(keep).ToList();
            if (keep.Count == 0)
            {
                cursor = r.PageInfo.EndCursor;
                hasMore = r.PageInfo.HasNextPage;
            }
            lastUpdated = DateTimeOffset.Now;
            error = null;
        }
        catch (Exception e)
        {
            error = e.Message;
        }
        finally
        {
            loading = false;
            refreshing = false;
            Render();
        }
    }

    private async Task LoadMore()
    {
        if (Model.Api is not { } api || !hasMore || loading) return;
        loading = true;
        try
        {
            var r = await api.Repos(cursor);
            var known = repos.Select(x => x.Id).ToHashSet();
            repos.AddRange(r.Nodes.Where(n => !known.Contains(n.Id)));
            cursor = r.PageInfo.EndCursor;
            hasMore = r.PageInfo.HasNextPage;
        }
        catch
        {
        }
        finally
        {
            loading = false;
            Render();
        }
    }

    private async Task LoadAll()
    {
        for (var i = 0; i < 20 && hasMore; i++) await LoadMore();
    }

    private void Render()
    {
        syncing = true;
        var visible = Visible.ToList();
        List.ItemsSource = visible;
        if (Model.DetailRoot is { Kind: RouteKind.Repo } route) List.SelectedItem = visible.FirstOrDefault(r => r.Ref == route.Repo);
        syncing = false;
        Update();
    }

    private void Update()
    {
        SetLoading(refreshing, repos.Count > 0);
        Footer.Update(lastUpdated, refreshing, repos.Count == 0 ? null : error);
        if (repos.Count == 0)
        {
            if (loading) ShowOverlay(Ui.Loading());
            else if (error is { } e) ShowOverlay(Ui.Error(e, () => Load().Forget()));
            else ShowOverlay(Ui.Empty("", "teal", L.T("Common_NothingHere", "Nothing here")));
        }
        else
        {
            ShowOverlay(null);
        }
    }
}

public sealed class ProjectsView : ListPane
{
    private List<ProjectSummary> projects = new();
    private bool loading;
    private string? error;
    private bool showClosed;
    private DateTimeOffset? lastUpdated;
    private bool syncing;
    private readonly ContentControl hint = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, IsTabStop = false };

    public ProjectsView() : base(L.T("Section_Projects", "Projects"), "ProjectRowTemplate")
    {
        Tools.Children.Add(Toggle("", L.T("Projects_ShowClosed", "Show closed"), showClosed, on =>
        {
            showClosed = on;
            Render();
        }));
        FilterRow.Children.Add(hint);
        List.SelectionChanged += (_, _) =>
        {
            if (syncing || List.SelectedItem is not ProjectSummary p) return;
            Model.Select(Route.ForProject(p.Ref));
        };
        List.ContextRequested += (s, args) =>
        {
            if ((args.OriginalSource as FrameworkElement)?.DataContext is not ProjectSummary p) return;
            args.Handled = true;
            Ui.Menu(Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(p.Url))).ShowAt((FrameworkElement)args.OriginalSource);
        };
    }

    protected override void OnAttached() => Load().Forget();

    protected override void OnLiveTick() => Load().Forget();

    protected override Task OnPullRefresh() => Load();

    private async Task Load()
    {
        if (Model.Api is not { } api) return;
        loading = true;
        Update();
        try
        {
            projects = await api.Projects();
            lastUpdated = DateTimeOffset.Now;
            error = null;
        }
        catch (Exception e)
        {
            error = e.Message;
        }
        finally
        {
            loading = false;
            Render();
        }
    }

    private void Render()
    {
        syncing = true;
        var visible = projects.Where(p => showClosed || !p.Closed).ToList();
        List.ItemsSource = visible;
        if (Model.DetailRoot is { Kind: RouteKind.Project } route) List.SelectedItem = visible.FirstOrDefault(p => p.Id == route.Project!.Id);
        syncing = false;
        Update();
    }

    private void Update()
    {
        var scopeProblem = !Model.HasProjectScope || (error?.Contains("scope", StringComparison.OrdinalIgnoreCase) ?? false);
        hint.Content = scopeProblem ? ScopeHint() : null;
        SetLoading(loading, projects.Count > 0);
        Footer.Update(lastUpdated, loading, projects.Count == 0 ? null : error);
        if (projects.Count == 0)
        {
            if (loading) ShowOverlay(Ui.Loading());
            else if (error is { } e && !scopeProblem) ShowOverlay(Ui.Error(e, () => Load().Forget()));
            else ShowOverlay(Ui.Empty("", "indigo", L.T("Projects_None", "No projects")));
        }
        else
        {
            ShowOverlay(null);
        }
    }

    private FrameworkElement ScopeHint()
    {
        const string command = "gh auth refresh -s read:project,project";
        var code = Ui.Card(Ui.Text(command, 12, "text", mono: true, select: true), 6, "sunken", 8);
        var copy = Ui.Button(L.T("Projects_CopyCommand", "Copy Command"), () => WindowInterop.Copy(command));
        var refresh = Ui.Button(L.T("Projects_RefreshToken", "Refresh Token from gh"), () => RefreshToken().Forget());
        var panel = Ui.V(8,
            Ui.H(8, Ui.IconTile("", "amber", 22), Ui.Text(L.T("Projects_MissingScopes", "Token is missing Projects scopes"), 13, "text", Microsoft.UI.Text.FontWeights.SemiBold)),
            Ui.Text(L.T("Projects_ScopeHelp", "Run this in a terminal, then click “Refresh Token”:"), 12, "muted", wrap: true),
            code,
            Ui.H(8, copy, refresh));
        var card = new Border { Background = Palette.Brush("amber", 0.08), CornerRadius = new CornerRadius(10), Padding = new Thickness(10), Child = panel };
        return card;
    }

    private async Task RefreshToken()
    {
        await Model.RefreshTokenFromGh();
        Model.Refresh();
        await Load();
    }
}
