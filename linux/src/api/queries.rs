pub const PR_ITEM_FRAGMENT: &str = "fragment PRItem on PullRequest { number title url prState: state isDraft createdAt updatedAt baseRefName author { login avatarUrl } repository { nameWithOwner defaultBranchRef { name } } reviewDecision additions deletions comments { totalCount } labels(first: 6) { nodes { name color } } commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } }";

pub const ISSUE_ITEM_FRAGMENT: &str = "fragment IssueItem on Issue { number title url issueState: state stateReason createdAt updatedAt author { login avatarUrl } repository { nameWithOwner defaultBranchRef { name } } comments { totalCount } labels(first: 6) { nodes { name color } } }";

pub fn pr_item() -> String {
    format!("query($o: String!, $n: String!, $num: Int!) {{ repository(owner: $o, name: $n) {{ pullRequest(number: $num) {{ __typename ...PRItem }} }} }}\n{PR_ITEM_FRAGMENT}")
}

pub fn repo_pull_requests() -> String {
    format!("query($o: String!, $n: String!, $b: String, $states: [PullRequestState!], $first: Int!) {{ repository(owner: $o, name: $n) {{ pullRequests(headRefName: $b, states: $states, first: $first, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ nodes {{ __typename ...PRItem }} }} }} }}\n{PR_ITEM_FRAGMENT}")
}

pub const STACK_NODE: &str = "fragment StackPR on PullRequest { number title state isDraft headRefName baseRefName }";

pub fn pr_stack() -> String {
    format!("query($o: String!, $n: String!, $num: Int!) {{ repository(owner: $o, name: $n) {{ defaultBranchRef {{ name }} pullRequest(number: $num) {{ ...StackPR stack {{ entries(first: 30) {{ nodes {{ position pullRequest {{ ...StackPR }} }} }} }} }} }} }}\n{STACK_NODE}")
}

pub fn pr_stack_fallback() -> String {
    format!("query($o: String!, $n: String!, $num: Int!) {{ repository(owner: $o, name: $n) {{ defaultBranchRef {{ name }} pullRequest(number: $num) {{ ...StackPR }} }} }}\n{STACK_NODE}")
}

pub fn pr_stack_neighbors() -> String {
    format!(
        "query($o: String!, $n: String!, $head: String!, $base: String!) {{ repository(owner: $o, name: $n) {{
  parents: pullRequests(headRefName: $base, states: [OPEN, MERGED], first: 3, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ nodes {{ ...StackPR }} }}
  children: pullRequests(baseRefName: $head, states: [OPEN, MERGED], first: 5, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ nodes {{ ...StackPR }} }}
}} }}\n{STACK_NODE}"
    )
}

pub fn search() -> String {
    format!("query($q: String!, $after: String) {{ search(query: $q, type: ISSUE, first: 40, after: $after) {{ issueCount pageInfo {{ hasNextPage endCursor }} nodes {{ __typename ...PRItem ...IssueItem }} }} }}\n{PR_ITEM_FRAGMENT}\n{ISSUE_ITEM_FRAGMENT}")
}

pub const BADGE_ITEM_FRAGMENT: &str = "fragment BadgeItem on PullRequest { number title url prState: state isDraft createdAt updatedAt author { login } repository { nameWithOwner } commits(last: 1) { nodes { commit { statusCheckRollup { state } } } } }";

pub fn badges() -> String {
    format!(
        "query {{
  rr: search(query: \"is:pr is:open review-requested:@me archived:false sort:updated-desc\", type: ISSUE, first: 10) {{ issueCount nodes {{ __typename ...BadgeItem }} }}
  mine: search(query: \"is:pr is:open author:@me archived:false sort:updated-desc\", type: ISSUE, first: 10) {{ issueCount nodes {{ __typename ...BadgeItem }} }}
}}\n{BADGE_ITEM_FRAGMENT}"
    )
}

const ACTOR: &str = "{ login avatarUrl }";

