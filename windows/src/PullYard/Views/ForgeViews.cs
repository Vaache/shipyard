using System.ComponentModel;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Controls;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public sealed class ForgeListView : ListPane
{
    private readonly PRFilter filter;
    private readonly ForgeListViewModel vm = new();
    private StateFilter stateFilter = StateFilter.Open;
    private string text = "";
    private List<SearchItem> visible = new();
    private bool syncing;

    public ForgeListView(PRFilter filter) : base(filter.Title(), "FeedRowTemplate")
    {
        this.filter = filter;
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

    private Task Reload(bool silent = false) => vm.Load(Model.Forge, Model.GitAccount?.Id ?? "", filter, stateFilter, silent);

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
        List.ItemsSource = visible;
        syncing = false;
        SyncSelection();
        UpdateState();
    }

    private void UpdateState()
    {
        CountText.Text = visible.Count > 0 ? $"{visible.Count} {Model.Provider.RequestShort()}" : "";
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

    private void SyncSelection()
    {
        syncing = true;
        List.SelectedItem = Model.ForgeSelection is { } selected ? visible.FirstOrDefault(i => i.Id == selected.Id) : null;
        syncing = false;
    }

    private void OnSelection(object sender, SelectionChangedEventArgs e)
    {
        if (syncing || List.SelectedItem is not SearchItem item || item.Id == Model.ForgeSelection?.Id) return;
        Model.ForgeSelection = item;
    }

    private void OnContext(UIElement sender, Microsoft.UI.Xaml.Input.ContextRequestedEventArgs args)
    {
        if ((args.OriginalSource as FrameworkElement)?.DataContext is not SearchItem item) return;
        args.Handled = true;
        ItemMenu(item).ShowAt((FrameworkElement)args.OriginalSource);
    }

    public static MenuFlyout ItemMenu(SearchItem item) => Ui.Menu(
        Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(item.Url), "\uE774"),
        Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(item.Url), "\uE71B"),
        item.BaseRefName is { } baseRef ? Ui.MenuItem(L.T("Menu_CopyTargetBranch", "Copy Target Branch"), () => WindowInterop.Copy(baseRef), "\uE8C8") : null);
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
