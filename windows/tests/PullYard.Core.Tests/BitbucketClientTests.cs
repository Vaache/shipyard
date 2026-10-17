using System.Net;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Forges;
using Xunit;

namespace PullYard.Core.Tests;

public class BitbucketClientTests
{
    private const string Workspaces = """{"values":[{"workspace":{"slug":"acme"}}]}""";
    private const string Repos = """{"values":[{"full_name":"acme/api","is_private":true,"updated_on":"2026-10-04T10:00:00+00:00","links":{"html":{"href":"https://bitbucket.org/acme/api"}}}]}""";

    private static BitbucketClient Client(ForgeStub stub) =>
        new(new GitAccount { Id = "test", Provider = GitProvider.Bitbucket, Host = "bitbucket.org", Login = "sofia", Email = "s@example.com", UserId = "{u-1}", CredentialKey = "test" }, GitCredential.Of("tok"), stub.Client());

    private static string Pull(int id, string author, string[]? reviewers = null, string[]? participants = null)
    {
        var reviewerJson = string.Join(",", (reviewers ?? Array.Empty<string>()).Select(r => "{\"uuid\":\"" + r + "\"}"));
        return "{\"id\":" + id + ",\"title\":\"PR " + id + "\",\"state\":\"OPEN\",\"created_on\":\"2026-10-01T10:00:00+00:00\",\"updated_on\":\"2026-10-03T09:00:00+00:00\","
            + "\"author\":{\"uuid\":\"" + author + "\",\"nickname\":\"n" + id + "\"},"
            + "\"destination\":{\"branch\":{\"name\":\"main\"},\"repository\":{\"full_name\":\"acme/api\"}},"
            + "\"reviewers\":[" + reviewerJson + "],\"participants\":[" + string.Join(",", participants ?? Array.Empty<string>()) + "]}";
    }

    private static string Values(params string[] items) => "{\"values\":[" + string.Join(",", items) + "]}";

    [Fact]
    public async Task Capabilities()
    {
        var caps = await Client(new ForgeStub()).Capabilities();
        Assert.Equal(new[] { ForgeFilter.ReviewRequested, ForgeFilter.Mine, ForgeFilter.Reviewed }, caps.PullFilters);
        Assert.True(caps.Repositories);
        Assert.Empty(caps.IssueFilters);
        Assert.False(caps.HasInbox);
    }

