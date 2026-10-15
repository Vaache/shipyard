using System.Text.Json;
using PullYard.Core;
using PullYard.Core.Api;
using PullYard.Core.Claude;
using PullYard.Core.Git;
using PullYard.Core.Html;
using PullYard.Core.Update;
using PullYard.Core.Util;
using Xunit;

namespace PullYard.Core.Tests;

public class CoreTests
{
    [Theory]
    [InlineData("https://github.com/octo/repo/pull/12", RouteKind.PullRequest, 12)]
    [InlineData("https://github.com/octo/repo/issues/7", RouteKind.Issue, 7)]
    [InlineData("github.com/octo/repo/pull/3/files", RouteKind.PullRequest, 3)]
    [InlineData("octo/repo#44", RouteKind.PullRequest, 44)]
    [InlineData("https://api.github.com/repos/octo/repo/pulls/9", RouteKind.PullRequest, 9)]
    public void ParsesItemLinks(string url, RouteKind kind, int number)
    {
        var r = GitHubUrl.Parse(url);
        Assert.NotNull(r);
        Assert.Equal(kind, r!.Kind);
        Assert.Equal(number, r.Item!.Number);
        Assert.Equal("octo/repo", r.Item.Repo.Full);
    }

    [Fact]
    public void ParsesRepoAndCommitLinks()
    {
        Assert.Equal(RouteKind.Repo, GitHubUrl.Parse("https://github.com/octo/repo")!.Kind);
        var c = GitHubUrl.Parse("https://github.com/octo/repo/commit/abc123")!;
        Assert.Equal(RouteKind.Commit, c.Kind);
        Assert.Equal("abc123", c.Sha);
        Assert.Null(GitHubUrl.Parse("https://github.com/settings/tokens"));
        Assert.Null(GitHubUrl.Parse("hello world"));
    }

    [Fact]
    public void ParsesDiff()
    {
        var lines = DiffParser.Parse("@@ -10,3 +10,4 @@ func\n a\n-b\n+c\n+d\n e\n\\ No newline at end of file");
        Assert.Equal(DiffLineKind.Hunk, lines[0].Kind);
        Assert.Equal(10, lines[1].OldLine);
        Assert.Equal(10, lines[1].NewLine);
        Assert.Equal(DiffLineKind.Del, lines[2].Kind);
        Assert.Equal(11, lines[2].OldLine);
        Assert.Equal(DiffLineKind.Add, lines[3].Kind);
        Assert.Equal(11, lines[3].NewLine);
        Assert.Equal(12, lines[4].NewLine);
        Assert.Equal(DiffLineKind.Meta, lines[6].Kind);
    }

    [Fact]
    public void ComparesVersions()
    {
        Assert.Equal(1, Releases.Compare("2.10.0", "2.9.9"));
        Assert.Equal(0, Releases.Compare("v2.5", "2.5.0"));
        Assert.Equal(-1, Releases.Compare("2.5.0-beta", "2.5.1"));
    }

    [Fact]
    public void ParsesRelease()
    {
        var json = $$"""
        {"tag_name":"v2.6.0","name":"","body":"notes","html_url":"https://example.com/r","published_at":"2026-01-02T03:04:05Z",
         "assets":[{"name":"{{AppInfo.InstallerAssetName("x64")}}","browser_download_url":"https://example.com/x64.exe","size":1234},
                   {"name":"{{AppInfo.PortableAssetName("arm64")}}","browser_download_url":"https://example.com/arm64.zip","size":99}]}
        """;
        var r = Releases.Parse(json, "x64");
        Assert.Equal("2.6.0", r.Version);
        Assert.Equal("https://example.com/x64.exe", r.InstallerUrl);
        Assert.Equal(1234, r.InstallerSize);
        var arm = Releases.Parse(json, "arm64");
        Assert.Null(arm.InstallerUrl);
        Assert.Equal("https://example.com/arm64.zip", arm.PortableUrl);
    }

