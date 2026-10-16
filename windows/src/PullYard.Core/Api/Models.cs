using System.Text.Json.Serialization;
namespace PullYard.Core.Api;

public sealed class Nodes<T> where T : class
{
    [JsonIgnore]
    public List<T> Items { get; set; } = new();
    public int? TotalCount { get; set; }

    [JsonPropertyName("nodes")]
    public List<T> RawNodes { get => Items; set => Items = value ?? new(); }
}

public sealed class PageInfo
{
    public bool HasNextPage { get; set; }
    public string? EndCursor { get; set; }
}

public sealed class Count
{
    public int TotalCount { get; set; }
}

public sealed class IdOnly
{
    public string Id { get; set; } = "";
}

public sealed class UserLogin
{
    public string Login { get; set; } = "";
}

public sealed class Actor
{
    public string Login { get; set; } = "";
    public string? AvatarUrl { get; set; }
}

public sealed class LabelInfo
{
    public string Name { get; set; } = "";
    public string Color { get; set; } = "888888";
}

public sealed class BranchName
{
    public string Name { get; set; } = "";
}

public sealed class RepoName
{
    public string NameWithOwner { get; set; } = "";
    public BranchName? DefaultBranchRef { get; set; }
}

public sealed class StackPR
{
    public int Number { get; set; }
    public string Title { get; set; } = "";
    public string State { get; set; } = "OPEN";
    public bool IsDraft { get; set; }
    public string HeadRefName { get; set; } = "";
    public string BaseRefName { get; set; } = "";
    public VisualState VisualState => VisualStates.Pr(State, IsDraft);
}

public sealed class Viewer
{
    public string Login { get; set; } = "";
}

public sealed class RestUser
{
    public string Login { get; set; } = "";
    public string? Name { get; set; }
    public string? AvatarUrl { get; set; }
}

public sealed class RollupState
{
    public string State { get; set; } = "";
}

public sealed class RollupCommit
{
    public RollupState? StatusCheckRollup { get; set; }
}

public sealed class RollupCommitNode
{
    public RollupCommit Commit { get; set; } = new();
}

public sealed class SearchItem
{
    [JsonPropertyName("__typename")]
    public string Typename { get; set; } = "";
    public int Number { get; set; }
    public string Title { get; set; } = "";
    public string Url { get; set; } = "";
    public string? PrState { get; set; }
    public string? IssueState { get; set; }
    public string? StateReason { get; set; }
    public bool? IsDraft { get; set; }
    public DateTimeOffset CreatedAt { get; set; }
    public DateTimeOffset UpdatedAt { get; set; }
    public string? BaseRefName { get; set; }
    public Actor? Author { get; set; }
    public RepoName Repository { get; set; } = new();
    public string? ReviewDecision { get; set; }
    public int? Additions { get; set; }
    public int? Deletions { get; set; }
    public Count Comments { get; set; } = new();
    public Nodes<LabelInfo>? Labels { get; set; }
    public Nodes<RollupCommitNode>? Commits { get; set; }

    public string Id => Url;
    public bool IsPR => Typename == "PullRequest";
    public RepoRef Repo => RepoRef.Parse(Repository.NameWithOwner) ?? new RepoRef("?", "?");
    public ItemRef Ref => new(Repo, Number);
    public Route Route => IsPR ? Route.Pr(Ref) : Route.Issue(Ref);
    public string? CiState => Commits?.Items.FirstOrDefault()?.Commit.StatusCheckRollup?.State;

    public string? StackedOn
    {
        get
        {
            if (!IsPR || BaseRefName is null || Repository.DefaultBranchRef?.Name is not { } main || BaseRefName == main) return null;
            return BaseRefName;
        }
    }

    public VisualState VisualState => IsPR ? VisualStates.Pr(PrState ?? "OPEN", IsDraft ?? false) : VisualStates.Issue(IssueState ?? "OPEN", StateReason);
}

public sealed class SearchConnection
{
    public int IssueCount { get; set; }
    public PageInfo? PageInfo { get; set; }
    public List<SearchItem> Nodes { get; set; } = new();
}

public sealed class SearchResponse
{
    public SearchConnection Search { get; set; } = new();
}

public sealed class BadgeResponse
{
    public SearchConnection Rr { get; set; } = new();
    public SearchConnection Mine { get; set; } = new();
}

