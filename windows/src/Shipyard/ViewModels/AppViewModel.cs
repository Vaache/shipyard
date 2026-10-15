using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.ComponentModel;
using Shipyard.Core;
using Shipyard.Core.Api;
using Shipyard.Core.Auth;
using Shipyard.Core.Claude;
using Shipyard.Core.Util;
using Shipyard.Services;

namespace Shipyard.ViewModels;

public enum Phase { Checking, LoggedOut, LoggedIn }

public sealed partial class AppViewModel : ObservableObject
{
    public static AppViewModel Shared { get; } = new();

    [ObservableProperty] private Phase phase = Phase.Checking;
    [ObservableProperty] private GitHubApi? api;
    [ObservableProperty] private RestUser? user;
    [ObservableProperty] private List<string> scopes = new();
    [ObservableProperty] private Section section = Section.Prs(PRFilter.ReviewRequested);
    [ObservableProperty] private Route? detailRoot;
    [ObservableProperty] private List<SearchItem> reviewRequests = new();
    [ObservableProperty] private List<SearchItem> myPRs = new();
    [ObservableProperty] private int reviewRequestCount;
    [ObservableProperty] private int myPRCount;
    [ObservableProperty] private int unreadCount;
    [ObservableProperty] private DateTimeOffset? lastBadgeRefresh;
    [ObservableProperty] private Route? clipboardRoute;

    public ClaudeWorkspace Claude { get; private set; } = null!;
    public UpdaterViewModel Updater { get; } = new();

    public event Action? RefreshRequested;
    public event Action? LiveTick;
    public event Action<Route, bool>? RouteRequested;
    public event Action? OpenLinkRequested;
    public event Action? SettingsRequested;
    public event Action? ActivateRequested;

    private DateTimeOffset lastLiveRefresh = DateTimeOffset.Now;
    private CancellationTokenSource? pollCts;
    private readonly Dictionary<ItemRef, PRDetailViewModel> prModels = new();
    private readonly List<ItemRef> prOrder = new();
    private string? lastClipboard;
    public bool IsActive { get; set; } = true;

    public void Initialize()
    {
        Claude = new ClaudeWorkspace();
        ClaudeAccountStore.Shared.Attach(SynchronizationContext.Current!);
        _ = Restore();
    }

    public bool HasProjectScope => Scopes.Count == 0 || Scopes.Contains("read:project") || Scopes.Contains("project");

    public string UserDisplayName => User?.Name ?? User?.Login ?? "";

    partial void OnUserChanged(RestUser? value) => OnPropertyChanged(nameof(UserDisplayName));

    partial void OnScopesChanged(List<string> value) => OnPropertyChanged(nameof(HasProjectScope));

    private async Task Restore()
    {
        var env = AppInfo.Env("TOKEN");
        var token = env.Length > 0 ? env : CredentialStore.Load();
        if (token is null)
        {
            Phase = Phase.LoggedOut;
            return;
        }
        try
        {
            await Login(token, persist: false);
        }
        catch (ApiException e) when (e.Status == 401)
        {
            CredentialStore.Delete();
            Phase = Phase.LoggedOut;
        }
        catch
        {
            Api = new GitHubApi(token);
            Phase = Phase.LoggedIn;
            StartPolling();
        }
    }

    public async Task Login(string token, bool persist = true)
    {
        var api = new GitHubApi(token);
        var (user, scopes) = await api.CurrentUser();
        if (persist) CredentialStore.Save(token);
        Api = api;
        User = Demo.IsOn ? Demo.User : user;
        Scopes = scopes;
        prModels.Clear();
        prOrder.Clear();
        Phase = Phase.LoggedIn;
        StartPolling();
    }

    public void Logout()
    {
        pollCts?.Cancel();
        CredentialStore.Delete();
        Api = null;
        User = null;
        Scopes = new();
        DetailRoot = null;
        prModels.Clear();
        prOrder.Clear();
        ReviewRequests = new();
        MyPRs = new();
        ReviewRequestCount = 0;
        MyPRCount = 0;
        UnreadCount = 0;
        Phase = Phase.LoggedOut;
    }

    public void Refresh()
    {
        lastLiveRefresh = DateTimeOffset.Now;
        RefreshRequested?.Invoke();
        _ = RefreshBadges();
    }

