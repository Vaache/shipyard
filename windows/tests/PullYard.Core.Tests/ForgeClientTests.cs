using System.Net;
using System.Text;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Forges;
using PullYard.Core.Util;
using Xunit;

namespace PullYard.Core.Tests;

public class ForgeClientTests
{
    private sealed class StubHandler : HttpMessageHandler
    {
        public Dictionary<string, (HttpStatusCode Status, string Body)> Routes { get; } = new();
        public List<HttpRequestMessage> Requests { get; } = new();

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
        {
            lock (Requests) Requests.Add(request);
            var path = request.RequestUri!.AbsolutePath;
            var (status, body) = Routes.FirstOrDefault(r => path.EndsWith(r.Key, StringComparison.Ordinal)) is { Key: not null } hit
                ? hit.Value
                : (HttpStatusCode.NotFound, "{\"message\":\"not found\"}");
            return Task.FromResult(new HttpResponseMessage(status) { Content = new StringContent(body, Encoding.UTF8, "application/json") });
        }
    }

    private static GitAccount Account(GitProvider provider, string host, string login = "", string? email = null, string? userId = null) =>
        new() { Id = "test", Provider = provider, Host = host, Login = login, Email = email, UserId = userId, CredentialKey = "test" };

    private static string Auth(HttpRequestMessage r) => r.Headers.TryGetValues("Authorization", out var v) ? v.First() : "";

    [Fact]
    public async Task GitLabMapsMergeRequests()
    {
        var stub = new StubHandler();
        stub.Routes["/api/v4/user"] = (HttpStatusCode.OK, """{"id":7,"username":"sofia","name":"Sofia","avatar_url":"https://a/s.png"}""");
        stub.Routes["/api/v4/merge_requests"] = (HttpStatusCode.OK, """
            [{"iid":12,"title":"Fix login","web_url":"https://gitlab.example.com/group/sub/app/-/merge_requests/12","state":"merged","draft":false,
              "created_at":"2026-10-01T10:00:00.123Z","updated_at":"2026-10-02T11:30:00.456Z","target_branch":"main",
              "author":{"username":"max","avatar_url":null},"references":{"full":"group/sub/app!12"},"user_notes_count":3,
              "labels":[{"name":"bug","color":"#FF0000"}]}]
            """);
        var client = new GitLabClient(Account(GitProvider.GitLab, "gitlab.example.com"), GitCredential.Of("glpat-x"), client: new HttpClient(stub));
        var user = await client.CurrentUser();
        Assert.Equal("sofia", user.Login);
        Assert.Equal("7", user.Id);
        var items = await client.PullRequests(ForgeFilter.ReviewRequested, ForgeState.Open);
        var mr = Assert.Single(items);
        Assert.Equal(12, mr.Number);
        Assert.Equal("MERGED", mr.PrState);
        Assert.True(mr.IsPR);
        Assert.Equal("group/sub/app", mr.Repository.NameWithOwner);
        Assert.Equal("main", mr.BaseRefName);
        Assert.Equal("FF0000", mr.Labels!.Items[0].Color);
        Assert.Equal(3, mr.Comments.TotalCount);
        Assert.Equal("max", mr.Author!.Login);
        Assert.Equal(new DateTimeOffset(2026, 10, 2, 11, 30, 0, 456, TimeSpan.Zero), mr.UpdatedAt);
        var last = stub.Requests[^1];
        var query = last.RequestUri!.Query;
        Assert.Contains("reviewer_username=sofia", query);
        Assert.Contains("state=opened", query);
        Assert.Contains("scope=all", query);
        Assert.Equal("Bearer glpat-x", Auth(last));
    }

    [Fact]
    public async Task GitLabClosedQueriesMergedAndClosed()
    {
        var stub = new StubHandler();
        stub.Routes["/api/v4/merge_requests"] = (HttpStatusCode.OK, "[]");
        var client = new GitLabClient(Account(GitProvider.GitLab, "gitlab.com", "sofia"), GitCredential.Of("t"), client: new HttpClient(stub));
        await client.PullRequests(ForgeFilter.Mine, ForgeState.Closed);
        Assert.Equal(2, stub.Requests.Count);
        Assert.Contains("state=merged", stub.Requests[0].RequestUri!.Query);
        Assert.Contains("state=closed", stub.Requests[1].RequestUri!.Query);
        Assert.Contains("scope=created_by_me", stub.Requests[0].RequestUri!.Query);
    }