public sealed class Reviewer
{
    [JsonPropertyName("__typename")]
    public string? Typename { get; set; }
    public string? Login { get; set; }
    public string? AvatarUrl { get; set; }
    public string? Name { get; set; }
    public string Display => Login ?? Name ?? "?";
}

public sealed class ReviewRequest
{
    public Reviewer? RequestedReviewer { get; set; }
}

public sealed class ReviewCommit
{
    public string Oid { get; set; } = "";
}

public sealed class LatestReview
{
    public string State { get; set; } = "";
    public DateTimeOffset? SubmittedAt { get; set; }
    public ReviewCommit? Commit { get; set; }
    public Actor? Author { get; set; }
}

public sealed class WorkflowInfo
{
    public string Name { get; set; } = "";
}

public sealed class WorkflowRun
{
    public long? DatabaseId { get; set; }
    public WorkflowInfo? Workflow { get; set; }
}

public sealed class CheckSuite
{
    public WorkflowRun? WorkflowRun { get; set; }
}

public enum CheckOutcome { Failure = 0, Pending, Neutral, Skipped, Success }

public sealed class CheckContext
{
    [JsonPropertyName("__typename")]
    public string Typename { get; set; } = "";
    public long? DatabaseId { get; set; }
    public string? Name { get; set; }
    public string? Status { get; set; }
    public string? Conclusion { get; set; }
    public string? DetailsUrl { get; set; }
    public DateTimeOffset? StartedAt { get; set; }
    public DateTimeOffset? CompletedAt { get; set; }
    public CheckSuite? CheckSuite { get; set; }
    public string? Context { get; set; }
    public string? State { get; set; }
    public string? TargetUrl { get; set; }
    public string? Description { get; set; }

    public string Id => $"{Typename}-{(DatabaseId?.ToString() ?? Context ?? Name ?? "?")}";
    public string Title => Name ?? Context ?? "?";
    public string? WorkflowName => CheckSuite?.WorkflowRun?.Workflow?.Name;
    public long? RunId => CheckSuite?.WorkflowRun?.DatabaseId;
    public string? Link => DetailsUrl ?? TargetUrl;
    public bool IsActionsJob => Typename == "CheckRun" && RunId is not null && DatabaseId is not null;
    public string FullTitle => WorkflowName is { } w ? $"{w} / {Title}" : Title;

    public CheckOutcome Outcome
    {
        get
        {
            if (Typename == "StatusContext")
            {
                return State switch
                {
                    "SUCCESS" => CheckOutcome.Success,
                    "FAILURE" or "ERROR" => CheckOutcome.Failure,
                    _ => CheckOutcome.Pending
                };
            }
            if (Status != "COMPLETED") return CheckOutcome.Pending;
            return Conclusion switch
            {
                "SUCCESS" => CheckOutcome.Success,
                "FAILURE" or "TIMED_OUT" or "ACTION_REQUIRED" or "STARTUP_FAILURE" => CheckOutcome.Failure,
                "SKIPPED" => CheckOutcome.Skipped,
                _ => CheckOutcome.Neutral
            };
        }
    }

    public string? Duration
    {
        get
        {
            if (StartedAt is not { } s || CompletedAt is not { } e) return null;
            var secs = (int)(e - s).TotalSeconds;
            return secs >= 60 ? $"{secs / 60}m {secs % 60}s" : $"{secs}s";
        }
    }
}

public sealed class StatusRollup
{
    public string State { get; set; } = "";
    public Nodes<CheckContext>? Contexts { get; set; }
}

public sealed class LastCommit
{
    public string Oid { get; set; } = "";
    public DateTimeOffset? CommittedDate { get; set; }
    public StatusRollup? StatusCheckRollup { get; set; }
}

public sealed class LastCommitNode
{
    public LastCommit Commit { get; set; } = new();
}

public sealed class GitActor
{
    public string? Name { get; set; }
    public string? AvatarUrl { get; set; }
    public UserLogin? User { get; set; }
}

public sealed class CommitInfo
{
    public string Oid { get; set; } = "";
    public string AbbreviatedOid { get; set; } = "";
    public string MessageHeadline { get; set; } = "";
    public DateTimeOffset CommittedDate { get; set; }
    public string Url { get; set; } = "";
    public GitActor? Author { get; set; }
    public RollupState? StatusCheckRollup { get; set; }
}

public sealed class CommitInfoNode
{
    public CommitInfo Commit { get; set; } = new();
}

