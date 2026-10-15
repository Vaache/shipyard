namespace PullYard.Core.Api;

public static class Q
{
    public static readonly string PrItemFragment = @"fragment PRItem on PullRequest { number title url prState: state isDraft createdAt updatedAt baseRefName author { login avatarUrl } repository { nameWithOwner defaultBranchRef { name } } reviewDecision additions deletions comments { totalCount } labels(first: 6) { nodes { name color } } commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } }";

    public static readonly string PrItem = @"query($o: String!, $n: String!, $num: Int!) { repository(owner: $o, name: $n) { pullRequest(number: $num) { __typename ...PRItem } } }" + PrItemFragment;

    public static readonly string RepoPullRequests = @"query($o: String!, $n: String!, $b: String, $states: [PullRequestState!], $first: Int!) { repository(owner: $o, name: $n) { pullRequests(headRefName: $b, states: $states, first: $first, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { __typename ...PRItem } } } }" + PrItemFragment;

    public const string StackNode = "fragment StackPR on PullRequest { number title state isDraft headRefName baseRefName }";

    public static readonly string PrStack = @"query($o: String!, $n: String!, $num: Int!) { repository(owner: $o, name: $n) { defaultBranchRef { name } pullRequest(number: $num) { ...StackPR stack { entries(first: 30) { nodes { position pullRequest { ...StackPR } } } } } } }" + StackNode;

    public static readonly string PrStackNeighbors = @"query($o: String!, $n: String!, $head: String!, $base: String!) { repository(owner: $o, name: $n) {
  parents: pullRequests(headRefName: $base, states: [OPEN, MERGED], first: 3, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { ...StackPR } }
  children: pullRequests(baseRefName: $head, states: [OPEN, MERGED], first: 5, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { ...StackPR } }
} }" + StackNode;

    public static readonly string ItemFragments = @"fragment PRItem on PullRequest { number title url prState: state isDraft createdAt updatedAt baseRefName author { login avatarUrl } repository { nameWithOwner defaultBranchRef { name } } reviewDecision additions deletions comments { totalCount } labels(first: 6) { nodes { name color } } commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } }
fragment IssueItem on Issue { number title url issueState: state stateReason createdAt updatedAt author { login avatarUrl } repository { nameWithOwner defaultBranchRef { name } } comments { totalCount } labels(first: 6) { nodes { name color } } }";

    public static readonly string Search = @"query($q: String!, $after: String) { search(query: $q, type: ISSUE, first: 40, after: $after) { issueCount pageInfo { hasNextPage endCursor } nodes { __typename ...PRItem ...IssueItem } } }" + ItemFragments;

    public static readonly string Badges = @"query {
  rr: search(query: ""is:pr is:open review-requested:@me archived:false sort:updated-desc"", type: ISSUE, first: 10) { issueCount nodes { __typename ...PRItem ...IssueItem } }
  mine: search(query: ""is:pr is:open author:@me archived:false sort:updated-desc"", type: ISSUE, first: 10) { issueCount nodes { __typename ...PRItem ...IssueItem } }
}" + ItemFragments;

    public const string Actor = "{ login avatarUrl }";

    public static readonly string CommonTimeline = $@"... on IssueComment {{ id author {Actor} body bodyHTML createdAt url }}
... on ClosedEvent {{ actor {Actor} createdAt }}
... on ReopenedEvent {{ actor {Actor} createdAt }}
... on LabeledEvent {{ actor {Actor} createdAt label {{ name color }} }}
... on RenamedTitleEvent {{ actor {Actor} createdAt previousTitle currentTitle }}
... on CrossReferencedEvent {{ actor {Actor} createdAt source {{ __typename ... on PullRequest {{ number title url repository {{ nameWithOwner }} }} ... on Issue {{ number title url repository {{ nameWithOwner }} }} }} }}";

    public static readonly string PrTimeline = $@"... on PullRequestReview {{ id author {Actor} state body bodyHTML createdAt url }}