    [Fact]
    public async Task GitLabUnsupportedFilterReturnsEmpty()
    {
        var stub = new StubHandler();
        var client = new GitLabClient(Account(GitProvider.GitLab, "gitlab.com", "sofia"), GitCredential.Of("t"), client: new HttpClient(stub));
        Assert.Empty(await client.PullRequests(ForgeFilter.Mentioned, ForgeState.Open));
        Assert.Empty(stub.Requests);
    }

    [Fact]
    public async Task GitLabRefreshesExpiredToken()
    {
        var stub = new StubHandler();
        var calls = 0;
        var handler = new DelegateHandler(r =>
        {
            calls++;
            if (r.RequestUri!.AbsolutePath == "/oauth/token")
                return Reply(HttpStatusCode.OK, """{"access_token":"fresh","refresh_token":"r2","expires_in":7200}""");
            return Auth(r) == "Bearer fresh"
                ? Reply(HttpStatusCode.OK, """{"id":1,"username":"sofia"}""")
                : Reply(HttpStatusCode.Unauthorized, """{"message":"401 Unauthorized"}""");
        });
        GitCredential? stored = null;
        var credential = new GitCredential { Token = "old", RefreshToken = "r1", ClientId = "app" };
        var client = new GitLabClient(Account(GitProvider.GitLab, "gitlab.com"), credential, c => stored = c, new HttpClient(handler));
        var user = await client.CurrentUser();
        Assert.Equal("sofia", user.Login);
        Assert.Equal(3, calls);
        Assert.NotNull(stored);
        Assert.Equal("fresh", stored!.Token);
        Assert.Equal("r2", stored.RefreshToken);
        Assert.Equal("app", stored.ClientId);
    }

