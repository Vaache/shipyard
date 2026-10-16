using System.Collections.Specialized;
using System.ComponentModel;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using PullYard.Controls;
using PullYard.Core.Claude;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public sealed class ClaudeWorkspaceView : Grid
{
    private readonly AppViewModel app = AppViewModel.Shared;
    private ClaudeWorkspace Ws => app.Claude;
    private readonly ContentControl host = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly Dictionary<Guid, FrameworkElement> tabViews = new();
    private readonly StackPanel tabBar = Ui.H(4);
    private readonly ContentControl controlBar = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, IsTabStop = false };
    private readonly Grid tabContent = new();
    private readonly ContentControl inspectorHost = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly ColumnDefinition inspectorColumn = new() { Width = new GridLength(380) };
    private Grid? workspace;
    private ClaudeTab? shownTab;
    private ClaudeInspector? inspector;
    private ClaudeAuthView? authView;
    private readonly DispatcherTimer tick = new() { Interval = TimeSpan.FromSeconds(2) };

    public ClaudeWorkspaceView()
    {
        Children.Add(host);
        Loaded += (_, _) =>
        {
            Ws.PropertyChanged -= OnWs;
            Ws.PropertyChanged += OnWs;
            Ws.Tabs.CollectionChanged -= OnTabs;
            Ws.Tabs.CollectionChanged += OnTabs;
            ClaudeAccountStore.Shared.PropertyChanged -= OnAccount;
            ClaudeAccountStore.Shared.PropertyChanged += OnAccount;
            tick.Start();
            Render();
        };
        Unloaded += (_, _) =>
        {
            Ws.PropertyChanged -= OnWs;
            Ws.Tabs.CollectionChanged -= OnTabs;
            ClaudeAccountStore.Shared.PropertyChanged -= OnAccount;
            tick.Stop();
        };
        tick.Tick += (_, _) =>
        {
            foreach (var t in Ws.Tabs) t.UpdateState();
            RenderTabBar();
            RenderControlBar();
        };
        Ws.AttentionRequested += _ =>
        {
            if (App.MainWindow is { } w) WindowInterop.Flash(w);
        };
    }

    private void OnAccount(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(ClaudeAccountStore.Status)) Render();
    }

    private void OnWs(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(ClaudeWorkspace.SelectedTab) or nameof(ClaudeWorkspace.ShowInspector) or nameof(ClaudeWorkspace.Projects)) Render();
    }

    private void OnTabs(object? sender, NotifyCollectionChangedEventArgs e)
    {
        foreach (var id in tabViews.Keys.ToList())
        {
            if (Ws.Tabs.Any(t => t.Id == id)) continue;
            if (tabViews[id] is IDisposable d) d.Dispose();
            tabContent.Children.Remove(tabViews[id]);
            tabViews.Remove(id);
        }
        Render();
    }

    private void Render()
    {
        var status = ClaudeAccountStore.Shared.Status;
        App.MainWindow?.SetSubtitle(Ws.SelectedTab is { } st ? $"{st.Title} — {Fmt.HomeRelative(st.Folder)}" : L.T("Section_Claude", "Claude Code"));
        if (status is ClaudeAccountStatus.LoggedOut or ClaudeAccountStatus.NotInstalled)
        {
            authView ??= new ClaudeAuthView();
            host.Content = authView;
            return;
        }
        if (Ws.SelectedTab is not { } tab)
        {
            host.Content = EmptyState();
            return;
        }
        if (workspace is null) BuildWorkspace();
        host.Content = workspace;
        if (!tabViews.TryGetValue(tab.Id, out var view))
        {
            view = tab.Kind == ClaudeTabKind.Chat ? new ChatView(tab) : new TerminalView(tab);
            tabViews[tab.Id] = view;
            tabContent.Children.Add(view);
        }
        foreach (var (id, v) in tabViews) v.Visibility = id == tab.Id ? Visibility.Visible : Visibility.Collapsed;
        if (view is TerminalView tv) tv.FocusTerminal();
        if (shownTab != tab)
        {
            shownTab = tab;
            inspector = new ClaudeInspector(tab);
            inspectorHost.Content = inspector;
        }
        inspectorColumn.Width = Ws.ShowInspector ? new GridLength(380) : new GridLength(0);
        var revealInspector = Ws.ShowInspector && inspectorHost.Visibility == Visibility.Collapsed;
        inspectorHost.Visibility = Ws.ShowInspector ? Visibility.Visible : Visibility.Collapsed;
        if (revealInspector) DispatcherQueue.TryEnqueue(() => LiveRefresh.Reveal(inspectorHost));
        RenderTabBar();
        RenderControlBar();
    }

    private void BuildWorkspace()
    {
        workspace = new Grid();
        workspace.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star), MinWidth = 420 });
        workspace.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        workspace.ColumnDefinitions.Add(inspectorColumn);
        var main = new Grid();
        main.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        main.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        main.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var barScroll = new ScrollViewer { Content = tabBar, HorizontalScrollBarVisibility = ScrollBarVisibility.Hidden, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled, HorizontalScrollMode = ScrollMode.Enabled, VerticalScrollMode = ScrollMode.Disabled };
        var barHost = new Border { Child = barScroll, Padding = new Thickness(10, 6, 10, 6), Background = ThemeService.IsBrand ? Palette.Brush("bar") : null, BorderBrush = Palette.Brush("text", 0.08), BorderThickness = new Thickness(0, 0, 0, 1) };
        main.Children.Add(barHost);
        var controlHost = new Border { Child = controlBar, Padding = new Thickness(12, 6, 8, 6), Background = ThemeService.IsBrand ? Palette.Brush("bar") : null, BorderBrush = Palette.Brush("text", 0.08), BorderThickness = new Thickness(0, 0, 0, 1) };
        SetRow(controlHost, 1);
        main.Children.Add(controlHost);
        SetRow(tabContent, 2);
        main.Children.Add(tabContent);
        workspace.Children.Add(main);
        var line = Ui.Line(vertical: true);
        SetColumn(line, 1);
        workspace.Children.Add(line);
        SetColumn(inspectorHost, 2);
        workspace.Children.Add(inspectorHost);
    }

    private FrameworkElement EmptyState()
    {
        var tile = Ui.IconTile("", "coral", 56);
        tile.HorizontalAlignment = HorizontalAlignment.Center;
        var title = Ui.Text(L.T("Section_Claude", "Claude Code"), 20, "text", FontWeights.SemiBold);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        var sub = Ui.Text(L.T("Claude_EmptyHelp", "Start a new session in a project or resume one from the history on the left."), 13, "muted", wrap: true);
        sub.TextAlignment = TextAlignment.Center;
        sub.MaxWidth = 420;
        sub.HorizontalAlignment = HorizontalAlignment.Center;
        var list = Ui.V(6);
        list.Width = 380;
        foreach (var p in Ws.Projects.Where(p => p.Exists).Take(6))
        {
            var path = p.Path;
            var row = Ui.Columns((Ui.IconTile("", Palette.ProjectKey(p.Path), 22), Ui.Auto), (Ui.Text(p.Name, 13, "text", FontWeights.Medium), Ui.Star), (Ui.Caption(Fmt.Rel(p.LastActivity), "dim"), Ui.Auto), (Ui.Glyph("", 13, "coral"), Ui.Auto));
            var b = new Button { Content = row, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Stretch, Padding = new Thickness(12, 8, 12, 8), Background = Palette.Brush("raised"), BorderThickness = new Thickness(0), CornerRadius = new CornerRadius(10) };
            b.Click += (_, _) => Ws.NewSession(path);
            list.Children.Add(b);
        }
        var panel = Ui.V(16, tile, title, sub, list);
        list.HorizontalAlignment = HorizontalAlignment.Center;
        panel.HorizontalAlignment = HorizontalAlignment.Center;
        panel.VerticalAlignment = VerticalAlignment.Center;
        return panel;
    }

    private void RenderTabBar()
    {
        if (workspace is null) return;
        tabBar.Children.Clear();
        foreach (var tab in Ws.Tabs)
        {
            var selected = tab == Ws.SelectedTab;
            var stripe = new Border { Width = 3, Height = 14, CornerRadius = new CornerRadius(2), Background = Palette.Brush(Palette.ProjectKey(tab.Folder)) };
            var title = Ui.Text(tab.Title, 13, selected ? "text" : "muted");
            title.MaxWidth = 180;
            var close = Ui.IconButton("", L.T("Common_Close", "Close"), () => Ws.Close(tab), "dim");
            close.Padding = new Thickness(4);
            var content = Ui.H(6, stripe, SessionRow.StateBadge(tab.State, 0, compact: true), Ui.Glyph(tab.Kind == ClaudeTabKind.Chat ? "" : "", 10), title, Ui.Text(tab.ProjectName, 12, "dim"), close);
            var b = new Button
            {
                Content = content,
                Padding = new Thickness(10, 3, 4, 3),
                CornerRadius = new CornerRadius(8),
                Background = selected ? Palette.Brush("accent", 0.14) : Palette.Brush("text", 0.04),
                BorderBrush = selected ? Palette.Brush("accent", 0.45) : Palette.Brush("text", 0.08),
                BorderThickness = new Thickness(1)
            };
            b.Click += (_, _) => Ws.SelectedTab = tab;
            tabBar.Children.Add(b);
        }
        var add = new DropDownButton { Content = Ui.Glyph("", 12, "text"), Padding = new Thickness(8, 4, 8, 4) };
        Ui.Tip(add, L.T("Claude_NewSession", "New session"));
        var menu = new MenuFlyout();
        if (Ws.SelectedTab is { } current)
        {
            var folder = current.Folder;
            menu.Items.Add(Ui.MenuItem(L.F("Claude_ChatIn", "Chat in {0}", current.ProjectName), () => Ws.NewSession(folder, kind: ClaudeTabKind.Chat), ""));
            menu.Items.Add(Ui.MenuItem(L.F("Claude_TerminalIn", "Terminal in {0}", current.ProjectName), () => Ws.NewSession(folder, kind: ClaudeTabKind.Terminal), ""));
            menu.Items.Add(Ui.Separator());
        }
        foreach (var p in Ws.Projects.Where(p => p.Exists).Take(12))
        {
            var path = p.Path;
            menu.Items.Add(Ui.MenuItem(p.Name, () => Ws.NewSession(path)));
        }
        add.Flyout = menu;
        tabBar.Children.Add(add);
    }

    private void RenderControlBar()
    {
        if (workspace is null || Ws.SelectedTab is not { } tab) return;
        var left = Ui.H(6, Ui.IconTile("", Palette.ProjectKey(tab.Folder), 18), Ui.Text(tab.ProjectName, 13, "text", FontWeights.SemiBold));
        if (tab.Git.Branch is { } b && b != "HEAD") left.Children.Add(SessionRow.BranchTag(b));
        var badge = SessionRow.StateBadge(tab.State, tab.MemoryBytes);
        badge.Margin = new Thickness(4, 0, 0, 0);
        left.Children.Add(badge);
        var right = Ui.H(6);
        if (tab.Kind == ClaudeTabKind.Terminal)
        {
            right.Children.Add(ControlMenu("", Ws.Model.Title(), L.T("Claude_Model", "Model"), menu =>
            {
                foreach (var m in Enum.GetValues<ClaudeModelOption>()) menu.Items.Add(Ui.CheckItem(m.Title(), Ws.Model == m, () => { Ws.Model = m; RenderControlBar(); }));
                menu.Items.Add(Ui.Separator());
                menu.Items.Add(Ui.MenuItem(L.T("Claude_ApplyModel", "Apply to current session (/model)"), Ws.ApplyModelToCurrent, enabled: tab.Running && Ws.Model != ClaudeModelOption.Standard));
            }));
            right.Children.Add(ControlMenu("", Ws.Effort == ClaudeEffort.Standard ? L.T("Claude_Effort", "Effort") : Ws.Effort.Raw(), L.T("Claude_Effort", "Effort"), menu =>
            {
                foreach (var e in Enum.GetValues<ClaudeEffort>()) menu.Items.Add(Ui.CheckItem(e.Title(), Ws.Effort == e, () => { Ws.Effort = e; RenderControlBar(); }));
            }));
            right.Children.Add(ControlMenu("", Ws.Mode.Title(), L.T("Claude_PermissionMode", "Permission mode"), menu =>
            {
                foreach (var m in Enum.GetValues<ClaudeMode>()) menu.Items.Add(Ui.CheckItem(m.Title(), Ws.Mode == m, () => { Ws.Mode = m; RenderControlBar(); }));
            }));
        }
        else if (tab.Chat is { } chat && (chat.TotalCost > 0 || chat.SessionId is not null))
        {
            right.Children.Add(Ui.Caption(chat.Model.Replace("claude-", "")));
        }
        var kind = new SelectorBar();
        var chatItem = new SelectorBarItem { Icon = new FontIcon { Glyph = "", FontFamily = Ui.Icons }, Tag = "chat" };
        var termItem = new SelectorBarItem { Icon = new FontIcon { Glyph = "", FontFamily = Ui.Icons }, Tag = "terminal" };
        Ui.Tip(chatItem, L.T("Claude_Chat", "Chat"));
        Ui.Tip(termItem, L.T("Claude_Terminal", "Terminal"));
        kind.Items.Add(chatItem);
        kind.Items.Add(termItem);
        kind.SelectedItem = tab.Kind == ClaudeTabKind.Chat ? chatItem : termItem;
        kind.IsEnabled = !string.IsNullOrEmpty(tab.SessionId);
        kind.SelectionChanged += (s, _) =>
        {
            var wantsTerminal = s.SelectedItem == termItem;
            if (wantsTerminal && tab.Kind == ClaudeTabKind.Chat) Ws.OpenInTerminal(tab);
            else if (!wantsTerminal && tab.Kind == ClaudeTabKind.Terminal) Ws.OpenInChat(tab);
        };
        Ui.Tip(kind, L.T("Claude_ChatOrTerminal", "Chat or terminal (same session)"));
        right.Children.Add(kind);
        var more = Ui.IconButton("", L.T("Common_More", "More"), () => { }, "text");
        more.Flyout = MoreMenu(tab);
        right.Children.Add(more);
        right.Children.Add(Ui.IconButton("", L.T("Claude_ToggleInspector", "Toggle changes panel"), () => Ws.ShowInspector = !Ws.ShowInspector, "text"));
        controlBar.Content = Ui.Columns((left, Ui.Star), (right, Ui.Auto));
    }

    private static DropDownButton ControlMenu(string glyph, string title, string help, Action<MenuFlyout> fill)
    {
        var menu = new MenuFlyout();
        fill(menu);
        var b = new DropDownButton { Content = Ui.H(4, Ui.Glyph(glyph, 12, "text"), Ui.Text(title, 12.5)), Flyout = menu, Padding = new Thickness(8, 3, 8, 3) };
        Ui.Tip(b, help + " — " + L.T("Claude_AppliesToNew", "applies to new and restarted sessions"));
        return b;
    }

    private MenuFlyout MoreMenu(ClaudeTab tab)
    {
        var menu = new MenuFlyout();
        menu.Items.Add(Ui.MenuItem(L.T("Claude_RestartNewSettings", "Restart with new settings"), () => RestartTab(tab).Forget()));
        menu.Items.Add(Ui.MenuItem(L.T("Claude_FreeMemoryNow", "Free memory now"), tab.SleepNow, enabled: tab.Running && tab.State is not (ClaudeTabState.Working or ClaudeTabState.Waiting)));
        var sleep = new MenuFlyoutSubItem { Text = L.T("Claude_SleepAfterIdle", "Sleep after idle") };
        foreach (var m in new[] { 0, 5, 15, 30, 60 })
        {
            var minutes = m;
            sleep.Items.Add(Ui.CheckItem(m == 0 ? L.T("Claude_Never", "Never") : $"{m} min", Ws.IdleMinutes == m, () => Ws.IdleMinutes = minutes));
        }
        menu.Items.Add(sleep);
        var kind = new MenuFlyoutSubItem { Text = L.T("Claude_OpenNewAs", "Open new sessions as") };
        kind.Items.Add(Ui.CheckItem(L.T("Claude_Chat", "Chat"), Ws.PreferredKind == ClaudeTabKind.Chat, () => Ws.PreferredKind = ClaudeTabKind.Chat));
        kind.Items.Add(Ui.CheckItem(L.T("Claude_Terminal", "Terminal"), Ws.PreferredKind == ClaudeTabKind.Terminal, () => Ws.PreferredKind = ClaudeTabKind.Terminal));
        menu.Items.Add(kind);
        menu.Items.Add(Ui.Separator());
        menu.Items.Add(Ui.MenuItem(L.T("Claude_ContinueInTerminal", "Continue in Windows Terminal"), () =>
        {
            try
            {
                ClaudeLauncher.Launch("", tab.Folder, ClaudeTarget.Terminal, ClaudePermissionMode.Standard, tab.Title, string.IsNullOrEmpty(tab.SessionId) ? null : tab.SessionId, tab.ConfigDir, WindowInterop.Copy);
            }
            catch
            {
            }
        }, enabled: ClaudeLauncher.Available));
        menu.Items.Add(Ui.MenuItem(L.T("Claude_RevealFolder", "Show in Explorer"), () => Proc.Reveal(tab.Folder)));
        menu.Items.Add(Ui.MenuItem(L.T("Claude_CopyId", "Copy session ID"), () => WindowInterop.Copy(tab.SessionId), enabled: !string.IsNullOrEmpty(tab.SessionId)));
        return menu;
    }

    private async Task RestartTab(ClaudeTab tab)
    {
        if (tab.Kind == ClaudeTabKind.Chat) tab.Chat?.Stop();
        else tab.Terminal?.Terminate();
        await Task.Delay(600);
        Ws.Restart(tab);
    }
}