    public void Select(Route? route)
    {
        DetailRoot = route;
        if (route is not null) RouteRequested?.Invoke(route, false);
    }

    public void Open(Route route, bool push = false)
    {
        if (Section.Kind == SectionKind.Claude)
        {
            Section = Section.Prs(PRFilter.Mine);
            DetailRoot = route;
            RouteRequested?.Invoke(route, false);
            ActivateRequested?.Invoke();
            return;
        }
        if (push && DetailRoot is not null) RouteRequested?.Invoke(route, true);
        else Select(route);
        ActivateRequested?.Invoke();
    }

    public void RequestOpenLink() => OpenLinkRequested?.Invoke();

    public void RequestSettings() => SettingsRequested?.Invoke();

    private static readonly Regex SchemeRx = new(@"[a-zA-Z][a-zA-Z0-9+.-]*://\S+", RegexOptions.Compiled);

    public void HandleArguments(string? arguments)
    {
        if (string.IsNullOrWhiteSpace(arguments)) return;
        foreach (Match m in SchemeRx.Matches(arguments))
        {
            var url = m.Value.Trim('"', '\'');
            if (url.StartsWith(AppInfo.UrlScheme + "://", StringComparison.OrdinalIgnoreCase))
            {
                Handle(url);
                return;
            }
        }
    }

    public void Handle(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        if (!uri.Scheme.StartsWith(AppInfo.UrlScheme, StringComparison.OrdinalIgnoreCase) && !uri.Scheme.StartsWith("ghlite", StringComparison.OrdinalIgnoreCase)) return;
        if (uri.Host == "section")
        {
            var parts = uri.AbsolutePath.Split('/', StringSplitOptions.RemoveEmptyEntries);
            var first = parts.ElementAtOrDefault(0);
            var second = parts.ElementAtOrDefault(1);
            switch (first)
            {
                case "notifications": Section = Section.Notifications; break;
                case "repos": Section = Section.Repos; break;
                case "projects": Section = Section.Projects; break;
                case "claude": Section = Section.Claude; break;
                case "prs" when second == "byProject": Section = Section.MyPrsByRepo; break;
                case "prs": Section = Section.Prs(Enum.TryParse<PRFilter>(second, true, out var f) ? f : PRFilter.ReviewRequested); break;
                case "issues": Section = Section.Issues(Enum.TryParse<IssueFilter>(second, true, out var i) ? i : IssueFilter.Assigned); break;
            }
            ActivateRequested?.Invoke();
            return;
        }
        if (uri.Host == "claude")
        {
            Section = Section.Claude;
            var query = System.Web.HttpUtility.ParseQueryString(uri.Query);
            if (uri.AbsolutePath == "/new" && query["folder"] is { } folder && Directory.Exists(folder))
            {
                var mode = query["mode"] is { } m ? ClaudeEnums.ParseMode(m) : (ClaudeMode?)null;
                Claude.NewSession(folder, query["prompt"], query["name"], mode);
            }
            if (uri.AbsolutePath == "/resume" && query["id"] is { } id)
            {
                var kind = query["kind"] switch
                {
                    "chat" => ClaudeTabKind.Chat,
                    "terminal" => ClaudeTabKind.Terminal,
                    _ => (ClaudeTabKind?)null
                };
                _ = ResumeById(id, kind);
            }
            ActivateRequested?.Invoke();
            return;
        }
        var q = System.Web.HttpUtility.ParseQueryString(uri.Query);
        var target = q["url"] ?? "https://" + url[(url.IndexOf("://", StringComparison.Ordinal) + 3)..];
        if (GitHubUrl.Parse(target) is { } route)
        {
            Open(route);
            if (route.Kind == RouteKind.PullRequest && GitHubUrl.IsFilesLink(target) && PrModel(route.Item!) is { } model) model.Tab = PRTab.Files;
        }
    }

    private async Task ResumeById(string id, ClaudeTabKind? kind)
    {
        if (!Claude.AllSessions.Any(s => s.Id == id)) await Claude.ReloadProjects();
        if (Claude.AllSessions.FirstOrDefault(s => s.Id == id) is { } s) Claude.Resume(s, kind: kind);
    }

