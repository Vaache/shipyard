using System.Net;
using System.Text;
using System.Text.Json;
using PullYard.Core.Api;
using PullYard.Core.Util;
using Xunit;

namespace PullYard.Core.Tests;

public class PullRequestSignatureTests
{
    private const string Checks = @"[
        {""__typename"":""CheckRun"",""databaseId"":11,""name"":""build"",""status"":""COMPLETED"",""conclusion"":""SUCCESS""},
        {""__typename"":""CheckRun"",""databaseId"":12,""name"":""test"",""status"":""IN_PROGRESS"",""conclusion"":null},
        {""__typename"":""StatusContext"",""context"":""ci/legacy"",""state"":""PENDING""}]";

    private static string Full(string head = "abc", string checks = Checks, bool resolved = false) => $@"{{
        ""viewer"":{{""login"":""me""}},
        ""repository"":{{""viewerPermission"":""WRITE"",""pullRequest"":{{
            ""id"":""PR_1"",""number"":7,""title"":""T"",""state"":""OPEN"",""isDraft"":false,""mergeable"":""MERGEABLE"",""mergeStateStatus"":""CLEAN"",
            ""updatedAt"":""2026-10-01T10:00:00Z"",""headRefOid"":""{head}"",""reviewDecision"":""REVIEW_REQUIRED"",
            ""comments"":{{""totalCount"":3}},""reviews"":{{""totalCount"":2}},
            ""lastCommit"":{{""nodes"":[{{""commit"":{{""oid"":""{head}"",""committedDate"":""2026-10-01T09:00:00Z"",""statusCheckRollup"":{{""state"":""PENDING"",""contexts"":{{""nodes"":{checks}}}}}}}}}]}},
            ""allCommits"":{{""totalCount"":4,""nodes"":[{{""commit"":{{""oid"":""{head}"",""abbreviatedOid"":""abc"",""messageHeadline"":""m""}}}}]}},
            ""timelineItems"":{{""totalCount"":9,""nodes"":[]}},
            ""reviewThreads"":{{""nodes"":[{{""id"":""t1"",""isResolved"":{(resolved ? "true" : "false")},""path"":""a.cs"",""comments"":{{""nodes"":[]}}}},{{""id"":""t2"",""isResolved"":true,""path"":""b.cs"",""comments"":{{""nodes"":[]}}}}]}}
        }}}}}}";

    private static string Pulse(string head = "abc", string checks = Checks, bool resolved = false) => $@"{{
        ""repository"":{{""pullRequest"":{{
            ""updatedAt"":""2026-10-01T10:00:00Z"",""headRefOid"":""{head}"",""state"":""OPEN"",""isDraft"":false,""mergeable"":""MERGEABLE"",""mergeStateStatus"":""CLEAN"",""reviewDecision"":""REVIEW_REQUIRED"",
            ""comments"":{{""totalCount"":3}},""reviews"":{{""totalCount"":2}},
            ""timelineItems"":{{""totalCount"":9}},
            ""reviewThreads"":{{""nodes"":[{{""isResolved"":{(resolved ? "true" : "false")}}},{{""isResolved"":true}}]}},
            ""allCommits"":{{""totalCount"":4}},
            ""lastCommit"":{{""nodes"":[{{""commit"":{{""oid"":""{head}"",""statusCheckRollup"":{{""state"":""PENDING"",""contexts"":{{""nodes"":{checks}}}}}}}}}]}}
        }}}}}}";

    private static PullRequestDetail Parse(string json) => JsonSerializer.Deserialize<PRDetailResponse>(json, Json.GraphQL)!.Repository!.PullRequest!;

    [Fact]
    public void PulseMatchesFullQuery()
    {
        Assert.Equal(PullRequestSignature.Of(Parse(Full())), PullRequestSignature.Of(Parse(Pulse())));
    }

    [Fact]
    public void ChecksOrderDoesNotMatter()
    {
        var reordered = @"[
            {""__typename"":""StatusContext"",""state"":""PENDING""},
            {""__typename"":""CheckRun"",""status"":""IN_PROGRESS"",""conclusion"":null},
            {""__typename"":""CheckRun"",""status"":""COMPLETED"",""conclusion"":""SUCCESS""}]";
        Assert.Equal(PullRequestSignature.Of(Parse(Full())), PullRequestSignature.Of(Parse(Pulse(checks: reordered))));
    }

    [Fact]
    public void DetectsCheckProgress()
    {
        var finished = Checks.Replace(@"""status"":""IN_PROGRESS"",""conclusion"":null", @"""status"":""COMPLETED"",""conclusion"":""FAILURE""");
        Assert.NotEqual(PullRequestSignature.Of(Parse(Full())), PullRequestSignature.Of(Parse(Pulse(checks: finished))));
    }

    [Fact]
    public void DetectsNewHeadAndResolvedThreads()
    {
        var baseline = PullRequestSignature.Of(Parse(Full()));
        Assert.NotEqual(baseline, PullRequestSignature.Of(Parse(Pulse(head: "def"))));
        Assert.NotEqual(baseline, PullRequestSignature.Of(Parse(Pulse(resolved: true))));
    }

    private sealed class Capture : HttpMessageHandler
    {
        public string Body { get; private set; } = "";

        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
        {
            Body = await request.Content!.ReadAsStringAsync(ct);
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent("{\"data\":" + Pulse() + "}", Encoding.UTF8, "application/json") };
        }
    }

    [Fact]
    public async Task PulseQueryIsSmall()
    {
        var handler = new Capture();
        var api = new GitHubApi("t", new HttpClient(handler));
        var pr = await api.PullRequestPulse(new ItemRef(new RepoRef("o", "r"), 7));
        Assert.Equal("abc", pr!.HeadRefOid);
        Assert.Contains("query PullRequestPulse", handler.Body);
        Assert.DoesNotContain("bodyHTML", handler.Body);
        Assert.DoesNotContain("timelineItems(last", handler.Body);
        Assert.DoesNotContain("diffHunk", handler.Body);
    }
}
