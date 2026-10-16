namespace PullYard.Core.Api;

public static class PullRequestSignature
{
    public static string Of(PullRequestDetail pr)
    {
        var commit = pr.LastCommit.Items.FirstOrDefault()?.Commit;
        var checks = (commit?.StatusCheckRollup?.Contexts?.Items ?? new())
            .Select(c => $"{c.Typename}:{c.Status}:{c.Conclusion}:{c.State}")
            .OrderBy(s => s, StringComparer.Ordinal);
        var threads = pr.ReviewThreads.Items;
        return string.Join("|",
            pr.UpdatedAt?.ToUniversalTime().ToString("O"),
            pr.HeadRefOid,
            commit?.Oid,
            pr.State,
            pr.IsDraft,
            pr.Mergeable,
            pr.MergeStateStatus,
            pr.ReviewDecision,
            pr.Comments?.TotalCount,
            pr.Reviews?.TotalCount,
            pr.TimelineItems.TotalCount,
            pr.AllCommits.TotalCount,
            $"{threads.Count}/{threads.Count(t => t.IsResolved)}",
            commit?.StatusCheckRollup?.State,
            string.Join(",", checks));
    }
}
