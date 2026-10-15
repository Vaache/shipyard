using System.Collections.Specialized;
using System.ComponentModel;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Shipyard.Controls;
using Shipyard.Core.Api;
using Shipyard.Core.Claude;
using Shipyard.Core.Util;
using Shipyard.Services;
using Shipyard.ViewModels;

namespace Shipyard.Views;

public sealed class ClaudeSessionsView : ListPane
{
    private ClaudeWorkspace Ws => Model.Claude;
    private readonly HashSet<string> expanded = new(StringComparer.OrdinalIgnoreCase);
    private string query = "";
    private readonly ContentControl accountHost = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, IsTabStop = false };
    private readonly DispatcherTimer stateTimer = new() { Interval = TimeSpan.FromSeconds(3) };
    private bool syncing;

    public ClaudeSessionsView() : base(L.T("Section_Claude", "Claude Code"), "SessionRowTemplate")
    {
        FilterRow.Children.Add(accountHost);
        FilterRow.Children.Add(SearchBox(L.T("Claude_SearchSessions", "Search sessions"), t =>
        {
            query = t.Trim();
            Render();
        }, 200));
        var open = Ui.IconButton("", L.T("Claude_OpenFolder", "New session in folder…"), () => _ = OpenFolder(), "text");
        Tools.Children.Add(open);
        List.SelectionMode = ListViewSelectionMode.None;
        List.ItemClick += (_, e) =>
        {
            switch (e.ClickedItem)
            {
                case SessionListItem s:
                    Ws.Resume(s.Session);
                    break;
                case ProjectHeaderItem p:
                    if (!expanded.Remove(p.Project.Id)) expanded.Add(p.Project.Id);
                    Render();
                    break;
                case OpenTabItem t:
                    Ws.SelectedTab = t.Tab;
                    break;
            }
        };
        List.ContextRequested += (_, args) =>
        {
            var element = args.OriginalSource as FrameworkElement;
            MenuFlyout? menu = element?.DataContext switch
            {
                SessionListItem s => SessionMenu(s.Session),
                OpenTabItem t => TabMenu(t.Tab),
                _ => null
            };
            if (menu is null || element is null) return;
            args.Handled = true;
            menu.ShowAt(element);
        };
        stateTimer.Tick += (_, _) =>
        {
            if (Ws.Tabs.Count > 0) Render();
        };
    }

    protected override void OnAttached()
    {
        Ws.PropertyChanged += OnWs;
        Ws.Tabs.CollectionChanged += OnTabs;
        ClaudeAccountStore.Shared.PropertyChanged += OnAccount;
        ClaudeProfiles.Shared.PropertyChanged += OnAccount;
        Unloaded += Detach;
        stateTimer.Start();
        RenderAccount();
        Render();
        _ = Initial();
    }

    private void Detach(object sender, RoutedEventArgs e)
    {
        Ws.PropertyChanged -= OnWs;
        Ws.Tabs.CollectionChanged -= OnTabs;
        ClaudeAccountStore.Shared.PropertyChanged -= OnAccount;
        ClaudeProfiles.Shared.PropertyChanged -= OnAccount;
        Unloaded -= Detach;
        stateTimer.Stop();
    }

    private async Task Initial()
    {
        await ClaudeAccountStore.Shared.Refresh();
        await Ws.ReloadProjects();
        if (expanded.Count == 0 && Ws.Projects.FirstOrDefault() is { } first)
        {
            expanded.Add(first.Id);
            Render();
        }
    }

    protected override async Task OnPullRefresh()
    {
        await ClaudeAccountStore.Shared.Refresh();
        await Ws.ReloadProjects();
    }

    private void OnTabs(object? sender, NotifyCollectionChangedEventArgs e) => Render();

    private void OnWs(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(ClaudeWorkspace.Projects):
            case nameof(ClaudeWorkspace.SelectedTab):
                Render();
                break;
            case nameof(ClaudeWorkspace.LoadingProjects):
            case nameof(ClaudeWorkspace.ProjectsUpdated):
                Footer.Update(Ws.ProjectsUpdated, Ws.LoadingProjects);
                if (Ws.Projects.Count == 0) Render();
                break;
        }
    }

    private void OnAccount(object? sender, PropertyChangedEventArgs e)
    {
        RenderAccount();
        Render();
    }

    private void RenderAccount()
    {
        var store = ClaudeAccountStore.Shared;
        accountHost.Content = store.Status switch
        {
            ClaudeAccountStatus.LoggedIn when store.Info is { } info => ClaudeAccountCard.Create(info, compact: true),
            ClaudeAccountStatus.LoggedOut => Ui.V(6, Ui.H(6, Ui.Glyph("", 12, "amber"), Ui.Text(L.T("Claude_SignInRight", "Not signed in to Claude — sign in on the right"), 12.5, "amber", wrap: true)),
                ClaudeProfiles.Shared.Profiles.Count > 1 ? ClaudeAccountCard.Switcher() : null),
            ClaudeAccountStatus.NotInstalled => Ui.H(6, Ui.Glyph("", 12, "amber"), Ui.Text(L.T("Claude_NotFound", "claude CLI not found — see the right side"), 12.5, "amber", wrap: true)),
            _ => null
        };
    }

    private void Render()
    {
        syncing = true;
        var store = ClaudeAccountStore.Shared;
        var items = new List<object>();
        if (store.IsLoggedIn)
        {
            if (Ws.Tabs.Count > 0)
            {
                var mem = Ws.TotalMemory > 0 ? Fmt.Memory(Ws.TotalMemory) : "0 MB";
                items.Add(new SectionLabelItem($"{L.T("Claude_Open", "Open")}  ·  {mem}"));
                foreach (var t in Ws.Tabs) items.Add(new OpenTabItem(t, Ws.SelectedTab == t));
            }
            foreach (var project in Filtered())
            {
                var isOpen = expanded.Contains(project.Id) || query.Length > 0;
                var path = project.Path;
                items.Add(new ProjectHeaderItem(project, isOpen) { OnNew = () => Ws.NewSession(path) });
                if (!isOpen) continue;
                foreach (var s in project.Sessions.Take(query.Length == 0 ? 30 : 200)) items.Add(new SessionListItem(s, Ws.TabForSession(s.Id) is not null));
            }
        }
        List.ItemsSource = items;
        syncing = false;
        Footer.Update(Ws.ProjectsUpdated, Ws.LoadingProjects);
        if (!store.IsLoggedIn)
        {
            ShowOverlay(store.Status is ClaudeAccountStatus.Checking or ClaudeAccountStatus.Unknown ? Ui.Loading() : Ui.Empty("", "slate", L.T("Claude_HistoryAfterSignIn", "Session history and projects appear after you sign in to Claude.")));
        }
        else if (items.Count == 0)
        {
            ShowOverlay(Ws.LoadingProjects ? Ui.Loading() : Ui.Empty("", "coral", L.T("Claude_NoSessions", "No Claude Code sessions yet")));
        }
        else
        {
            ShowOverlay(null);
        }
    }

    private IEnumerable<ClaudeProject> Filtered()
    {
        if (query.Length == 0) return Ws.Projects;
        return Ws.Projects.Select(p =>
        {
            if (p.Name.Contains(query, StringComparison.OrdinalIgnoreCase)) return p;
            var s = p.Sessions.Where(x => x.Title.Contains(query, StringComparison.OrdinalIgnoreCase) || (x.Branch ?? "").Contains(query, StringComparison.OrdinalIgnoreCase)).ToList();
            return s.Count == 0 ? null : new ClaudeProject { Id = p.Id, Path = p.Path, Sessions = s };
        }).Where(p => p is not null)!;
    }

    private MenuFlyout SessionMenu(ClaudeSessionInfo s)
    {
        var menu = Ui.Menu(
            Ui.MenuItem(L.T("Claude_ResumeChat", "Resume as chat"), () => Ws.Resume(s, kind: ClaudeTabKind.Chat), ""),
            Ui.MenuItem(L.T("Claude_ResumeTerminal", "Resume in terminal"), () => Ws.Resume(s, kind: ClaudeTabKind.Terminal), ""),
            Ui.MenuItem(L.T("Claude_Fork", "Fork session"), () => Ws.Resume(s, fork: true), ""),
            Ui.Separator(),
            Ui.MenuItem(L.T("Claude_CopyId", "Copy session ID"), () => WindowInterop.Copy(s.Id)),
            Ui.MenuItem(L.T("Claude_CopyResume", "Copy resume command"), () => WindowInterop.Copy($"cd {Proc.QuoteArg(s.Cwd)} && claude --resume {s.Id}")),
            Ui.MenuItem(L.T("Claude_Reveal", "Show transcript in Explorer"), () => Proc.Reveal(s.File)));
        if (s.PrUrl is { } pr && GitHubUrl.Parse(pr) is { } route)
            menu.Items.Add(Ui.MenuItem(L.T("Claude_OpenPR", "Open PR"), () => Model.Open(route)));
        return menu;
    }

    private MenuFlyout TabMenu(ClaudeTab tab)
    {
        return Ui.Menu(
            tab.Running
                ? Ui.MenuItem(L.T("Claude_FreeMemory", "Free memory (sleep)"), tab.SleepNow, enabled: tab.State is not (ClaudeTabState.Working or ClaudeTabState.Waiting))
                : Ui.MenuItem(L.T("Claude_WakeUp", "Wake up"), () => Ws.Restart(tab)),
            Ui.MenuItem(L.T("Common_Close", "Close"), () => Ws.Close(tab)));
    }

    private async Task OpenFolder()
    {
        if (!ClaudeAccountStore.Shared.IsLoggedIn || App.MainWindow is null) return;
        if (await WindowInterop.PickFolder(App.MainWindow) is { } folder) Ws.NewSession(folder);
    }
}