... on PullRequestCommit {{ commit {{ oid abbreviatedOid messageHeadline committedDate author {{ name avatarUrl user {{ login }} }} }} }}
... on MergedEvent {{ actor {Actor} createdAt mergeRefName commit {{ abbreviatedOid }} }}
... on HeadRefForcePushedEvent {{ actor {Actor} createdAt }}
... on HeadRefDeletedEvent {{ actor {Actor} createdAt }}
... on ReadyForReviewEvent {{ actor {Actor} createdAt }}
... on ConvertToDraftEvent {{ actor {Actor} createdAt }}
... on ReviewRequestedEvent {{ actor {Actor} createdAt requestedReviewer {{ __typename ... on User {{ login avatarUrl }} ... on Team {{ name }} }} }}";

    public static readonly string PullRequest = $@"query($owner: String!, $name: String!, $number: Int!) {{
  viewer {{ login }}
  repository(owner: $owner, name: $name) {{
    viewerPermission mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed deleteBranchOnMerge autoMergeAllowed viewerDefaultMergeMethod
    pullRequest(number: $number) {{
      id number title url state isDraft merged mergeable mergeStateStatus body bodyHTML createdAt
      author {Actor}
      headRefName baseRefName headRefOid
      headRepository {{ nameWithOwner }}
      headRef {{ name }}
      additions deletions changedFiles
      reviewDecision viewerCanUpdate viewerDidAuthor
      autoMergeRequest {{ enabledAt mergeMethod }}
      labels(first: 20) {{ nodes {{ name color }} }}
      assignees(first: 10) {{ nodes {{ login avatarUrl }} }}
      reviewRequests(first: 20) {{ nodes {{ requestedReviewer {{ __typename ... on User {{ login avatarUrl }} ... on Team {{ name }} ... on Mannequin {{ login avatarUrl }} }} }} }}
      latestOpinionatedReviews(first: 20) {{ nodes {{ state submittedAt commit {{ oid }} author {Actor} }} }}
      lastCommit: commits(last: 1) {{ nodes {{ commit {{ oid committedDate statusCheckRollup {{ state contexts(first: 100) {{ nodes {{ __typename
        ... on CheckRun {{ databaseId name status conclusion detailsUrl startedAt completedAt checkSuite {{ workflowRun {{ databaseId workflow {{ name }} }} }} }}
        ... on StatusContext {{ context state targetUrl description }}
      }} }} }} }} }} }}
      allCommits: commits(first: 100) {{ totalCount nodes {{ commit {{ oid abbreviatedOid messageHeadline committedDate url author {{ name avatarUrl user {{ login }} }} statusCheckRollup {{ state }} }} }} }}
      files(first: 100) {{ nodes {{ path viewerViewedState }} }}
      timelineItems(last: 100, itemTypes: [ISSUE_COMMENT, PULL_REQUEST_REVIEW, PULL_REQUEST_COMMIT, MERGED_EVENT, CLOSED_EVENT, REOPENED_EVENT, HEAD_REF_FORCE_PUSHED_EVENT, HEAD_REF_DELETED_EVENT, READY_FOR_REVIEW_EVENT, CONVERT_TO_DRAFT_EVENT, REVIEW_REQUESTED_EVENT, CROSS_REFERENCED_EVENT, LABELED_EVENT, RENAMED_TITLE_EVENT]) {{
        totalCount nodes {{ __typename {CommonTimeline} {PrTimeline} }}
      }}
      reviewThreads(first: 100) {{ nodes {{ id isResolved isOutdated path line originalLine diffSide viewerCanResolve viewerCanUnresolve viewerCanReply
        comments(first: 50) {{ nodes {{ id author {Actor} body bodyHTML createdAt diffHunk pullRequestReview {{ id }} }} }} }} }}
    }}
  }}
}}";

    public static readonly string Issue = $@"query($owner: String!, $name: String!, $number: Int!) {{
  viewer {{ login }}
  repository(owner: $owner, name: $name) {{
    issueOrPullRequest(number: $number) {{ __typename
      ... on Issue {{ id number title url issueState: state stateReason bodyHTML createdAt viewerCanUpdate author {Actor}
        labels(first: 20) {{ nodes {{ name color }} }}
        assignees(first: 10) {{ nodes {{ login avatarUrl }} }}
        comments {{ totalCount }}
        timelineItems(last: 100, itemTypes: [ISSUE_COMMENT, CLOSED_EVENT, REOPENED_EVENT, CROSS_REFERENCED_EVENT, LABELED_EVENT, RENAMED_TITLE_EVENT]) {{ nodes {{ __typename {CommonTimeline} }} }}
      }}
    }}
  }}
}}";

    public static readonly string Repos = @"query($after: String) { viewer { repositories(first: 50, after: $after, orderBy: {field: PUSHED_AT, direction: DESC}, ownerAffiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {
  totalCount pageInfo { hasNextPage endCursor }
  nodes { nameWithOwner description isPrivate isFork isArchived stargazerCount pushedAt url primaryLanguage { name color } pullRequests(states: OPEN) { totalCount } issues(states: OPEN) { totalCount } }
} } }";

    public static readonly string RepoInfo = @"query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { nameWithOwner description url isPrivate stargazerCount forkCount defaultBranchRef { name } pullRequests(states: OPEN) { totalCount } issues(states: OPEN) { totalCount } } }";

    public static readonly string ProjectFragment = @"fragment ProjectItem on ProjectV2 { id number title url closed updatedAt shortDescription items { totalCount } owner { ... on User { login } ... on Organization { login } } }";

    public static readonly string Projects = @"query { viewer {
  projectsV2(first: 50, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { ...ProjectItem } }
  organizations(first: 30) { nodes { login projectsV2(first: 50, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { ...ProjectItem } } } }
} }" + ProjectFragment;

    public static readonly string Project = @"query($id: ID!, $after: String) { node(id: $id) { ... on ProjectV2 { id title url
  fields(first: 50) { nodes { __typename
    ... on ProjectV2SingleSelectField { id name options { id name color } }
    ... on ProjectV2Field { id name }
    ... on ProjectV2IterationField { id name }
  } }
  items(first: 100, after: $after) { pageInfo { hasNextPage endCursor } nodes { id type isArchived
    fieldValues(first: 30) { nodes { __typename ... on ProjectV2ItemFieldSingleSelectValue { optionId field { ... on ProjectV2SingleSelectField { id } } } } }
    content { __typename
      ... on Issue { number title url issueState: state repository { nameWithOwner } assignees(first: 3) { nodes { login avatarUrl } } }
      ... on PullRequest { number title url prState: state isDraft repository { nameWithOwner } assignees(first: 3) { nodes { login avatarUrl } } }
      ... on DraftIssue { title }
    }
  } }
} } }";

    public static readonly string MoveProjectItem = @"mutation($p: ID!, $i: ID!, $f: ID!, $o: String!) { updateProjectV2ItemFieldValue(input: {projectId: $p, itemId: $i, fieldId: $f, value: {singleSelectOptionId: $o}}) { projectV2Item { id } } }";

    public static readonly string ClearProjectItem = @"mutation($p: ID!, $i: ID!, $f: ID!) { clearProjectV2ItemFieldValue(input: {projectId: $p, itemId: $i, fieldId: $f}) { projectV2Item { id } } }";

    public const string AddComment = "mutation($id: ID!, $b: String!) { addComment(input: {subjectId: $id, body: $b}) { clientMutationId } }";

    public const string AddReview = "mutation($id: ID!, $e: PullRequestReviewEvent!, $b: String, $th: [DraftPullRequestReviewThread]) { addPullRequestReview(input: {pullRequestId: $id, event: $e, body: $b, threads: $th}) { clientMutationId } }";

    public const string ReplyThread = "mutation($id: ID!, $b: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $id, body: $b}) { clientMutationId } }";

    public const string ResolveThread = "mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { clientMutationId } }";

    public const string UnresolveThread = "mutation($id: ID!) { unresolveReviewThread(input: {threadId: $id}) { clientMutationId } }";

    public const string Merge = "mutation($id: ID!, $m: PullRequestMergeMethod!, $t: String, $b: String) { mergePullRequest(input: {pullRequestId: $id, mergeMethod: $m, commitHeadline: $t, commitBody: $b}) { clientMutationId } }";

    public const string EnableAutoMerge = "mutation($id: ID!, $m: PullRequestMergeMethod!) { enablePullRequestAutoMerge(input: {pullRequestId: $id, mergeMethod: $m}) { clientMutationId } }";

    public const string DisableAutoMerge = "mutation($id: ID!) { disablePullRequestAutoMerge(input: {pullRequestId: $id}) { clientMutationId } }";

    public const string ClosePR = "mutation($id: ID!) { closePullRequest(input: {pullRequestId: $id}) { clientMutationId } }";

    public const string ReopenPR = "mutation($id: ID!) { reopenPullRequest(input: {pullRequestId: $id}) { clientMutationId } }";

    public const string ReadyForReview = "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { clientMutationId } }";

    public const string ConvertToDraft = "mutation($id: ID!) { convertPullRequestToDraft(input: {pullRequestId: $id}) { clientMutationId } }";

    public const string UpdateBranch = "mutation($id: ID!) { updatePullRequestBranch(input: {pullRequestId: $id}) { clientMutationId } }";

    public const string EditPR = "mutation($id: ID!, $t: String!, $b: String!) { updatePullRequest(input: {pullRequestId: $id, title: $t, body: $b}) { clientMutationId } }";

    public const string MarkViewed = "mutation($id: ID!, $p: String!) { markFileAsViewed(input: {pullRequestId: $id, path: $p}) { clientMutationId } }";

    public const string UnmarkViewed = "mutation($id: ID!, $p: String!) { unmarkFileAsViewed(input: {pullRequestId: $id, path: $p}) { clientMutationId } }";

    public const string CloseIssue = "mutation($id: ID!) { closeIssue(input: {issueId: $id}) { clientMutationId } }";

    public const string ReopenIssue = "mutation($id: ID!) { reopenIssue(input: {issueId: $id}) { clientMutationId } }";
}
