using System.ComponentModel;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using PullYard.Controls;
using PullYard.Core;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Claude;
using PullYard.Core.Update;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public static class OpenLinkDialog
{
    public static async Task Show(XamlRoot? root)
    {
        if (root is null) return;
        var box = new TextBox { PlaceholderText = L.T("OpenLink_Placeholder", "https://github.com/org/repo/pull/123 or org/repo#123"), MinWidth = 480 };
        var invalid = Ui.Text(L.T("OpenLink_Invalid", "Couldn't recognize the link"), 12, "red");
        invalid.Visibility = Visibility.Collapsed;
        if (await WindowInterop.ReadClipboardText() is { } clip && GitHubUrl.Parse(clip) is not null) box.Text = clip.Trim();
        var d = Dialogs.Create(root, L.T("OpenLink_Title", "Open by link"), Ui.V(8, box, invalid), L.T("Common_Open", "Open"));
        d.PrimaryButtonClick += (_, args) =>
        {
            if (GitHubUrl.Parse(box.Text) is not { } route)
            {
                invalid.Visibility = Visibility.Visible;
                args.Cancel = true;
                return;
            }
            AppViewModel.Shared.Open(route);
        };
        box.KeyDown += (_, e) =>
        {
            if (e.Key != Windows.System.VirtualKey.Enter) return;
            if (GitHubUrl.Parse(box.Text) is { } route)
            {
                d.Hide();
                AppViewModel.Shared.Open(route);
            }
            else
            {
                invalid.Visibility = Visibility.Visible;
            }
        };
        d.Opened += (_, _) =>
        {
            box.Focus(FocusState.Programmatic);
            box.SelectAll();
        };
        await Dialogs.ShowAsync(d);
    }
}

public static class SettingsDialog
{
    public static async Task Show(XamlRoot? root)
    {
        if (root is null) return;
        var app = AppViewModel.Shared;
        var panel = Ui.V(18);

        var style = Segmented(new[] { ("brand", AppInfo.DisplayName), ("system", L.T("Settings_System", "System")) }, ThemeService.Style == ThemeStyle.System ? "system" : "brand",
            v => ThemeService.Style = v == "system" ? ThemeStyle.System : ThemeStyle.PullYard);
        var mode = Segmented(new[] { ("auto", L.T("Settings_MatchSystem", "Match system")), ("light", L.T("Settings_Light", "Light")), ("dark", L.T("Settings_Dark", "Dark")) },
            ThemeService.Appearance switch { AppearanceMode.Light => "light", AppearanceMode.Dark => "dark", _ => "auto" },
            v => ThemeService.Appearance = v switch { "light" => AppearanceMode.Light, "dark" => AppearanceMode.Dark, _ => AppearanceMode.Auto });
        panel.Children.Add(Group(L.T("Settings_Appearance", "Appearance"), "", "violet", Row(L.T("Settings_Theme", "Theme"), style), Row(L.T("Settings_Mode", "Mode"), mode)));

        var account = Ui.V(8);
        if (app.User is { } u)
        {
            account.Children.Add(Ui.H(10, Ui.Avatar(u.AvatarUrl, 36), Ui.V(0, Ui.Text(u.Name ?? u.Login, 14, "text", FontWeights.SemiBold), Ui.Caption("@" + u.Login))));
            account.Children.Add(Row(L.T("Settings_Scopes", "Token scopes"), Ui.Text(app.Scopes.Count == 0 ? "fine-grained / unknown" : string.Join(", ", app.Scopes), 12, "muted", wrap: true)));
            var buttons = Ui.H(8);
            if (GhCli.Binary is not null) buttons.Children.Add(Ui.Button(L.T("Settings_RefreshFromGh", "Refresh token from gh"), () => app.RefreshTokenFromGh().Forget()));
            buttons.Children.Add(Ui.Button(L.T("Settings_SignOut", "Sign out"), () =>
            {
                app.Logout();
                CloseOpenDialog();
            }));
            account.Children.Add(buttons);
        }
        else
        {
            account.Children.Add(Ui.Text(L.T("Settings_NotSignedIn", "Not signed in"), 13, "muted"));
        }
        panel.Children.Add(Group(L.T("Settings_Account", "Account"), "", "teal", account));

        var browsers = new ComboBox { MinWidth = 240 };
        browsers.Items.Add(new ComboBoxItem { Content = L.T("Settings_SystemDefault", "System default"), Tag = LinkBrowser.SystemDefault });
        foreach (var b in LinkBrowser.Installed) browsers.Items.Add(new ComboBoxItem { Content = b.Name, Tag = b.Id });
        browsers.SelectedItem = browsers.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (string?)i.Tag == LinkBrowser.Selected) ?? browsers.Items[0];
        browsers.SelectionChanged += (_, _) => LinkBrowser.Selected = (browsers.SelectedItem as ComboBoxItem)?.Tag as string ?? LinkBrowser.SystemDefault;
        panel.Children.Add(Group(L.T("Settings_Links", "Links"), "", "indigo", Row(L.T("Settings_OpenLinksIn", "Open links in"), browsers)));