public static class ClaudeAccountCard
{
    public static FrameworkElement Create(ClaudeAccountInfo info, bool compact = false)
    {
        var profile = ClaudeProfiles.Shared.Active;
        var nameRow = Ui.H(5, Ui.Text(info.Email ?? L.T("Claude_SignedIn", "Signed in"), compact ? 13 : 15, "text", FontWeights.Medium));
        if (ClaudeProfiles.Shared.Profiles.Count > 1) nameRow.Children.Add(Ui.Chip(profile.Label, "muted"));
        var sub = Ui.H(5);
        if (info.OrgName is { } org) sub.Children.Add(Ui.Caption(org));
        if (info.Subscription is { } plan) sub.Children.Add(Ui.Chip(plan, "coral"));
        else if (info.Method == "console" || info.Provider != "firstParty") sub.Children.Add(Ui.Caption(info.Provider == "firstParty" ? "API" : info.Provider ?? "API"));
        var more = Ui.IconButton("", L.T("Common_More", "More"), () => { }, "text");
        var menu = ProfileMenu();
        menu.Items.Add(Ui.Separator());
        menu.Items.Add(Ui.MenuItem(L.T("Claude_SignOutThis", "Sign out of this account…"), () => _ = ConfirmSignOut()));
        menu.Items.Add(Ui.Separator());
        menu.Items.Add(Ui.MenuItem(L.T("Claude_UsagePage", "Usage page"), () => LinkBrowser.Open("https://claude.ai/settings/usage")));
        menu.Items.Add(Ui.MenuItem(L.T("Claude_RefreshStatus", "Refresh status"), () => _ = ClaudeAccountStore.Shared.Refresh()));
        more.Flyout = menu;
        var grid = Ui.Columns((ProfileBadge(profile, compact ? 26 : 34), Ui.Auto), (Ui.V(1, nameRow, sub), Ui.Star), (more, Ui.Auto));
        grid.ColumnSpacing = 10;
        return Ui.Card(grid, 10, "raised", compact ? 8 : 14);
    }

