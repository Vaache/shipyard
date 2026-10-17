using System.ComponentModel;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Controls;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Forges;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public sealed class ForgeListView : ListPane
{
    private readonly Section kind;
    private readonly bool grouped;
    private readonly ForgeListViewModel vm = new();
    private readonly HashSet<string> collapsed = new();
    private StateFilter stateFilter = StateFilter.Open;
    private string text = "";
    private List<SearchItem> visible = new();
    private bool syncing;

    public ForgeListView(Section kind, string title) : base(title, "FeedRowTemplate")
    {
        this.kind = kind;
        grouped = kind.Kind == SectionKind.MyPrsByRepo;
        FilterRow.Children.Add(StateBar(stateFilter, f =>
        {
            stateFilter = f;
            Reload().Forget();
        }));
        FilterRow.Children.Add(SearchBox(L.T("Forge_SearchPlaceholder", "Filter: title, repository, author"), t =>
        {
            text = t.Trim();
            Render();
        }, 150));
        List.SelectionChanged += OnSelection;
        List.ItemClick += OnItemClick;
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

    protected override void OnAttached() => Reload().Forget();

    protected override void OnLiveTick() => Reload(silent: true).Forget();

    protected override Task OnPullRefresh() => Reload(silent: true);

    private Task Reload(bool silent = false) => vm.Load(Model.Forge, Model.GitAccount?.Id ?? "", kind, stateFilter, silent);

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(AppViewModel.ForgeSelection)) SyncSelection();
        if (e.PropertyName == nameof(AppViewModel.Forge)) Reload().Forget();
    }

    private void OnVm(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(ForgeListViewModel.Items):
                Appear.Ingest(vm.Items.Select(i => i.Id), vm.AppearMode);
                Render();
                break;
            case nameof(ForgeListViewModel.Loading):
            case nameof(ForgeListViewModel.Refreshing):
            case nameof(ForgeListViewModel.Error):
            case nameof(ForgeListViewModel.LastUpdated):
                UpdateState();
                break;
        }
    }

    private static bool Matches(SearchItem item, string q) =>
        item.Title.Contains(q, StringComparison.OrdinalIgnoreCase)
        || item.Repository.NameWithOwner.Contains(q, StringComparison.OrdinalIgnoreCase)
        || (item.Author?.Login.Contains(q, StringComparison.OrdinalIgnoreCase) ?? false)
        || item.Number.ToString() == q.TrimStart('#', '!');

    private void Render()
    {
        syncing = true;
        visible = text.Length == 0 ? vm.Items : vm.Items.Where(i => Matches(i, text)).ToList();
        if (grouped)
        {
            var rows = new List<object>();
            foreach (var g in visible.GroupBy(i => i.Repository.NameWithOwner))
            {
                var group = new RepoGroup(g.Key, g.ToList());
                var open = !collapsed.Contains(g.Key) || text.Length > 0;
                rows.Add(new GroupHeaderItem { Group = group, Expanded = open });
                if (open) rows.AddRange(group.Items.Select(i => (object)new GroupedSearchItem(i)));
            }
            List.ItemsSource = rows;
        }
        else
        {
            List.ItemsSource = visible;
        }
        syncing = false;
        SyncSelection();
        UpdateState();
    }

    private string CountSuffix => kind.Kind == SectionKind.Issues ? "" : " " + Model.Provider.RequestShort();

    private void UpdateState()
    {
        CountText.Text = visible.Count > 0 ? visible.Count + CountSuffix : "";
        SetLoading(vm.Loading, vm.Items.Count > 0);
        Footer.Update(vm.LastUpdated, vm.Refreshing && !vm.Loading, vm.Items.Count == 0 ? null : vm.Error);
        if (visible.Count == 0)
        {
            if (vm.Loading && vm.Items.Count == 0) ShowOverlay(Ui.Loading());
            else if (vm.Error is { } e && vm.Items.Count == 0) ShowOverlay(Ui.Error(e, () => Reload().Forget()));
            else ShowOverlay(Ui.Empty("\uE73E", "green", L.T("Common_NothingHere", "Nothing here")));
        }
        else
        {
            ShowOverlay(null);
        }
    }

    private static SearchItem? ItemOf(object? row) => row switch
    {
        SearchItem s => s,
        GroupedSearchItem g => g.Item,
        _ => null
    };

    private void SyncSelection()
    {
        syncing = true;
        object? match = null;
        if (Model.ForgeSelection is { } selected && List.ItemsSource is System.Collections.IEnumerable rows)
            foreach (var row in rows)
                if (ItemOf(row)?.Id == selected.Id)
                {
                    match = row;
                    break;
                }
        List.SelectedItem = match;
        syncing = false;
    }

    private void OnSelection(object sender, SelectionChangedEventArgs e)
    {
        if (syncing || ItemOf(List.SelectedItem) is not { } item || item.Id == Model.ForgeSelection?.Id) return;
        Model.ForgeSelection = item;
    }

    private void OnItemClick(object sender, ItemClickEventArgs e)
    {
        if (e.ClickedItem is not GroupHeaderItem h) return;
        if (!collapsed.Remove(h.Group.Id)) collapsed.Add(h.Group.Id);
        Render();
    }

    private void OnContext(UIElement sender, Microsoft.UI.Xaml.Input.ContextRequestedEventArgs args)
    {
        if (ItemOf((args.OriginalSource as FrameworkElement)?.DataContext) is not { } item) return;
        args.Handled = true;
        ItemMenu(item).ShowAt((FrameworkElement)args.OriginalSource);
    }

    public static MenuFlyout ItemMenu(SearchItem item) => Ui.Menu(
        Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(item.Url), "\uE774"),
        Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(item.Url), "\uE71B"),
        item.BaseRefName is { } baseRef ? Ui.MenuItem(L.T("Menu_CopyTargetBranch", "Copy Target Branch"), () => WindowInterop.Copy(baseRef), "\uE8C8") : null);
}