    [Fact]
    public async Task ReviewRequestedAndReviewedShareOneFetch()
    {
        var pulls = new[]
        {
            Pull(1, "{u-2}", new[] { "{u-1}" }, new[] { """{"user":{"uuid":"{u-1}"},"role":"REVIEWER","approved":false,"state":null}""" }),
            Pull(2, "{u-2}", new[] { "{U-1}" }, new[] { """{"user":{"uuid":"{u-1}"},"role":"REVIEWER","approved":true,"state":"approved"}""" }),
            Pull(3, "{u-3}", null, new[] { """{"user":{"uuid":"{u-1}"},"role":"PARTICIPANT","approved":false,"state":"changes_requested"}""" }),
            Pull(4, "{u-3}", null, new[] { """{"user":{"uuid":"{u-1}"},"role":"PARTICIPANT","approved":false,"state":null}""" }),
            Pull(5, "{u-1}", null, new[] { """{"user":{"uuid":"{u-1}"},"role":"PARTICIPANT","approved":true,"state":"approved"}""" }),
            Pull(6, "{u-2}", new[] { "{u-1}" })
        };
        var stub = new ForgeStub();
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, Workspaces);
        stub.Routes["/repositories/acme"] = (HttpStatusCode.OK, Repos);
        stub.Routes["/acme/api/pullrequests"] = (HttpStatusCode.OK, Values(pulls));
        var bb = Client(stub);
        var requested = await bb.PullRequests(ForgeFilter.ReviewRequested, ForgeState.Open);
        Assert.Equal(new[] { 1, 6 }, requested.Select(i => i.Number).OrderBy(n => n));
        var count = stub.Requests.Count;
        var reviewed = await bb.PullRequests(ForgeFilter.Reviewed, ForgeState.Open);
        Assert.Equal(new[] { 2, 3 }, reviewed.Select(i => i.Number).OrderBy(n => n));
        Assert.Equal("CHANGES_REQUESTED", reviewed.First(i => i.Number == 3).ReviewDecision);
        Assert.Equal(count, stub.Requests.Count);
        var query = System.Web.HttpUtility.ParseQueryString(stub.Containing("/pullrequests")[0].RequestUri!.Query);
        Assert.Equal("reviewers.uuid=\"{u-1}\" OR participants.user.uuid=\"{u-1}\"", query["q"]);
        Assert.Equal("+values.participants,+values.reviewers", query["fields"]);
    }

    [Fact]
    public async Task FallsBackToReviewerQueryWhenParticipantFilterRejected()
    {
        var stub = new ForgeStub();
        stub.QueryRoutes.Add(("participants.user.uuid", HttpStatusCode.BadRequest, """{"type":"error","error":{"message":"Invalid field name"}}"""));
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, Workspaces);
        stub.Routes["/repositories/acme"] = (HttpStatusCode.OK, Repos);
        stub.Routes["/acme/api/pullrequests"] = (HttpStatusCode.OK, Values(Pull(1, "{u-2}", new[] { "{u-1}" })));
        var bb = Client(stub);
        var requested = await bb.PullRequests(ForgeFilter.ReviewRequested, ForgeState.Open);
        Assert.Equal(new[] { 1 }, requested.Select(i => i.Number));
        Assert.Single(stub.Containing("participants.user.uuid"));
        await bb.PullRequests(ForgeFilter.ReviewRequested, ForgeState.Closed);
        Assert.Single(stub.Containing("participants.user.uuid"));
        Assert.Equal(3, stub.Containing("/pullrequests").Count);
    }

    [Fact]
    public async Task RepositoriesAcrossWorkspacesNewestFirst()
    {
        var stub = new ForgeStub();
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, """{"values":[{"workspace":{"slug":"acme"}},{"workspace":{"slug":"side"}}]}""");
        stub.Routes["/repositories/acme"] = (HttpStatusCode.OK, """
            {"values":[{"full_name":"acme/old","description":"Legacy","is_private":false,"updated_on":"2026-09-01T10:00:00.123456+00:00",
              "mainbranch":{"name":"master"},"links":{"html":{"href":"https://bitbucket.org/acme/old"}}}]}
            """);
        stub.Routes["/repositories/side"] = (HttpStatusCode.OK, """
            {"values":[{"full_name":"side/new","description":"","is_private":true,"updated_on":"2026-10-05T10:00:00+00:00","mainbranch":{"name":"main"}}]}
            """);
        stub.Routes["/pullrequests"] = (HttpStatusCode.OK, """{"values":[]}""");
        var bb = Client(stub);
        var list = await bb.Repositories();
        Assert.Equal(new[] { "side/new", "acme/old" }, list.Select(r => r.FullName));
        Assert.Equal("https://bitbucket.org/side/new", list[0].Url);
        Assert.Equal("main", list[0].DefaultBranch);
        Assert.Null(list[0].Description);
        Assert.False(list[1].IsPrivate);
        Assert.Equal("Legacy", list[1].Description);
        var query = stub.Containing("/repositories/acme")[0].RequestUri!.Query;
        Assert.Contains("role=member", query);
        Assert.Contains("sort=-updated_on", query);
        await bb.Repositories();
        await bb.PullRequests(ForgeFilter.Reviewed, ForgeState.Open);
        Assert.Single(stub.Containing("/repositories/acme?"));
        Assert.Single(stub.Containing("/user/workspaces"));
        Assert.Equal(new[] { "/2.0/repositories/side/new/pullrequests", "/2.0/repositories/acme/old/pullrequests" }, stub.Containing("/pullrequests").Select(r => r.RequestUri!.AbsolutePath));
    }

    [Fact]
    public async Task ReviewFetchCapsRepositories()
    {
        var many = Enumerable.Range(0, 30).Select(i => "{\"full_name\":\"acme/r" + i + "\",\"updated_on\":\"2026-10-" + (i + 1).ToString("00") + "T10:00:00+00:00\"}").ToArray();
        var stub = new ForgeStub();
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, Workspaces);
        stub.Routes["/repositories/acme"] = (HttpStatusCode.OK, Values(many));
        stub.Routes["/pullrequests"] = (HttpStatusCode.OK, """{"values":[]}""");
        await Client(stub).PullRequests(ForgeFilter.ReviewRequested, ForgeState.Open);
        var prRequests = stub.Containing("/pullrequests");
        Assert.Equal(BitbucketClient.MaxRepos, prRequests.Count);
        Assert.Equal("/2.0/repositories/acme/r29/pullrequests", prRequests[0].RequestUri!.AbsolutePath);
    }

    [Fact]
    public async Task ReviewRequestedReportsUnauthorized()
    {
        var stub = new ForgeStub();
        stub.Routes["/2.0/user/workspaces"] = (HttpStatusCode.OK, Workspaces);
        stub.Routes["/repositories/acme"] = (HttpStatusCode.OK, Repos);
        stub.Routes["/acme/api/pullrequests"] = (HttpStatusCode.Unauthorized, """{"error":{"message":"Unauthorized"}}""");
        var e = await Assert.ThrowsAsync<ApiException>(() => Client(stub).PullRequests(ForgeFilter.Reviewed, ForgeState.Open));
        Assert.Equal(401, e.Status);
    }
}