    private static async Task ConfirmSignOut()
    {
        var root = App.MainWindow?.Content?.XamlRoot;
        var active = ClaudeProfiles.Shared.Active;
        var message = active.IsDefault
            ? L.T("Claude_SignOutDefaultHelp", "This also signs out Claude Code in the terminal. You can sign in again afterwards, including with another account.")
            : L.F("Claude_SignOutProfileHelp", "Signs out only “{0}”. Other accounts are not affected.", active.Label);
        if (await Dialogs.Confirm(root, L.T("Claude_SignOutQ", "Sign out of Claude Code?"), L.T("Settings_SignOut", "Sign out"), true, message))
            await ClaudeAccountStore.Shared.Logout();
    }

    public static MenuFlyout ProfileMenu()
    {
        var app = AppViewModel.Shared;
        var profiles = ClaudeProfiles.Shared;
        var menu = new MenuFlyout();
        menu.Items.Add(new MenuFlyoutItem { Text = L.T("Claude_Accounts", "Claude accounts"), IsEnabled = false });
        foreach (var p in profiles.Profiles)
        {
            var id = p.Id;
            menu.Items.Add(Ui.CheckItem(p.MenuTitle, id == profiles.ActiveId, () => app.SwitchClaudeProfile(id)));
        }
        menu.Items.Add(Ui.MenuItem(L.T("Claude_AddAccountMenu", "Add account…"), () => _ = ClaudeDialogs.AddProfile(App.MainWindow?.Content?.XamlRoot)));
        if (!profiles.Active.IsDefault)
        {
            var active = profiles.Active;
            menu.Items.Add(Ui.MenuItem(L.F("Claude_RemoveAccount", "Remove “{0}”…", active.Label), () => _ = ClaudeDialogs.RemoveProfile(App.MainWindow?.Content?.XamlRoot, active)));
        }
        return menu;
    }