public sealed class FileViewed
{
    public string Path { get; set; } = "";
    public string ViewerViewedState { get; set; } = "";
}

public sealed class XRefSource
{
    [JsonPropertyName("__typename")]
    public string? Typename { get; set; }
    public int? Number { get; set; }
    public string? Title { get; set; }
    public string? Url { get; set; }
    public RepoName? Repository { get; set; }
}

public sealed class TimelineCommit
{
    public string? Oid { get; set; }
    public string? AbbreviatedOid { get; set; }
    public string? MessageHeadline { get; set; }
    public DateTimeOffset? CommittedDate { get; set; }
    public GitActor? Author { get; set; }
}

public sealed class TimelineItem
{
    [JsonPropertyName("__typename")]
    public string Typename { get; set; } = "";
    public string? Id { get; set; }
    public Actor? Author { get; set; }
    public Actor? Actor { get; set; }
    public string? Body { get; set; }
    public string? BodyHTML { get; set; }
    public DateTimeOffset? CreatedAt { get; set; }
    public string? Url { get; set; }
    public string? State { get; set; }
    public LabelInfo? Label { get; set; }
    public XRefSource? Source { get; set; }
    public string? PreviousTitle { get; set; }
    public string? CurrentTitle { get; set; }
    public TimelineCommit? Commit { get; set; }
    public string? MergeRefName { get; set; }
    public Reviewer? RequestedReviewer { get; set; }
}

public sealed class ThreadComment
{
    public string Id { get; set; } = "";
    public Actor? Author { get; set; }
    public string Body { get; set; } = "";
    public string BodyHTML { get; set; } = "";
    public DateTimeOffset CreatedAt { get; set; }
    public string? DiffHunk { get; set; }
    public IdOnly? PullRequestReview { get; set; }
}

public sealed class ReviewThread
{
    public string Id { get; set; } = "";
    public bool IsResolved { get; set; }
    public bool IsOutdated { get; set; }
    public string Path { get; set; } = "";
    public int? Line { get; set; }
    public int? OriginalLine { get; set; }
    public string DiffSide { get; set; } = "RIGHT";
    public bool ViewerCanResolve { get; set; }
    public bool ViewerCanUnresolve { get; set; }
    public bool ViewerCanReply { get; set; }
    public Nodes<ThreadComment> Comments { get; set; } = new();
    public Nodes<EditMark>? EditedComments { get; set; }

    public string Location => (Line ?? OriginalLine) is { } l ? $"{Path}:{l}" : Path;
}

public sealed class EditMark
{
    public DateTimeOffset? LastEditedAt { get; set; }
}

public sealed class AutoMergeRequest
{
    public DateTimeOffset? EnabledAt { get; set; }
    public string? MergeMethod { get; set; }
}

public sealed class PullRequestDetail
{
    public string Id { get; set; } = "";
    public int Number { get; set; }
    public string Title { get; set; } = "";
    public string Url { get; set; } = "";
    public string State { get; set; } = "OPEN";
    public bool IsDraft { get; set; }
    public bool Merged { get; set; }
    public string Mergeable { get; set; } = "UNKNOWN";
    public string? MergeStateStatus { get; set; }
    public string Body { get; set; } = "";
    public string BodyHTML { get; set; } = "";
    public DateTimeOffset CreatedAt { get; set; }
    public DateTimeOffset? UpdatedAt { get; set; }
    public Count? Comments { get; set; }
    public Count? Reviews { get; set; }
    public Nodes<EditMark>? EditedComments { get; set; }
    public Nodes<EditMark>? EditedReviews { get; set; }
    public Actor? Author { get; set; }
    public string HeadRefName { get; set; } = "";
    public string BaseRefName { get; set; } = "";
    public string HeadRefOid { get; set; } = "";
    public RepoName? HeadRepository { get; set; }
    public BranchName? HeadRef { get; set; }
    public int Additions { get; set; }
    public int Deletions { get; set; }
    public int ChangedFiles { get; set; }
    public string? ReviewDecision { get; set; }
    public bool ViewerCanUpdate { get; set; }
    public bool ViewerDidAuthor { get; set; }
    public AutoMergeRequest? AutoMergeRequest { get; set; }
    public Nodes<LabelInfo> Labels { get; set; } = new();
    public Nodes<Actor> Assignees { get; set; } = new();
    public Nodes<ReviewRequest> ReviewRequests { get; set; } = new();
    public Nodes<LatestReview> LatestOpinionatedReviews { get; set; } = new();
    public Nodes<LastCommitNode> LastCommit { get; set; } = new();
    public Nodes<CommitInfoNode> AllCommits { get; set; } = new();
    public Nodes<FileViewed>? Files { get; set; }
    public Nodes<TimelineItem> TimelineItems { get; set; } = new();
    public Nodes<ReviewThread> ReviewThreads { get; set; } = new();