public sealed class ClaudeAuthView : Grid
{
    private readonly StackPanel content = Ui.V(18);
    private readonly ComboBox method = new() { HorizontalAlignment = HorizontalAlignment.Stretch };
    private readonly TextBox email = new() { PlaceholderText = L.T("Claude_EmailOptional", "Email (optional)") };
    private readonly PasswordBox code = new() { PlaceholderText = L.T("Claude_PasteCode", "Paste the code from the page") };
    public const string InstallCommand = "irm https://claude.ai/install.ps1 | iex";

    public ClaudeAuthView()
    {
        foreach (var m in Enum.GetValues<ClaudeLoginMethod>()) method.Items.Add(new ComboBoxItem { Content = m.Title(), Tag = m });
        method.SelectedIndex = 0;
        content.MaxWidth = 520;
        content.Padding = new Thickness(36);
        content.HorizontalAlignment = HorizontalAlignment.Center;
        Children.Add(new ScrollViewer { Content = content });
        Loaded += (_, _) =>
        {
            ClaudeAccountStore.Shared.PropertyChanged -= OnStore;
            ClaudeAccountStore.Shared.PropertyChanged += OnStore;
            ClaudeProfiles.Shared.PropertyChanged -= OnStore;
            ClaudeProfiles.Shared.PropertyChanged += OnStore;
            Render();
            ClaudeAccountStore.Shared.Refresh().Forget();
        };
        Unloaded += (_, _) =>
        {
            ClaudeAccountStore.Shared.PropertyChanged -= OnStore;
            ClaudeProfiles.Shared.PropertyChanged -= OnStore;
        };
    }

