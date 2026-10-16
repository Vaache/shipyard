using System.Text.Json;
using System.Text.RegularExpressions;
using PullYard.Core.Api;
using PullYard.Core.Html;
using PullYard.Core.Util;
using Xunit;

namespace PullYard.Core.Tests;

public class BlameTests
{
    private const string Sample = """
    {"data":{"repository":{"object":{"__typename":"Commit","blame":{"ranges":[
      {"startingLine":4,"endingLine":6,"age":10,"commit":{"oid":"5e53caa8738f7b94d04360572803302656b6576c","abbreviatedOid":"5e53caa","messageHeadline":"Move SPM manifest","committedDate":"2020-04-25T10:11:33Z","url":"https://github.com/octo/repo/commit/5e53caa8738f7b94d04360572803302656b6576c","author":{"name":"Marcin K","user":null}}},
      {"startingLine":1,"endingLine":3,"age":1,"commit":{"oid":"e028a08b6cf31faf539fee07a8c583ad8580620d","abbreviatedOid":"e028a08","messageHeadline":"Parse <thread> optimizations","committedDate":"2026-08-18T14:54:43Z","url":"https://github.com/octo/repo/commit/e028a08b6cf31faf539fee07a8c583ad8580620d","author":{"name":"Miguel","user":{"login":"migueldeicaza","avatarUrl":"https://avatars.githubusercontent.com/u/36863"}}}}
    ]}}}}}
    """;

    private static List<BlameRangeNode> Nodes() =>
        JsonDocument.Parse(Sample).RootElement.GetProperty("data").Deserialize<BlameResponse>(Json.GraphQL)!.Repository!.Object!.Blame!.Ranges;

    private static BlameRangeNode Node(int start, int end, int age = 5, string? login = "octo", string? name = "Octo Cat") => new()
    {
        StartingLine = start,
        EndingLine = end,
        Age = age,
        Commit = new BlameCommit
        {
            Oid = "abcdef1234567890",
            MessageHeadline = "msg",
            Url = "https://github.com/octo/repo/commit/abcdef1234567890",
            Author = new BlameAuthor { Name = name, User = login is null ? null : new Actor { Login = login } }
        }
    };

    [Fact]
    public void DecodesAndMapsGraphQLResponse()
    {
        var ranges = Blame.Map(Nodes());
        Assert.Equal(2, ranges.Count);
        Assert.Equal((1, 3), (ranges[0].Start, ranges[0].End));
        Assert.Equal("migueldeicaza", ranges[0].Author);
        Assert.Equal("migueldeicaza", ranges[0].Login);
        Assert.Equal("e028a08", ranges[0].ShortSha);
        Assert.Equal(9, ranges[0].Heat);
        Assert.Equal("Marcin K", ranges[1].Author);
        Assert.Null(ranges[1].Login);
        Assert.Equal(0, ranges[1].Heat);
        Assert.Equal(3, ranges[1].Lines);
        Assert.Equal(new DateTimeOffset(2020, 4, 25, 10, 11, 33, TimeSpan.Zero), ranges[1].Date);
    }

    [Fact]
    public void ClampsOverlapsAndDropsEmptyRanges()
    {
        var ranges = Blame.Map(new[] { Node(5, 12), Node(1, 3), Node(3, 6), Node(20, 30), new BlameRangeNode { StartingLine = 13, EndingLine = 14 } }, 10);
        Assert.Equal(new[] { (1, 3), (4, 6), (7, 10) }, ranges.Select(r => (r.Start, r.End)));
    }

    [Fact]
    public void FallsBackToNameThenUnknownAndShortensSha()
    {
        var ranges = Blame.Map(new[] { Node(1, 1, login: null), Node(2, 2, login: null, name: "  "), Node(3, 3, login: "") });
        Assert.Equal(new[] { "Octo Cat", "unknown", "Octo Cat" }, ranges.Select(r => r.Author));
        Assert.All(ranges, r => Assert.Equal("abcdef1", r.ShortSha));
    }

    [Theory]
    [InlineData(-3, 9)]
    [InlineData(1, 9)]
    [InlineData(5, 5)]
    [InlineData(10, 0)]
    [InlineData(42, 0)]
    public void HeatIsStrongestForNewestChanges(int age, int heat) =>
        Assert.Equal(heat, Blame.Map(new[] { Node(1, 1, age) })[0].Heat);

