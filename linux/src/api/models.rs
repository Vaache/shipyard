use chrono::{DateTime, Utc};
use serde::de::{DeserializeOwned, Deserializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type Date = DateTime<Utc>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    pub fn new(owner: &str, name: &str) -> Self {
        RepoRef { owner: owner.to_string(), name: name.to_string() }
    }

    pub fn from_full(full: &str) -> Option<Self> {
        let mut parts = full.splitn(2, '/');
        let owner = parts.next()?;
        let name = parts.next()?;
        if owner.is_empty() || name.is_empty() {
            return None;
        }
        Some(RepoRef::new(owner, name))
    }

    pub fn full(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemRef {
    pub repo: RepoRef,
    pub number: i64,
}

impl ItemRef {
    pub fn title(&self) -> String {
        format!("{}#{}", self.repo.full(), self.number)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectRef {
    pub id: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Route {
    Pr(ItemRef),
    Issue(ItemRef),
    Repo(RepoRef),
    Project(ProjectRef),
    Commit(RepoRef, String),
}

impl Route {
    pub fn key(&self) -> String {
        match self {
            Route::Pr(r) => format!("pr:{}", r.title()),
            Route::Issue(r) => format!("issue:{}", r.title()),
            Route::Repo(r) => format!("repo:{}", r.full()),
            Route::Project(p) => format!("project:{}", p.id),
            Route::Commit(r, sha) => format!("commit:{}@{}", r.full(), sha),
        }
    }

    pub fn short_link(&self) -> Option<String> {
        match self {
            Route::Pr(r) | Route::Issue(r) => Some(r.title()),
            _ => None,
        }
    }
}

fn lenient_vec<T: DeserializeOwned>(values: Vec<Value>) -> Vec<T> {
    values
        .into_iter()
        .filter(|v| !v.is_null())
        .filter_map(|v| match serde_json::from_value::<T>(v) {
            Ok(t) => Some(t),
            Err(e) => {
                if crate::brand::env_var("DEBUG").is_some() {
                    eprintln!("decode: skipped node: {e}");
                }
                None
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Nodes<T> {
    pub nodes: Vec<T>,
    pub total_count: Option<i64>,
}

impl<T> Default for Nodes<T> {
    fn default() -> Self {
        Nodes { nodes: Vec::new(), total_count: None }
    }
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Nodes<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            #[serde(default)]
            nodes: Option<Vec<Value>>,
            #[serde(default)]
            total_count: Option<i64>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(Nodes { nodes: lenient_vec(raw.nodes.unwrap_or_default()), total_count: raw.total_count })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub has_next_page: bool,
    #[serde(default)]
    pub end_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Count {
    #[serde(default)]
    pub total_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IdOnly {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Login {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Actor {
    pub login: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct LabelInfo {
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Branch {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoName {
    pub name_with_owner: String,
    #[serde(default)]
    pub default_branch_ref: Option<Branch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualState {
    Open,
    Draft,
    Merged,
    Closed,
    IssueOpen,
    Completed,
    NotPlanned,
}

impl VisualState {
    pub fn pr(state: &str, draft: bool) -> Self {
        match state {
            "MERGED" => VisualState::Merged,
            "CLOSED" => VisualState::Closed,
            _ => {
                if draft {
                    VisualState::Draft
                } else {
                    VisualState::Open
                }
            }
        }
    }

    pub fn issue(state: &str, reason: Option<&str>) -> Self {
        if state != "CLOSED" {
            return VisualState::IssueOpen;
        }
        if reason == Some("NOT_PLANNED") {
            VisualState::NotPlanned
        } else {
            VisualState::Completed
        }
    }

    pub fn css(&self) -> &'static str {
        match self {
            VisualState::Open | VisualState::IssueOpen => "green",
            VisualState::Draft | VisualState::NotPlanned => "slate",
            VisualState::Merged | VisualState::Completed => "violet",
            VisualState::Closed => "red",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            VisualState::Open => "sy-pr-symbolic",
            VisualState::Draft => "sy-pr-draft-symbolic",
            VisualState::Merged => "sy-merge-symbolic",
            VisualState::Closed => "sy-pr-closed-symbolic",
            VisualState::IssueOpen => "sy-issue-symbolic",
            VisualState::Completed => "sy-issue-done-symbolic",
            VisualState::NotPlanned => "sy-issue-skip-symbolic",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            VisualState::Open | VisualState::IssueOpen => "Open",
            VisualState::Draft => "Draft",
            VisualState::Merged => "Merged",
            VisualState::Closed => "Closed",
            VisualState::Completed => "Completed",
            VisualState::NotPlanned => "Not planned",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StackPr {
    pub number: i64,
    pub title: String,
    pub state: String,
    pub is_draft: bool,
    pub head_ref_name: String,
    pub base_ref_name: String,
}

impl StackPr {
    pub fn visual_state(&self) -> VisualState {
        VisualState::pr(&self.state, self.is_draft)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Viewer {
    pub login: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RestUser {
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RollupState {
    pub state: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RollupCommit {
    #[serde(default)]
    pub status_check_rollup: Option<RollupState>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RollupCommitNode {
    pub commit: RollupCommit,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub pr_state: Option<String>,
    #[serde(default)]
    pub issue_state: Option<String>,
    #[serde(default)]
    pub state_reason: Option<String>,
    #[serde(default)]
    pub is_draft: Option<bool>,
    pub created_at: Date,
    pub updated_at: Date,
    #[serde(default)]
    pub base_ref_name: Option<String>,
    #[serde(default)]
    pub author: Option<Actor>,
    pub repository: RepoName,
    #[serde(default)]
    pub review_decision: Option<String>,
    #[serde(default)]
    pub additions: Option<i64>,
    #[serde(default)]
    pub deletions: Option<i64>,
    #[serde(default)]
    pub comments: Count,
    #[serde(default)]
    pub labels: Option<Nodes<LabelInfo>>,
    #[serde(default)]
    pub commits: Option<Nodes<RollupCommitNode>>,
}

impl SearchItem {
    pub fn id(&self) -> &str {
        &self.url
    }

    pub fn is_pr(&self) -> bool {
        self.typename == "PullRequest"
    }

    pub fn repo(&self) -> RepoRef {
        RepoRef::from_full(&self.repository.name_with_owner).unwrap_or_else(|| RepoRef::new("?", "?"))
    }

    pub fn item_ref(&self) -> ItemRef {
        ItemRef { repo: self.repo(), number: self.number }
    }

    pub fn route(&self) -> Route {
        if self.is_pr() {
            Route::Pr(self.item_ref())
        } else {
            Route::Issue(self.item_ref())
        }
    }

    pub fn ci_state(&self) -> Option<String> {
        self.commits.as_ref()?.nodes.first()?.commit.status_check_rollup.as_ref().map(|s| s.state.clone())
    }

    pub fn stacked_on(&self) -> Option<String> {
        if !self.is_pr() {
            return None;
        }
        let base = self.base_ref_name.clone()?;
        let main = self.repository.default_branch_ref.as_ref()?.name.clone();
        if base != main {
            Some(base)
        } else {
            None
        }
    }

    pub fn visual_state(&self) -> VisualState {
        if self.is_pr() {
            VisualState::pr(self.pr_state.as_deref().unwrap_or("OPEN"), self.is_draft.unwrap_or(false))
        } else {
            VisualState::issue(self.issue_state.as_deref().unwrap_or("OPEN"), self.state_reason.as_deref())
        }
    }

    pub fn labels(&self) -> Vec<LabelInfo> {
        self.labels.as_ref().map(|l| l.nodes.clone()).unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct SearchConnection {
    pub issue_count: i64,
    pub page_info: Option<PageInfo>,
    pub nodes: Vec<SearchItem>,
}

impl<'de> Deserialize<'de> for SearchConnection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            issue_count: i64,
            #[serde(default)]
            page_info: Option<PageInfo>,
            #[serde(default)]
            nodes: Option<Vec<Value>>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(SearchConnection { issue_count: raw.issue_count, page_info: raw.page_info, nodes: lenient_vec(raw.nodes.unwrap_or_default()) })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub search: SearchConnection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BadgeResponse {
    pub rr: SearchConnection,
    pub mine: SearchConnection,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Reviewer {
    #[serde(rename = "__typename", default)]
    pub typename: Option<String>,
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

impl Reviewer {
    pub fn display(&self) -> String {
        self.login.clone().or_else(|| self.name.clone()).unwrap_or_else(|| "?".into())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRequest {
    #[serde(default)]
    pub requested_reviewer: Option<Reviewer>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReviewCommit {
    pub oid: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatestReview {
    pub state: String,
    #[serde(default)]
    pub submitted_at: Option<Date>,
    #[serde(default)]
    pub commit: Option<ReviewCommit>,
    #[serde(default)]
    pub author: Option<Actor>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowInfo {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowRun {
    #[serde(default)]
    pub database_id: Option<i64>,
    #[serde(default)]
    pub workflow: Option<WorkflowInfo>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSuite {
    #[serde(default)]
    pub workflow_run: Option<WorkflowRun>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckOutcome {
    Failure = 0,
    Pending = 1,
    Neutral = 2,
    Skipped = 3,
    Success = 4,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckContext {
    #[serde(rename = "__typename")]
    pub typename: String,
    #[serde(default)]
    pub database_id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub details_url: Option<String>,
    #[serde(default)]
    pub started_at: Option<Date>,
    #[serde(default)]
    pub completed_at: Option<Date>,
    #[serde(default)]
    pub check_suite: Option<CheckSuite>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub target_url: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

impl CheckContext {
    pub fn id(&self) -> String {
        format!(
            "{}-{}",
            self.typename,
            self.database_id.map(|d| d.to_string()).or_else(|| self.context.clone()).or_else(|| self.name.clone()).unwrap_or_default()
        )
    }

    pub fn title(&self) -> String {
        self.name.clone().or_else(|| self.context.clone()).unwrap_or_else(|| "?".into())
    }

    pub fn workflow_name(&self) -> Option<String> {
        self.check_suite.as_ref()?.workflow_run.as_ref()?.workflow.as_ref().map(|w| w.name.clone())
    }

    pub fn run_id(&self) -> Option<i64> {
        self.check_suite.as_ref()?.workflow_run.as_ref()?.database_id
    }

    pub fn link(&self) -> Option<String> {
        self.details_url.clone().or_else(|| self.target_url.clone())
    }

    pub fn is_actions_job(&self) -> bool {
        self.typename == "CheckRun" && self.run_id().is_some() && self.database_id.is_some()
    }

    pub fn outcome(&self) -> CheckOutcome {
        if self.typename == "StatusContext" {
            return match self.state.as_deref() {
                Some("SUCCESS") => CheckOutcome::Success,
                Some("FAILURE") | Some("ERROR") => CheckOutcome::Failure,
                _ => CheckOutcome::Pending,
            };
        }
        if self.status.as_deref() != Some("COMPLETED") {
            return CheckOutcome::Pending;
        }
        match self.conclusion.as_deref() {
            Some("SUCCESS") => CheckOutcome::Success,
            Some("FAILURE") | Some("TIMED_OUT") | Some("ACTION_REQUIRED") | Some("STARTUP_FAILURE") => CheckOutcome::Failure,
            Some("SKIPPED") => CheckOutcome::Skipped,
            _ => CheckOutcome::Neutral,
        }
    }

    pub fn duration(&self) -> Option<String> {
        let secs = (self.completed_at? - self.started_at?).num_seconds();
        Some(if secs >= 60 { format!("{}m {}s", secs / 60, secs % 60) } else { format!("{secs}s") })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StatusRollup {
    pub state: String,
    #[serde(default)]
    pub contexts: Option<Nodes<CheckContext>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCommit {
    pub oid: String,
    #[serde(default)]
    pub committed_date: Option<Date>,
    #[serde(default)]
    pub status_check_rollup: Option<StatusRollup>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LastCommitNode {
    pub commit: LastCommit,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitActor {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub user: Option<Login>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub oid: String,
    pub abbreviated_oid: String,
    pub message_headline: String,
    pub committed_date: Date,
    pub url: String,
    #[serde(default)]
    pub author: Option<GitActor>,
    #[serde(default)]
    pub status_check_rollup: Option<RollupState>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitInfoNode {
    pub commit: CommitInfo,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewed {
    pub path: String,
    pub viewer_viewed_state: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct XRefSource {
    #[serde(rename = "__typename", default)]
    pub typename: Option<String>,
    #[serde(default)]
    pub number: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub repository: Option<RepoName>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineCommit {
    #[serde(default)]
    pub oid: Option<String>,
    #[serde(default)]
    pub abbreviated_oid: Option<String>,
    #[serde(default)]
    pub message_headline: Option<String>,
    #[serde(default)]
    pub committed_date: Option<Date>,
    #[serde(default)]
    pub author: Option<GitActor>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineItem {
    #[serde(rename = "__typename")]
    pub typename: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub author: Option<Actor>,
    #[serde(default)]
    pub actor: Option<Actor>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default, rename = "bodyHTML")]
    pub body_html: Option<String>,
    #[serde(default)]
    pub created_at: Option<Date>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub label: Option<LabelInfo>,
    #[serde(default)]
    pub source: Option<XRefSource>,
    #[serde(default)]
    pub previous_title: Option<String>,
    #[serde(default)]
    pub current_title: Option<String>,
    #[serde(default)]
    pub commit: Option<TimelineCommit>,
    #[serde(default)]
    pub merge_ref_name: Option<String>,
    #[serde(default)]
    pub requested_reviewer: Option<Reviewer>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadComment {
    pub id: String,
    #[serde(default)]
    pub author: Option<Actor>,
    pub body: String,
    #[serde(rename = "bodyHTML")]
    pub body_html: String,
    pub created_at: Date,
    #[serde(default)]
    pub diff_hunk: Option<String>,
    #[serde(default)]
    pub pull_request_review: Option<IdOnly>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewThread {
    pub id: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub path: String,
    #[serde(default)]
    pub line: Option<i64>,
    #[serde(default)]
    pub original_line: Option<i64>,
    pub diff_side: String,
    pub viewer_can_resolve: bool,
    pub viewer_can_unresolve: bool,
    pub viewer_can_reply: bool,
    pub comments: Nodes<ThreadComment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoMergeRequest {
    #[serde(default)]
    pub enabled_at: Option<Date>,
    #[serde(default)]
    pub merge_method: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestDetail {
    pub id: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub is_draft: bool,
    pub merged: bool,
    pub mergeable: String,
    #[serde(default)]
    pub merge_state_status: Option<String>,
    pub body: String,
    #[serde(rename = "bodyHTML")]
    pub body_html: String,
    pub created_at: Date,
    #[serde(default)]
    pub author: Option<Actor>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub head_ref_oid: String,
    #[serde(default)]
    pub head_repository: Option<RepoName>,
    #[serde(default)]
    pub head_ref: Option<Branch>,
    pub additions: i64,
    pub deletions: i64,
    pub changed_files: i64,
    #[serde(default)]
    pub review_decision: Option<String>,
    pub viewer_can_update: bool,
    pub viewer_did_author: bool,
    #[serde(default)]
    pub auto_merge_request: Option<AutoMergeRequest>,
    #[serde(default)]
    pub labels: Nodes<LabelInfo>,
    #[serde(default)]
    pub assignees: Nodes<Actor>,
    #[serde(default)]
    pub review_requests: Nodes<ReviewRequest>,
    #[serde(default)]
    pub latest_opinionated_reviews: Nodes<LatestReview>,
    #[serde(default)]
    pub last_commit: Nodes<LastCommitNode>,
    #[serde(default)]
    pub all_commits: Nodes<CommitInfoNode>,
    #[serde(default)]
    pub files: Option<Nodes<FileViewed>>,
    #[serde(default)]
    pub timeline_items: Nodes<TimelineItem>,
    #[serde(default)]
    pub review_threads: Nodes<ReviewThread>,
    #[serde(flatten)]
    pub light: PrLight,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct StateCount {
    pub state: String,
    pub count: i64,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LightCheck {
    #[serde(rename = "__typename", default)]
    pub typename: String,
    #[serde(default)]
    pub database_id: Option<i64>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub completed_at: Option<Date>,
    #[serde(default)]
    pub context: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
}

impl LightCheck {
    fn sort_key(&self) -> (String, String, i64) {
        (self.typename.clone(), self.name.clone().or_else(|| self.context.clone()).unwrap_or_default(), self.database_id.unwrap_or(0))
    }
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LightThread {
    #[serde(default)]
    pub is_resolved: bool,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LightContexts {
    #[serde(default)]
    pub total_count: i64,
    #[serde(default)]
    pub check_run_counts_by_state: Option<Vec<StateCount>>,
    #[serde(default)]
    pub status_context_counts_by_state: Option<Vec<StateCount>>,
    #[serde(default)]
    pub nodes: Option<Vec<LightCheck>>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
pub struct LightRollup {
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub contexts: Option<LightContexts>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LightCommit {
    #[serde(default)]
    pub oid: String,
    #[serde(default)]
    pub status_check_rollup: Option<LightRollup>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LightCommitNode {
    pub commit: LightCommit,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PrLight {
    #[serde(default)]
    pub light_updated_at: Option<Date>,
    #[serde(default)]
    pub light_head_oid: Option<String>,
    #[serde(default)]
    pub light_state: Option<String>,
    #[serde(default)]
    pub light_draft: Option<bool>,
    #[serde(default)]
    pub light_mergeable: Option<String>,
    #[serde(default)]
    pub light_merge_state: Option<String>,
    #[serde(default)]
    pub light_decision: Option<String>,
    #[serde(default)]
    pub light_comments: Option<Count>,
    #[serde(default)]
    pub light_reviews: Option<Count>,
    #[serde(default)]
    pub light_threads: Option<LightThreads>,
    #[serde(default)]
    pub light_commits: Option<Nodes<LightCommitNode>>,
}

#[derive(Debug, Clone, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LightThreads {
    #[serde(default)]
    pub total_count: i64,
    #[serde(default)]
    pub nodes: Option<Vec<LightThread>>,
}

impl PrLight {
    pub fn normalized(mut self) -> Self {
        let commits = self.light_commits.iter_mut().flat_map(|c| c.nodes.iter_mut());
        for node in commits {
            let Some(contexts) = node.commit.status_check_rollup.as_mut().and_then(|r| r.contexts.as_mut()) else { continue };
            for counts in [contexts.check_run_counts_by_state.as_mut(), contexts.status_context_counts_by_state.as_mut()].into_iter().flatten() {
                counts.retain(|c| c.count > 0);
                counts.sort_by(|a, b| a.state.cmp(&b.state));
            }
            if let Some(checks) = contexts.nodes.as_mut() {
                checks.sort_by_key(|c| c.sort_key());
            }
        }
        self
    }

    pub fn is_known(&self) -> bool {
        self.light_updated_at.is_some() && self.light_head_oid.is_some()
    }

    pub fn merge_unknown(&self) -> bool {
        self.light_state.as_deref() == Some("OPEN") && (self.light_merge_state.as_deref() == Some("UNKNOWN") || self.light_mergeable.as_deref() == Some("UNKNOWN"))
    }
}

impl PullRequestDetail {
    pub fn visual_state(&self) -> VisualState {
        VisualState::pr(&self.state, self.is_draft)
    }

    pub fn rollup(&self) -> Option<&StatusRollup> {
        self.last_commit.nodes.first()?.commit.status_check_rollup.as_ref()
    }

    pub fn checks(&self) -> Vec<CheckContext> {
        self.rollup().and_then(|r| r.contexts.as_ref()).map(|c| c.nodes.clone()).unwrap_or_default()
    }

    pub fn is_open(&self) -> bool {
        self.state == "OPEN"
    }

    pub fn last_commit_date(&self) -> Option<Date> {
        self.last_commit.nodes.first()?.commit.committed_date
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrRepository {
    #[serde(default)]
    pub viewer_permission: Option<String>,
    pub merge_commit_allowed: bool,
    pub squash_merge_allowed: bool,
    pub rebase_merge_allowed: bool,
    pub delete_branch_on_merge: bool,
    pub auto_merge_allowed: bool,
    pub viewer_default_merge_method: String,
    #[serde(default)]
    pub pull_request: Option<PullRequestDetail>,
}

impl PrRepository {
    pub fn can_write(&self) -> bool {
        matches!(self.viewer_permission.as_deref(), Some("ADMIN") | Some("MAINTAIN") | Some("WRITE"))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrDetailResponse {
    pub viewer: Viewer,
    #[serde(default)]
    pub repository: Option<PrRepository>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    pub id: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub issue_state: String,
    #[serde(default)]
    pub state_reason: Option<String>,
    #[serde(rename = "bodyHTML")]
    pub body_html: String,
    pub created_at: Date,
    pub viewer_can_update: bool,
    #[serde(default)]
    pub author: Option<Actor>,
    #[serde(default)]
    pub labels: Nodes<LabelInfo>,
    #[serde(default)]
    pub assignees: Nodes<Actor>,
    #[serde(default)]
    pub comments: Count,
    #[serde(default)]
    pub timeline_items: Nodes<TimelineItem>,
}

impl IssueDetail {
    pub fn visual_state(&self) -> VisualState {
        VisualState::issue(&self.issue_state, self.state_reason.as_deref())
    }
}

#[derive(Debug, Clone)]
pub struct IssueOrPr {
    pub typename: String,
    pub issue: Option<IssueDetail>,
}

impl<'de> Deserialize<'de> for IssueOrPr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Value::deserialize(d)?;
        let typename = v.get("__typename").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let issue = if typename == "Issue" { Some(serde_json::from_value::<IssueDetail>(v).map_err(serde::de::Error::custom)?) } else { None };
        Ok(IssueOrPr { typename, issue })
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueRepo {
    #[serde(default)]
    pub issue_or_pull_request: Option<IssueOrPr>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssueDetailResponse {
    pub viewer: Viewer,
    #[serde(default)]
    pub repository: Option<IssueRepo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PrFile {
    pub filename: String,
    pub status: String,
    #[serde(default)]
    pub additions: i64,
    #[serde(default)]
    pub deletions: i64,
    #[serde(default)]
    pub patch: Option<String>,
    #[serde(default)]
    pub previous_filename: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitPerson {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub date: Option<Date>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitInner {
    pub message: String,
    #[serde(default)]
    pub author: Option<CommitPerson>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitStats {
    pub additions: i64,
    pub deletions: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommitDetailRest {
    pub sha: String,
    pub html_url: String,
    pub commit: CommitInner,
    #[serde(default)]
    pub author: Option<RestUser>,
    #[serde(default)]
    pub stats: Option<CommitStats>,
    #[serde(default)]
    pub files: Option<Vec<PrFile>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Language {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSummary {
    pub name_with_owner: String,
    #[serde(default)]
    pub description: Option<String>,
    pub is_private: bool,
    pub is_fork: bool,
    pub is_archived: bool,
    pub stargazer_count: i64,
    #[serde(default)]
    pub pushed_at: Option<Date>,
    pub url: String,
    #[serde(default)]
    pub primary_language: Option<Language>,
    #[serde(default)]
    pub pull_requests: Count,
    #[serde(default)]
    pub issues: Count,
}

impl RepoSummary {
    pub fn repo(&self) -> Option<RepoRef> {
        RepoRef::from_full(&self.name_with_owner)
    }
}

#[derive(Debug, Clone)]
pub struct RepoConnection {
    pub total_count: i64,
    pub page_info: PageInfo,
    pub nodes: Vec<RepoSummary>,
}

impl<'de> Deserialize<'de> for RepoConnection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            total_count: i64,
            page_info: PageInfo,
            #[serde(default)]
            nodes: Option<Vec<Value>>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(RepoConnection { total_count: raw.total_count, page_info: raw.page_info, nodes: lenient_vec(raw.nodes.unwrap_or_default()) })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReposViewer {
    pub repositories: RepoConnection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReposResponse {
    pub viewer: ReposViewer,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub name_with_owner: String,
    #[serde(default)]
    pub description: Option<String>,
    pub url: String,
    pub is_private: bool,
    pub stargazer_count: i64,
    pub fork_count: i64,
    #[serde(default)]
    pub default_branch_ref: Option<Branch>,
    #[serde(default)]
    pub pull_requests: Count,
    #[serde(default)]
    pub issues: Count,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoInfoResponse {
    #[serde(default)]
    pub repository: Option<RepoInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectOwner {
    #[serde(default)]
    pub login: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub number: i64,
    pub title: String,
    pub url: String,
    pub closed: bool,
    pub updated_at: Date,
    #[serde(default)]
    pub short_description: Option<String>,
    #[serde(default)]
    pub items: Count,
    #[serde(default)]
    pub owner: Option<ProjectOwner>,
}

impl ProjectSummary {
    pub fn project_ref(&self) -> ProjectRef {
        ProjectRef { id: self.id.clone(), title: self.title.clone(), url: self.url.clone() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsOrg {
    pub login: String,
    pub projects_v2: Nodes<ProjectSummary>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsViewer {
    pub projects_v2: Nodes<ProjectSummary>,
    pub organizations: Nodes<ProjectsOrg>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectsResponse {
    pub viewer: ProjectsViewer,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ProjectOption {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectField {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub options: Option<Vec<ProjectOption>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldRef {
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFieldValue {
    #[serde(rename = "__typename")]
    pub typename: String,
    #[serde(default)]
    pub option_id: Option<String>,
    #[serde(default)]
    pub field: Option<FieldRef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContent {
    #[serde(rename = "__typename")]
    pub typename: String,
    #[serde(default)]
    pub number: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub issue_state: Option<String>,
    #[serde(default)]
    pub pr_state: Option<String>,
    #[serde(default)]
    pub is_draft: Option<bool>,
    #[serde(default)]
    pub repository: Option<RepoName>,
    #[serde(default)]
    pub assignees: Option<Nodes<Actor>>,
}

impl ProjectContent {
    pub fn route(&self) -> Option<Route> {
        let number = self.number?;
        let repo = RepoRef::from_full(&self.repository.as_ref()?.name_with_owner)?;
        let r = ItemRef { repo, number };
        Some(if self.typename == "PullRequest" { Route::Pr(r) } else { Route::Issue(r) })
    }

    pub fn visual_state(&self) -> Option<VisualState> {
        match self.typename.as_str() {
            "PullRequest" => Some(VisualState::pr(self.pr_state.as_deref().unwrap_or("OPEN"), self.is_draft.unwrap_or(false))),
            "Issue" => Some(VisualState::issue(self.issue_state.as_deref().unwrap_or("OPEN"), None)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectItem {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub is_archived: bool,
    #[serde(default)]
    pub field_values: Nodes<ProjectFieldValue>,
    #[serde(default)]
    pub content: Option<ProjectContent>,
}

impl ProjectItem {
    pub fn option_id(&self, field_id: &str) -> Option<String> {
        self.field_values.nodes.iter().find(|v| v.field.as_ref().and_then(|f| f.id.as_deref()) == Some(field_id) && v.option_id.is_some()).and_then(|v| v.option_id.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ProjectItemsConnection {
    pub page_info: PageInfo,
    pub nodes: Vec<ProjectItem>,
}

impl<'de> Deserialize<'de> for ProjectItemsConnection {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            page_info: PageInfo,
            #[serde(default)]
            nodes: Option<Vec<Value>>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(ProjectItemsConnection { page_info: raw.page_info, nodes: lenient_vec(raw.nodes.unwrap_or_default()) })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectDetail {
    pub id: String,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub fields: Nodes<ProjectField>,
    pub items: ProjectItemsConnection,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectDetailResponse {
    #[serde(default)]
    pub node: Option<ProjectDetail>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationSubject {
    pub title: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationOwner {
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationRepo {
    pub full_name: String,
    #[serde(default)]
    pub owner: Option<NotificationOwner>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationThread {
    pub id: String,
    pub unread: bool,
    pub reason: String,
    pub updated_at: Date,
    pub subject: NotificationSubject,
    pub repository: NotificationRepo,
}

impl NotificationThread {
    pub fn route(&self) -> Option<Route> {
        self.subject.url.as_deref().and_then(|u| crate::util::parse_github_url(u, false))
    }

    pub fn web_url(&self) -> String {
        if let Some(r) = self.route() {
            match r {
                Route::Pr(r) => return format!("https://github.com/{}/pull/{}", r.repo.full(), r.number),
                Route::Issue(r) => return format!("https://github.com/{}/issues/{}", r.repo.full(), r.number),
                _ => {}
            }
        }
        let full = &self.repository.full_name;
        match self.subject.kind.as_str() {
            "Release" => format!("https://github.com/{full}/releases"),
            "CheckSuite" => format!("https://github.com/{full}/actions"),
            "Discussion" => format!("https://github.com/{full}/discussions"),
            _ => format!("https://github.com/{full}"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlobMeta {
    #[serde(default, rename = "byteSize")]
    pub byte_size: Option<i64>,
    #[serde(default, rename = "isBinary")]
    pub is_binary: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TreeEntry {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub object: Option<BlobMeta>,
}

impl TreeEntry {
    pub fn is_dir(&self) -> bool {
        self.kind == "tree"
    }

    pub fn is_submodule(&self) -> bool {
        self.kind == "commit"
    }

    pub fn size(&self) -> Option<i64> {
        self.object.as_ref().and_then(|o| o.byte_size)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitObject {
    #[serde(rename = "__typename")]
    pub typename: String,
    #[serde(default)]
    pub entries: Option<Vec<TreeEntry>>,
    #[serde(default)]
    pub byte_size: Option<i64>,
    #[serde(default)]
    pub is_binary: Option<bool>,
    #[serde(default)]
    pub is_truncated: Option<bool>,
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameUser {
    pub login: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlameAuthor {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub user: Option<BlameUser>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameCommit {
    pub oid: String,
    pub abbreviated_oid: String,
    #[serde(default)]
    pub message_headline: String,
    #[serde(default)]
    pub committed_date: Option<Date>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub author: Option<BlameAuthor>,
}

impl BlameCommit {
    pub fn author_name(&self) -> String {
        let a = self.author.as_ref();
        a.and_then(|a| a.user.as_ref().map(|u| u.login.clone()))
            .or_else(|| a.and_then(|a| a.name.clone()))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "unknown".into())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlameRange {
    pub starting_line: i64,
    pub ending_line: i64,
    #[serde(default)]
    pub age: i64,
    pub commit: BlameCommit,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlameRanges {
    #[serde(default)]
    pub ranges: Vec<BlameRange>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlameObject {
    #[serde(default)]
    pub blame: Option<BlameRanges>,
    #[serde(default)]
    pub target: Option<Box<BlameObject>>,
}

impl BlameObject {
    pub fn ranges(self) -> Option<Vec<BlameRange>> {
        match self.blame {
            Some(b) => Some(b.ranges),
            None => self.target.and_then(|t| t.blame).map(|b| b.ranges),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlameRepo {
    #[serde(default)]
    pub object: Option<BlameObject>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BlameResponse {
    #[serde(default)]
    pub repository: Option<BlameRepo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitObjectRepo {
    #[serde(default)]
    pub object: Option<GitObject>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitObjectResponse {
    #[serde(default)]
    pub repository: Option<GitObjectRepo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BranchCommit {
    pub sha: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BranchInfo {
    pub name: String,
    pub commit: BranchCommit,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitTreeItem {
    pub path: String,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitTreeRest {
    pub tree: Vec<GitTreeItem>,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct PendingComment {
    pub id: String,
    pub path: String,
    pub line: i64,
    pub side: String,
    pub body: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_links() {
        let r = ItemRef { repo: RepoRef::new("octo", "hello-world"), number: 123 };
        assert_eq!(Route::Pr(r.clone()).short_link().as_deref(), Some("octo/hello-world#123"));
        assert_eq!(Route::Issue(r).short_link().as_deref(), Some("octo/hello-world#123"));
        assert_eq!(Route::Repo(RepoRef::new("octo", "x")).short_link(), None);
        assert_eq!(Route::Commit(RepoRef::new("octo", "x"), "abc".into()).short_link(), None);
        let from_url = crate::util::parse_github_url("https://github.com/Octo/Repo/pull/42/files", false).and_then(|r| r.short_link());
        assert_eq!(from_url.as_deref(), Some("Octo/Repo#42"));
        let api = crate::util::parse_github_url("https://api.github.com/repos/octo/repo/issues/7", false).and_then(|r| r.short_link());
        assert_eq!(api.as_deref(), Some("octo/repo#7"));
    }

    #[test]
    fn decodes_blame() {
        let json = r#"{"repository":{"object":{"__typename":"Commit","blame":{"ranges":[
            {"startingLine":1,"endingLine":2,"age":10,"commit":{"oid":"aaaa","abbreviatedOid":"aa","messageHeadline":"Init","committedDate":"2020-01-01T00:00:00Z","url":"u","author":{"name":"Ann","user":null}}},
            {"startingLine":3,"endingLine":3,"age":1,"commit":{"oid":"bbbb","abbreviatedOid":"bb","messageHeadline":"Fix","committedDate":"2026-01-01T00:00:00Z","url":"u","author":{"name":"Bob","user":{"login":"bob","avatarUrl":null}}}}
        ]}}}}"#;
        let r: BlameResponse = serde_json::from_str(json).unwrap();
        let ranges = r.repository.unwrap().object.unwrap().ranges().unwrap();
        assert_eq!(ranges.len(), 2);
        let tag = r#"{"repository":{"object":{"__typename":"Tag","target":{"__typename":"Commit","blame":{"ranges":[
            {"startingLine":1,"endingLine":1,"age":3,"commit":{"oid":"cccc","abbreviatedOid":"cc","messageHeadline":"Tagged","committedDate":null,"url":"u","author":null}}
        ]}}}}}"#;
        let r: BlameResponse = serde_json::from_str(tag).unwrap();
        let ranges = r.repository.unwrap().object.unwrap().ranges().unwrap();
        assert_eq!(ranges[0].commit.author_name(), "unknown");
        let tree: BlameResponse = serde_json::from_str(r#"{"repository":{"object":{"__typename":"Tree"}}}"#).unwrap();
        assert!(tree.repository.unwrap().object.unwrap().ranges().is_none());
        assert_eq!(ranges[0].commit.author_name(), "Ann");
        assert_eq!(ranges[1].commit.author_name(), "bob");
    }

    #[test]
    fn decodes_fixtures() {
        let r: PrDetailResponse = serde_json::from_str(crate::fixtures::PR_DETAIL).unwrap();
        let pr = r.repository.unwrap().pull_request.unwrap();
        assert_eq!(pr.checks().len(), 3);
        assert_eq!(pr.review_threads.nodes.len(), 2);
        assert_eq!(pr.timeline_items.nodes.len(), 6);
        assert_eq!(pr.all_commits.nodes.len(), 2);
        let files: Vec<PrFile> = serde_json::from_str(crate::fixtures::PR_FILES).unwrap();
        assert_eq!(files[1].previous_filename.as_deref(), Some("README.txt"));
        assert!(files[2].patch.is_none());
        let html = crate::conversation_html::render_pr(&pr, crate::conversation_html::Look { dark: true, brand: true });
        assert!(html.contains("requested changes"));
        assert!(crate::fix_prompt::latest(&pr).is_some());
    }

    #[test]
    fn decodes_search() {
        let json = r#"{"search":{"issueCount":2,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[
            {"__typename":"PullRequest","number":5,"title":"Fix","url":"https://github.com/a/b/pull/5","prState":"OPEN","isDraft":false,
             "createdAt":"2024-01-01T10:00:00Z","updatedAt":"2024-01-02T10:00:00.123Z","baseRefName":"feature","author":{"login":"x","avatarUrl":null},
             "repository":{"nameWithOwner":"a/b","defaultBranchRef":{"name":"main"}},"reviewDecision":null,"additions":3,"deletions":1,
             "comments":{"totalCount":2},"labels":{"nodes":[{"name":"bug","color":"ff0000"}]},"commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]}},
            null,
            {"__typename":"Issue","number":6,"title":"Bug","url":"https://github.com/a/b/issues/6","issueState":"CLOSED","stateReason":"NOT_PLANNED",
             "createdAt":"2024-01-01T10:00:00Z","updatedAt":"2024-01-02T10:00:00Z","repository":{"nameWithOwner":"a/b"},"comments":{"totalCount":0}}
        ]}}"#;
        let r: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(r.search.nodes.len(), 2);
        assert_eq!(r.search.nodes[0].stacked_on().as_deref(), Some("feature"));
        assert_eq!(r.search.nodes[0].ci_state().as_deref(), Some("SUCCESS"));
        assert_eq!(r.search.nodes[1].visual_state(), VisualState::NotPlanned);
        let again: Vec<SearchItem> = serde_json::from_value(serde_json::to_value(&r.search.nodes).unwrap()).unwrap();
        assert_eq!(again[0].stacked_on().as_deref(), Some("feature"));
        assert_eq!(again[0].ci_state().as_deref(), Some("SUCCESS"));
        assert_eq!(again[1].visual_state(), VisualState::NotPlanned);
    }

    #[test]
    fn light_signature_matches_full_detail() {
        let light = serde_json::json!({
            "lightUpdatedAt": "2026-10-16T10:37:24Z", "lightHeadOid": "abc", "lightState": "OPEN", "lightDraft": false,
            "lightMergeable": "UNKNOWN", "lightMergeState": "UNKNOWN", "lightDecision": null,
            "lightComments": { "totalCount": 1 }, "lightReviews": { "totalCount": 2 },
            "lightThreads": { "totalCount": 2, "nodes": [{ "isResolved": false }, { "isResolved": true }] },
            "lightCommits": { "totalCount": 2, "nodes": [{ "commit": { "oid": "abc", "statusCheckRollup": { "state": "PENDING", "contexts": {
                "totalCount": 2,
                "checkRunCountsByState": [{ "state": "SUCCESS", "count": 1 }, { "state": "IN_PROGRESS", "count": 1 }, { "state": "FAILURE", "count": 0 }],
                "statusContextCountsByState": [],
                "nodes": [
                    { "__typename": "CheckRun", "databaseId": 2, "name": "test", "status": "IN_PROGRESS", "conclusion": null, "completedAt": null },
                    { "__typename": "CheckRun", "databaseId": 1, "name": "build", "status": "COMPLETED", "conclusion": "SUCCESS", "completedAt": "2026-10-16T10:40:00Z" }
                ] } } } }] }
        });
        let mut full: serde_json::Value = serde_json::from_str(crate::fixtures::PR_DETAIL).unwrap();
        let pr = full.pointer_mut("/repository/pullRequest").unwrap().as_object_mut().unwrap();
        for (k, v) in light.as_object().unwrap() {
            pr.insert(k.clone(), v.clone());
        }
        let r: PrDetailResponse = serde_json::from_value(full).unwrap();
        let detail = r.repository.unwrap().pull_request.unwrap();
        assert_eq!(detail.review_threads.nodes.len(), 2);
        let alone: PrLight = serde_json::from_value(light.clone()).unwrap();
        assert!(alone.is_known());
        assert!(alone.merge_unknown());
        assert_eq!(detail.light.clone().normalized(), alone.clone().normalized());

        let mut reordered = light.clone();
        let contexts = &mut reordered["lightCommits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"];
        contexts["checkRunCountsByState"] = serde_json::json!([{ "state": "IN_PROGRESS", "count": 1 }, { "state": "SUCCESS", "count": 1 }]);
        let nodes = contexts["nodes"].as_array_mut().unwrap();
        nodes.reverse();
        let reordered: PrLight = serde_json::from_value(reordered).unwrap();
        assert_eq!(reordered.normalized(), alone.clone().normalized());

        let mut resolved = light.clone();
        resolved["lightThreads"]["nodes"][0]["isResolved"] = serde_json::json!(true);
        let resolved: PrLight = serde_json::from_value(resolved).unwrap();
        assert_ne!(resolved.normalized(), alone.clone().normalized());

        let mut rerun = light.clone();
        rerun["lightCommits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"][1]["completedAt"] = serde_json::json!("2026-10-16T11:00:00Z");
        let rerun: PrLight = serde_json::from_value(rerun).unwrap();
        assert_ne!(rerun.normalized(), alone.normalized());

        let plain: PrDetailResponse = serde_json::from_str(crate::fixtures::PR_DETAIL).unwrap();
        assert!(!plain.repository.unwrap().pull_request.unwrap().light.is_known());
    }
}