    [Fact]
    public async Task BitbucketMineAcrossWorkspaces()
    {
        var stub = new StubHandler();
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, """{"values":[{"workspace":{"slug":"acme"}},{"workspace":{"slug":"side"}}]}""");
        stub.Routes["/pullrequests/%7Bu-1%7D"] = (HttpStatusCode.OK, """
            {"values":[{"id":5,"title":"Add cache","state":"OPEN","draft":true,
              "created_on":"2026-10-01T10:00:00.123456+00:00","updated_on":"2026-10-03T09:00:00.000001+00:00",
              "author":{"uuid":"{u-1}","display_name":"Sofia","nickname":"sofia","links":{"avatar":{"href":"https://a/s.png"}}},
              "destination":{"branch":{"name":"main"},"repository":{"full_name":"acme/api"}},
              "links":{"html":{"href":"https://bitbucket.org/acme/api/pull-requests/5"}},"comment_count":2,
              "participants":[{"approved":true,"state":"approved"}]}]}
            """);
        var client = new BitbucketClient(Account(GitProvider.Bitbucket, "bitbucket.org", "sofia", "s@example.com", "{u-1}"), GitCredential.Of("tok"), new HttpClient(stub));
        var items = await client.PullRequests(ForgeFilter.Mine, ForgeState.Open);
        var pr = Assert.Single(items);
        Assert.Equal("acme/api", pr.Repository.NameWithOwner);
        Assert.True(pr.IsDraft);
        Assert.Equal("APPROVED", pr.ReviewDecision);
        Assert.Equal("https://bitbucket.org/acme/api/pull-requests/5", pr.Url);
        Assert.Equal("sofia", pr.Author!.Login);
        Assert.Equal(2, pr.Comments.TotalCount);
        Assert.Equal("Basic " + Convert.ToBase64String(Encoding.UTF8.GetBytes("s@example.com:tok")), Auth(stub.Requests[0]));
        var prRequests = stub.Requests.Where(r => r.RequestUri!.AbsolutePath.Contains("/pullrequests/")).ToList();
        Assert.Equal(2, prRequests.Count);
        Assert.Contains("/workspaces/acme/pullrequests/%7Bu-1%7D", prRequests[0].RequestUri!.AbsoluteUri);
        Assert.Contains("fields=%2Bvalues.participants", prRequests[0].RequestUri!.Query);
    }

    [Fact]
    public async Task BitbucketReviewRequestsQueryReviewerAcrossRepos()
    {
        var stub = new StubHandler();
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, """{"values":[{"workspace":{"slug":"acme"}}]}""");
        stub.Routes["/2.0/repositories/acme"] = (HttpStatusCode.OK, """{"values":[{"full_name":"acme/api"},{"full_name":"acme/web"}]}""");
        stub.Routes["/acme/api/pullrequests"] = (HttpStatusCode.OK, """
            {"values":[{"id":9,"title":"Review me","state":"OPEN","created_on":"2026-10-01T10:00:00Z","updated_on":"2026-10-04T10:00:00Z",
              "destination":{"repository":{"full_name":"acme/api"}},"participants":[{"approved":false,"state":"changes_requested"}]}]}
            """);
        var client = new BitbucketClient(Account(GitProvider.Bitbucket, "bitbucket.org", "sofia", "s@example.com", "{u-1}"), GitCredential.Of("tok"), new HttpClient(stub));
        var items = await client.PullRequests(ForgeFilter.ReviewRequested, ForgeState.Open);
        var pr = Assert.Single(items);
        Assert.Equal("CHANGES_REQUESTED", pr.ReviewDecision);
        Assert.Equal("https://bitbucket.org/acme/api/pull-requests/9", pr.Url);
        var search = stub.Requests.First(r => r.RequestUri!.AbsolutePath.EndsWith("/acme/api/pullrequests"));
        Assert.Contains("q=reviewers.uuid%3D%22%7Bu-1%7D%22", search.RequestUri!.Query);
    }

    [Fact]
    public async Task BitbucketCachesResults()
    {
        var stub = new StubHandler();
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, """{"values":[{"workspace":{"slug":"acme"}}]}""");
        stub.Routes["/pullrequests/%7Bu-1%7D"] = (HttpStatusCode.OK, """{"values":[]}""");
        var now = DateTimeOffset.Now;
        var client = new BitbucketClient(Account(GitProvider.Bitbucket, "bitbucket.org", "sofia", "s@example.com", "{u-1}"), GitCredential.Of("tok"), new HttpClient(stub), () => now);
        await client.PullRequests(ForgeFilter.Mine, ForgeState.Open);
        var count = stub.Requests.Count;
        await client.PullRequests(ForgeFilter.Mine, ForgeState.Open);
        Assert.Equal(count, stub.Requests.Count);
        now += BitbucketClient.CacheTtl + TimeSpan.FromSeconds(1);
        await client.PullRequests(ForgeFilter.Mine, ForgeState.Open);
        Assert.True(stub.Requests.Count > count);
    }

    [Fact]
    public async Task GiteaMapsMergedPull()
    {
        var stub = new StubHandler();
        stub.Routes["/api/v1/repos/issues/search"] = (HttpStatusCode.OK, """
            [{"number":3,"title":"Docs","html_url":"https://codeberg.org/me/site/pulls/3","state":"closed",
              "created_at":"2026-09-01T10:00:00+02:00","updated_at":"2026-09-02T10:00:00Z",
              "user":{"id":1,"login":"me","full_name":"","avatar_url":"https://a/me.png"},
              "repository":{"full_name":"me/site"},"comments":0,"labels":null,"pull_request":{"merged":true,"draft":false}}]
            """);
        var client = new GiteaClient(Account(GitProvider.Gitea, "codeberg.org", "me"), GitCredential.Of("tok"), new HttpClient(stub));
        var items = await client.PullRequests(ForgeFilter.Mine, ForgeState.Closed);
        var pr = Assert.Single(items);
        Assert.Equal("MERGED", pr.PrState);
        Assert.Equal("me/site", pr.Repository.NameWithOwner);
        Assert.Null(pr.Labels);
        var query = stub.Requests[^1].RequestUri!.Query;
        Assert.Contains("created=true", query);
        Assert.Contains("state=closed", query);
        Assert.Contains("type=pulls", query);
        Assert.Equal("token tok", Auth(stub.Requests[^1]));
    }

    [Fact]
    public async Task GiteaUserWithoutFullName()
    {
        var stub = new StubHandler();
        stub.Routes["/api/v1/user"] = (HttpStatusCode.OK, """{"id":42,"login":"me","full_name":"","avatar_url":"https://a/me.png"}""");
        var client = new GiteaClient(Account(GitProvider.Gitea, "git.example.com"), GitCredential.Of("tok"), new HttpClient(stub));
        var user = await client.CurrentUser();
        Assert.Equal("me", user.Login);
        Assert.Null(user.Name);
        Assert.Equal("42", user.Id);
        Assert.Equal("https://git.example.com/api/v1/user", stub.Requests[0].RequestUri!.AbsoluteUri);
    }

    [Fact]
    public async Task UnauthorizedIsReported()
    {
        var stub = new StubHandler();
        stub.Routes["/api/v1/user"] = (HttpStatusCode.Unauthorized, """{"message":"bad token"}""");
        var client = new GiteaClient(Account(GitProvider.Gitea, "codeberg.org"), GitCredential.Of("x"), new HttpClient(stub));
        var e = await Assert.ThrowsAsync<ApiException>(() => client.CurrentUser());
        Assert.Equal(401, e.Status);
    }

    [Fact]
    public async Task ServerErrorMessageIsSurfaced()
    {
        var stub = new StubHandler();
        stub.Routes["/api/v4/user"] = (HttpStatusCode.Forbidden, """{"error":"insufficient_scope","error_description":"needs api"}""");
        var client = new GitLabClient(Account(GitProvider.GitLab, "gitlab.com"), GitCredential.Of("x"), client: new HttpClient(stub));
        var e = await Assert.ThrowsAsync<ApiException>(() => client.CurrentUser());
        Assert.Equal(403, e.Status);
        Assert.Contains("needs api", e.Message);
    }

    [Fact]
    public void FactoryOnlyBuildsForges()
    {
        Assert.IsType<GitLabClient>(ForgeClients.Create(Account(GitProvider.GitLab, "gitlab.com"), GitCredential.Of("t")));
        Assert.IsType<BitbucketClient>(ForgeClients.Create(Account(GitProvider.Bitbucket, "bitbucket.org"), GitCredential.Of("t")));
        Assert.IsType<GiteaClient>(ForgeClients.Create(Account(GitProvider.Gitea, "codeberg.org"), GitCredential.Of("t")));
        Assert.Null(ForgeClients.Create(Account(GitProvider.GitHub, "github.com"), GitCredential.Of("t")));
        Assert.Null(ForgeClients.Create(Account(GitProvider.GitHubEnterprise, "git.corp"), GitCredential.Of("t")));
    }

    [Fact]
    public async Task GitHubEnterpriseUsesHostEndpoints()
    {
        var seen = new List<HttpRequestMessage>();
        var handler = new DelegateHandler(r =>
        {
            seen.Add(r);
            return Reply(HttpStatusCode.OK, r.Method == HttpMethod.Post ? """{"data":{}}""" : """{"login":"sofia"}""");
        });
        var api = new GitHubApi("t", new HttpClient(handler), host: "git.corp.example");
        await api.Rest<RestUser>("GET", "/user");
        await api.GraphQL<GitHubApi.EmptyData>("query { viewer { login } }");
        Assert.Equal("https://git.corp.example/api/v3/user", seen[0].RequestUri!.AbsoluteUri);
        Assert.Equal("https://git.corp.example/api/graphql", seen[1].RequestUri!.AbsoluteUri);
        Assert.Equal("Bearer t", Auth(seen[0]));
        Assert.Equal("Bearer t", Auth(seen[1]));
    }

    private static HttpResponseMessage Reply(HttpStatusCode status, string body) => new(status) { Content = new StringContent(body, Encoding.UTF8, "application/json") };

    private sealed class DelegateHandler : HttpMessageHandler
    {
        private readonly Func<HttpRequestMessage, HttpResponseMessage> reply;

        public DelegateHandler(Func<HttpRequestMessage, HttpResponseMessage> reply) => this.reply = reply;

        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct) => Task.FromResult(reply(request));
    }
}