    private void OnStore(object? sender, PropertyChangedEventArgs e) => Render();

    private void Render()
    {
        var store = ClaudeAccountStore.Shared;
        content.Children.Clear();
        var tile = Ui.IconTile("", "coral", 56);
        tile.HorizontalAlignment = HorizontalAlignment.Center;
        var title = Ui.Text(L.T("Section_Claude", "Claude Code"), 26, "text", FontWeights.Bold);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        var switcher = ClaudeAccountCard.Switcher();
        switcher.HorizontalAlignment = HorizontalAlignment.Center;
        content.Children.Add(tile);
        content.Children.Add(title);
        content.Children.Add(switcher);
        switch (store.Status)
        {
            case ClaudeAccountStatus.NotInstalled:
                content.Children.Add(Card(
                    Ui.Text(L.T("Claude_NotInstalledTitle", "Claude Code isn't installed"), 15, "text", FontWeights.SemiBold),
                    Ui.Text(L.T("Claude_InstallHelp", "Install the CLI from PowerShell, then press “Check again”."), 13, "muted", wrap: true),
                    Ui.Card(Ui.Text(InstallCommand, 13, "text", wrap: true, mono: true, select: true), 8, "sunken", 8),
                    Ui.Columns((Ui.H(8, Ui.Button(L.T("Claude_CopyCommand", "Copy command"), () => WindowInterop.Copy(InstallCommand)), Ui.Button(L.T("Claude_Docs", "Docs"), () => LinkBrowser.Open("https://code.claude.com/docs/en/setup"))), Ui.Star),
                        (Ui.Button(L.T("Claude_CheckAgain", "Check again"), () => store.Refresh().Forget(), accent: true), Ui.Auto))));
                break;
            case ClaudeAccountStatus.Unknown:
            case ClaudeAccountStatus.Checking:
                content.Children.Add(Ui.Loading());
                break;
            case ClaudeAccountStatus.LoggedOut:
                content.Children.Add(LoginCard(store));
                break;
            case ClaudeAccountStatus.LoggedIn when store.Info is { } info:
                content.Children.Add(ClaudeAccountCard.Create(info));
                break;
        }
    }