public sealed record ForgeNotificationItem(ForgeNotification Notification, Action Done) : IRowContent
{
    public UIElement BuildRow()
    {
        var n = Notification;
        var meta = Ui.H(5, Ui.Caption(n.Reason, "accent"));
        if (n.Repo.Length > 0)
        {
            meta.Children.Add(Ui.Caption("·"));
            meta.Children.Add(Ui.Caption(n.Repo));
        }
        meta.Children.Add(Ui.Caption("·"));
        meta.Children.Add(Ui.Caption(Fmt.Rel(n.UpdatedAt)));
        var body = Ui.V(3, Ui.Text(n.Title, 13.5, n.Unread ? "text" : "muted", FontWeights.SemiBold, maxLines: 2), meta);
        if (n.Author is { } author) body.Children.Add(Ui.Caption($"{n.Kind} · @{author.Login}", "dim"));
        var done = Ui.IconButton("\uE73E", L.T("Forge_MarkDone", "Mark as done"), Done);
        done.VerticalAlignment = VerticalAlignment.Top;
        var avatar = Ui.Avatar(n.Author?.AvatarUrl, 22);
        avatar.VerticalAlignment = VerticalAlignment.Top;
        var grid = Ui.Columns((avatar, Ui.Auto), (body, Ui.Star), (done, Ui.Auto));
        grid.ColumnSpacing = 10;
        grid.Padding = new Thickness(0, 6, 0, 6);
        return grid;
    }
}

public sealed record ForgeRepoItem(ForgeRepo Repo) : IRowContent
{
    public UIElement BuildRow()
    {
        var r = Repo;
        var tile = Ui.IconTile(r.IsPrivate ? "\uE72E" : "\uE8F1", Palette.ProjectKey(r.FullName), 22);
        tile.VerticalAlignment = VerticalAlignment.Top;
        var body = Ui.V(3, Ui.Text(r.FullName, 13.5, "text", FontWeights.SemiBold, maxLines: 1));
        if (!string.IsNullOrEmpty(r.Description)) body.Children.Add(Ui.Text(r.Description, 12, "muted", maxLines: 2));
        var stats = Ui.H(10);
        if (r.DefaultBranch is { } branch) stats.Children.Add(Ui.H(3, Ui.Glyph("\uE8F1", 11), Ui.Caption(branch)));
        if (r.Stars is > 0) stats.Children.Add(Ui.H(3, Ui.Glyph("\uE734", 11, "amber"), Ui.Caption(r.Stars.Value.ToString(), "amber")));
        if (r.UpdatedAt is { } updated) stats.Children.Add(Ui.Caption(L.F("Forge_Updated", "updated {0}", Fmt.Rel(updated)), "dim"));
        body.Children.Add(stats);
        var grid = Ui.Columns((tile, Ui.Auto), (body, Ui.Star));
        grid.ColumnSpacing = 10;
        grid.Padding = new Thickness(0, 5, 0, 5);
        return grid;
    }
}

public sealed class ForgeInboxView : ListPane
{
    private List<ForgeNotification> items = new();
    private bool loading;
    private string? error;
    private DateTimeOffset? lastUpdated;