public class GitAccountTests : IDisposable
{
    private sealed class MemoryVault : ICredentialVault
    {
        public Dictionary<string, string> Items { get; } = new();
        public string? Load(string key) => Items.TryGetValue(key, out var v) ? v : null;
        public void Save(string key, string value) => Items[key] = value;
        public bool Delete(string key) => Items.Remove(key);
    }

    private readonly string dir = Path.Combine(Path.GetTempPath(), "pullyard-accounts-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        try
        {
            Directory.Delete(dir, true);
        }
        catch
        {
        }
    }

    private Settings NewSettings() => new(Path.Combine(dir, "settings.json"));

    [Fact]
    public void NormalizesHost()
    {
        Assert.Equal("gitlab.example.com", GitProviders.NormalizeHost(" https://GitLab.Example.com/ "));
        Assert.Equal("git.local", GitProviders.NormalizeHost("http://git.local//"));
        Assert.Equal("bitbucket.org", GitProvider.Bitbucket.EffectiveHost("anything"));
    }

    [Fact]
    public void CredentialRoundTrip()
    {
        Assert.Equal("plain", GitCredential.Of("plain").Encoded);
        Assert.Equal("ghp_legacy", GitCredential.Decode("ghp_legacy").Token);
        var full = new GitCredential { Token = "a", RefreshToken = "r", ExpiresAt = DateTimeOffset.FromUnixTimeSeconds(100), ClientId = "c" };
        var back = GitCredential.Decode(full.Encoded);
        Assert.Equal("a", back.Token);
        Assert.Equal("r", back.RefreshToken);
        Assert.Equal("c", back.ClientId);
        Assert.Equal(DateTimeOffset.FromUnixTimeSeconds(100), back.ExpiresAt);
        Assert.Equal("{not json", GitCredential.Decode("{not json").Token);
    }