    private static Border Card(params UIElement[] children)
    {
        var panel = Ui.V(10);
        foreach (var c in children) panel.Children.Add(c);
        return Ui.Card(panel, 12, "raised", 18);
    }

    private FrameworkElement LoginCard(ClaudeAccountStore store)
    {
        var items = new List<UIElement>
        {
            Ui.Text(L.T("Claude_SignInTitle", "Sign in to Claude"), 15, "text", FontWeights.SemiBold),
            Ui.Text(L.T("Claude_SignInHelp", "Sign-in opens in your browser. The account is shared with Claude Code in the terminal."), 13, "muted", wrap: true)
        };
        if (store.LoggingIn)
        {
            items.Add(Ui.H(8, Ui.Spinner(16), Ui.Text(L.T("Claude_ConfirmInBrowser", "Confirm the sign-in in your browser…"), 13)));
            if (store.FallbackUrl is { } url)
            {
                items.Add(Ui.Text(L.T("Claude_BrowserDidntOpen", "Browser didn't open, or the page shows a code?"), 12, "muted"));
                items.Add(Ui.H(8, Ui.Button(L.T("Claude_OpenSignInLink", "Open sign-in link"), () => LinkBrowser.Open(url)), Ui.Button(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(url))));
                var submit = Ui.Button(L.T("Claude_Submit", "Submit"), () =>
                {
                    store.SubmitCode(code.Password);
                    code.Password = "";
                }, accent: true);
                items.Add(Ui.Columns((code, Ui.Star), (submit, Ui.Auto)));
            }
            items.Add(Ui.Button(L.T("Common_Cancel", "Cancel"), store.CancelLogin));
        }
        else
        {
            items.Add(method);
            items.Add(email);
            var signIn = Ui.Button(L.T("Claude_SignInBrowser", "Sign in with browser"), () =>
            {
                var m = (method.SelectedItem as ComboBoxItem)?.Tag is ClaudeLoginMethod lm ? lm : ClaudeLoginMethod.ClaudeAi;
                store.Login(m, email.Text);
            }, accent: true, glyph: "");
            signIn.HorizontalAlignment = HorizontalAlignment.Right;
            items.Add(signIn);
        }
        if (store.LoginError is { } err) items.Add(Ui.Text(err, 12, "red", wrap: true, select: true));
        return Card(items.ToArray());
    }
}
