using Microsoft.UI.Xaml;
using Microsoft.Windows.AppLifecycle;
using Shipyard.Core;
using Shipyard.Core.Api;
using Shipyard.Core.Util;
using Shipyard.Services;
using Shipyard.ViewModels;

namespace Shipyard;

public partial class App : Application
{
    public static MainWindow? MainWindow { get; private set; }
    public static int ExitCode { get; set; }
    private static TrayIcon? tray;
    private static bool quitting;

    public App()
    {
        InitializeComponent();
        UnhandledException += (_, e) =>
        {
            e.Handled = true;
            if (Program.SmokeUi) ExitCode = 4;
            try
            {
                File.AppendAllText(Path.Combine(AppInfo.DataDirectory, "errors.log"), $"{DateTimeOffset.Now:o} {e.Exception}\n");
            }
            catch
            {
            }
        };
    }

    public static bool Quitting => quitting;

    protected override void OnLaunched(LaunchActivatedEventArgs args)
    {
        Notifications.Invoked += action => MainWindow?.DispatcherQueue.TryEnqueue(() =>
        {
            ShowMainWindow();
            if (action == "update") AppViewModel.Shared.Updater.Present();
        });
        Notifications.Register();
        AppViewModel.Shared.Initialize();
        MainWindow = new MainWindow();
        MainWindow.Activate();
        CreateTray();
        AppViewModel.Shared.HandleArguments(Environment.CommandLine);
        if (Program.SmokeTour)
        {
            Services.SmokeTour.Run(MainWindow).Forget();
        }
        else if (Program.SmokeUi)
        {
            var timer = MainWindow.DispatcherQueue.CreateTimer();
            timer.Interval = TimeSpan.FromSeconds(6);
            timer.IsRepeating = false;
            timer.Tick += (_, _) =>
            {
                ExitCode = MainWindow.Content is null ? 3 : 0;
                Quit();
            };
            timer.Start();
        }
    }

    public static void OnRedirectedActivation(AppActivationArguments e)
    {
        var window = MainWindow;
        if (window is null) return;
        window.DispatcherQueue.TryEnqueue(() =>
        {
            ShowMainWindow();
            string? text = null;
            if (e.Data is Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs launch) text = launch.Arguments;
            else if (e.Data is Windows.ApplicationModel.Activation.IProtocolActivatedEventArgs protocol) text = protocol.Uri?.AbsoluteUri;
            AppViewModel.Shared.HandleArguments(text);
        });
    }

    public static void ShowMainWindow()
    {
        if (MainWindow is null) return;
        MainWindow.AppWindow.Show();
        WindowInterop.BringToFront(MainWindow);
    }

    private static void CreateTray()
    {
        try
        {
            var icon = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico");
            tray = new TrayIcon(icon, AppInfo.DisplayName) { MenuProvider = TrayMenu };
            var model = AppViewModel.Shared;
            model.PropertyChanged += (_, e) =>
            {
                if (e.PropertyName is nameof(AppViewModel.ReviewRequestCount) or nameof(AppViewModel.UnreadCount)) UpdateTrayTooltip();
            };
            UpdateTrayTooltip();
        }
        catch
        {
            tray = null;
        }
    }

    private static void UpdateTrayTooltip()
    {
        var m = AppViewModel.Shared;
        var tip = AppInfo.DisplayName;
        if (m.ReviewRequestCount > 0) tip += " — " + L.F("Tray_ReviewCount", "{0} review requests", m.ReviewRequestCount);
        tray?.SetTooltip(tip);
    }

    private static List<TrayMenuItem> TrayMenu()
    {
        var m = AppViewModel.Shared;
        var items = new List<TrayMenuItem>
        {
            new() { Text = AppInfo.DisplayName + (m.User is { } u ? $"  @{u.Login}" : ""), Disabled = true, Bold = true }
        };
        if (m.Updater.AvailableVersion is { } v)
        {
            items.Add(new() { Text = L.F("Update_To", "Update to {0}", v), Action = () => { ShowMainWindow(); m.Updater.Present(); } });
        }
        items.Add(new() { Separator = true });
        if (m.Phase != Phase.LoggedIn)
        {
            items.Add(new() { Text = L.T("Tray_SignIn", "Sign in…"), Action = ShowMainWindow });
        }
        else
        {
            void AddSection(string title, int count, List<SearchItem> list, PRFilter filter)
            {
                items.Add(new() { Text = $"{title.ToUpperInvariant()}  ({count})", Action = () => { m.Section = Section.Prs(filter); ShowMainWindow(); } });
                if (list.Count == 0) items.Add(new() { Text = "    " + L.T("Common_NothingHere", "Nothing here"), Disabled = true });
                foreach (var item in list.Take(10))
                {
                    var ci = item.CiState switch
                    {
                        "SUCCESS" => " ✓",
                        "FAILURE" or "ERROR" => " ✗",
                        "PENDING" or "EXPECTED" => " …",
                        _ => ""
                    };
                    var title2 = item.Title.Length > 60 ? item.Title[..60] + "…" : item.Title;
                    var route = item.Route;
                    items.Add(new() { Text = $"    {title2}   {item.Repository.NameWithOwner}#{item.Number}{ci}", Action = () => { ShowMainWindow(); m.Select(route); } });
                }
                items.Add(new() { Separator = true });
            }
            AddSection(L.T("Tray_ReviewRequests", "Review requests"), m.ReviewRequestCount, m.ReviewRequests, PRFilter.ReviewRequested);
            AddSection(L.T("Tray_MyOpenPRs", "My open PRs"), m.MyPRCount, m.MyPRs, PRFilter.Mine);
            var unread = m.UnreadCount >= 50 ? "50+" : m.UnreadCount.ToString();
            items.Add(new() { Text = L.T("Section_Notifications", "Notifications") + (m.UnreadCount > 0 ? $"  ({unread})" : ""), Action = () => { m.Section = Section.Notifications; ShowMainWindow(); } });
            items.Add(new() { Text = L.T("Common_Refresh", "Refresh"), Action = () => m.RefreshBadges().Forget() });
        }
        items.Add(new() { Separator = true });
        items.Add(new() { Text = L.F("Tray_Open", "Open {0}", AppInfo.DisplayName), Action = ShowMainWindow });
        items.Add(new() { Text = L.T("Menu_CheckForUpdates", "Check for Updates…"), Action = () => { ShowMainWindow(); m.Updater.Check(true).Forget(); } });
        items.Add(new() { Text = L.T("Tray_Quit", "Quit"), Action = Quit });
        return items;
    }

    public static void Quit()
    {
        quitting = true;
        try
        {
            AppViewModel.Shared.Claude?.CloseAllForProfileSwitch();
        }
        catch
        {
        }
        tray?.Dispose();
        tray = null;
        Notifications.Unregister();
        try
        {
            MainWindow?.Close();
        }
        catch
        {
        }
        Current.Exit();
    }
}