    public VisualState VisualState => VisualStates.Pr(State, IsDraft);
    public StatusRollup? Rollup => LastCommit.Items.FirstOrDefault()?.Commit.StatusCheckRollup;
    public List<CheckContext> Checks => Rollup?.Contexts?.Items ?? new();
    public bool IsOpen => State == "OPEN";
}

public sealed class PRRepository
{
    public string? ViewerPermission { get; set; }
    public bool MergeCommitAllowed { get; set; }
    public bool SquashMergeAllowed { get; set; }
    public bool RebaseMergeAllowed { get; set; }
    public bool DeleteBranchOnMerge { get; set; }
    public bool AutoMergeAllowed { get; set; }
    public string ViewerDefaultMergeMethod { get; set; } = "MERGE";
    public PullRequestDetail? PullRequest { get; set; }

    public bool CanWrite => ViewerPermission is "ADMIN" or "MAINTAIN" or "WRITE";
}

public sealed class PRDetailResponse
{
    public Viewer Viewer { get; set; } = new();
    public PRRepository? Repository { get; set; }
}

public sealed class IssueDetail
{
    [JsonPropertyName("__typename")]
    public string Typename { get; set; } = "";
    public string Id { get; set; } = "";
    public int Number { get; set; }
    public string Title { get; set; } = "";
    public string Url { get; set; } = "";
    public string IssueState { get; set; } = "OPEN";
    public string? StateReason { get; set; }
    public string BodyHTML { get; set; } = "";
    public DateTimeOffset CreatedAt { get; set; }
    public bool ViewerCanUpdate { get; set; }
    public Actor? Author { get; set; }
    public Nodes<LabelInfo> Labels { get; set; } = new();
    public Nodes<Actor> Assignees { get; set; } = new();
    public Count Comments { get; set; } = new();
    public Nodes<TimelineItem> TimelineItems { get; set; } = new();

    public VisualState VisualState => VisualStates.Issue(IssueState, StateReason);
}

public sealed class IssueRepo
{
    public IssueDetail? IssueOrPullRequest { get; set; }
}

public sealed class IssueDetailResponse
{
    public Viewer Viewer { get; set; } = new();
    public IssueRepo? Repository { get; set; }
}

public sealed class PRFile
{
    public string Filename { get; set; } = "";
    public string Status { get; set; } = "modified";
    public int Additions { get; set; }
    public int Deletions { get; set; }
    public string? Patch { get; set; }
    public string? PreviousFilename { get; set; }
}

public sealed class CommitDetailRest
{
    public sealed class Person
    {
        public string? Name { get; set; }
        public DateTimeOffset? Date { get; set; }
    }

    public sealed class Inner
    {
        public string Message { get; set; } = "";
        public Person? Author { get; set; }
    }

    public sealed class StatsInfo
    {
        public int Additions { get; set; }
        public int Deletions { get; set; }
    }

    public string Sha { get; set; } = "";
    public string HtmlUrl { get; set; } = "";
    public Inner Commit { get; set; } = new();
    public RestUser? Author { get; set; }
    public StatsInfo? Stats { get; set; }
    public List<PRFile>? Files { get; set; }
}

public sealed class Language
{
    public string Name { get; set; } = "";
    public string? Color { get; set; }
}

public sealed class RepoSummary
{
    public string NameWithOwner { get; set; } = "";
    public string? Description { get; set; }
    public bool IsPrivate { get; set; }
    public bool IsFork { get; set; }
    public bool IsArchived { get; set; }
    public int StargazerCount { get; set; }
    public DateTimeOffset? PushedAt { get; set; }
    public string Url { get; set; } = "";
    public Language? PrimaryLanguage { get; set; }
    public Count PullRequests { get; set; } = new();
    public Count Issues { get; set; } = new();

    public string Id => NameWithOwner;
    public RepoRef? Ref => RepoRef.Parse(NameWithOwner);
}