        var poll = new ComboBox { MinWidth = 240 };
        var intervals = new[] { (15.0, L.T("Settings_Every15s", "Every 15 seconds")), (30.0, L.T("Settings_Every30s", "Every 30 seconds")), (60.0, L.T("Settings_EveryMinute", "Every minute")), (120.0, L.T("Settings_Every2m", "Every 2 minutes")), (300.0, L.T("Settings_Every5m", "Every 5 minutes")), (900.0, L.T("Settings_Every15m", "Every 15 minutes")) };
        var current = Settings.Shared.GetDouble(Keys.PollInterval, 30);
        foreach (var (v, t) in intervals) poll.Items.Add(new ComboBoxItem { Content = t, Tag = v });
        poll.SelectedItem = poll.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (double)i.Tag! == current) ?? poll.Items[1];
        poll.SelectionChanged += (_, _) =>
        {
            if ((poll.SelectedItem as ComboBoxItem)?.Tag is double v)
            {
                Settings.Shared.Set(Keys.PollInterval, v);
                app.StartPolling();
            }
        };
        var tray = new ToggleSwitch { IsOn = Settings.Shared.GetBool(Keys.CloseToTray, true), OnContent = "", OffContent = "" };
        tray.Toggled += (_, _) => Settings.Shared.Set(Keys.CloseToTray, tray.IsOn);
        panel.Children.Add(Group(L.T("Settings_Refresh", "Refresh"), "", "green", Row(L.T("Settings_AutoRefresh", "Auto-refresh pull requests"), poll), Row(L.T("Settings_CloseToTray", "Keep running in the notification area when the window is closed"), tray)));

        panel.Children.Add(Group(L.T("Section_Claude", "Claude Code"), "", "coral", ClaudeSettings(app)));

        var auto = new ToggleSwitch { IsOn = UpdaterViewModel.AutoCheck, OnContent = "", OffContent = "" };
        auto.Toggled += (_, _) => UpdaterViewModel.AutoCheck = auto.IsOn;
        var check = Ui.Button(L.T("Settings_CheckNow", "Check Now"), () =>
        {
            CloseOpenDialog();
            app.Updater.Check(true).Forget();
        });
        panel.Children.Add(Group(L.T("Settings_Updates", "Updates"), "", "green",
            Row(L.T("Settings_AutoUpdates", "Check for updates automatically"), auto),
            Ui.Columns((Ui.Text(L.F("Settings_CurrentVersion", "Current version {0}", AppInfo.Version), 13, "muted"), Ui.Star), (check, Ui.Auto))));

        var clientId = new TextBox { PlaceholderText = L.T("Login_ClientId", "Client ID"), Text = Settings.Shared.GetString(Keys.OAuthClientId) ?? "", MinWidth = 240 };
        clientId.TextChanged += (_, _) => Settings.Shared.Set(Keys.OAuthClientId, clientId.Text);
        panel.Children.Add(Group(L.T("Settings_DeviceFlow", "OAuth Device Flow"), "", "amber", Row(L.T("Login_ClientId", "Client ID"), clientId)));

        var d = Dialogs.Create(root, L.T("Common_Settings", "Settings"), new ScrollViewer { Content = panel, MaxHeight = 620, Padding = new Thickness(0, 0, 12, 0) }, close: L.T("Common_Done", "Done"), maxWidth: 640);
        openDialog = d;
        await Dialogs.ShowAsync(d);
        openDialog = null;
    }

    private static ContentDialog? openDialog;

    private static void CloseOpenDialog() => openDialog?.Hide();

    private static FrameworkElement ClaudeSettings(AppViewModel app)
    {
        var panel = Ui.V(8);
        var profiles = new ComboBox { MinWidth = 240 };
        foreach (var p in ClaudeProfiles.Shared.Profiles) profiles.Items.Add(new ComboBoxItem { Content = p.MenuTitle, Tag = p.Id });
        profiles.SelectedItem = profiles.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (string?)i.Tag == ClaudeProfiles.Shared.ActiveId);
        profiles.SelectionChanged += (_, _) =>
        {
            if ((profiles.SelectedItem as ComboBoxItem)?.Tag is string id) app.SwitchClaudeProfile(id);
        };
        var add = Ui.Button(L.T("Claude_AddAccount", "Add…"), () =>
        {
            CloseOpenDialog();
            ClaudeDialogs.AddProfile(App.MainWindow?.Content?.XamlRoot).Forget();
        });
        panel.Children.Add(Row(L.T("Claude_Account", "Account"), Ui.H(8, profiles, add)));
        var store = ClaudeAccountStore.Shared;
        switch (store.Status)
        {
            case ClaudeAccountStatus.LoggedIn when store.Info is { } info:
                panel.Children.Add(Ui.Text(info.Email ?? L.T("Claude_SignedIn", "Signed in") + (info.Subscription is { } s ? $" · {s}" : ""), 13, "text"));
                panel.Children.Add(Ui.Button(L.T("Claude_SignOut", "Sign out of Claude Code"), () => ConfirmLogout(app).Forget()));
                break;
            case ClaudeAccountStatus.NotInstalled:
                panel.Children.Add(Ui.Text(L.T("Claude_NotInstalled", "claude CLI isn't installed"), 13, "muted"));
                break;
            default:
                panel.Children.Add(Ui.Columns((Ui.Text(L.T("Claude_NotSignedIn", "Not signed in to Claude"), 13, "muted"), Ui.Star), (Ui.Button(L.T("Claude_SignIn", "Sign in…"), () =>
                {
                    CloseOpenDialog();
                    app.Section = Section.Claude;
                }), Ui.Auto)));
                break;
        }
        var idle = new ComboBox { MinWidth = 160 };
        foreach (var (m, t) in new[] { (0, L.T("Claude_Never", "Never")), (5, "5 minutes"), (15, "15 minutes"), (30, "30 minutes"), (60, "1 hour") })
            idle.Items.Add(new ComboBoxItem { Content = t, Tag = m });
        idle.SelectedItem = idle.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (int)i.Tag! == app.Claude.IdleMinutes);
        idle.SelectionChanged += (_, _) =>
        {
            if ((idle.SelectedItem as ComboBoxItem)?.Tag is int m) app.Claude.IdleMinutes = m;
        };
        panel.Children.Add(Row(L.T("Claude_SleepAfter", "Sleep idle sessions after"), idle));
        panel.Children.Add(Ui.Text(L.T("Claude_SleepHelp", "A sleeping session uses no memory: the claude process stops, the conversation stays, and the next message resumes it."), 12, "muted", wrap: true));
        store.Refresh().Forget();
        return panel;
    }

    private static async Task ConfirmLogout(AppViewModel app)
    {
        CloseOpenDialog();
        var root = App.MainWindow?.Content?.XamlRoot;
        if (!await Dialogs.Confirm(root, L.T("Claude_SignOutQ", "Sign out of Claude Code?"), L.T("Settings_SignOut", "Sign out"), true,
                L.T("Claude_SignOutHelp", "This also signs out Claude Code in the terminal. Open sessions will go to sleep. You can sign in again (including with another account) here or in the Claude Code section."))) return;
        app.Claude.SleepAll();
        await ClaudeAccountStore.Shared.Logout();
    }

    private static FrameworkElement Group(string title, string glyph, string color, params UIElement[] rows)
    {
        var panel = Ui.V(10, Ui.H(8, Ui.IconTile(glyph, color, 18), Ui.Text(title, 14, "text", FontWeights.SemiBold)));
        var inner = Ui.V(10);
        foreach (var r in rows) inner.Children.Add(r);
        panel.Children.Add(Ui.Card(inner, 10, "raised", 12));
        return panel;
    }

    private static FrameworkElement Row(string label, UIElement control)
    {
        var l = Ui.Text(label, 13, "text", wrap: true);
        l.MaxWidth = 260;
        l.HorizontalAlignment = HorizontalAlignment.Left;
        var g = Ui.Columns((l, Ui.Star), (control, Ui.Auto));
        g.ColumnSpacing = 16;
        return g;
    }

    private static FrameworkElement Segmented((string Id, string Title)[] options, string selected, Action<string> changed)
    {
        var bar = new SelectorBar();
        foreach (var (id, title) in options)
        {
            var item = new SelectorBarItem { Text = title, Tag = id };
            bar.Items.Add(item);
            if (id == selected) bar.SelectedItem = item;
        }
        bar.SelectionChanged += (s, _) =>
        {
            if (s.SelectedItem?.Tag is string id) changed(id);
        };
        return bar;
    }
}

