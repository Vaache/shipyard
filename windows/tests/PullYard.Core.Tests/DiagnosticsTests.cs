using System.Text.Json;
using PullYard.Core.Api;
using PullYard.Core.Util;
using Xunit;

namespace PullYard.Core.Tests;

public class DiagnosticsTests : IDisposable
{
    private static readonly Uri Closed = new("http://127.0.0.1:1/logs/run.txt?sig=secret");

    public DiagnosticsTests()
    {
        Diagnostics.Enabled = false;
        Diagnostics.Shared.Clear();
    }

    public void Dispose()
    {
        Diagnostics.Enabled = false;
        Diagnostics.Shared.Clear();
    }

    private static RequestEntry Entry(string name, double ms, long bytes = 10, DateTimeOffset? ts = null, int? remaining = null, string? resource = null, long? reset = null) =>
        new(ts ?? DateTimeOffset.Now, Diagnostics.KindRest, name, Diagnostics.OriginUser, ms, bytes, 200, null, remaining, reset, resource);

    [Fact]
    public void RingBufferKeepsNewestThousand()
    {
        for (var i = 0; i < Diagnostics.RequestCapacity + 5; i++) Diagnostics.Shared.Add(Entry("r" + i, i));
        var all = Diagnostics.Shared.Requests();
        Assert.Equal(Diagnostics.RequestCapacity, all.Count);
        Assert.Equal("r" + (Diagnostics.RequestCapacity + 4), all[0].Name);
        Assert.Equal("r5", all[^1].Name);
    }

    [Theory]
    [InlineData("query Badges { viewer { login } }", "Whatever", "Badges")]
    [InlineData("  mutation AddComment($id: ID!) { x }", "", "AddComment")]
    [InlineData("query($o: String!) { repository(owner: $o) { id } }", "PullRequestDetail", "pullRequestDetail")]
    [InlineData("query { viewer { login } }", "", "graphql")]
    public void NamesGraphQLOperations(string query, string caller, string expected)
    {
        Assert.Equal(expected, Diagnostics.GraphQLName(query, caller));
    }

    [Fact]
    public void RestNameDropsHostAndQuery()
    {
        Assert.Equal("GET /repos/o/r/pulls", Diagnostics.RestName(HttpMethod.Get, new Uri("https://api.github.com/repos/o/r/pulls?per_page=100&page=2")));
        Assert.Equal("POST /graphql", Diagnostics.RestName(HttpMethod.Post, new Uri("https://api.github.com/graphql")));
    }

    [Fact]
    public void SummarizesDurationsAndRateLimits()
    {
        var now = DateTimeOffset.Now;
        var list = Enumerable.Range(1, 100).Select(i => Entry("a", i, 5, now.AddSeconds(i))).ToList();
        list.Add(Entry("b", 50, 5, now.AddSeconds(200), 4000, "core", 1_800_000_000));
        list.Add(Entry("c", 50, 5, now.AddSeconds(300), 4900, "graphql", 1_800_000_100));
        list.Add(Entry("c", 50, 5, now.AddSeconds(100), 4999, "graphql", 1_800_000_000));
        var s = Diagnostics.Summarize(list, Array.Empty<TickEntry>(), new[] { new SlowUiEntry(now, 40), new SlowUiEntry(now, 120) });
        Assert.Equal(103, s.Requests);
        Assert.Equal(515, s.TotalBytes);
        Assert.Equal(95, s.P95Ms);
        Assert.Equal("graphql", s.LastRateLimit!.Resource);
        Assert.Equal(4900, s.LastRateLimit.Remaining);
        Assert.Equal(DateTimeOffset.FromUnixTimeSeconds(1_800_000_100), s.LastRateLimit.Reset);
        Assert.Equal(new[] { "core", "graphql" }, s.RateLimits.Select(l => l.Resource));
        Assert.Equal(4000, s.RateLimits[0].Remaining);
        Assert.Equal(2, s.SlowUiCount);
        Assert.Equal(120, s.SlowUiMaxMs);
    }

    [Fact]
    public void GroupsByName()
    {
        var list = new[] { Entry("x", 10, 100), Entry("x", 30, 50), Entry("y", 5, 1) };
        var groups = Diagnostics.Group(list);
        Assert.Equal("x", groups[0].Name);
        Assert.Equal(2, groups[0].Count);
        Assert.Equal(20, groups[0].AvgMs);
        Assert.Equal(30, groups[0].P95Ms);
        Assert.Equal(150, groups[0].TotalBytes);
        Assert.Equal("y", groups[1].Name);
    }

