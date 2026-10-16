using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Claude;
using PullYard.Core.Util;
using PullYard.Services;

namespace PullYard.ViewModels;

public enum Phase { Checking, LoggedOut, LoggedIn }

public sealed partial class AppViewModel : ObservableObject
{
    public static AppViewModel Shared { get; } = new();

    [ObservableProperty] private Phase phase = Phase.Checking;
    [ObservableProperty] private GitHubApi? api;
    [ObservableProperty] private RestUser? user;
    [ObservableProperty] private string? account;
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
    [ObservableProperty] private DateTimeOffset? rateLimitedUntil;

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
    private bool viewsStale;
    private SynchronizationContext? context;
    private CancellationTokenSource? rateLimitCts;
    private GitHubApi? loginAttempt;
    public bool IsActive { get; set; } = true;
    public Func<bool> IsShown { get; set; } = () => true;

    public void Initialize()
    {
        Claude = new ClaudeWorkspace();
        context = SynchronizationContext.Current;
        ClaudeAccountStore.Shared.Attach(SynchronizationContext.Current!);
        Task.Run(ListSnapshotStore.Shared.Prune).Forget();
        Restore().Forget();
    }

    public bool HasProjectScope => Scopes.Count == 0 || Scopes.Contains("read:project") || Scopes.Contains("project");

    public string UserDisplayName => User?.Name ?? User?.Login ?? "";

    partial void OnUserChanged(RestUser? value) => OnPropertyChanged(nameof(UserDisplayName));

    partial void OnScopesChanged(List<string> value) => OnPropertyChanged(nameof(HasProjectScope));

    partial void OnApiChanged(GitHubApi? oldValue, GitHubApi? newValue)
    {
        if (oldValue is not null) oldValue.RateLimit.Paused -= OnRateLimited;
        if (newValue is not null) newValue.RateLimit.Paused += OnRateLimited;
        rateLimitCts?.Cancel();
        RateLimitedUntil = null;
        if (newValue is null) return;
        if (oldValue is not null && oldValue.Token == newValue.Token && oldValue.RateLimit.Until is { } carried) newValue.RateLimit.PauseUntil(carried);
        if (newValue.RateLimit.Until is { } until && until > DateTimeOffset.Now) ApplyRateLimit(until);
    }

    private void OnRateLimited(DateTimeOffset until)
    {
        if (context is { } ctx) ctx.Post(_ => ApplyRateLimit(until), null);
        else ApplyRateLimit(until);
    }

    private void ApplyRateLimit(DateTimeOffset until)
    {
        if (RateLimitedUntil is { } current && current >= until) return;
        RateLimitedUntil = until;
        rateLimitCts?.Cancel();
        var cts = new CancellationTokenSource();
        rateLimitCts = cts;
        ResumeAfterRateLimit(until, cts.Token).Forget();
    }

    private async Task ResumeAfterRateLimit(DateTimeOffset until, CancellationToken ct)
    {
        var wait = until - DateTimeOffset.Now + TimeSpan.FromSeconds(1);
        try
        {
            await Task.Delay(wait > TimeSpan.Zero ? wait : TimeSpan.Zero, ct);
        }
        catch (OperationCanceledException)
        {
            return;
        }
        if (Api?.RateLimit.Until is { } next && next > until)
        {
            ApplyRateLimit(next);
            return;
        }
        RateLimitedUntil = null;
        if (Phase == Phase.LoggedIn) TickLive();
    }

    private bool LivePaused => Api?.RateLimit.IsPaused(DateTimeOffset.Now) ?? false;

    private async Task Restore()
    {
        var env = AppInfo.Env("TOKEN");
        var token = env.Length > 0 ? env : CredentialStore.Load();
        if (token is null)
        {
            Phase = Phase.LoggedOut;
            return;
        }
        GitHubApi? early = null;
        if (!Demo.IsOn && await Task.Run(() => ListSnapshotStore.Shared.RememberedAccount(token)) is { } remembered)
        {
            early = new GitHubApi(token);
            Account = remembered;
            Api = early;
            Phase = Phase.LoggedIn;
            StartPolling();
        }
        try
        {
            await Login(token, persist: false, early);
        }
        catch (ApiException e) when (e.Status == 401)
        {
            if (early is not null) Logout();
            else
            {
                CredentialStore.Delete();
                Phase = Phase.LoggedOut;
            }
        }
        catch
        {
            if (early is not null && ReferenceEquals(Api, early)) return;
            Api = loginAttempt is { } attempt && attempt.Token == token ? attempt : new GitHubApi(token);
            Phase = Phase.LoggedIn;
            StartPolling();
        }
    }