    [Fact]
    public void GitLabTerminology()
    {
        Assert.True(GitProvider.GitLab.UsesMergeRequests());
        Assert.Equal("!", GitProvider.GitLab.NumberPrefix());
        Assert.Equal("#", GitProvider.Bitbucket.NumberPrefix());
        Assert.Equal(new[] { ForgeFilter.ReviewRequested, ForgeFilter.Mine }, GitProvider.Bitbucket.SupportedFilters());
        Assert.Equal(5, GitProvider.Gitea.SupportedFilters().Count);
    }

    [Fact]
    public void CacheKeysDifferPerHost()
    {
        var gh = new GitAccount { Id = "1", Provider = GitProvider.GitHub, Host = "github.com", Login = "sofia" };
        var ghe = new GitAccount { Id = "2", Provider = GitProvider.GitHubEnterprise, Host = "git.corp", Login = "sofia" };
        Assert.Equal("sofia", gh.CacheKey);
        Assert.NotEqual(gh.CacheKey, ghe.CacheKey);
        Assert.False(gh.Same(ghe));
        Assert.Equal("@sofia · git.corp", ghe.Subtitle);
    }

    [Fact]
    public void MigratesLegacyGitHubToken()
    {
        var vault = new MemoryVault();
        vault.Save(GitAccounts.DefaultCredentialKey, "ghp_old");
        var store = new GitAccounts(NewSettings(), vault);
        var legacy = Assert.Single(store.All);
        Assert.Equal(GitProvider.GitHub, legacy.Provider);
        Assert.Equal(GitAccounts.DefaultCredentialKey, legacy.CredentialKey);
        Assert.Equal(legacy.Id, store.ActiveId);
        Assert.Equal("ghp_old", store.Credential(legacy)!.Token);

        var signedIn = store.Upsert(new GitAccount { Id = "new", Provider = GitProvider.GitHub, Host = "github.com", Login = "sofia", CredentialKey = "account-new" }, GitCredential.Of("ghp_new"));
        Assert.Equal(legacy.Id, signedIn.Id);
        Assert.Equal(GitAccounts.DefaultCredentialKey, signedIn.CredentialKey);
        Assert.Single(store.All);
        Assert.Equal("ghp_new", vault.Load(GitAccounts.DefaultCredentialKey));
    }