    [Fact]
    public void ExportsOrderedJsonLines()
    {
        var now = DateTimeOffset.Now;
        var requests = new[] { Entry("late", 2, ts: now.AddSeconds(5), remaining: 10, resource: "core", reset: 1_800_000_000), Entry("early", 1, ts: now) };
        var ticks = new[] { new TickEntry(now.AddSeconds(1), 850, 3, true), new TickEntry(now.AddSeconds(2), 10, 1, false) };
        var slow = new[] { new SlowUiEntry(now.AddSeconds(3), 42) };
        var lines = Diagnostics.ExportJsonLines(requests, ticks, slow).Split('\n', StringSplitOptions.RemoveEmptyEntries);
        Assert.Equal(4, lines.Length);
        var docs = lines.Select(l => JsonDocument.Parse(l).RootElement).ToList();
        Assert.Equal(new[] { "request", "tick", "slow-ui", "request" }, docs.Select(d => d.GetProperty("type").GetString()));
        Assert.Equal("early", docs[0].GetProperty("name").GetString());
        Assert.Equal(JsonValueKind.Null, docs[0].GetProperty("rateLimitRemaining").ValueKind);
        Assert.Equal(3, docs[1].GetProperty("requests").GetInt32());
        Assert.Equal(10, docs[3].GetProperty("rateLimitRemaining").GetInt32());
        Assert.Equal("core", docs[3].GetProperty("rateLimitResource").GetString());
        Assert.EndsWith("Z", docs[3].GetProperty("ts").GetString());
        foreach (var key in new[] { "kind", "origin", "durationMs", "bytes", "status", "error", "rateLimitReset" }) Assert.True(docs[3].TryGetProperty(key, out _), key);
    }

    [Fact]
    public async Task DisabledRecordsNothing()
    {
        Assert.False(RequestProbe.Start().Active);
        Assert.Null(Diagnostics.BeginLiveTick());
        await Assert.ThrowsAnyAsync<Exception>(() => GitHubApi.Plain(Closed));
        Assert.Empty(Diagnostics.Shared.Requests());
    }

    [Fact]
    public async Task RecordsFailedRequestWithoutQueryString()
    {
        Diagnostics.Enabled = true;
        await Assert.ThrowsAnyAsync<Exception>(() => GitHubApi.Plain(Closed));
        var e = Assert.Single(Diagnostics.Shared.Requests());
        Assert.Equal(Diagnostics.KindRest, e.Kind);
        Assert.Equal("GET /logs/run.txt", e.Name);
        Assert.Equal(Diagnostics.OriginUser, e.Origin);
        Assert.Equal(0, e.Status);
        Assert.NotNull(e.Error);
        Assert.DoesNotContain("secret", Diagnostics.Shared.ExportJsonLines());
    }

    [Fact]
    public async Task AttributesRequestsToLiveTick()
    {
        Diagnostics.Enabled = true;
        var tick = Diagnostics.BeginLiveTick();
        Assert.NotNull(tick);
        Task<string> inTick;
        try
        {
            inTick = GitHubApi.Plain(Closed);
        }
        finally
        {
            Diagnostics.EndLiveTick(tick!);
        }
        await Assert.ThrowsAnyAsync<Exception>(() => inTick);
        await Assert.ThrowsAnyAsync<Exception>(() => GitHubApi.Plain(Closed));
        var all = Diagnostics.Shared.Requests();
        Assert.Equal(2, all.Count);
        Assert.Equal(Diagnostics.OriginUser, all[0].Origin);
        Assert.Equal(Diagnostics.OriginLiveTick, all[1].Origin);
        var t = Assert.Single(Diagnostics.Shared.Ticks());
        Assert.True(t.Finished);
        Assert.Equal(1, t.Requests);
        Assert.True(t.DurationMs >= all[1].DurationMs - 1);
    }

    [Fact]
    public void TickWithoutRequestsFinishesOnDispatch()
    {
        Diagnostics.Enabled = true;
        var tick = Diagnostics.BeginLiveTick()!;
        Assert.False(Diagnostics.Shared.Ticks()[0].Finished);
        Diagnostics.EndLiveTick(tick);
        var t = Assert.Single(Diagnostics.Shared.Ticks());
        Assert.True(t.Finished);
        Assert.Equal(0, t.Requests);
        Assert.Contains("Live ticks", Diagnostics.Shared.SummaryText());
    }
}
