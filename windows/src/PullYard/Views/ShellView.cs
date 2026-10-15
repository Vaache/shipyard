using System.ComponentModel;
using Microsoft.UI.Input;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;
using PullYard.Controls;
using PullYard.Core.Api;
using PullYard.Core.Claude;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public sealed class ShellView : Grid
{
    public static ShellView? Current { get; private set; }

    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly NavigationView nav = new();
    private readonly Grid body = new();
    private readonly ContentControl listHost = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly Grid detailArea = new();
    private readonly ContentControl detailHost = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly Border clipboardBanner = new();
    private readonly StackPanel footer = Ui.V(0);
    private readonly ColumnDefinition listColumn = new() { Width = new GridLength(380) };
    private readonly Dictionary<string, NavigationViewItem> navItems = new();
    private readonly List<(Route Route, FrameworkElement View)> stack = new();
    private ClaudeWorkspaceView? claudeView;
    private bool syncingSelection;

    public ShellView()
    {
        Current = this;
        BuildNav();
        BuildBody();
        nav.Content = body;
        Children.Add(nav);
        app.PropertyChanged += OnAppChanged;
        app.RouteRequested += OnRoute;
        app.Claude.PropertyChanged += OnClaudeChanged;
        app.Updater.PropertyChanged += OnUpdaterChanged;
        ThemeService.Changed += OnTheme;
        Unloaded += (_, _) =>
        {
            app.PropertyChanged -= OnAppChanged;
            app.RouteRequested -= OnRoute;
            app.Claude.PropertyChanged -= OnClaudeChanged;
            app.Updater.PropertyChanged -= OnUpdaterChanged;
            ThemeService.Changed -= OnTheme;
            if (Current == this) Current = null;
        };
        ShowSection();
        UpdateBadges();
        RenderFooter();
        if (app.DetailRoot is { } root) OnRoute(root, false);
        else ShowEmptyDetail();
    }

    private void OnTheme()
    {
        DispatcherQueue.TryEnqueue(() =>
        {
            BuildNavItems();
            ShowSection(force: true);
            RenderFooter();
            if (stack.Count > 0)
            {
                var top = stack[^1].Route;
                stack.Clear();
                OnRoute(top, false);
            }
            UpdateBackground();
        });
    }

    private void UpdateBackground()
    {
        listHost.Background = ThemeService.IsBrand ? Palette.Brush("list") : null;
        detailArea.Background = ThemeService.IsBrand ? Palette.Brush("panel") : null;
        nav.Resources["NavigationViewExpandedPaneBackground"] = ThemeService.IsBrand ? Palette.Brush("sidebar") : new SolidColorBrush(Microsoft.UI.Colors.Transparent);
    }

    private void BuildNav()
    {
        nav.PaneDisplayMode = NavigationViewPaneDisplayMode.Auto;
        nav.CompactModeThresholdWidth = 0;
        nav.ExpandedModeThresholdWidth = 1180;
        nav.IsBackButtonVisible = NavigationViewBackButtonVisible.Collapsed;
        nav.IsSettingsVisible = false;
        nav.OpenPaneLength = 236;
        nav.IsPaneToggleButtonVisible = true;
        nav.IsTitleBarAutoPaddingEnabled = false;
        nav.SelectionChanged += (_, e) =>
        {
            if (syncingSelection) return;
            if (e.SelectedItem is NavigationViewItem { Tag: string key } && Section.FromKey(key) is { } s) app.Section = s;
        };
        BuildNavItems();
        nav.PaneFooter = footer;
        UpdateBackground();
    }

    private void BuildNavItems()
    {
        nav.MenuItems.Clear();
        navItems.Clear();
        void Header(string text) => nav.MenuItems.Add(new NavigationViewItemHeader { Content = text });
        void Item(Section s, string title, string glyph, string color)
        {
            var item = new NavigationViewItem
            {
                Content = title,
                Tag = s.Key,
                Icon = new FontIcon { Glyph = glyph, FontFamily = Ui.Icons, Foreground = Palette.Brush(color) }
            };
            navItems[s.Key] = item;
            nav.MenuItems.Add(item);
        }
        Header(L.T("Nav_Inbox", "Inbox"));
        Item(Section.Notifications, L.T("Section_Notifications", "Notifications"), "", "amber");
        Header(L.T("Nav_PullRequests", "Pull requests"));
        foreach (var f in Enum.GetValues<PRFilter>()) Item(Section.Prs(f), f.Title(), f.Glyph(), f.ColorKey());
        Item(Section.MyPrsByRepo, L.T("Section_MyPrsByProject", "My PRs by project"), "", "coral");
        Header(L.T("Nav_Issues", "Issues"));
        foreach (var f in Enum.GetValues<IssueFilter>()) Item(Section.Issues(f), f.Title(), f.Glyph(), f.ColorKey());
        Header(L.T("Nav_Code", "Code"));
        Item(Section.Repos, L.T("Section_Repositories", "Repositories"), "", "teal");
        Item(Section.Projects, L.T("Section_Projects", "Projects"), "", "indigo");
        Header(L.T("Nav_AI", "AI"));
        Item(Section.Claude, L.T("Section_Claude", "Claude Code"), "", "coral");
        SyncNavSelection();
        UpdateBadges();
    }

    private void SyncNavSelection()
    {
        if (!navItems.TryGetValue(app.Section.Key, out var item)) return;
        syncingSelection = true;
        nav.SelectedItem = item;
        syncingSelection = false;
    }

    private void UpdateBadges()
    {
        void Badge(string key, int count, string color = "muted")
        {
            if (!navItems.TryGetValue(key, out var item)) return;
            item.InfoBadge = count > 0 ? new InfoBadge { Value = count, Background = Palette.Brush(color, 0.9) } : null;
        }
        Badge(Section.Notifications.Key, app.UnreadCount, "amber");
        Badge(Section.Prs(PRFilter.ReviewRequested).Key, app.ReviewRequestCount, "sky");
        Badge(Section.Prs(PRFilter.Mine).Key, app.MyPRCount, "violet");
        var claude = app.Claude.AttentionCount > 0 ? app.Claude.AttentionCount : app.Claude.RunningCount;
        Badge(Section.Claude.Key, claude, app.Claude.AttentionCount > 0 ? "amber" : "coral");
    }

    private void RenderFooter()
    {
        footer.Children.Clear();
        if (app.Updater.AvailableVersion is { } version)
        {
            var row = new Button
            {
                HorizontalAlignment = HorizontalAlignment.Stretch,
                HorizontalContentAlignment = HorizontalAlignment.Left,
                Background = Palette.Brush("sky", 0.12),
                BorderThickness = new Thickness(0),
                Margin = new Thickness(8, 0, 8, 8),
                Content = Ui.H(8, Ui.Glyph("", 14, "sky"), Ui.Text(L.F("Update_To", "Update to {0}", version), 13, "text", FontWeights.Medium))
            };
            Ui.Tip(row, L.F("Update_Available", "A new version of {0} is available", PullYard.Core.AppInfo.DisplayName));
            row.Click += (_, _) => app.Updater.Present();
            footer.Children.Add(row);
        }
        if (app.User is { } user)
        {
            var settings = Ui.IconButton("", L.T("Common_Settings", "Settings"), app.RequestSettings);
            var info = Ui.V(0, Ui.Text(user.Name ?? user.Login, 13, "text", FontWeights.Medium), Ui.Caption("@" + user.Login));
            var grid = Ui.Columns((Ui.Avatar(user.AvatarUrl, 26), Ui.Auto), (info, Ui.Star), (settings, Ui.Auto));
            grid.Padding = new Thickness(12, 8, 6, 10);
            footer.Children.Add(Ui.Line());
            footer.Children.Add(grid);
        }
    }

    private void BuildBody()
    {
        body.ColumnDefinitions.Add(listColumn);
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1) });
        body.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        Grid.SetColumn(listHost, 0);
        body.Children.Add(listHost);

        var splitter = new Border { Background = Palette.Brush("text", 0.08), Width = 1 };
        var grip = new Border { Width = 9, Margin = new Thickness(-4, 0, -4, 0), Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent) };
        Grid.SetColumn(splitter, 1);
        Grid.SetColumn(grip, 1);
        body.Children.Add(splitter);
        body.Children.Add(grip);
        double startWidth = 0, startX = 0;
        var dragging = false;
        grip.PointerEntered += (_, _) => ProtectedCursor = InputSystemCursor.Create(InputSystemCursorShape.SizeWestEast);
        grip.PointerExited += (_, _) => { if (!dragging) ProtectedCursor = null; };
        grip.PointerPressed += (s, e) =>
        {
            dragging = true;
            startWidth = listColumn.ActualWidth;
            startX = e.GetCurrentPoint(body).Position.X;
            grip.CapturePointer(e.Pointer);
        };
        grip.PointerMoved += (_, e) =>
        {
            if (!dragging) return;
            var x = e.GetCurrentPoint(body).Position.X;
            listColumn.Width = new GridLength(Math.Clamp(startWidth + x - startX, 280, 620));
        };
        grip.PointerReleased += (_, e) =>
        {
            dragging = false;
            grip.ReleasePointerCapture(e.Pointer);
            ProtectedCursor = null;
        };

        Grid.SetColumn(detailArea, 2);
        detailArea.Children.Add(detailHost);
        clipboardBanner.VerticalAlignment = VerticalAlignment.Bottom;
        clipboardBanner.HorizontalAlignment = HorizontalAlignment.Center;
        clipboardBanner.Margin = new Thickness(0, 0, 0, 16);
        clipboardBanner.Visibility = Visibility.Collapsed;
        detailArea.Children.Add(clipboardBanner);
        body.Children.Add(detailArea);
    }

    private void OnUpdaterChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(UpdaterViewModel.State) or nameof(UpdaterViewModel.Release)) RenderFooter();
    }

    private void OnClaudeChanged(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(ClaudeWorkspace.AttentionCount) or nameof(ClaudeWorkspace.RunningCount)) UpdateBadges();
    }

    private void OnAppChanged(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(AppViewModel.Section):
                ShowSection();
                break;
            case nameof(AppViewModel.UnreadCount):
            case nameof(AppViewModel.ReviewRequestCount):
            case nameof(AppViewModel.MyPRCount):
                UpdateBadges();
                break;
            case nameof(AppViewModel.User):
                RenderFooter();
                break;
            case nameof(AppViewModel.ClipboardRoute):
                RenderClipboard();
                break;
        }
    }

    private string? shownSection;

    private void ShowSection(bool force = false)
    {
        SyncNavSelection();
        var s = app.Section;
        if (!force && shownSection == s.Key) return;
        shownSection = s.Key;
        FrameworkElement list = s.Kind switch
        {
            SectionKind.Notifications => new NotificationsView(),
            SectionKind.Prs => new SearchListView(s.Title, "pr", s.Pr.Query(), false),
            SectionKind.MyPrsByRepo => new SearchListView(s.Title, "pr", PRFilter.Mine.Query(), true),
            SectionKind.Issues => new SearchListView(s.Title, "issue", s.Issue.Query(), false),
            SectionKind.Repos => new ReposView(),
            SectionKind.Projects => new ProjectsView(),
            SectionKind.Claude => new ClaudeSessionsView(),
            _ => new Grid()
        };
        list.Transitions = new TransitionCollection { new ContentThemeTransition() };
        listHost.Content = list;
        App.MainWindow?.SetSubtitle(s.Title);
        if (s.Kind == SectionKind.Claude)
        {
            claudeView ??= new ClaudeWorkspaceView();
            detailHost.Content = claudeView;
        }
        else if (stack.Count > 0)
        {
            detailHost.Content = stack[^1].View;
        }
        else
        {
            ShowEmptyDetail();
        }
    }

    private void ShowEmptyDetail()
    {
        if (app.Section.Kind == SectionKind.Claude) return;
        var tile = Ui.IconTile("", "violet", 56);
        tile.HorizontalAlignment = HorizontalAlignment.Center;
        tile.Opacity = 0.9;
        var a = Ui.Text(L.T("Detail_Empty", "Select a pull request, issue or project"), 13, "muted");
        a.HorizontalAlignment = HorizontalAlignment.Center;
        var b = Ui.Text(L.T("Detail_EmptyHint", "Ctrl+L — open by link"), 12, "dim");
        b.HorizontalAlignment = HorizontalAlignment.Center;
        var panel = Ui.V(12, tile, a, b);
        panel.HorizontalAlignment = HorizontalAlignment.Center;
        panel.VerticalAlignment = VerticalAlignment.Center;
        detailHost.Content = panel;
    }

    private void OnRoute(Route route, bool push)
    {
        if (app.Section.Kind == SectionKind.Claude) return;
        if (!push) stack.Clear();
        if (stack.Count > 0 && stack[^1].Route == route) return;
        var view = CreateDetail(route);
        stack.Add((route, view));
        if (stack.Count > 12) stack.RemoveAt(0);
        view.Transitions = new TransitionCollection { new EntranceThemeTransition { FromVerticalOffset = 10, FromHorizontalOffset = 0 } };
        detailHost.Content = view;
    }

    public bool CanGoBack => stack.Count > 1;

    public bool CanGoBackAfterPush() => stack.Count > 0;

    public void GoBack()
    {
        if (stack.Count <= 1) return;
        stack.RemoveAt(stack.Count - 1);
        detailHost.Content = stack[^1].View;
        app.DetailRoot = stack[^1].Route;
    }

    private FrameworkElement CreateDetail(Route route)
    {
        var content = route.Kind switch
        {
            RouteKind.PullRequest => (FrameworkElement)new PRDetailView(route.Item!),
            RouteKind.Issue => new IssueDetailView(route.Item!),
            RouteKind.Repo => new RepoDetailView(route.Repo!),
            RouteKind.Project => new ProjectBoardView(route.Project!),
            RouteKind.Commit => new CommitDetailView(route.Repo!, route.Sha!),
            _ => new Grid()
        };
        return new DetailFrame(content, this);
    }

    private void RenderClipboard()
    {
        if (app.ClipboardRoute is not { } route)
        {
            clipboardBanner.Visibility = Visibility.Collapsed;
            return;
        }
        var title = route.Kind == RouteKind.PullRequest ? $"PR {route.Item!.Title}" : $"Issue {route.Item!.Title}";
        var open = Ui.Button(L.T("Common_Open", "Open"), () =>
        {
            app.Open(route);
            app.ClipboardRoute = null;
        }, accent: true);
        var close = Ui.IconButton("", L.T("Common_Dismiss", "Dismiss"), () => app.ClipboardRoute = null);
        clipboardBanner.Child = Ui.H(10, Ui.Glyph("", 14), Ui.Text(L.F("Clipboard_InClipboard", "In clipboard: {0}", title), 13), open, close);
        clipboardBanner.Padding = new Thickness(14, 6, 6, 6);
        clipboardBanner.CornerRadius = new CornerRadius(20);
        clipboardBanner.Background = Palette.Brush("raised");
        clipboardBanner.BorderBrush = Palette.Brush("text", 0.12);
        clipboardBanner.BorderThickness = new Thickness(1);
        clipboardBanner.Shadow = new ThemeShadow();
        clipboardBanner.Translation = new System.Numerics.Vector3(0, 0, 16);
        clipboardBanner.Visibility = Visibility.Visible;
    }
}

public sealed class DetailFrame : Grid
{
    public DetailFrame(FrameworkElement content, ShellView shell)
    {
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        if (shell.CanGoBackAfterPush())
        {
            var back = Ui.IconButton("", L.T("Common_BackHint", "Back (Alt+Left)"), shell.GoBack);
            back.Margin = new Thickness(8, 4, 0, 0);
            back.HorizontalAlignment = HorizontalAlignment.Left;
            Children.Add(back);
        }
        Grid.SetRow(content, 1);
        Children.Add(content);
    }
}