public sealed class RepoConnection
{
    public int TotalCount { get; set; }
    public PageInfo PageInfo { get; set; } = new();
    public List<RepoSummary> Nodes { get; set; } = new();
}

public sealed class ReposResponse
{
    public sealed class V
    {
        public RepoConnection Repositories { get; set; } = new();
    }

    public V Viewer { get; set; } = new();
}

public sealed class RepoInfo
{
    public string NameWithOwner { get; set; } = "";
    public string? Description { get; set; }
    public string Url { get; set; } = "";
    public bool IsPrivate { get; set; }
    public int StargazerCount { get; set; }
    public int ForkCount { get; set; }
    public BranchName? DefaultBranchRef { get; set; }
    public Count PullRequests { get; set; } = new();
    public Count Issues { get; set; } = new();
}

public sealed class RepoInfoResponse
{
    public RepoInfo? Repository { get; set; }
}

public sealed class ProjectOwner
{
    public string? Login { get; set; }
}

public sealed class ProjectSummary
{
    public string Id { get; set; } = "";
    public int Number { get; set; }
    public string Title { get; set; } = "";
    public string Url { get; set; } = "";
    public bool Closed { get; set; }
    public DateTimeOffset UpdatedAt { get; set; }
    public string? ShortDescription { get; set; }
    public Count Items { get; set; } = new();
    public ProjectOwner? Owner { get; set; }

    public ProjectRef Ref => new(Id, Title, Url);
}

public sealed class ProjectsResponse
{
    public sealed class Org
    {
        public string Login { get; set; } = "";
        public Nodes<ProjectSummary> ProjectsV2 { get; set; } = new();
    }

    public sealed class V
    {
        public Nodes<ProjectSummary> ProjectsV2 { get; set; } = new();
        public Nodes<Org> Organizations { get; set; } = new();
    }

    public V Viewer { get; set; } = new();
}

public sealed class ProjectOption
{
    public string Id { get; set; } = "";
    public string Name { get; set; } = "";
    public string Color { get; set; } = "GRAY";
}

public sealed class ProjectField
{
    [JsonPropertyName("__typename")]
    public string? Typename { get; set; }
    public string? Id { get; set; }
    public string? Name { get; set; }
    public List<ProjectOption>? Options { get; set; }
}

public sealed class FieldRef
{
    public string? Id { get; set; }
}

public sealed class ProjectFieldValue
{
    [JsonPropertyName("__typename")]
    public string Typename { get; set; } = "";
    public string? OptionId { get; set; }
    public FieldRef? Field { get; set; }
}

public sealed class ProjectContent
{
    [JsonPropertyName("__typename")]
    public string Typename { get; set; } = "";
    public int? Number { get; set; }
    public string? Title { get; set; }
    public string? Url { get; set; }
    public string? IssueState { get; set; }
    public string? PrState { get; set; }
    public bool? IsDraft { get; set; }
    public RepoName? Repository { get; set; }
    public Nodes<Actor>? Assignees { get; set; }

    public Route? Route
    {
        get
        {
            if (Number is not { } n || RepoRef.Parse(Repository?.NameWithOwner) is not { } repo) return null;
            var r = new ItemRef(repo, n);
            return Typename == "PullRequest" ? Api.Route.Pr(r) : Api.Route.Issue(r);
        }
    }

    public VisualState? VisualState => Typename switch
    {
        "PullRequest" => VisualStates.Pr(PrState ?? "OPEN", IsDraft ?? false),
        "Issue" => VisualStates.Issue(IssueState ?? "OPEN", null),
        _ => null
    };
}

public sealed class ProjectItem
{
    public string Id { get; set; } = "";
    public string Type { get; set; } = "";
    public bool IsArchived { get; set; }
    public Nodes<ProjectFieldValue> FieldValues { get; set; } = new();
    public ProjectContent? Content { get; set; }

    public string? OptionId(string fieldId) =>
        FieldValues.Items.FirstOrDefault(v => v.Field?.Id == fieldId && v.OptionId is not null)?.OptionId;
}

public sealed class ProjectItemsConnection
{
    public PageInfo PageInfo { get; set; } = new();
    public List<ProjectItem> Nodes { get; set; } = new();
}

public sealed class ProjectDetail
{
    public string Id { get; set; } = "";
    public string Title { get; set; } = "";
    public string Url { get; set; } = "";
    public Nodes<ProjectField> Fields { get; set; } = new();
    public ProjectItemsConnection Items { get; set; } = new();
}