    [Theory]
    [InlineData("", 1)]
    [InlineData("a", 1)]
    [InlineData("a\n", 1)]
    [InlineData("a\nb", 2)]
    [InlineData("a\nb\n\n", 3)]
    public void CountsLines(string text, int lines) => Assert.Equal(lines, Blame.LineCount(text));

    [Fact]
    public void RendersOneLabelPerRangeInTheSameTable()
    {
        var text = string.Join("\n", Enumerable.Range(1, 8).Select(i => $"line {i}")) + "\n";
        var ranges = Blame.Map(Nodes().Concat(new[] { Node(8, 8) }), Blame.LineCount(text));
        var html = CodeHtml.Render(text, "txt", false, ranges, new DateTimeOffset(2026, 10, 16, 0, 0, 0, TimeSpan.Zero));
        Assert.Contains("<table class=\"code blame\">", html);
        Assert.Equal(8, Regex.Matches(html, "<tr>").Count);
        Assert.Equal(8, Regex.Matches(html, "<td class=\"ln\">").Count);
        Assert.Equal(3, Regex.Matches(html, "<a class=\"ba\"").Count);
        Assert.Single(Regex.Matches(html, "<td class=\"bl\"></td>"));
        Assert.Contains("<td class=\"bl h9\" rowspan=\"3\">", html);
        Assert.Contains("<td class=\"bl h0\" rowspan=\"3\">", html);
        Assert.Contains("<td class=\"bl h5\">", html);
        Assert.Contains("href=\"https://github.com/octo/repo/commit/e028a08b6cf31faf539fee07a8c583ad8580620d\"", html);
        Assert.Contains("e028a08 · Parse &lt;thread&gt; optimizations", html);
        Assert.Contains("<span class=\"when\">1 mo. ago</span>", html);
        Assert.Contains("<span class=\"when\">6 yr. ago</span>", html);
    }

    [Fact]
    public void RendersWithoutBlameAsBefore()
    {
        var html = CodeHtml.Render("a\nb\n", "txt", true);
        Assert.Equal("<table class=\"code wrap\"><tbody><tr><td class=\"ln\">1</td><td class=\"src\">a</td></tr><tr><td class=\"ln\">2</td><td class=\"src\">b</td></tr></tbody></table>", html);
    }

    [Fact]
    public void PeelsAnnotatedTagsToTheirCommit()
    {
        const string tag = """
        {"repository":{"object":{"__typename":"Tag","target":{"__typename":"Commit","oid":"abc","blame":{"ranges":[{"startingLine":1,"endingLine":2,"age":3,"commit":{"oid":"abc","abbreviatedOid":"abc","messageHeadline":"m","url":"","author":{"name":"N"}}}]}}}}}
        """;
        var r = JsonSerializer.Deserialize<BlameResponse>(tag, Json.GraphQL)!;
        Assert.Single(r.Repository!.Object!.Ranges!);
        Assert.Null(JsonSerializer.Deserialize<BlameResponse>("""{"repository":{"object":{"__typename":"Blob"}}}""", Json.GraphQL)!.Repository!.Object!.Ranges);
    }

    [Fact]
    public void UsesTheGivenLabelForUnknownAuthors() =>
        Assert.Equal("inconnu", Blame.Map(new[] { Node(1, 1, login: null, name: null) }, unknown: "inconnu")[0].Author);

    [Fact]
    public void DiffBlameButtonUsesLabelsAndSkipsRemovedFiles()
    {
        var files = new[]
        {
            new DiffFile(new PRFile { Filename = "a.txt", Status = "modified", Patch = "@@ -1 +1 @@\n-a\n+b" }),
            new DiffFile(new PRFile { Filename = "gone.txt", Status = "removed", Patch = "@@ -1 +0,0 @@\n-a" })
        };
        var html = DiffHtml.Render(files, new DiffRenderOptions { ShowBlame = true, BlameLabel = "Auteur", BlameTitle = "T<x>" });
        Assert.Single(Regex.Matches(html, "postFile\\(this,'blame'\\)"));
        Assert.Contains("title=\"T&lt;x&gt;\">Auteur</button>", html);
        Assert.DoesNotContain("'blame'", DiffHtml.Render(files, new DiffRenderOptions()));
    }