    [Fact]
    public void DecodesSearchResponse()
    {
        const string json = """
        {"data":{"search":{"issueCount":2,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
          {"__typename":"PullRequest","number":5,"title":"Fix","url":"https://github.com/o/r/pull/5","prState":"OPEN","isDraft":false,
           "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-02T00:00:00.123Z","baseRefName":"feature","author":{"login":"me","avatarUrl":null},
           "repository":{"nameWithOwner":"o/r","defaultBranchRef":{"name":"main"}},"reviewDecision":"APPROVED","additions":3,"deletions":1,
           "comments":{"totalCount":2},"labels":{"nodes":[{"name":"bug","color":"ff0000"},null]},"commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]}},
          null,
          {"__typename":"Issue","number":6,"title":"Bug","url":"https://github.com/o/r/issues/6","issueState":"CLOSED","stateReason":"NOT_PLANNED",
           "createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","author":null,"repository":{"nameWithOwner":"o/r"},"comments":{"totalCount":0}}
        ]}}}
        """;
        var doc = JsonDocument.Parse(json);
        var search = doc.RootElement.GetProperty("data").Deserialize<SearchResponse>(Json.GraphQL)!.Search;
        Assert.Equal(2, search.Nodes.Count);
        var pr = search.Nodes[0];
        Assert.True(pr.IsPR);
        Assert.Equal("SUCCESS", pr.CiState);
        Assert.Equal("feature", pr.StackedOn);
        Assert.Single(pr.Labels!.Items);
        Assert.Equal(VisualState.NotPlanned, search.Nodes[1].VisualState);
    }

    [Fact]
    public void ExtractsFixPrompt()
    {
        Assert.Equal("Do the thing", FixPromptExtractor.Extract("intro\n--- FIX PROMPT ---\nDo the thing\n--- END FIX PROMPT ---\nrest"));
        Assert.Equal("fix it", FixPromptExtractor.Extract("## Prompt for AI agent\n```text\nfix it\n```"));
        Assert.Null(FixPromptExtractor.Extract("nothing here"));
    }

    [Fact]
    public void ParsesUnifiedDiff()
    {
        var text = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/old.txt b/new.txt\nsimilarity index 90%\nrename from old.txt\nrename to new.txt\n";
        var files = GitCli.ParseUnifiedDiff(text);
        Assert.Equal(2, files.Count);
        Assert.Equal(1, files[0].Additions);
        Assert.Equal(1, files[0].Deletions);
        Assert.Equal("renamed", files[1].Status);
        Assert.Equal("old.txt", files[1].PreviousFilename);
        Assert.Equal("github/cli", GitCli.ParseRemote("git@github.com:github/cli.git")!.Full);
    }

    [Fact]
    public void EscapesHtml()
    {
        Assert.Equal("&lt;a href=&quot;x&quot;&gt;&amp;&#39;", ConversationHtml.Esc("<a href=\"x\">&'"));
        var html = CodeHtml.Render("let x = \"s\" // c\n", "swift", false);
        Assert.Contains("class=\"k\"", html);
        Assert.Contains("class=\"cm\"", html);
    }

    [Fact]
    public void CleansSessionPrompt()
    {
        Assert.Null(ClaudeSessionParser.CleanPrompt("<command-name>/clear</command-name>"));
        Assert.Equal("hello world", ClaudeSessionParser.CleanPrompt("  hello\n  world "));
        Assert.Equal("C:\\code\\proj", ClaudeSessionParser.ProjectRoot("C:\\code\\proj\\.claude\\worktrees\\x"));
    }

    [Fact]
    public void QuotesArguments()
    {
        Assert.Equal("simple", Proc.QuoteArg("simple"));
        Assert.Equal("\"has space\"", Proc.QuoteArg("has space"));
        Assert.Equal("\"say \\\"hi\\\"\"", Proc.QuoteArg("say \"hi\""));
        Assert.Equal("\"\"", Proc.QuoteArg(""));
    }

    [Fact]
    public void ReadsBranding()
    {
        Assert.False(string.IsNullOrWhiteSpace(AppInfo.DisplayName));
        Assert.Contains("/", AppInfo.RepoSlug);
        Assert.EndsWith("/releases/latest", AppInfo.LatestReleaseApi);
    }
}
