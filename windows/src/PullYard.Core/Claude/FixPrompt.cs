using System.Text;
using PullYard.Core.Api;

namespace PullYard.Core.Claude;

public sealed class FixOptions
{
    public bool IncludeGeneral { get; set; } = true;
    public bool Commit { get; set; } = true;
    public bool Push { get; set; }
    public bool ReplyAndResolve { get; set; }
}

public sealed record GeneralFeedback(string Id, string Author, string Body);

public sealed record FoundFixPrompt(string Text, string Author, DateTimeOffset? Date, string? Url);

public static class FixPrompt
{
    public static List<GeneralFeedback> GeneralFeedback(PullRequestDetail pr) =>
        pr.TimelineItems.Items
            .Where(i => i.Typename == "PullRequestReview" && i.State == "CHANGES_REQUESTED" && !string.IsNullOrWhiteSpace(i.Body))
            .Select(i => new GeneralFeedback(i.Id ?? Guid.NewGuid().ToString(), i.Author?.Login ?? "ghost", i.Body!.Trim()))
            .ToList();

    private static string Quote(string body) => string.Join("\n", body.Replace("\r\n", "\n").Split('\n').Select(l => "> " + l));

    public static string Build(PullRequestDetail pr, RepoRef repo, IReadOnlyList<ReviewThread> threads, IReadOnlyList<GeneralFeedback> general, FixOptions options)
    {
        var sb = new StringBuilder();
        sb.Append($"Address the code review feedback on pull request #{pr.Number} \"{pr.Title}\" in {repo.Full}.\n");
        sb.Append($"PR: {pr.Url}\n");
        sb.Append($"Branch: {pr.HeadRefName} → {pr.BaseRefName}\n\n");
        sb.Append("Steps:\n");
        sb.Append($"1. Make sure the working tree is on the PR branch: run `gh pr checkout {pr.Number}` and `git pull`. If there are uncommitted local changes, stop and ask me first.\n");
        sb.Append("2. Go through every item below. Make the change the reviewer asked for. If a comment is unclear, already handled, or you disagree with it, do not change code for it — explain why in your summary.\n");
        sb.Append("3. Keep changes minimal and in the style of the surrounding code. Run the project's analyzer/linter and the tests related to the touched code.\n");
        var step = 4;
        if (options.Commit) sb.Append($"{step++}. Commit the fixes with a clear message.\n");
        if (options.Push) sb.Append($"{step++}. Push the branch (`git push`).\n");
        else sb.Append($"{step++}. Do not push — I will review the changes first.\n");
        if (options.ReplyAndResolve)
        {
            sb.Append($"{step++}. After pushing, for each fixed thread reply with a short note of what changed and resolve it:\n");
            sb.Append("   gh api graphql -f query='mutation($id:ID!,$b:String!){addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$id,body:$b}){clientMutationId}}' -f id=THREAD_ID -f b='Fixed: …'\n");
            sb.Append("   gh api graphql -f query='mutation($id:ID!){resolveReviewThread(input:{threadId:$id}){clientMutationId}}' -f id=THREAD_ID\n");
            sb.Append("   Leave threads you did not fix unresolved.\n");
        }
        sb.Append($"{step}. Finish with a short list: each item → fixed / skipped (and why).\n");
        if (threads.Count > 0)
        {
            sb.Append($"\n## Review comments in code ({threads.Count})\n");
            for (var i = 0; i < threads.Count; i++)
            {
                var th = threads[i];
                var line = th.Line ?? th.OriginalLine;
                sb.Append($"\n### {i + 1}. `{th.Path}{(line is { } l ? $":{l}" : "")}`");
                if (th.IsOutdated) sb.Append(" (outdated — code may have moved)");
                sb.Append($"\nThread ID: {th.Id}\n");
                if (th.Comments.Items.FirstOrDefault()?.DiffHunk is { } hunk)
                {
                    var tail = hunk.Replace("\r\n", "\n").Split('\n').Where(x => !x.StartsWith("@@")).ToList();
                    tail = tail.Skip(Math.Max(0, tail.Count - 8)).ToList();
                    sb.Append($"```diff\n{string.Join("\n", tail)}\n```\n");
                }
                foreach (var c in th.Comments.Items)
                    sb.Append($"**@{c.Author?.Login ?? "ghost"}**:\n{Quote(c.Body.Trim())}\n");
            }
        }
        if (options.IncludeGeneral && general.Count > 0)
        {
            sb.Append("\n## General review feedback (changes requested)\n");
            foreach (var g in general) sb.Append($"\n**@{g.Author}**:\n{Quote(g.Body)}\n");
        }
        return sb.ToString();
    }
}

