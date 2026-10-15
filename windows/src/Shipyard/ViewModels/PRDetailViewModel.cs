using CommunityToolkit.Mvvm.ComponentModel;
using Shipyard.Core.Api;
using Shipyard.Core.Html;

namespace Shipyard.ViewModels;

public enum PRTab { Conversation, Commits, Checks, Files }

public sealed record ReviewerInfo(string Login, string? Avatar, string State);

public sealed record StaleReview(string Login, int? Commits);

public sealed partial class PRDetailViewModel : ObservableObject
{
    public ItemRef Ref { get; }
    public GitHubApi Api { get; }

    [ObservableProperty] private PRRepository? repo;
    [ObservableProperty] private PullRequestDetail? pr;
    [ObservableProperty] private string viewerLogin = "";
    [ObservableProperty] private bool notPR;
    [ObservableProperty] private bool loading;
    [ObservableProperty] private DateTimeOffset? lastUpdated;
    [ObservableProperty] private string? refreshError;
    [ObservableProperty] private string? error;
    [ObservableProperty] private string? actionError;
    [ObservableProperty] private bool busy;
    [ObservableProperty] private PRTab tab = PRTab.Conversation;
    [ObservableProperty] private List<StackPR> stack = new();
    [ObservableProperty] private string html = "";
    [ObservableProperty] private int scrollToBottomToken;
    [ObservableProperty] private List<DiffFile> files = new();
    [ObservableProperty] private bool filesLoaded;
    [ObservableProperty] private bool filesLoading;
    [ObservableProperty] private string? filesError;
    [ObservableProperty] private string? scrollTarget;
    [ObservableProperty] private int diffVersion;

    public HashSet<string> ViewedPaths { get; private set; } = new();
    public HashSet<string> Collapsed { get; private set; } = new();
    public List<PendingComment> Pending { get; } = new();

    private Task? activeLoad;
    private Task? queuedLoad;
    private bool reloadFiles;
    private string? filesOid;
    private CancellationTokenSource? mergeRecheck;
    private int mergeRechecks;
    private Dictionary<string, List<ReviewThread>> threadIndex = new();
    private Dictionary<string, List<ReviewThread>> outdatedByPath = new();

    public PRDetailViewModel(ItemRef r, GitHubApi api)
    {
        Ref = r;
        Api = api;
    }

    public bool CanWrite => Repo?.CanWrite ?? false;

    public int PendingCount => Pending.Count;

    public List<ReviewerInfo> Reviewers
    {
        get
        {
            if (Pr is not { } pr) return new();
            var result = pr.LatestOpinionatedReviews.Items.Where(r => r.Author is not null).Select(r => new ReviewerInfo(r.Author!.Login, r.Author.AvatarUrl, r.State)).ToList();
            var done = result.Select(r => r.Login).ToHashSet();
            foreach (var req in pr.ReviewRequests.Items)
            {
                if (req.RequestedReviewer is not { } rv || done.Contains(rv.Display)) continue;
                result.Add(new ReviewerInfo(rv.Display, rv.AvatarUrl, "PENDING"));
            }
            return result;
        }
    }

    public DateTimeOffset? LastCommitDate => Pr?.LastCommit.Items.FirstOrDefault()?.Commit.CommittedDate;

    public List<StaleReview> StaleReviews
    {
        get
        {
            if (Pr is not { IsOpen: true } pr) return new();
            var pending = pr.ReviewRequests.Items.Select(r => r.RequestedReviewer?.Display).Where(d => d is not null).ToHashSet();
            var oids = pr.AllCommits.Items.Select(c => c.Commit.Oid).ToList();
            var result = new List<StaleReview>();
            foreach (var r in pr.LatestOpinionatedReviews.Items)
            {
                if (r.Author?.Login is not { } login || login == ViewerLogin || login.EndsWith("[bot]") || pending.Contains(login)) continue;
                if (r.Commit?.Oid is not { } oid || oid == pr.HeadRefOid) continue;
                var idx = oids.LastIndexOf(oid);
                int? after = idx >= 0 ? oids.Count - 1 - idx : null;
                result.Add(new StaleReview(login, after is > 0 ? after : null));
            }
            return result;
        }
    }

    public List<string> ReviewersToReRequest => StaleReviews.Select(s => s.Login).ToList();

    public Task Load(bool full = false)
    {
        if (full) reloadFiles = true;
        if (queuedLoad is not null) return queuedLoad;
        if (activeLoad is not null)
        {
            var current = activeLoad;
            queuedLoad = Chain(current);
            return queuedLoad;
        }
        return StartLoad();
    }

    private async Task Chain(Task current)
    {
        try
        {
            await current;
        }
        catch
        {
        }
        await StartLoad();
    }

    private async Task StartLoad()
    {
        queuedLoad = null;
        var task = Fetch();
        activeLoad = task;
        try
        {
            await task;
        }
        finally
        {
            if (activeLoad == task) activeLoad = null;
        }
    }