public static class UpdateDialog
{
    private static bool showing;

    public static async Task Show(XamlRoot? root)
    {
        if (root is null || showing) return;
        showing = true;
        var updater = AppViewModel.Shared.Updater;
        var title = Ui.Text("", 17, "text", FontWeights.SemiBold);
        var current = Ui.Text(L.F("Update_YouHave", "You have {0}", updater.CurrentVersion), 13, "muted");
        var date = Ui.Text("", 12, "dim");
        var spinner = Ui.Spinner(16);
        var icon = new Image { Source = new BitmapImage(new Uri("ms-appx:///Assets/AppIcon-256.png")), Width = 64, Height = 64 };
        var header = Ui.Columns((icon, Ui.Auto), (Ui.V(4, title, current, date), Ui.Star), (spinner, Ui.Auto));
        header.ColumnSpacing = 14;
        var notes = new HtmlView { Height = 220 };
        var notesHost = new Border { CornerRadius = new CornerRadius(10), Background = Palette.Brush("sunken"), Child = notes };
        var progress = new ProgressBar { Minimum = 0, Maximum = 1 };
        var progressText = Ui.Caption("");
        var progressRow = Ui.V(4, progress, progressText);
        var error = Ui.Text("", 13, "red", wrap: true, select: true);
        var errorLink = Ui.Link(L.T("Update_ViewOnGitHub", "View on GitHub"), () => LinkBrowser.Open(updater.PageUrl), 13);
        var errorRow = Ui.V(4, error, errorLink);
        var panel = Ui.V(14, header, notesHost, progressRow, errorRow);
        panel.Width = 500;
        var d = Dialogs.Create(root, "", panel, "", "", L.T("Update_Later", "Later"), 600);
        d.Title = null;
        var notesShown = "";

        void Render()
        {
            var s = updater.State;
            var r = updater.Release;
            title.Text = s switch
            {
                UpdateState.Idle or UpdateState.Checking => L.T("Update_Checking", "Checking for updates…"),
                UpdateState.UpToDate => L.T("Update_UpToDate", "You're up to date"),
                UpdateState.Failed => r is not null && updater.ShowsRelease ? L.F("Update_IsAvailable", "{0} {1} is available", AppInfo.DisplayName, r.Version) : L.T("Update_CheckFailed", "Couldn't check for updates"),
                UpdateState.Installing => L.T("Update_Preparing", "Preparing update…"),
                UpdateState.ReadyToRelaunch => L.F("Update_Ready", "{0} {1} is ready to install", AppInfo.DisplayName, r?.Version ?? ""),
                _ => L.F("Update_IsAvailable", "{0} {1} is available", AppInfo.DisplayName, r?.Version ?? "")
            };
            date.Text = updater.ShowsRelease && r?.PublishedAt is { } p ? Fmt.Full(p) : "";
            spinner.Visibility = s is UpdateState.Checking or UpdateState.Installing ? Visibility.Visible : Visibility.Collapsed;
            string body;
            if (updater.ShowsRelease && r is not null)
                body = $"<div class=\"tl\"><h3 style=\"margin:0 0 8px\">{Core.Html.ConversationHtml.Esc(r.Name)}</h3><div class=\"md\" style=\"padding:0;white-space:pre-wrap\">{(string.IsNullOrWhiteSpace(r.Notes) ? L.T("Update_NoNotes", "No release notes.") : Core.Html.ConversationHtml.Esc(r.Notes))}</div></div>";
            else if (s == UpdateState.UpToDate)
                body = $"<div class=\"tl\"><p class=\"empty\">{Core.Html.ConversationHtml.Esc(L.F("Update_Newest", "{0} {1} is the newest version available.", AppInfo.DisplayName, updater.CurrentVersion))}</p></div>";
            else body = "";
            if (body != notesShown)
            {
                notesShown = body;
                notes.Render(body, "conversation", "top");
            }
            progressRow.Visibility = s == UpdateState.Downloading ? Visibility.Visible : Visibility.Collapsed;
            progress.Value = updater.Progress;
            var size = r?.InstallerSize ?? 0;
            progressText.Text = size > 0 ? $"{L.T("Update_Downloading", "Downloading…")} {Fmt.Bytes((long)(size * updater.Progress))} / {Fmt.Bytes(size)}" : L.T("Update_Downloading", "Downloading…");
            errorRow.Visibility = s == UpdateState.Failed ? Visibility.Visible : Visibility.Collapsed;
            error.Text = updater.Error ?? "";
            switch (s)
            {
                case UpdateState.Available:
                case UpdateState.Downloading:
                case UpdateState.Failed when updater.ShowsRelease:
                    d.PrimaryButtonText = updater.IsInstalled ? L.T("Update_DownloadInstall", "Download & Install") : L.T("Update_OpenDownload", "Open download page");
                    d.SecondaryButtonText = L.T("Update_Skip", "Skip This Version");
                    d.IsPrimaryButtonEnabled = !updater.IsBusy;
                    d.IsSecondaryButtonEnabled = !updater.IsBusy;
                    break;
                case UpdateState.ReadyToRelaunch:
                    d.PrimaryButtonText = L.T("Update_Restart", "Restart to Update");
                    d.SecondaryButtonText = "";
                    d.IsPrimaryButtonEnabled = true;
                    break;
                case UpdateState.Installing:
                    d.PrimaryButtonText = "";
                    d.SecondaryButtonText = "";
                    break;
                case UpdateState.Failed:
                    d.PrimaryButtonText = L.T("Update_TryAgain", "Try Again");
                    d.SecondaryButtonText = "";
                    d.IsPrimaryButtonEnabled = true;
                    break;
                default:
                    d.PrimaryButtonText = "";
                    d.SecondaryButtonText = "";
                    d.CloseButtonText = s == UpdateState.Checking ? L.T("Update_Later", "Later") : L.T("Common_OK", "OK");
                    break;
            }
        }

        PropertyChangedEventHandler handler = (_, _) => d.DispatcherQueue.TryEnqueue(Render);
        updater.PropertyChanged += handler;
        d.PrimaryButtonClick += (_, args) =>
        {
            args.Cancel = true;
            switch (updater.State)
            {
                case UpdateState.ReadyToRelaunch:
                    updater.Relaunch();
                    break;
                case UpdateState.Failed when !updater.ShowsRelease:
                    updater.Check(true).Forget();
                    break;
                default:
                    updater.DownloadAndInstall().Forget();
                    break;
            }
        };
        d.SecondaryButtonClick += (_, _) => updater.Skip();
        Render();
        if (updater.State == UpdateState.Idle) updater.Check(true).Forget();
        await Dialogs.ShowAsync(d);
        updater.PropertyChanged -= handler;
        updater.Later();
        showing = false;
    }
}