public static class FixPromptExtractor
{
    public static string? Extract(string body)
    {
        var start = body.IndexOf("--- FIX PROMPT ---", StringComparison.OrdinalIgnoreCase);
        if (start >= 0)
        {
            var rest = body[(start + "--- FIX PROMPT ---".Length)..];
            var end = rest.IndexOf("--- END FIX PROMPT ---", StringComparison.OrdinalIgnoreCase);
            if (end >= 0) rest = rest[..end];
            else if (rest.IndexOf("```", StringComparison.Ordinal) is var fence and >= 0) rest = rest[..fence];
            var text = rest.Trim();
            return text.Length == 0 ? null : text;
        }
        foreach (var marker in new[] { "fix prompt", "prompt for ai agent" })
        {
            var m = body.IndexOf(marker, StringComparison.OrdinalIgnoreCase);
            if (m < 0) continue;
            var after = body[(m + marker.Length)..];
            var open = after.IndexOf("```", StringComparison.Ordinal);
            if (open < 0) continue;
            var afterOpen = after[(open + 3)..];
            var newline = afterOpen.IndexOf('\n');
            if (newline < 0) continue;
            var content = afterOpen[(newline + 1)..];
            var close = content.IndexOf("```", StringComparison.Ordinal);
            if (close < 0) continue;
            var text = content[..close].Trim();
            if (text.Length > 0) return text;
        }
        return null;
    }

    public static FoundFixPrompt? Latest(PullRequestDetail pr)
    {
        FoundFixPrompt? best = null;
        void Consider(string? body, string? author, DateTimeOffset? date, string? url)
        {
            if (body is null || Extract(body) is not { } text) return;
            if (best?.Date is { } bd && date is { } d && d < bd) return;
            best = new FoundFixPrompt(text, author ?? "ghost", date, url);
        }
        Consider(pr.Body, pr.Author?.Login, pr.CreatedAt, pr.Url);
        foreach (var item in pr.TimelineItems.Items.Where(i => i.Typename is "IssueComment" or "PullRequestReview"))
            Consider(item.Body, item.Author?.Login, item.CreatedAt, item.Url);
        return best;
    }

    public static string Wrap(FoundFixPrompt found, PullRequestDetail pr, FixOptions options)
    {
        var sb = new StringBuilder();
        sb.Append($"Before starting: make sure the working tree is on the PR branch — run `gh pr checkout {pr.Number}` and `git pull`. If there are uncommitted local changes, stop and ask me first.\n\n");
        sb.Append(found.Text).Append('\n');
        var extra = new List<string>();
        if (options.Commit) extra.Add("Commit the fixes with a clear message.");
        extra.Add(options.Push ? "Push the branch when done (`git push`)." : "Do not push — I will review the changes first.");
        if (options.ReplyAndResolve)
            extra.Add("After pushing, reply on each review thread you fixed with a short note and resolve it via `gh api graphql` (addPullRequestReviewThreadReply / resolveReviewThread). Leave unfixed threads unresolved.");
        sb.Append("\nAdditional instructions:\n").Append(string.Join("\n", extra.Select(e => "- " + e))).Append('\n');
        return sb.ToString();
    }
}