    private async Task Fetch()
    {
        Loading = true;
        try
        {
            var r = await Api.PullRequest(Ref);
            ViewerLogin = r.Viewer.Login;
            if (r.Repository?.PullRequest is not { } pr)
            {
                NotPR = true;
                return;
            }
            Repo = r.Repository;
            var newHtml = ConversationHtml.Render(pr);
            ViewedPaths = (pr.Files?.Items ?? new()).Where(f => f.ViewerViewedState == "VIEWED").Select(f => f.Path).ToHashSet();
            IndexThreads(pr.ReviewThreads.Items);
            Pr = pr;
            if (newHtml != Html) Html = newHtml;
            Error = null;
            RefreshError = null;
            LastUpdated = DateTimeOffset.Now;
            NotifyDerived();
            _ = LoadStack();
            ScheduleMergeRecheck(pr);
            var filesStale = reloadFiles || (filesOid is not null && filesOid != pr.HeadRefOid);
            reloadFiles = false;
            if (FilesLoaded && filesStale) _ = LoadFiles(true);
            else if (!FilesLoaded && !FilesLoading) _ = LoadFiles();
            else DiffVersion++;
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception e)
        {
            if (Pr is null) Error = e.Message;
            else RefreshError = e.Message;
        }
        finally
        {
            Loading = false;
        }
    }

    private void NotifyDerived()
    {
        OnPropertyChanged(nameof(CanWrite));
        OnPropertyChanged(nameof(Reviewers));
        OnPropertyChanged(nameof(StaleReviews));
        OnPropertyChanged(nameof(LastCommitDate));
    }

    private void ScheduleMergeRecheck(PullRequestDetail pr)
    {
        var unknown = pr.IsOpen && (pr.MergeStateStatus == "UNKNOWN" || pr.Mergeable == "UNKNOWN");
        if (!unknown)
        {
            mergeRechecks = 0;
            return;
        }
        if (mergeRechecks >= 5) return;
        mergeRechecks++;
        var delay = TimeSpan.FromSeconds(mergeRechecks * 2);
        mergeRecheck?.Cancel();
        var cts = new CancellationTokenSource();
        mergeRecheck = cts;
        _ = Recheck(delay, cts.Token);
    }

    private async Task Recheck(TimeSpan delay, CancellationToken ct)
    {
        try
        {
            await Task.Delay(delay, ct);
        }
        catch (OperationCanceledException)
        {
            return;
        }
        await Load();
    }

    public async Task LoadStack()
    {
        try
        {
            var s = await Api.PullRequestStack(Ref);
            if (s.Select(x => x.Number).SequenceEqual(Stack.Select(x => x.Number)) && s.Select(x => x.State).SequenceEqual(Stack.Select(x => x.State))) return;
            Stack = s;
        }
        catch
        {
        }
    }

    public async Task LoadFiles(bool force = false)
    {
        if (!force && (FilesLoaded || FilesLoading)) return;
        FilesLoading = true;
        var oid = Pr?.HeadRefOid;
        try
        {
            var raw = await Api.PullRequestFiles(Ref);
            var parsed = await Task.Run(() => raw.Select(f => new DiffFile(f)).ToList());
            if (!FilesLoaded) Collapsed = parsed.Where(f => ViewedPaths.Contains(f.Id) || f.Lines.Count > 1500).Select(f => f.Id).ToHashSet();
            Files = parsed;
            filesOid = oid;
            FilesLoaded = true;
            FilesError = null;
            DiffVersion++;
        }
        catch (OperationCanceledException)
        {
        }
        catch (Exception e)
        {
            FilesError = e.Message;
        }
        finally
        {
            FilesLoading = false;
        }
    }

    private void IndexThreads(IEnumerable<ReviewThread> threads)
    {
        var idx = new Dictionary<string, List<ReviewThread>>();
        var outdated = new Dictionary<string, List<ReviewThread>>();
        foreach (var t in threads)
        {
            if (t.Line is { } line)
            {
                var key = $"{t.Path}|{t.DiffSide}|{line}";
                if (!idx.TryGetValue(key, out var l)) idx[key] = l = new();
                l.Add(t);
            }
            else
            {
                if (!outdated.TryGetValue(t.Path, out var l)) outdated[t.Path] = l = new();
                l.Add(t);
            }
        }
        threadIndex = idx;
        outdatedByPath = outdated;
    }

    public DiffRenderOptions RenderOptions(string filter) => new()
    {
        Commentable = Pr?.IsOpen ?? false,
        ShowViewed = true,
        Viewed = ViewedPaths,
        Collapsed = Collapsed,
        Threads = threadIndex,
        Outdated = outdatedByPath,
        Pending = Pending,
        Filter = filter
    };

    public int ThreadCount(string path) => (Pr?.ReviewThreads.Items ?? new()).Count(t => t.Path == path);

    public void RevealFile(string path)
    {
        Tab = PRTab.Files;
        if (Collapsed.Remove(path)) DiffVersion++;
        ScrollTarget = null;
        ScrollTarget = path;
    }