    [Fact]
    public void PersistsAccountsAndRemovesCredentials()
    {
        var vault = new MemoryVault();
        var settings = NewSettings();
        var store = new GitAccounts(settings, vault);
        Assert.Empty(store.All);
        var gitlab = store.Upsert(new GitAccount { Id = "a", Provider = GitProvider.GitLab, Host = "gitlab.com", Login = "Sofia", CredentialKey = "account-a" },
            new GitCredential { Token = "glpat", RefreshToken = "r", ClientId = "app" });
        var again = store.Upsert(new GitAccount { Id = "b", Provider = GitProvider.GitLab, Host = "gitlab.com", Login = "sofia", CredentialKey = "account-b" }, GitCredential.Of("glpat2"));
        Assert.Equal(gitlab.Id, again.Id);
        Assert.Equal("account-a", again.CredentialKey);
        store.SetActive(gitlab.Id);

        var reloaded = new GitAccounts(new Settings(Path.Combine(dir, "settings.json")), vault);
        var account = Assert.Single(reloaded.All);
        Assert.Equal(GitProvider.GitLab, account.Provider);
        Assert.Equal(gitlab.Id, reloaded.ActiveId);
        Assert.Equal("glpat2", reloaded.Credential(account)!.Token);

        Assert.True(reloaded.HasCredential(account));
        reloaded.ForgetCredential(account.Id);
        Assert.False(reloaded.HasCredential(account));
        Assert.Single(reloaded.All);
        Assert.Equal(gitlab.Id, reloaded.ActiveId);
        vault.Save(account.CredentialKey, "glpat3");

        reloaded.Remove(account.Id);
        Assert.Empty(reloaded.All);
        Assert.Null(reloaded.ActiveId);
        Assert.Empty(vault.Items);
    }
}

public class ClaudeProfileOwnershipTests : IDisposable
{
    private readonly string dir = Path.Combine(Path.GetTempPath(), "pullyard-profiles-" + Guid.NewGuid().ToString("N"));

    public void Dispose()
    {
        try
        {
            Directory.Delete(dir, true);
        }
        catch
        {
        }
    }

    private Claude.ClaudeProfiles Make() => new(new Settings(Path.Combine(dir, "settings.json")), Path.Combine(dir, "ClaudeAccounts"));

    [Fact]
    public void FirstAccountClaimsExistingProfilesAndOthersGetTheirOwn()
    {
        var profiles = Make();
        var work = profiles.Add("Work", false);
        Assert.Null(work.Owner);
        Assert.True(profiles.ClaimUnowned("a"));
        Assert.Equal(2, profiles.Owned("a").Count);
        Assert.False(profiles.ClaimUnowned("b"));
        var mine = profiles.Add("GitLab @sofia", false, "b");
        profiles.Scope = "b";
        Assert.Equal(new[] { mine.Id }, profiles.Visible.Select(p => p.Id));
        Assert.False(profiles.OwnsMain);
        profiles.Scope = "a";
        Assert.True(profiles.OwnsMain);
        Assert.Equal(2, profiles.Visible.Count);
        Assert.Equal("a", profiles.Add("Personal", false).Owner);
    }

    [Fact]
    public void RemembersActiveProfilePerAccount()
    {
        var profiles = Make();
        profiles.ClaimUnowned("a");
        var second = profiles.Add("Second", false, "a");
        var other = profiles.Add("Other", false, "b");
        profiles.SetActive(second.Id);
        profiles.SetActive(other.Id);
        Assert.Equal(second.Id, profiles.Preferred("a")!.Id);
        Assert.Equal(other.Id, profiles.Preferred("b")!.Id);
        var reloaded = Make();
        Assert.Equal(second.Id, reloaded.Preferred("a")!.Id);
        Assert.Equal("a", reloaded.Profiles.First(p => p.IsDefault).Owner);
        Assert.Null(reloaded.Preferred("nobody"));
    }

    [Fact]
    public void ReleasedMainProfileCanBeClaimedByNextAccount()
    {
        var profiles = Make();
        profiles.ClaimUnowned("a");
        profiles.UpdateCachedInfo("s@example.com", "Max");
        profiles.Release(Claude.ClaudeProfiles.DefaultId);
        var main = profiles.Profiles.First(p => p.IsDefault);
        Assert.Null(main.Owner);
        Assert.Null(main.Email);
        Assert.Null(profiles.Preferred("a"));
        Assert.True(profiles.ClaimUnowned("b"));
        Assert.Equal(Claude.ClaudeProfiles.DefaultId, profiles.Preferred("b")!.Id);
    }
}
