using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Claude;
using PullYard.Core.Forges;
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
    [ObservableProperty] private GitAccount? gitAccount;
    [ObservableProperty] private IForgeClient? forge;
    [ObservableProperty] private SearchItem? forgeSelection;

    public ClaudeWorkspace Claude { get; private set; } = null!;
    public UpdaterViewModel Updater { get; } = new();
    public GitAccounts Accounts { get; } = new(Settings.Shared, new CredentialVault());

    public event Action? RefreshRequested;
    public event Action? LiveTick;
    public event Action<Route, bool>? RouteRequested;
    public event Action? OpenLinkRequested;
    public event Action? SettingsRequested;
    public event Action? ActivateRequested;
    public event Action<string>? ToastRequested;
    public event Action? AddAccountRequested;

    private DateTimeOffset lastLiveRefresh = DateTimeOffset.Now;
    private CancellationTokenSource? pollCts;
    private readonly Dictionary<ItemRef, PRDetailViewModel> prModels = new();
    private readonly List<ItemRef> prOrder = new();
    private string? lastClipboard;
    private bool viewsStale;
    private SynchronizationContext? context;
    private CancellationTokenSource? rateLimitCts;
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

    partial void OnGitAccountChanged(GitAccount? value) => OnPropertyChanged(nameof(Provider));

    public GitProvider Provider => GitAccount?.Provider ?? GitProvider.GitHub;

    public void RequestAddAccount() => AddAccountRequested?.Invoke();

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
        if (env.Length > 0)
        {
            await ResumeGitHub(env, null);
            return;
        }
        if (Accounts.Active is not { } account || Accounts.Credential(account) is not { } credential)
        {
            Phase = Phase.LoggedOut;
            return;
        }
        await Resume(account, credential);
    }

    private async Task Resume(GitAccount account, GitCredential credential)
    {
        if (account.Provider.IsGitHub())
        {
            await ResumeGitHub(credential.Token, account);
            return;
        }
        if (MakeForge(account, credential) is not { } client)
        {
            Phase = Phase.LoggedOut;
            return;
        }
        EnterForge(account, client);
        await VerifyForge(client);
    }

    private async Task ResumeGitHub(string token, GitAccount? account)
    {
        var host = account?.Host ?? GitProviders.GitHubHost;
        var known = account is { Login.Length: > 0 } ? account : null;
        var cached = known?.CacheKey;
        if (cached is null && (account is null || account.Provider == GitProvider.GitHub)) cached = await Task.Run(() => ListSnapshotStore.Shared.RememberedAccount(token));
        GitHubApi? early = null;
        if (!Demo.IsOn && cached is not null)
        {
            early = new GitHubApi(token, host);
            GitAccount = account;
            Forge = null;
            Account = cached;
            if (known is not null) User = known.User;
            Api = early;
            Phase = Phase.LoggedIn;
            StartPolling();
            if (account is not null) Accounts.MarkUsed(account);
        }
        var api = early ?? new GitHubApi(token, host);
        try
        {
            var (user, scopes) = await api.CurrentUser();
            if (early is null) EnterGitHub(account?.With(user), api, user, scopes, token);
            else if (ReferenceEquals(Api, early)) ApplyVerified(account, user, scopes, token);
        }
        catch (ApiException e) when (e.Status == 401)
        {
            if (early is null || ReferenceEquals(Api, early)) Revoked();
        }
        catch
        {
            if (early is not null) return;
            GitAccount = account;
            Forge = null;
            Account = known?.CacheKey;
            if (known is not null) User = known.User;
            Api = api;
            Phase = Phase.LoggedIn;
            StartPolling();
        }
    }

    private void ApplyVerified(GitAccount? account, RestUser user, List<string> scopes, string token)
    {
        User = Demo.IsOn ? Demo.User : user;
        Scopes = scopes;
        var updated = account?.With(user);
        if (updated is not null)
        {
            GitAccount = updated;
            Accounts.Update(updated);
            Accounts.MarkUsed(updated);
        }
        var key = updated?.CacheKey ?? user.Login;
        if (!string.Equals(Account, key, StringComparison.OrdinalIgnoreCase))
        {
            var stale = Account;
            Task.Run(() => ListSnapshotStore.Shared.Clear(stale)).Forget();
            Account = key;
            Refresh();
        }
        if (updated is null || updated.Provider == GitProvider.GitHub) Remember(token, user.Login);
    }

    private void EnterGitHub(GitAccount? account, GitHubApi api, RestUser user, List<string> scopes, string token)
    {
        var key = account?.CacheKey ?? user.Login;
        if (Account is { } previous && !string.Equals(previous, key, StringComparison.OrdinalIgnoreCase)) SearchListViewModel.ClearCache();
        if (account is not null)
        {
            Accounts.Update(account);
            Accounts.MarkUsed(account);
        }
        GitAccount = account;
        Forge = null;
        Account = key;
        Api = api;
        User = Demo.IsOn ? Demo.User : user;
        Scopes = scopes;
        prModels.Clear();
        prOrder.Clear();
        Phase = Phase.LoggedIn;
        StartPolling();
        if (account is null || account.Provider == GitProvider.GitHub) Remember(token, user.Login);
    }

    private void EnterForge(GitAccount account, IForgeClient client)
    {
        if (GitAccount?.Id != account.Id) ForgeListViewModel.ClearCache();
        Accounts.MarkUsed(account);
        GitAccount = account;
        Forge = client;
        Api = null;
        Account = account.CacheKey;
        User = account.User;
        Scopes = new();
        DetailRoot = null;
        ForgeSelection = null;
        if (Section.Kind == SectionKind.Prs ? !account.Provider.Supports(Section.Pr) : Section.Kind != SectionKind.Claude)
            Section = Section.Prs(account.Provider.SupportedFilters().FirstOrDefault(ForgeFilter.Mine).Filter());
        Phase = Phase.LoggedIn;
        StartPolling();
    }

    private async Task VerifyForge(IForgeClient client)
    {
        try
        {
            var u = await client.CurrentUser();
            if (!ReferenceEquals(Forge, client) || GitAccount is not { } account) return;
            var updated = account.With(u.User, u.Id ?? account.UserId);
            GitAccount = updated;
            User = updated.User;
            Accounts.Update(updated);
            Accounts.MarkUsed(updated);
        }
        catch (ApiException e) when (e.Status == 401)
        {
            if (ReferenceEquals(Forge, client)) Revoked();
        }
        catch
        {
        }
    }

    private IForgeClient? MakeForge(GitAccount account, GitCredential credential) =>
        ForgeClients.Create(account, credential, fresh =>
        {
            try
            {
                Accounts.Store(fresh, account);
            }
            catch (Exception e)
            {
                ErrorLog.Write(e, "storing refreshed credential failed");
            }
        });

    public async Task AddAccount(GitProvider provider, string rawHost, GitCredential credential, string? email = null)
    {
        var host = provider.EffectiveHost(rawHost);
        if (host.Length == 0) throw new ApiException(L.T("Login_EnterServer", "Enter the server address"));
        var draft = new GitAccount
        {
            Id = GitAccounts.NewId(),
            Provider = provider,
            Host = host,
            Email = string.IsNullOrWhiteSpace(email) ? null : email.Trim(),
            CredentialKey = "account-" + GitAccounts.NewId()
        };
        if (provider.IsGitHub())
        {
            var api = new GitHubApi(credential.Token, host);
            var (user, scopes) = await api.CurrentUser();
            var saved = Accounts.Upsert(draft.With(user), credential);
            await LeaveCurrent(saved);
            EnterGitHub(saved, api, user, scopes, credential.Token);
            return;
        }
        if (MakeForge(draft, credential) is not { } probe) return;
        var u = await probe.CurrentUser();
        var stored = Accounts.Upsert(draft.With(u.User, u.Id), credential);
        await LeaveCurrent(stored);
        if (MakeForge(stored, credential) is { } client) EnterForge(stored, client);
    }

    public async Task SwitchAccount(string id)
    {
        if (id == GitAccount?.Id && Phase == Phase.LoggedIn) return;
        if (Accounts.Find(id) is not { } target) return;
        if (Accounts.Credential(target) is not { } credential)
        {
            Accounts.Remove(id);
            ShowToast(L.F("Accounts_Missing", "Sign-in for {0} is missing. Add the account again", target.Subtitle));
            return;
        }
        await LeaveCurrent(target);
        await Resume(target, credential);
    }

    private async Task LeaveCurrent(GitAccount next)
    {
        if (Accounts.LastUsedIdentity is { } last && last != next.Identity) await SignOutOfClaude();
        if (Phase == Phase.LoggedIn && GitAccount?.Id != next.Id) ResetSession(forgetCache: false, Phase.Checking);
        Accounts.SetActive(next.Id);
    }

    public async Task SignOutOfClaude()
    {
        Claude.CloseAllForProfileSwitch();
        await ClaudeAccountStore.Shared.LogoutAll();
        await Claude.ReloadProjects();
        ShowToast(L.T("Accounts_ClaudeSignedOut", "Signed out of Claude Code"));
    }

    public async Task SignOut()
    {
        var current = GitAccount;
        await SignOutOfClaude();
        ResetSession(forgetCache: true);
        if (current is not null) Accounts.Remove(current.Id);
        else CredentialStore.Delete();
        Accounts.MarkUsed(null);
    }

    private void Revoked()
    {
        if (GitAccount is { } account) Accounts.Remove(account.Id);
        else CredentialStore.Delete();
        ResetSession(forgetCache: true);
    }

    public void ShowToast(string text) => ToastRequested?.Invoke(text);

    private static void Remember(string token, string login)
    {
        if (Demo.IsOn) return;
        var generation = ListSnapshotStore.Shared.AccountGeneration;
        Task.Run(() => ListSnapshotStore.Shared.RememberAccount(token, login, generation)).Forget();
    }

    private void ResetSession(bool forgetCache, Phase next = Phase.LoggedOut)
    {
        pollCts?.Cancel();
        if (forgetCache)
        {
            if (Provider == GitProvider.GitHub) ListSnapshotStore.Shared.ForgetAccount();
            ListSnapshotStore.Shared.Clear(Account);
        }
        SearchListViewModel.ClearCache();
        ForgeListViewModel.ClearCache();
        BlameCache.Shared.Clear();
        Account = null;
        Api = null;
        Forge = null;
        GitAccount = null;
        User = null;
        Scopes = new();
        DetailRoot = null;
        ForgeSelection = null;
        prModels.Clear();
        prOrder.Clear();
        ReviewRequests = new();
        MyPRs = new();
        ReviewRequestCount = 0;
        MyPRCount = 0;
        UnreadCount = 0;
        Phase = next;
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
        if (Api is not null && GitHubUrl.Parse(target) is { } route)
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
        if (text is null || text == lastClipboard || Api is null || Provider != GitProvider.GitHub) return;
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
        if (Forge is { } forge)
        {
            await RefreshForgeBadges(forge);
            return;
        }
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

    private async Task RefreshForgeBadges(IForgeClient forge)
    {
        var supported = forge.Provider.SupportedFilters();
        if (supported.Contains(ForgeFilter.ReviewRequested))
        {
            try
            {
                var rr = await forge.PullRequests(ForgeFilter.ReviewRequested, ForgeState.Open);
                if (!ReferenceEquals(Forge, forge)) return;
                ReviewRequests = rr.Take(10).ToList();
                ReviewRequestCount = rr.Count;
            }
            catch
            {
            }
        }
        if (supported.Contains(ForgeFilter.Mine))
        {
            try
            {
                var mine = await forge.PullRequests(ForgeFilter.Mine, ForgeState.Open);
                if (!ReferenceEquals(Forge, forge)) return;
                MyPRs = mine.Take(10).ToList();
                MyPRCount = mine.Count;
            }
            catch
            {
            }
        }
        UnreadCount = 0;
        LastBadgeRefresh = DateTimeOffset.Now;
    }

    public async Task RefreshTokenFromGh()
    {
        if (GitAccount is not { } account || !account.Provider.IsGitHub()) return;
        try
        {
            await AddAccount(account.Provider, account.Host, GitCredential.Of(await GhCli.Token(account.Host)));
        }
        catch
        {
        }
    }
}