    public ForgeInboxView(string title) : base(title, "GenericRowTemplate")
    {
        List.IsItemClickEnabled = true;
        List.SelectionMode = ListViewSelectionMode.None;
        List.ItemClick += (_, e) =>
        {
            if (e.ClickedItem is ForgeNotificationItem n) LinkBrowser.Open(n.Notification.Url);
        };
        List.ContextRequested += (_, args) =>
        {
            if ((args.OriginalSource as FrameworkElement)?.DataContext is not ForgeNotificationItem row) return;
            args.Handled = true;
            var n = row.Notification;
            Ui.Menu(
                Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(n.Url), "\uE774"),
                Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(n.Url), "\uE71B"),
                Ui.MenuItem(L.T("Forge_MarkDone", "Mark as done"), () => MarkDone(n).Forget(), "\uE73E")).ShowAt((FrameworkElement)args.OriginalSource);
        };
    }

    protected override void OnAttached() => Load(true).Forget();

    protected override void OnLiveTick() => Load(false).Forget();

    protected override Task OnPullRefresh() => Load(false);

    private async Task Load(bool first)
    {
        if (Model.Forge is not { } forge) return;
        loading = first && items.Count == 0;
        Render();
        try
        {
            var fresh = await forge.Notifications();
            if (!ReferenceEquals(Model.Forge, forge)) return;
            items = fresh;
            Model.UnreadCount = fresh.Count(n => n.Unread);
            lastUpdated = DateTimeOffset.Now;
            error = null;
        }
        catch (Exception e)
        {
            if (e is not OperationCanceledException) error = e.Message;
        }
        finally
        {
            loading = false;
        }
        Render();
    }

    private async Task MarkDone(ForgeNotification n)
    {
        if (Model.Forge is not { } forge) return;
        try
        {
            await forge.MarkDone(n);
            items = items.Where(i => i.Id != n.Id).ToList();
            Model.UnreadCount = items.Count(i => i.Unread);
            Render();
        }
        catch (Exception e)
        {
            await Dialogs.Alert(XamlRoot, L.T("Forge_MarkDoneFailed", "Couldn't mark as done"), e.Message);
        }
    }

    private void Render()
    {
        List.ItemsSource = items.Select(n => new ForgeNotificationItem(n, () => MarkDone(n).Forget())).ToList();
        CountText.Text = items.Count > 0 ? items.Count.ToString() : "";
        Footer.Update(lastUpdated, false, items.Count == 0 ? null : error);
        if (items.Count > 0) ShowOverlay(null);
        else if (loading) ShowOverlay(Ui.Loading());
        else if (error is { } e) ShowOverlay(Ui.Error(e, () => Load(true).Forget()));
        else ShowOverlay(Ui.Empty("\uE73E", "green", L.T("Forge_AllDone", "All done")));
    }
}

public sealed class ForgeReposView : ListPane
{
    private List<ForgeRepo> repos = new();
    private string text = "";
    private bool loading;
    private string? error;
    private DateTimeOffset? lastUpdated;

    public ForgeReposView(string title) : base(title, "GenericRowTemplate")
    {
        FilterRow.Children.Add(SearchBox(L.F("Forge_FilterRepos", "Filter {0}", title.ToLower(Fmt.Culture)), t =>
        {
            text = t.Trim();
            Render();
        }, 150));
        List.IsItemClickEnabled = true;
        List.SelectionMode = ListViewSelectionMode.None;
        List.ItemClick += (_, e) =>
        {
            if (e.ClickedItem is ForgeRepoItem r) LinkBrowser.Open(r.Repo.Url);
        };
        List.ContextRequested += (_, args) =>
        {
            if ((args.OriginalSource as FrameworkElement)?.DataContext is not ForgeRepoItem row) return;
            args.Handled = true;
            var r = row.Repo;
            Ui.Menu(
                Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(r.Url), "\uE774"),
                Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(r.Url), "\uE71B"),
                Ui.MenuItem(L.T("Forge_CopyName", "Copy Name"), () => WindowInterop.Copy(r.FullName), "\uE8C8")).ShowAt((FrameworkElement)args.OriginalSource);
        };
    }

    protected override void OnAttached() => Load().Forget();

    protected override Task OnPullRefresh() => Load();

    private async Task Load()
    {
        if (Model.Forge is not { } forge) return;
        loading = repos.Count == 0;
        Render();
        try
        {
            var fresh = await forge.Repositories();
            if (!ReferenceEquals(Model.Forge, forge)) return;
            repos = fresh;
            lastUpdated = DateTimeOffset.Now;
            error = null;
        }
        catch (Exception e)
        {
            if (e is not OperationCanceledException) error = e.Message;
        }
        finally
        {
            loading = false;
        }
        Render();
    }

    private void Render()
    {
        var visible = text.Length == 0 ? repos : repos.Where(r => r.FullName.Contains(text, StringComparison.OrdinalIgnoreCase) || (r.Description?.Contains(text, StringComparison.OrdinalIgnoreCase) ?? false)).ToList();
        List.ItemsSource = visible.Select(r => new ForgeRepoItem(r)).ToList();
        CountText.Text = repos.Count > 0 ? repos.Count.ToString() : "";
        Footer.Update(lastUpdated, false, repos.Count == 0 ? null : error);
        if (visible.Count > 0) ShowOverlay(null);
        else if (loading) ShowOverlay(Ui.Loading());
        else if (error is { } e && repos.Count == 0) ShowOverlay(Ui.Error(e, () => Load().Forget()));
        else ShowOverlay(Ui.Empty("\uE8F1", "teal", L.T("Common_NothingHere", "Nothing here")));
    }
}

