using System.Net;
using PullYard.Core.Auth;
using PullYard.Core.Forges;
using Xunit;

namespace PullYard.Core.Tests;

public class GiteaClientTests
{
    private static GiteaClient Client(ForgeStub stub) =>
        new(new GitAccount { Id = "test", Provider = GitProvider.Gitea, Host = "codeberg.org", Login = "me", CredentialKey = "test" }, GitCredential.Of("tok"), stub.Client());

    [Fact]
    public async Task CapabilitiesMatchGitHubSections()
    {
        var caps = await Client(new ForgeStub()).Capabilities();
        Assert.Equal(Enum.GetValues<ForgeFilter>(), caps.PullFilters);
        Assert.Equal(Enum.GetValues<ForgeIssueFilter>(), caps.IssueFilters);
        Assert.Equal(ForgeInbox.Notifications, caps.Inbox);
        Assert.True(caps.Repositories);
    }

    [Theory]
    [InlineData(ForgeFilter.ReviewRequested, "review_requested=true")]
    [InlineData(ForgeFilter.Mine, "created=true")]
    [InlineData(ForgeFilter.Assigned, "assigned=true")]
    [InlineData(ForgeFilter.Mentioned, "mentioned=true")]
    [InlineData(ForgeFilter.Reviewed, "reviewed=true")]
    public async Task EveryPullFilterUsesSearchFlag(ForgeFilter filter, string flag)
    {
        var stub = new ForgeStub();
        stub.Routes["/api/v1/repos/issues/search"] = (HttpStatusCode.OK, "[]");
        await Client(stub).PullRequests(filter, ForgeState.Open);
        Assert.Single(stub.Requests);
        foreach (var part in new[] { flag, "type=pulls", "state=open", "sort=recentupdate", "limit=50" }) Assert.Contains(part, stub.LastQuery);
    }

    [Fact]
    public async Task PullRequestMapping()
    {
        var stub = new ForgeStub();
        stub.Routes["/api/v1/repos/issues/search"] = (HttpStatusCode.OK, """
            [{"number":3,"title":"Docs","html_url":"https://codeberg.org/me/site/pulls/3","state":"open",
              "created_at":"2026-09-01T10:00:00+02:00","updated_at":"2026-09-02T10:00:00Z",
              "user":{"id":1,"login":"max","full_name":"Max","avatar_url":"https://a/max.png"},
              "repository":{"id":9,"name":"site","owner":"me","full_name":"me/site"},"comments":4,
              "labels":[{"name":"docs","color":"#00aabb"}],"pull_request":{"merged":false,"draft":true}},
             {"number":2,"title":"Old","html_url":"https://codeberg.org/me/site/pulls/2","state":"closed",
              "created_at":"2026-08-01T10:00:00Z","updated_at":"2026-08-02T10:00:00Z",
              "repository":{"full_name":"me/site"},"pull_request":{"merged":false}}]
            """);
        var items = await Client(stub).PullRequests(ForgeFilter.ReviewRequested, ForgeState.All);
        Assert.Equal(new[] { 3, 2 }, items.Select(i => i.Number));
        var pr = items[0];
        Assert.True(pr.IsPR);
        Assert.Equal("OPEN", pr.PrState);
        Assert.True(pr.IsDraft);
        Assert.Equal("max", pr.Author!.Login);
        Assert.Equal(4, pr.Comments.TotalCount);
        Assert.Equal("00aabb", pr.Labels!.Items[0].Color);
        Assert.Equal("CLOSED", items[1].PrState);
        Assert.Contains("state=all", stub.LastQuery);
    }

    [Theory]
    [InlineData(ForgeIssueFilter.Assigned, "assigned=true")]
    [InlineData(ForgeIssueFilter.Mine, "created=true")]
    [InlineData(ForgeIssueFilter.Mentioned, "mentioned=true")]
    public async Task IssuesUseSearchWithIssueType(ForgeIssueFilter filter, string flag)
    {
        var stub = new ForgeStub();
        stub.Routes["/api/v1/repos/issues/search"] = (HttpStatusCode.OK, """
            [{"number":11,"title":"Crash on start","html_url":"https://codeberg.org/org/app/issues/11","state":"closed",
              "created_at":"2026-10-01T09:00:00Z","updated_at":"2026-10-02T09:00:00Z",
              "user":{"id":2,"login":"ana","avatar_url":null},"repository":{"full_name":"org/app"},"comments":2,
              "labels":[{"name":"bug","color":"ee0701"}],"pull_request":null}]
            """);
        var issue = Assert.Single(await Client(stub).Issues(filter, ForgeState.Closed));
        Assert.Contains(flag, stub.LastQuery);
        Assert.Contains("type=issues", stub.LastQuery);
        Assert.Contains("state=closed", stub.LastQuery);
        Assert.False(issue.IsPR);
        Assert.Equal("CLOSED", issue.IssueState);
        Assert.Equal("COMPLETED", issue.StateReason);
        Assert.Equal("org/app", issue.Repository.NameWithOwner);
        Assert.Equal("https://codeberg.org/org/app/issues/11", issue.Url);
        Assert.Equal("bug", issue.Labels!.Items[0].Name);
    }