fn common_timeline() -> String {
    format!(
        "... on IssueComment {{ id author {ACTOR} body bodyHTML createdAt url }}
... on ClosedEvent {{ actor {ACTOR} createdAt }}
... on ReopenedEvent {{ actor {ACTOR} createdAt }}
... on LabeledEvent {{ actor {ACTOR} createdAt label {{ name color }} }}
... on RenamedTitleEvent {{ actor {ACTOR} createdAt previousTitle currentTitle }}
... on CrossReferencedEvent {{ actor {ACTOR} createdAt source {{ __typename ... on PullRequest {{ number title url repository {{ nameWithOwner }} }} ... on Issue {{ number title url repository {{ nameWithOwner }} }} }} }}"
    )
}

fn pr_timeline() -> String {
    format!(
        "... on PullRequestReview {{ id author {ACTOR} state body bodyHTML createdAt url }}
... on PullRequestCommit {{ commit {{ oid abbreviatedOid messageHeadline committedDate author {{ name avatarUrl user {{ login }} }} }} }}
... on MergedEvent {{ actor {ACTOR} createdAt mergeRefName commit {{ abbreviatedOid }} }}
... on HeadRefForcePushedEvent {{ actor {ACTOR} createdAt }}
... on HeadRefDeletedEvent {{ actor {ACTOR} createdAt }}
... on ReadyForReviewEvent {{ actor {ACTOR} createdAt }}
... on ConvertToDraftEvent {{ actor {ACTOR} createdAt }}
... on ReviewRequestedEvent {{ actor {ACTOR} createdAt requestedReviewer {{ __typename ... on User {{ login avatarUrl }} ... on Team {{ name }} }} }}"
    )
}

pub const PR_LIGHT_FRAGMENT: &str = "fragment PrLight on PullRequest { lightUpdatedAt: updatedAt lightHeadOid: headRefOid lightState: state lightDraft: isDraft lightMergeable: mergeable lightMergeState: mergeStateStatus lightDecision: reviewDecision lightComments: comments { totalCount } lightReviews: reviews { totalCount } lightThreads: reviewThreads(first: 100) { totalCount nodes { isResolved } } lightCommits: commits(last: 1) { totalCount nodes { commit { oid statusCheckRollup { state contexts(first: 100) { totalCount checkRunCountsByState { state count } statusContextCountsByState { state count } nodes { __typename ... on CheckRun { databaseId name status conclusion completedAt } ... on StatusContext { context state } } } } } } } }";

pub fn pull_request_light() -> String {
    format!("query($owner: String!, $name: String!, $number: Int!) {{ repository(owner: $owner, name: $name) {{ pullRequest(number: $number) {{ ...PrLight }} }} }}\n{PR_LIGHT_FRAGMENT}")
}

pub fn pull_request() -> String {
    format!(
        "query($owner: String!, $name: String!, $number: Int!) {{
  viewer {{ login }}
  repository(owner: $owner, name: $name) {{
    viewerPermission mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed deleteBranchOnMerge autoMergeAllowed viewerDefaultMergeMethod
    pullRequest(number: $number) {{
      id number title url state isDraft merged mergeable mergeStateStatus body bodyHTML createdAt
      author {ACTOR}
      headRefName baseRefName headRefOid
      headRepository {{ nameWithOwner }}
      headRef {{ name }}
      additions deletions changedFiles
      reviewDecision viewerCanUpdate viewerDidAuthor
      autoMergeRequest {{ enabledAt mergeMethod }}
      labels(first: 20) {{ nodes {{ name color }} }}
      assignees(first: 10) {{ nodes {{ login avatarUrl }} }}
      reviewRequests(first: 20) {{ nodes {{ requestedReviewer {{ __typename ... on User {{ login avatarUrl }} ... on Team {{ name }} ... on Mannequin {{ login avatarUrl }} }} }} }}
      latestOpinionatedReviews(first: 20) {{ nodes {{ state submittedAt commit {{ oid }} author {ACTOR} }} }}
      lastCommit: commits(last: 1) {{ nodes {{ commit {{ oid committedDate statusCheckRollup {{ state contexts(first: 100) {{ nodes {{ __typename
        ... on CheckRun {{ databaseId name status conclusion detailsUrl startedAt completedAt checkSuite {{ workflowRun {{ databaseId workflow {{ name }} }} }} }}
        ... on StatusContext {{ context state targetUrl description }}
      }} }} }} }} }} }}
      allCommits: commits(first: 100) {{ totalCount nodes {{ commit {{ oid abbreviatedOid messageHeadline committedDate url author {{ name avatarUrl user {{ login }} }} statusCheckRollup {{ state }} }} }} }}
      files(first: 100) {{ nodes {{ path viewerViewedState }} }}
      timelineItems(last: 100, itemTypes: [ISSUE_COMMENT, PULL_REQUEST_REVIEW, PULL_REQUEST_COMMIT, MERGED_EVENT, CLOSED_EVENT, REOPENED_EVENT, HEAD_REF_FORCE_PUSHED_EVENT, HEAD_REF_DELETED_EVENT, READY_FOR_REVIEW_EVENT, CONVERT_TO_DRAFT_EVENT, REVIEW_REQUESTED_EVENT, CROSS_REFERENCED_EVENT, LABELED_EVENT, RENAMED_TITLE_EVENT]) {{
        totalCount nodes {{ __typename {} {} }}
      }}
      reviewThreads(first: 100) {{ nodes {{ id isResolved isOutdated path line originalLine diffSide viewerCanResolve viewerCanUnresolve viewerCanReply
        comments(first: 50) {{ nodes {{ id author {ACTOR} body bodyHTML createdAt diffHunk pullRequestReview {{ id }} }} }} }} }}
      ...PrLight
    }}
  }}
}}
{PR_LIGHT_FRAGMENT}",
        common_timeline(),
        pr_timeline()
    )
}