public sealed class ForgeDetailView : Grid
{
    public ForgeDetailView(SearchItem item, GitProvider provider)
    {
        var header = Ui.H(8, Ui.StateBadge(item.VisualState), Ui.Text($"{item.Repository.NameWithOwner}{provider.NumberPrefix()}{item.Number}", 13, "muted", mono: true, select: true));
        var title = Ui.Text(item.Title, 22, "text", FontWeights.SemiBold, wrap: true, select: true);
        var author = Ui.H(6,
            Ui.Avatar(item.Author?.AvatarUrl, 18),
            Ui.Text(item.Author?.Login ?? "", 13, "text", FontWeights.Medium),
            Ui.Text(L.F("Forge_OpenedUpdated", "opened {0} · updated {1}", Fmt.Rel(item.CreatedAt), Fmt.Rel(item.UpdatedAt)), 13, "muted"));
        var stack = Ui.V(14, header, title, author);
        if (item.BaseRefName is { } baseRef) stack.Children.Add(Ui.H(6, Ui.Glyph("\uE8F1", 13), Ui.Text(L.F("Forge_Into", "into {0}", baseRef), 13, "muted", select: true)));
        if (item.Labels?.Items is { Count: > 0 } labels)
        {
            var row = Ui.H(4);
            foreach (var l in labels.Take(8)) row.Children.Add(Ui.LabelChip(l));
            stack.Children.Add(row);
        }
        var open = Ui.Button(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(item.Url), accent: true, glyph: "\uE774");
        var copy = Ui.Button(L.T("Menu_CopyLink", "Copy Link"), () =>
        {
            WindowInterop.Copy(item.Url);
            ShellView.Toast(L.T("Toast_CopiedLink", "Copied link"));
        });
        var buttons = Ui.H(8, open, copy);
        buttons.Margin = new Thickness(0, 4, 0, 0);
        stack.Children.Add(buttons);
        var note = Ui.Columns(
            (Ui.Glyph("\uE946", 14), Ui.Auto),
            (Ui.Text(L.F("Forge_LaterNote", "Changed files, comments and review for {0} are coming in a later version. Open the {1} in the browser to review it.", provider.Title(), provider.RequestTitle()), 13, "muted", wrap: true), Ui.Star));
        note.ColumnSpacing = 8;
        stack.Children.Add(Ui.Card(note, 10, "sunken", 12));
        stack.Padding = new Thickness(24);
        stack.MaxWidth = 760;
        stack.HorizontalAlignment = HorizontalAlignment.Left;
        Children.Add(new ScrollViewer { Content = stack, VerticalScrollBarVisibility = ScrollBarVisibility.Auto });
    }

    public static FrameworkElement Empty(GitProvider provider)
    {
        var tile = Ui.IconTile(provider.Glyph(), "violet", 56);
        tile.HorizontalAlignment = HorizontalAlignment.Center;
        tile.Opacity = 0.9;
        var text = Ui.Text(L.F("Forge_Select", "Select a {0}", provider.RequestTitle()), 13, "muted");
        text.HorizontalAlignment = HorizontalAlignment.Center;
        var panel = Ui.V(12, tile, text);
        panel.HorizontalAlignment = HorizontalAlignment.Center;
        panel.VerticalAlignment = VerticalAlignment.Center;
        return panel;
    }

    public static FrameworkElement Unavailable(GitProvider provider)
    {
        var tile = Ui.IconTile(provider.Glyph(), "dim", 40);
        tile.HorizontalAlignment = HorizontalAlignment.Center;
        var text = Ui.Text(L.F("Forge_Unavailable", "Not available for {0} yet", provider.Title()), 13, "muted");
        text.HorizontalAlignment = HorizontalAlignment.Center;
        var panel = Ui.V(10, tile, text);
        panel.HorizontalAlignment = HorizontalAlignment.Center;
        panel.VerticalAlignment = VerticalAlignment.Center;
        return panel;
    }
}
