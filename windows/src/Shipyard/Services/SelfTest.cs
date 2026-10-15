using System.Text.Json;
using Shipyard.Core;
using Shipyard.Core.Api;
using Shipyard.Core.Claude;
using Shipyard.Core.Html;
using Shipyard.Core.Update;
using Shipyard.Core.Util;

namespace Shipyard.Services;

public static class SelfTest
{
    private static int failures;

    private static async Task Step(string name, Func<Task<string>> op)
    {
        try
        {
            Console.WriteLine($"OK   {name}: {await op()}");
        }
        catch (Exception e)
        {
            failures++;
            Console.WriteLine($"FAIL {name}: {e.Message}");
        }
    }

    private static void Require(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    public static async Task<int> Run()
    {
        failures = 0;
        Console.WriteLine($"{AppInfo.DisplayName} {AppInfo.Version} self test ({Releases.Arch})");
        await Step("branding", () =>
        {
            Require(AppInfo.DisplayName.Length > 0 && AppInfo.AppId.Contains('.'), "branding metadata missing");
            return Task.FromResult($"{AppInfo.AppId} {AppInfo.RepoSlug}");
        });
        await Step("resources", () =>
        {
            var s = L.Lookup("App_Tagline");
            Require(L.Available && s.Length > 0, "resources.pri could not be loaded");
            return Task.FromResult(s);
        });
        await Step("web assets", () =>
        {
            foreach (var f in new[] { "shell.html", "shell.js", "shell.css", "terminal.html", "terminal.js", "chat.html", "chat.js", "chat.css", "vendor/xterm.js", "vendor/xterm.css", "vendor/addon-fit.js", "vendor/marked.js" })
                Require(File.Exists(Path.Combine(WebViewEnvironment.WebRoot, f)), $"missing Web/{f}");
            return Task.FromResult(WebViewEnvironment.WebRoot);
        });
        await Step("icon", () =>
        {
            var ico = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico");
            Require(File.Exists(ico), "missing Assets/AppIcon.ico");
            return Task.FromResult("ok");
        });
        await Step("url parsing", () =>
        {
            Require(GitHubUrl.Parse("https://github.com/o/r/pull/1")?.Kind == RouteKind.PullRequest, "pull link");
            Require(GitHubUrl.Parse("o/r#5")?.Item?.Number == 5, "short link");
            Require(GitHubUrl.Parse($"{AppInfo.UrlScheme}://github.com/o/r/issues/2".Replace($"{AppInfo.UrlScheme}://", "https://"))?.Kind == RouteKind.Issue, "issue link");
            return Task.FromResult("ok");
        });
        await Step("diff + html", () =>
        {
            var file = new DiffFile(new PRFile { Filename = "a/b.cs", Status = "modified", Additions = 1, Deletions = 1, Patch = "@@ -1,2 +1,2 @@\n-x\n+y\n z" });
            var html = DiffHtml.Render(new[] { file }, new DiffRenderOptions { Commentable = true, ShowViewed = true });
            Require(html.Contains("data-c=") && html.Contains("a/b.cs"), "diff html");
            var pr = JsonSerializer.Deserialize<PullRequestDetail>("""{"id":"1","number":2,"title":"T","url":"u","state":"OPEN","body":"","bodyHTML":"<p>x</p>","createdAt":"2026-01-01T00:00:00Z","headRefName":"h","baseRefName":"b","headRefOid":"o","timelineItems":{"nodes":[{"__typename":"IssueComment","id":"c","body":"b","bodyHTML":"<p>hi</p>","createdAt":"2026-01-01T00:00:00Z"}]},"reviewThreads":{"nodes":[]}}""", Json.GraphQL)!;
            var conversation = ConversationHtml.Render(pr);
            Require(conversation.Contains("<p>hi</p>"), "conversation html");
            return Task.FromResult($"{html.Length + conversation.Length} chars");
        });
        await Step("versions", () =>
        {
            Require(Releases.Compare("2.10.0", "2.9.0") > 0 && Releases.Compare(AppInfo.Version, AppInfo.Version) == 0, "compare");
            return Task.FromResult(AppInfo.InstallerAssetName(Releases.Arch));
        });
        await Step("settings", () =>
        {
            var path = Path.Combine(Path.GetTempPath(), $"{AppInfo.ExeName}-selftest-{Guid.NewGuid():N}.json");
            var s = new Settings(path);
            s.Set("x", 42);
            Require(new Settings(path).GetInt("x", 0) == 42, "settings roundtrip");
            File.Delete(path);
            return Task.FromResult("ok");
        });
        await Step("conpty", async () =>
        {
            using var session = new TerminalSession();
            var output = new System.Text.StringBuilder();
            var exited = new TaskCompletionSource<int>();
            session.Exited += code => exited.TrySetResult(code);
            var cmd = Environment.GetEnvironmentVariable("ComSpec") ?? "cmd.exe";
            session.Start($"\"{cmd}\" /c echo conpty-ok", Environment.CurrentDirectory, new Dictionary<string, string?>(), 80, 24);
            var history = session.Attach(chunk =>
            {
                lock (output) output.Append(System.Text.Encoding.UTF8.GetString(chunk));
            });
            lock (output) output.Insert(0, System.Text.Encoding.UTF8.GetString(history));
            var finished = await Task.WhenAny(exited.Task, Task.Delay(15000));
            await Task.Delay(300);
            string text;
            lock (output) text = output.ToString();
            Require(finished == exited.Task, "process did not exit");
            Require(text.Contains("conpty-ok"), "no output: " + text.Replace("\u001b", "ESC"));
            return $"exit {exited.Task.Result}";
        });
        await Step("webview2 runtime", () => Task.FromResult(WebViewEnvironment.AvailableVersion() ?? "not installed (HTML views need the Edge WebView2 Runtime)"));
        await Step("claude cli", () => Task.FromResult(ClaudeLauncher.Command?.File ?? "not installed"));

        var token = AppInfo.Env("TOKEN");
        if (token.Length > 0) await Online(new GitHubApi(token));

        Console.WriteLine(failures == 0 ? "PASS" : $"FAILED: {failures}");
        return failures == 0 ? 0 : 1;
    }

    private static async Task Online(GitHubApi api)
    {
        ItemRef? samplePR = null;
        ItemRef? sampleIssue = null;
        await Step("user", async () =>
        {
            var (u, s) = await api.CurrentUser();
            return $"{u.Login} scopes={string.Join(",", s)}";
        });
        await Step("badges", async () =>
        {
            var b = await api.Badges();
            return $"rr={b.Rr.IssueCount} mine={b.Mine.IssueCount}";
        });
        foreach (var q in new[] { "is:pr author:@me is:closed archived:false sort:updated-desc", "is:pr review-requested:@me archived:false sort:updated-desc", "is:pr author:@me sort:updated-desc" })
        {
            await Step($"search {q}", async () =>
            {
                var r = await api.Search(q);
                samplePR = r.Nodes.FirstOrDefault()?.Ref ?? samplePR;
                return $"count={r.IssueCount} first={r.Nodes.FirstOrDefault()?.Ref.Title ?? "-"}";
            });
        }
        await Step("search issues", async () =>
        {
            var r = await api.Search("is:issue involves:@me sort:updated-desc");
            sampleIssue = r.Nodes.FirstOrDefault()?.Ref;
            return $"count={r.IssueCount}";
        });
        if (GitHubUrl.Parse(AppInfo.Env("PR")) is { Kind: RouteKind.PullRequest } forced) samplePR = forced.Item;
        if (samplePR is { } pr)
        {
            await Step($"pr {pr.Title}", async () =>
            {
                var r = await api.PullRequest(pr);
                if (r.Repository?.PullRequest is not { } p) return "not a PR";
                var html = ConversationHtml.Render(p);
                var unresolved = p.ReviewThreads.Items.Where(t => !t.IsResolved).ToList();
                var composed = FixPrompt.Build(p, pr.Repo, unresolved, FixPrompt.GeneralFeedback(p), new FixOptions());
                return $"state={p.State} timeline={p.TimelineItems.Items.Count} threads={p.ReviewThreads.Items.Count} unresolved={unresolved.Count} html={html.Length} composed={composed.Length}ch";
            });
            await Step("files", async () =>
            {
                var f = await api.PullRequestFiles(pr);
                return $"files={f.Count} lines={f.Select(x => new DiffFile(x)).Sum(x => x.Lines.Count)}";
            });
            await Step($"repo PRs {pr.Repo.Full}", async () =>
            {
                var open = await api.RepoPullRequests(pr.Repo, states: new[] { "OPEN" });
                var stack = await api.PullRequestStack(pr);
                return $"open={open.Count} stack={stack.Count}";
            });
        }
        if (sampleIssue is { } issue)
        {
            await Step($"issue {issue.Title}", async () =>
            {
                var r = await api.Issue(issue);
                return $"type={r.Repository?.IssueOrPullRequest?.Typename ?? "-"} timeline={r.Repository?.IssueOrPullRequest?.TimelineItems.Items.Count ?? -1}";
            });
        }
        await Step("repos", async () =>
        {
            var r = await api.Repos(null);
            return $"total={r.TotalCount} first={r.Nodes.FirstOrDefault()?.NameWithOwner ?? "-"}";
        });
        await Step("projects", async () =>
        {
            var p = await api.Projects();
            if (p.FirstOrDefault() is { } first)
            {
                var (d, items) = await api.Project(first.Id);
                return $"count={p.Count} first={d.Title} fields={d.Fields.Items.Count(f => f.Options is not null)} items={items.Count}";
            }
            return "count=0";
        });
        await Step("notifications", async () => $"unread={(await api.Notifications(false)).Count}");
    }
}