pub fn issue() -> String {
    format!(
        "query($owner: String!, $name: String!, $number: Int!) {{
  viewer {{ login }}
  repository(owner: $owner, name: $name) {{
    issueOrPullRequest(number: $number) {{ __typename
      ... on Issue {{ id number title url issueState: state stateReason bodyHTML createdAt viewerCanUpdate author {ACTOR}
        labels(first: 20) {{ nodes {{ name color }} }}
        assignees(first: 10) {{ nodes {{ login avatarUrl }} }}
        comments {{ totalCount }}
        timelineItems(last: 100, itemTypes: [ISSUE_COMMENT, CLOSED_EVENT, REOPENED_EVENT, CROSS_REFERENCED_EVENT, LABELED_EVENT, RENAMED_TITLE_EVENT]) {{ nodes {{ __typename {} }} }}
      }}
    }}
  }}
}}",
        common_timeline()
    )
}

pub const REPOS: &str = "query($after: String) { viewer { repositories(first: 50, after: $after, orderBy: {field: PUSHED_AT, direction: DESC}, ownerAffiliations: [OWNER, COLLABORATOR, ORGANIZATION_MEMBER]) {
  totalCount pageInfo { hasNextPage endCursor }
  nodes { nameWithOwner description isPrivate isFork isArchived stargazerCount pushedAt url primaryLanguage { name color } pullRequests(states: OPEN) { totalCount } issues(states: OPEN) { totalCount } }
} } }";

pub const REPO_INFO: &str = "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { nameWithOwner description url isPrivate stargazerCount forkCount defaultBranchRef { name } pullRequests(states: OPEN) { totalCount } issues(states: OPEN) { totalCount } } }";

const PROJECT_FRAGMENT: &str = "fragment ProjectItem on ProjectV2 { id number title url closed updatedAt shortDescription items { totalCount } owner { ... on User { login } ... on Organization { login } } }";

pub fn projects() -> String {
    format!(
        "query {{ viewer {{
  projectsV2(first: 50, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ nodes {{ ...ProjectItem }} }}
  organizations(first: 30) {{ nodes {{ login projectsV2(first: 50, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ nodes {{ ...ProjectItem }} }} }} }}
}} }}\n{PROJECT_FRAGMENT}"
    )
}

pub const PROJECT: &str = "query($id: ID!, $after: String) { node(id: $id) { ... on ProjectV2 { id title url
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