    public PRDetailViewModel? PrModel(ItemRef r)
    {
        if (Api is null) return null;
        if (prModels.TryGetValue(r, out var m))
        {
            prOrder.Remove(r);
            prOrder.Add(r);
            return m;
        }
        m = new PRDetailViewModel(r, Api);
        prModels[r] = m;
        prOrder.Add(r);
        if (prOrder.Count > 15)
        {
            var old = prOrder[0];
            prOrder.RemoveAt(0);
            prModels.Remove(old);
        }
        return m;
    }

    public void CheckClipboard(string? text)
    {
        if (text is null || text == lastClipboard) return;
        lastClipboard = text;
        if (text.Length >= 300 || !text.Contains("github.com") || GitHubUrl.Parse(text, allowShort: false) is not { } route) return;
        if (route.Kind is RouteKind.PullRequest or RouteKind.Issue && route != DetailRoot) ClipboardRoute = route;
    }

    public static TimeSpan RefreshInterval => TimeSpan.FromSeconds(Math.Max(Settings.Shared.GetDouble(Keys.PollInterval, 30), 15));

    public void StartPolling()
    {
        pollCts?.Cancel();
        var cts = new CancellationTokenSource();
        pollCts = cts;
        lastLiveRefresh = DateTimeOffset.Now;
        _ = RefreshBadges();
        _ = Poll(cts.Token);
    }

    private async Task Poll(CancellationToken ct)
    {
        var cycle = 0;
        while (!ct.IsCancellationRequested)
        {
            try
            {
                await Task.Delay(RefreshInterval, ct);
            }
            catch (OperationCanceledException)
            {
                return;
            }
            cycle++;
            var due = DateTimeOffset.Now - lastLiveRefresh >= RefreshInterval * 0.8;
            if (due && (IsActive || cycle % 4 == 0)) TickLive();
        }
    }

    public void DidBecomeActive()
    {
        if (Phase != Phase.LoggedIn || DateTimeOffset.Now - lastLiveRefresh <= TimeSpan.FromSeconds(10)) return;
        TickLive();
    }

    private void TickLive()
    {
        lastLiveRefresh = DateTimeOffset.Now;
        LiveTick?.Invoke();
        _ = RefreshBadges();
    }

    public async Task RefreshBadges()
    {
        if (Api is not { } api) return;
        if (Demo.IsOn)
        {
            ReviewRequestCount = 3;
            MyPRCount = 7;
            UnreadCount = 12;
            return;
        }
        try
        {
            var b = await api.Badges();
            ReviewRequests = b.Rr.Nodes;
            ReviewRequestCount = b.Rr.IssueCount;
            MyPRs = b.Mine.Nodes;
            MyPRCount = b.Mine.IssueCount;
        }
        catch
        {
        }
        try
        {
            UnreadCount = await api.UnreadNotificationCount();
        }
        catch
        {
        }
        LastBadgeRefresh = DateTimeOffset.Now;
    }

    public void SwitchClaudeProfile(string id)
    {
        if (id == ClaudeProfiles.Shared.ActiveId) return;
        Claude.CloseAllForProfileSwitch();
        ClaudeProfiles.Shared.SetActive(id);
        ClaudeUsageStore.Shared.Windows = new();
        ClaudeUsageStore.Shared.Subscription = null;
        ClaudeAccountStore.Shared.Status = ClaudeAccountStatus.Checking;
        _ = SwitchProfileRefresh();
    }

    private async Task SwitchProfileRefresh()
    {
        await ClaudeAccountStore.Shared.Refresh();
        await Claude.ReloadProjects();
    }

    public void RemoveClaudeProfile(string id)
    {
        var profiles = ClaudeProfiles.Shared;
        if (profiles.Profiles.FirstOrDefault(p => p.Id == id) is not { IsDefault: false } profile) return;
        if (profiles.ActiveId == id) SwitchClaudeProfile(ClaudeProfiles.DefaultId);
        _ = RemoveProfile(profile);
    }

    private async Task RemoveProfile(ClaudeProfile profile)
    {
        await ClaudeAccountStore.Shared.Logout(profile.ConfigDir);
        ClaudeProfiles.Shared.Remove(profile.Id);
    }

    public async Task RefreshTokenFromGh()
    {
        try
        {
            await Login(await GhCli.Token());
        }
        catch
        {
        }
    }
}