    public static FrameworkElement Switcher()
    {
        var active = ClaudeProfiles.Shared.Active;
        var b = new DropDownButton { Content = Ui.H(6, ProfileBadge(active, 18), Ui.Text(active.Label, 13)), Flyout = ProfileMenu() };
        Ui.Tip(b, L.T("Claude_SwitchAccount", "Switch Claude account"));
        return b;
    }

    public static FrameworkElement ProfileBadge(ClaudeProfile profile, double size)
    {
        var color = profile.IsDefault ? "coral" : Palette.ProjectKey(profile.Id);
        var grid = new Grid { Width = size, Height = size };
        grid.Children.Add(new Microsoft.UI.Xaml.Shapes.Ellipse { Fill = Palette.Brush(color, 0.18), Stroke = Palette.Brush(color, 0.35), StrokeThickness = 1 });
        var t = Ui.Text(profile.Initial, size * 0.48, color, FontWeights.Bold);
        t.HorizontalAlignment = HorizontalAlignment.Center;
        grid.Children.Add(t);
        return grid;
    }
}

public static class ClaudeDialogs
{
    public static async Task AddProfile(XamlRoot? root)
    {
        if (root is null) return;
        var name = new TextBox { PlaceholderText = L.T("Claude_ProfileName", "Name, e.g. “Personal” or “Work”") };
        var share = new CheckBox { Content = L.T("Claude_ShareSettings", "Use my settings, CLAUDE.md, skills and plugins from the main account"), IsChecked = true };
        var error = Ui.Text("", 12, "red", wrap: true);
        var panel = Ui.V(10, Ui.Text(L.T("Claude_ProfileHelp", "The account gets its own sign-in, session history and limits. Sign-in opens right after."), 13, "muted", wrap: true), name, share, error);
        var d = Dialogs.Create(root, L.T("Claude_NewAccount", "New Claude account"), panel, L.T("Claude_CreateAndSignIn", "Create and sign in"));
        d.PrimaryButtonClick += (_, args) =>
        {
            try
            {
                var p = ClaudeProfiles.Shared.Add(name.Text, share.IsChecked == true);
                var app = AppViewModel.Shared;
                app.SwitchClaudeProfile(p.Id);
                app.Section = Section.Claude;
            }
            catch (Exception e)
            {
                error.Text = e.Message;
                args.Cancel = true;
            }
        };
        await Dialogs.ShowAsync(d);
    }

    public static async Task RemoveProfile(XamlRoot? root, ClaudeProfile profile)
    {
        if (await Dialogs.Confirm(root, L.T("Claude_RemoveQ", "Remove account?"), L.F("Claude_RemoveButton", "Sign out and remove “{0}”", profile.Label), true,
                L.F("Claude_RemoveHelp", "The app signs out of this account and removes it from the list. Its history folder stays on disk: {0}", profile.ConfigDir ?? "")))
            AppViewModel.Shared.RemoveClaudeProfile(profile.Id);
    }
}