pub const MOVE_PROJECT_ITEM: &str = "mutation($p: ID!, $i: ID!, $f: ID!, $o: String!) { updateProjectV2ItemFieldValue(input: {projectId: $p, itemId: $i, fieldId: $f, value: {singleSelectOptionId: $o}}) { projectV2Item { id } } }";
pub const CLEAR_PROJECT_ITEM: &str = "mutation($p: ID!, $i: ID!, $f: ID!) { clearProjectV2ItemFieldValue(input: {projectId: $p, itemId: $i, fieldId: $f}) { projectV2Item { id } } }";

pub const ADD_COMMENT: &str = "mutation($id: ID!, $b: String!) { addComment(input: {subjectId: $id, body: $b}) { clientMutationId } }";
pub const ADD_REVIEW: &str = "mutation($id: ID!, $e: PullRequestReviewEvent!, $b: String, $th: [DraftPullRequestReviewThread]) { addPullRequestReview(input: {pullRequestId: $id, event: $e, body: $b, threads: $th}) { clientMutationId } }";
pub const REPLY_THREAD: &str = "mutation($id: ID!, $b: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $id, body: $b}) { clientMutationId } }";
pub const RESOLVE_THREAD: &str = "mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { clientMutationId } }";
pub const UNRESOLVE_THREAD: &str = "mutation($id: ID!) { unresolveReviewThread(input: {threadId: $id}) { clientMutationId } }";
pub const MERGE: &str = "mutation($id: ID!, $m: PullRequestMergeMethod!, $t: String, $b: String) { mergePullRequest(input: {pullRequestId: $id, mergeMethod: $m, commitHeadline: $t, commitBody: $b}) { clientMutationId } }";
pub const ENABLE_AUTO_MERGE: &str = "mutation($id: ID!, $m: PullRequestMergeMethod!) { enablePullRequestAutoMerge(input: {pullRequestId: $id, mergeMethod: $m}) { clientMutationId } }";
pub const DISABLE_AUTO_MERGE: &str = "mutation($id: ID!) { disablePullRequestAutoMerge(input: {pullRequestId: $id}) { clientMutationId } }";
pub const CLOSE_PR: &str = "mutation($id: ID!) { closePullRequest(input: {pullRequestId: $id}) { clientMutationId } }";
pub const REOPEN_PR: &str = "mutation($id: ID!) { reopenPullRequest(input: {pullRequestId: $id}) { clientMutationId } }";
pub const READY_FOR_REVIEW: &str = "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { clientMutationId } }";
pub const CONVERT_TO_DRAFT: &str = "mutation($id: ID!) { convertPullRequestToDraft(input: {pullRequestId: $id}) { clientMutationId } }";
pub const UPDATE_BRANCH: &str = "mutation($id: ID!) { updatePullRequestBranch(input: {pullRequestId: $id}) { clientMutationId } }";
pub const EDIT_PR: &str = "mutation($id: ID!, $t: String!, $b: String!) { updatePullRequest(input: {pullRequestId: $id, title: $t, body: $b}) { clientMutationId } }";
pub const MARK_VIEWED: &str = "mutation($id: ID!, $p: String!) { markFileAsViewed(input: {pullRequestId: $id, path: $p}) { clientMutationId } }";
pub const UNMARK_VIEWED: &str = "mutation($id: ID!, $p: String!) { unmarkFileAsViewed(input: {pullRequestId: $id, path: $p}) { clientMutationId } }";
pub const CLOSE_ISSUE: &str = "mutation($id: ID!) { closeIssue(input: {issueId: $id}) { clientMutationId } }";
pub const REOPEN_ISSUE: &str = "mutation($id: ID!) { reopenIssue(input: {issueId: $id}) { clientMutationId } }";

pub const BLAME: &str = "query Blame($owner: String!, $name: String!, $ref: String!, $path: String!) { repository(owner: $owner, name: $name) { object(expression: $ref) { __typename ... on Commit { blame(path: $path) { ranges { startingLine endingLine age commit { oid abbreviatedOid messageHeadline committedDate url author { name user { login avatarUrl } } } } } } } } }";

pub const GIT_OBJECT: &str = "query($owner: String!, $name: String!, $expr: String!) { repository(owner: $owner, name: $name) { object(expression: $expr) { __typename
  ... on Tree { entries { name path type object { ... on Blob { byteSize isBinary } } } }
  ... on Blob { byteSize isBinary isTruncated text }
} } }";