    [Fact]
    public void BlameQueryIsNamedForDiagnostics() => Assert.StartsWith("query Blame(", GitHubApi.BlameQuery);
}

public class ShortLinkTests
{
    [Fact]
    public void FormatsItemRefs()
    {
        Assert.Equal("octo/repo#123", ShortLink.Of(new ItemRef(new RepoRef("octo", "repo"), 123)));
        Assert.Equal("octo/repo#7", ShortLink.Of("octo/repo", 7));
    }

    [Theory]
    [InlineData("https://github.com/octo/repo/pull/12", "octo/repo#12")]
    [InlineData("https://github.com/octo/repo/pull/12/files", "octo/repo#12")]
    [InlineData("https://github.com/octo/repo/issues/7", "octo/repo#7")]
    [InlineData("https://api.github.com/repos/octo/repo/pulls/9", "octo/repo#9")]
    [InlineData("https://github.com/octo/repo/commit/abc123", null)]
    [InlineData("https://github.com/octo/repo", null)]
    [InlineData("octo/repo#5", null)]
    [InlineData(null, null)]
    public void FormatsFromUrls(string? url, string? expected) => Assert.Equal(expected, ShortLink.FromUrl(url));

    [Fact]
    public void OnlyPullRequestsAndIssuesHaveShortLinks()
    {
        var repo = new RepoRef("octo", "repo");
        Assert.Equal("octo/repo#3", ShortLink.Of(Route.Pr(new ItemRef(repo, 3))));
        Assert.Equal("octo/repo#4", ShortLink.Of(Route.Issue(new ItemRef(repo, 4))));
        Assert.Null(ShortLink.Of(Route.ForRepo(repo)));
        Assert.Null(ShortLink.Of(Route.Commit(repo, "abc")));
        Assert.Null(ShortLink.Of((Route?)null));
    }
}

public class BlameCacheTests
{
    private static readonly RepoRef Repo = new("Octo", "Repo");
    private static readonly List<BlameRange> Ranges = new() { new BlameRange(1, 1, 1, "a", "a", "m", null, "", "x", null, null) };
    private const string Sha = "0123456789abcdef0123456789abcdef01234567";

    [Fact]
    public void KeysIncludeAccountRepoRefAndPath()
    {
        Assert.Equal("me|octo/repo|main|src/a.cs", BlameCache.Key("Me", Repo, "main", "src/a.cs"));
        Assert.NotEqual(BlameCache.Key("me", Repo, "main", "a"), BlameCache.Key("other", Repo, "main", "a"));
    }

    [Fact]
    public void BranchEntriesExpireButCommitEntriesDoNot()
    {
        var cache = new BlameCache(10, TimeSpan.FromMinutes(10));
        var t0 = new DateTimeOffset(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);
        cache.Set("b", Ranges, t0);
        cache.Set("c", Ranges, t0);
        Assert.Same(Ranges, cache.Get("b", "main", t0.AddMinutes(9)));
        Assert.Null(cache.Get("b", "main", t0.AddMinutes(11)));
        Assert.Same(Ranges, cache.Get("c", Sha, t0.AddDays(30)));
        Assert.Equal(1, cache.Count);
    }

    [Fact]
    public void EvictsLeastRecentlyUsedAndInvalidates()
    {
        var cache = new BlameCache(2);
        cache.Set("1", Ranges);
        cache.Set("2", Ranges);
        Assert.NotNull(cache.Get("1", Sha));
        cache.Set("3", Ranges);
        Assert.Null(cache.Get("2", Sha));
        Assert.NotNull(cache.Get("1", Sha));
        cache.Invalidate("1");
        Assert.Null(cache.Get("1", Sha));
        cache.Clear();
        Assert.Equal(0, cache.Count);
    }

    [Theory]
    [InlineData("0123456789abcdef0123456789abcdef01234567", true)]
    [InlineData("0123456789ABCDEF0123456789abcdef01234567", true)]
    [InlineData("main", false)]
    [InlineData("0123456", false)]
    [InlineData("refs/pull/27/head", false)]
    public void RecognisesCommitIds(string @ref, bool expected) => Assert.Equal(expected, BlameCache.IsCommitId(@ref));
}