    public async Task Login(string token, bool persist = true, GitHubApi? current = null)
    {
        var api = current ?? new GitHubApi(token);
        loginAttempt = api;
        var (user, scopes) = await api.CurrentUser();
        if (persist) CredentialStore.Save(token);
        if (current is not null && ReferenceEquals(Api, current))
        {
            User = Demo.IsOn ? Demo.User : user;
            Scopes = scopes;
            if (!string.Equals(Account, user.Login, StringComparison.OrdinalIgnoreCase))
            {
                var stale = Account;
                Task.Run(() => ListSnapshotStore.Shared.Clear(stale)).Forget();
                Account = user.Login;
                Refresh();
            }
            Remember(token, user.Login);
            return;
        }
        if (Account is { } previous && !string.Equals(previous, user.Login, StringComparison.OrdinalIgnoreCase)) SearchListViewModel.ClearCache();
        Account = user.Login;
        Api = api;
        User = Demo.IsOn ? Demo.User : user;
        Scopes = scopes;
        prModels.Clear();
        prOrder.Clear();
        Phase = Phase.LoggedIn;
        StartPolling();
        Remember(token, user.Login);
    }

    private static void Remember(string token, string login)
    {
        if (Demo.IsOn) return;
        var generation = ListSnapshotStore.Shared.AccountGeneration;
        Task.Run(() => ListSnapshotStore.Shared.RememberAccount(token, login, generation)).Forget();
    }

    public void Logout()
    {
        pollCts?.Cancel();
        CredentialStore.Delete();
        ListSnapshotStore.Shared.ForgetAccount();
        ListSnapshotStore.Shared.Clear(Account);
        SearchListViewModel.ClearCache();
        BlameCache.Shared.Clear();
        Account = null;
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
        RefreshBadges().Forget();
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
            if (url.StartsWith(AppInfo.UrlScheme + "://", StringComparison.OrdinalIgnoreCase) || url.StartsWith(AppInfo.LegacyUrlScheme + "://", StringComparison.OrdinalIgnoreCase))
            {
                Handle(url);
                return;
            }
        }
    }

    public void Handle(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        if (!uri.Scheme.StartsWith(AppInfo.UrlScheme, StringComparison.OrdinalIgnoreCase) && !uri.Scheme.StartsWith(AppInfo.LegacyUrlScheme, StringComparison.OrdinalIgnoreCase) && !uri.Scheme.StartsWith("ghlite", StringComparison.OrdinalIgnoreCase)) return;
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
                ResumeById(id, kind).Forget();
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
        if (!LivePaused) RefreshBadges().Forget();
        Poll(cts.Token).Forget();
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
        if (Phase != Phase.LoggedIn) return;
        var due = DateTimeOffset.Now - lastLiveRefresh > TimeSpan.FromSeconds(10);
        if (due || viewsStale) TickLive(badges: due);
    }

    private void TickLive(bool badges = true)
    {
        if (LivePaused) return;
        if (badges) lastLiveRefresh = DateTimeOffset.Now;
        var tick = Diagnostics.BeginLiveTick();
        try
        {
            if (IsShown())
            {
                viewsStale = false;
                LiveTick?.Invoke();
            }
            else
            {
                viewsStale = true;
            }
            if (badges) RefreshBadges(live: true).Forget();
        }
        finally
        {
            if (tick is not null) Diagnostics.EndLiveTick(tick);
        }
    }

    public async Task RefreshBadges(bool live = false)
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
            if (await api.UnreadNotificationCount(live) is { } unread) UnreadCount = unread;
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
        SwitchProfileRefresh().Forget();
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
        RemoveProfile(profile).Forget();
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