    [Fact]
    public async Task NotificationsInbox()
    {
        var stub = new ForgeStub();
        stub.Routes["/api/v1/notifications"] = (HttpStatusCode.OK, """
            [{"id":5,"unread":true,"pinned":false,"updated_at":"2026-10-03T10:00:00Z",
              "subject":{"title":"Fix login","type":"Pull","state":"merged","url":"https://codeberg.org/api/v1/repos/org/app/pulls/7","html_url":"","latest_comment_html_url":""},
              "repository":{"full_name":"org/app","html_url":"https://codeberg.org/org/app"}},
             {"id":6,"unread":false,"pinned":true,"updated_at":"2026-10-04T10:00:00Z",
              "subject":{"title":"Crash","type":"Issue","state":"open","url":"https://codeberg.org/api/v1/repos/org/app/issues/8","html_url":"https://codeberg.org/org/app/issues/8","latest_comment_html_url":"https://codeberg.org/org/app/issues/8#issuecomment-42"},
              "repository":{"full_name":"org/app"}},
             {"id":7,"unread":true,"updated_at":"2026-10-01T10:00:00Z",
              "subject":{"title":"Bump","type":"Commit","url":"https://codeberg.org/api/v1/repos/org/app/git/commits/abc123"},
              "repository":{"full_name":"org/app"}}]
            """);
        var inbox = await Client(stub).Notifications();
        Assert.Equal(new[] { "6", "5", "7" }, inbox.Select(n => n.Id));
        var pinned = inbox[0];
        Assert.Equal("Pinned", pinned.Reason);
        Assert.Equal("Issue", pinned.Kind);
        Assert.False(pinned.Unread);
        Assert.Equal("https://codeberg.org/org/app/issues/8#issuecomment-42", pinned.Url);
        var pull = inbox[1];
        Assert.Equal("Fix login", pull.Title);
        Assert.Equal("PR", pull.Kind);
        Assert.Equal("Pull request merged", pull.Reason);
        Assert.Equal("org/app", pull.Repo);
        Assert.Equal("https://codeberg.org/org/app/pulls/7", pull.Url);
        Assert.True(pull.Unread);
        Assert.Equal("https://codeberg.org/org/app/commit/abc123", inbox[2].Url);
        Assert.Equal("Commit", inbox[2].Reason);
        Assert.Contains("limit=50", stub.LastQuery);
        Assert.Single(stub.Requests);
    }

    [Fact]
    public async Task MarkDoneMarksThreadRead()
    {
        var stub = new ForgeStub();
        stub.Routes["/api/v1/notifications/threads/5"] = ((HttpStatusCode)205, "");
        var note = new ForgeNotification("5", "x", "org/app", "PR", "Pull request", "https://codeberg.org/org/app/pulls/7", DateTimeOffset.Now, null, true);
        await Client(stub).MarkDone(note);
        var request = stub.Requests[^1];
        Assert.Equal("PATCH", request.Method.Method);
        Assert.Equal("/api/v1/notifications/threads/5", request.RequestUri!.AbsolutePath);
        Assert.Contains("to-status=read", request.RequestUri.Query);
        Assert.Equal("token tok", request.Headers.GetValues("Authorization").First());
    }

    [Fact]
    public async Task RepositoriesSortedByUpdate()
    {
        var stub = new ForgeStub();
        stub.Routes["/api/v1/user/repos"] = (HttpStatusCode.OK, """
            [{"full_name":"me/old","description":"","html_url":"https://codeberg.org/me/old","private":false,"archived":false,
              "updated_at":"2026-01-01T10:00:00Z","default_branch":"main","stars_count":1},
             {"full_name":"me/new","description":"Fresh","html_url":"https://codeberg.org/me/new","private":true,"archived":false,
              "updated_at":"2026-10-01T10:00:00Z","default_branch":"dev","stars_count":5},
             {"full_name":"me/gone","html_url":"https://codeberg.org/me/gone","archived":true,"updated_at":"2026-10-05T10:00:00Z"}]
            """);
        var repos = await Client(stub).Repositories();
        Assert.Equal(new[] { "me/new", "me/old" }, repos.Select(r => r.FullName));
        var fresh = repos[0];
        Assert.Equal("Fresh", fresh.Description);
        Assert.True(fresh.IsPrivate);
        Assert.Equal("dev", fresh.DefaultBranch);
        Assert.Equal(5, fresh.Stars);
        Assert.Equal("https://codeberg.org/me/new", fresh.Url);
        Assert.Null(repos[1].Description);
        Assert.Single(stub.Requests);
        foreach (var part in new[] { "order_by=recentupdate", "limit=50", "page=1" }) Assert.Contains(part, stub.LastQuery);
    }

    [Fact]
    public async Task RepositoriesFollowFullPages()
    {
        static string Repo(int n) => $$"""{"full_name":"me/r{{n}}","html_url":"https://codeberg.org/me/r{{n}}","updated_at":"2026-10-01T10:00:00Z"}""";
        var stub = new ForgeStub();
        stub.QueryRoutes.Add(("page=1", HttpStatusCode.OK, "[" + string.Join(",", Enumerable.Range(0, 50).Select(Repo)) + "]"));
        stub.QueryRoutes.Add(("page=2", HttpStatusCode.OK, "[" + Repo(50) + "]"));
        var repos = await Client(stub).Repositories();
        Assert.Equal(51, repos.Count);
        Assert.Equal(2, stub.Requests.Count);
    }
}