public sealed class ProjectDetailResponse
{
    public ProjectDetail? Node { get; set; }
}

public sealed class NotificationThread
{
    public sealed class SubjectInfo
    {
        public string Title { get; set; } = "";
        public string? Url { get; set; }
        public string Type { get; set; } = "";
    }

    public sealed class OwnerInfo
    {
        public string? AvatarUrl { get; set; }
    }

    public sealed class RepoInfo
    {
        public string FullName { get; set; } = "";
        public OwnerInfo? Owner { get; set; }
    }

    public string Id { get; set; } = "";
    public bool Unread { get; set; }
    public string Reason { get; set; } = "";
    public DateTimeOffset UpdatedAt { get; set; }
    public SubjectInfo Subject { get; set; } = new();
    public RepoInfo Repository { get; set; } = new();

    public Route? Route => Subject.Url is { } u ? GitHubUrl.Parse(u, allowShort: false) : null;

    public string WebUrl
    {
        get
        {
            if (Route is { } r)
            {
                if (r.Kind == RouteKind.PullRequest) return $"https://github.com/{r.Item!.Repo.Full}/pull/{r.Item.Number}";
                if (r.Kind == RouteKind.Issue) return $"https://github.com/{r.Item!.Repo.Full}/issues/{r.Item.Number}";
            }
            return Subject.Type switch
            {
                "Release" => $"https://github.com/{Repository.FullName}/releases",
                "CheckSuite" => $"https://github.com/{Repository.FullName}/actions",
                "Discussion" => $"https://github.com/{Repository.FullName}/discussions",
                _ => $"https://github.com/{Repository.FullName}"
            };
        }
    }
}

public sealed class BlobMeta
{
    public int? ByteSize { get; set; }
    public bool? IsBinary { get; set; }
}

public sealed class TreeEntry
{
    public string Name { get; set; } = "";
    public string Path { get; set; } = "";
    public string Type { get; set; } = "blob";
    public BlobMeta? Object { get; set; }

    public string Id => Path;
    public bool IsDir => Type == "tree";
    public bool IsSubmodule => Type == "commit";
}

public sealed class GitObject
{
    [JsonPropertyName("__typename")]
    public string Typename { get; set; } = "";
    public List<TreeEntry>? Entries { get; set; }
    public int? ByteSize { get; set; }
    public bool? IsBinary { get; set; }
    public bool? IsTruncated { get; set; }
    public string? Text { get; set; }
}

public sealed class GitObjectResponse
{
    public sealed class R
    {
        public GitObject? Object { get; set; }
    }

    public R? Repository { get; set; }
}

public sealed class BlameAuthor
{
    public string? Name { get; set; }
    public Actor? User { get; set; }
}

public sealed class BlameCommit
{
    public string Oid { get; set; } = "";
    public string AbbreviatedOid { get; set; } = "";
    public string MessageHeadline { get; set; } = "";
    public DateTimeOffset? CommittedDate { get; set; }
    public string Url { get; set; } = "";
    public BlameAuthor? Author { get; set; }
}

public sealed class BlameRangeNode
{
    public int StartingLine { get; set; }
    public int EndingLine { get; set; }
    public int Age { get; set; }
    public BlameCommit? Commit { get; set; }
}

public sealed class BlameResponse
{
    public sealed class B
    {
        public List<BlameRangeNode> Ranges { get; set; } = new();
    }

    public sealed class O
    {
        [JsonPropertyName("__typename")]
        public string Typename { get; set; } = "";
        public B? Blame { get; set; }
    }

    public sealed class R
    {
        public O? Object { get; set; }
    }

    public R? Repository { get; set; }
}

public sealed class BranchInfo
{
    public sealed class C
    {
        public string Sha { get; set; } = "";
    }

    public string Name { get; set; } = "";
    public C Commit { get; set; } = new();
}

public sealed class GitTreeRest
{
    public sealed class Item
    {
        public string Path { get; set; } = "";
        public string Type { get; set; } = "";
    }

    public List<Item> Tree { get; set; } = new();
    public bool Truncated { get; set; }
}

public sealed class PendingComment
{
    public Guid Id { get; } = Guid.NewGuid();
    public string Path { get; init; } = "";
    public int Line { get; init; }
    public string Side { get; init; } = "RIGHT";
    public string Body { get; init; } = "";
}