    private async Task<bool> Run(Func<Task> op, bool reload = true)
    {
        Busy = true;
        try
        {
            await op();
            if (reload) await Load();
            return true;
        }
        catch (Exception e)
        {
            ActionError = e.Message;
            return false;
        }
        finally
        {
            Busy = false;
        }
    }

    public async Task<bool> Comment(string body)
    {
        if (Pr is not { } pr) return false;
        var ok = await Run(() => Api.AddComment(pr.Id, body));
        if (ok) ScrollToBottomToken++;
        return ok;
    }

    public async Task<bool> SubmitReview(string @event, string body)
    {
        if (Pr is not { } pr) return false;
        var comments = Pending.ToList();
        var ok = await Run(() => Api.AddReview(pr.Id, @event, body, comments));
        if (ok)
        {
            Pending.Clear();
            OnPropertyChanged(nameof(PendingCount));
            DiffVersion++;
            ScrollToBottomToken++;
        }
        return ok;
    }

    public async Task<bool> AddInline(DiffAnchor anchor, string body, bool immediately)
    {
        var c = new PendingComment { Path = anchor.Path, Line = anchor.Line, Side = anchor.Side, Body = body };
        if (immediately)
        {
            if (Pr is not { } pr) return false;
            return await Run(() => Api.AddReview(pr.Id, "COMMENT", null, new[] { c }));
        }
        Pending.Add(c);
        OnPropertyChanged(nameof(PendingCount));
        DiffVersion++;
        return true;
    }

    public void RemovePending(Guid id)
    {
        if (Pending.RemoveAll(p => p.Id == id) > 0)
        {
            OnPropertyChanged(nameof(PendingCount));
            DiffVersion++;
        }
    }

    public void ClearPending()
    {
        Pending.Clear();
        OnPropertyChanged(nameof(PendingCount));
        DiffVersion++;
    }

    public Task<bool> Reply(string threadId, string body) => Run(() => Api.Mutate(Q.ReplyThread, GitHubApiVars.Of(("id", threadId), ("b", body))));

    public Task<bool> SetResolved(string threadId, bool resolved) =>
        Run(() => Api.Mutate(resolved ? Q.ResolveThread : Q.UnresolveThread, GitHubApiVars.Of(("id", threadId))));

    public Task<bool> Merge(string method, string? title, string? body, bool deleteBranch)
    {
        if (Pr is not { } pr) return Task.FromResult(false);
        return Run(async () =>
        {
            await Api.Mutate(Q.Merge, GitHubApiVars.Of(("id", pr.Id), ("m", method), ("t", title), ("b", body)));
            if (deleteBranch && pr.HeadRepository?.NameWithOwner is { } head)
            {
                try
                {
                    await Api.DeleteBranch(head, pr.HeadRefName);
                }
                catch
                {
                }
            }
        });
    }

    public Task<bool> EnableAutoMerge(string method) =>
        Pr is { } pr ? Run(() => Api.Mutate(Q.EnableAutoMerge, GitHubApiVars.Of(("id", pr.Id), ("m", method)))) : Task.FromResult(false);

    public Task<bool> Simple(string mutation) =>
        Pr is { } pr ? Run(() => Api.Mutate(mutation, GitHubApiVars.Of(("id", pr.Id)))) : Task.FromResult(false);

    public Task<bool> Edit(string title, string body) =>
        Pr is { } pr ? Run(() => Api.Mutate(Q.EditPR, GitHubApiVars.Of(("id", pr.Id), ("t", title), ("b", body)))) : Task.FromResult(false);

    public Task<bool> RequestReviewers(List<string> logins) => Run(() => Api.RequestReviewers(Ref, logins));

    public Task<bool> DeleteBranch() =>
        Pr is { HeadRepository.NameWithOwner: { } head } pr ? Run(() => Api.DeleteBranch(head, pr.HeadRefName)) : Task.FromResult(false);

    public async Task ToggleViewed(string path)
    {
        if (Pr is not { } pr) return;
        var viewed = ViewedPaths.Contains(path);
        if (viewed)
        {
            ViewedPaths.Remove(path);
        }
        else
        {
            ViewedPaths.Add(path);
            Collapsed.Add(path);
        }
        DiffVersion++;
        await Run(() => Api.Mutate(viewed ? Q.UnmarkViewed : Q.MarkViewed, GitHubApiVars.Of(("id", pr.Id), ("p", path))), reload: false);
    }

    public Task<bool> RerunFailed()
    {
        var runs = (Pr?.Checks ?? new()).Where(c => c.Outcome == CheckOutcome.Failure && c.RunId is not null).Select(c => c.RunId!.Value).Distinct().ToList();
        return Run(async () =>
        {
            foreach (var id in runs) await Api.RerunFailedJobs(Ref.Repo, id);
        });
    }
}

public static class GitHubApiVars
{
    public static Dictionary<string, object?> Of(params (string, object?)[] pairs) => pairs.ToDictionary(p => p.Item1, p => p.Item2);
}
