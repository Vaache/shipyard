use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::{issue, lenient, newest_first, state_matches, FlexLabel, ForgeCapabilities, ForgeClient, ForgeItem, ForgeNotification, ForgeRepo, ForgeUser, Query};
use crate::api::models::{Actor, Date, SearchItem};
use crate::api::ApiResult;
use crate::app::{IssueFilter, PrFilter};
use crate::ui::lists::StateFilter;

#[derive(Deserialize)]
struct User {
    id: i64,
    username: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct Author {
    username: String,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct References {
    #[serde(default)]
    full: Option<String>,
}

#[derive(Deserialize)]
struct MergeRequest {
    iid: i64,
    title: String,
    web_url: String,
    state: String,
    #[serde(default)]
    draft: Option<bool>,
    #[serde(default)]
    work_in_progress: Option<bool>,
    created_at: Date,
    updated_at: Date,
    #[serde(default)]
    target_branch: Option<String>,
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    references: Option<References>,
    #[serde(default)]
    user_notes_count: Option<i64>,
    #[serde(default)]
    labels: Option<Vec<FlexLabel>>,
}

#[derive(Deserialize)]
struct Issue {
    iid: i64,
    title: String,
    web_url: String,
    state: String,
    created_at: Date,
    updated_at: Date,
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    references: Option<References>,
    #[serde(default)]
    user_notes_count: Option<i64>,
    #[serde(default)]
    labels: Option<Vec<FlexLabel>>,
}

#[derive(Deserialize)]
struct TodoProject {
    #[serde(default)]
    path_with_namespace: Option<String>,
}

#[derive(Deserialize)]
struct TodoTargetInfo {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    web_url: Option<String>,
}

#[derive(Deserialize)]
struct Todo {
    id: i64,
    action_name: String,
    target_type: String,
    #[serde(default)]
    target_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
    state: String,
    created_at: Date,
    #[serde(default)]
    author: Option<Author>,
    #[serde(default)]
    project: Option<TodoProject>,
    #[serde(default, deserialize_with = "lenient")]
    target: Option<TodoTargetInfo>,
}

#[derive(Deserialize)]
#[serde(bound = "T: DeserializeOwned")]
struct TodoTarget<T> {
    #[serde(default, deserialize_with = "lenient")]
    target: Option<T>,
}

#[derive(Deserialize)]
struct Project {
    path_with_namespace: String,
    #[serde(default)]
    description: Option<String>,
    web_url: String,
    #[serde(default)]
    visibility: Option<String>,
    #[serde(default)]
    last_activity_at: Option<Date>,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    star_count: Option<i64>,
}

pub(super) async fn current_user(c: &ForgeClient) -> ApiResult<ForgeUser> {
    let u: User = c.http().get("/user", &[]).await?;
    Ok(ForgeUser { login: u.username, name: u.name.filter(|n| !n.is_empty()), avatar_url: u.avatar_url, id: Some(u.id.to_string()) })
}

pub(super) async fn capabilities(c: &ForgeClient) -> ForgeCapabilities {
    let mut pull_filters = vec![PrFilter::ReviewRequested, PrFilter::Mine, PrFilter::Assigned, PrFilter::Mentioned];
    if approval_filter_available(c).await {
        pull_filters.push(PrFilter::Reviewed);
    }
    ForgeCapabilities { pull_filters, issue_filters: IssueFilter::ALL.to_vec(), inbox_title: Some("To-Do List"), repositories: true }
}

fn list_query() -> Query {
    vec![("order_by", "updated_at".into()), ("sort", "desc".into()), ("per_page", "50".into()), ("with_labels_details", "true".into())]
}

fn states(state: StateFilter, closed: &'static [&'static str]) -> &'static [&'static str] {
    match state {
        StateFilter::Open => &["opened"],
        StateFilter::Closed => closed,
        StateFilter::All => &["all"],
    }
}

async fn approval_filter_available(c: &ForgeClient) -> bool {
    let known = c.state().approval_filter;
    if let Some(known) = known {
        return known;
    }
    let probe = format!("pullyard-probe-{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
    let query: Query = vec![("scope", "all".into()), ("state", "all".into()), ("per_page", "1".into()), ("approved_by_usernames[]", probe)];
    match c.http().get::<Vec<serde_json::Value>>("/merge_requests", &query).await {
        Ok(page) => {
            let works = page.is_empty();
            c.state().approval_filter = Some(works);
            works
        }
        Err(_) => false,
    }
}

async fn mention_todos<T: DeserializeOwned>(c: &ForgeClient, kind: &str) -> ApiResult<Vec<T>> {
    let mut all = vec![];
    for action in ["mentioned", "directly_addressed"] {
        for state in ["pending", "done"] {
            let query: Query = vec![("type", kind.to_string()), ("action", action.into()), ("state", state.into()), ("per_page", "50".into())];
            let page: Vec<TodoTarget<T>> = c.http().get("/todos", &query).await?;
            all.extend(page.into_iter().filter_map(|t| t.target));
        }
    }
    Ok(all)
}

pub(super) async fn pull_requests(c: &ForgeClient, filter: PrFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    if filter == PrFilter::Mentioned {
        let mrs: Vec<MergeRequest> = mention_todos(c, "MergeRequest").await?;
        return Ok(newest_first(mrs.into_iter().map(item).filter(|i| state_matches(state, i)).collect()));
    }
    let me = c.login().await?;
    let mut base = list_query();
    match filter {
        PrFilter::ReviewRequested => {
            base.push(("scope", "all".into()));
            base.push(("reviewer_username", me.clone()));
        }
        PrFilter::Mine => base.push(("scope", "created_by_me".into())),
        PrFilter::Assigned => base.push(("scope", "assigned_to_me".into())),
        PrFilter::Reviewed => {
            if !approval_filter_available(c).await {
                return Ok(vec![]);
            }
            base.push(("scope", "all".into()));
            base.push(("approved_by_usernames[]", me.clone()));
        }
        PrFilter::Mentioned => return Ok(vec![]),
    }
    let mut items = vec![];
    for s in states(state, &["merged", "closed"]) {
        let mut query = base.clone();
        query.push(("state", s.to_string()));
        let page: Vec<MergeRequest> = c.http().get("/merge_requests", &query).await?;
        items.extend(page.into_iter().map(item));
    }
    let me = me.to_lowercase();
    Ok(newest_first(items.into_iter().filter(|i| filter != PrFilter::Reviewed || i.author.as_ref().is_none_or(|a| a.login.to_lowercase() != me)).collect()))
}

pub(super) async fn issues(c: &ForgeClient, filter: IssueFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    if filter == IssueFilter::Mentioned {
        let found: Vec<Issue> = mention_todos(c, "Issue").await?;
        return Ok(newest_first(found.into_iter().map(issue_item).filter(|i| state_matches(state, i)).collect()));
    }
    let scope = if filter == IssueFilter::Assigned { "assigned_to_me" } else { "created_by_me" };
    let mut items = vec![];
    for s in states(state, &["closed"]) {
        let mut query = list_query();
        query.push(("scope", scope.into()));
        query.push(("state", s.to_string()));
        let page: Vec<Issue> = c.http().get("/issues", &query).await?;
        items.extend(page.into_iter().map(issue_item));
    }
    Ok(newest_first(items))
}

pub(super) async fn notifications(c: &ForgeClient) -> ApiResult<Vec<ForgeNotification>> {
    let query: Query = vec![("state", "pending".into()), ("per_page", "50".into())];
    let todos: Vec<Todo> = c.http().get("/todos", &query).await?;
    let host = c.http().base.trim_start_matches("https://").trim_end_matches("/api/v4").to_string();
    Ok(todos.into_iter().map(|t| notification(t, &host)).collect())
}

fn notification(t: Todo, host: &str) -> ForgeNotification {
    let title = t.target.as_ref().and_then(|x| x.title.clone()).or(t.body.clone()).unwrap_or_else(|| t.target_type.clone());
    let url = t.target_url.clone().or_else(|| t.target.as_ref().and_then(|x| x.web_url.clone())).unwrap_or_else(|| format!("https://{host}/dashboard/todos"));
    ForgeNotification {
        id: t.id.to_string(),
        title,
        repo: t.project.and_then(|p| p.path_with_namespace).unwrap_or_default(),
        kind: if t.target_type == "MergeRequest" { "MR".into() } else { t.target_type.clone() },
        reason: reason(&t.action_name),
        url,
        updated_at: t.created_at,
        author: t.author.map(|a| Actor { login: a.username, avatar_url: a.avatar_url }),
        unread: t.state == "pending",
    }
}

pub(super) async fn repositories(c: &ForgeClient) -> ApiResult<Vec<ForgeRepo>> {
    let query: Query = vec![("membership", "true".into()), ("order_by", "last_activity_at".into()), ("per_page", "100".into()), ("archived", "false".into())];
    let projects: Vec<Project> = c.http().get("/projects", &query).await?;
    Ok(projects.into_iter().map(repo).collect())
}

fn repo(p: Project) -> ForgeRepo {
    ForgeRepo {
        full_name: p.path_with_namespace,
        description: p.description.filter(|d| !d.is_empty()),
        url: p.web_url,
        is_private: p.visibility.as_deref() != Some("public"),
        updated_at: p.last_activity_at,
        default_branch: p.default_branch,
        stars: p.star_count,
    }
}

fn reason(action: &str) -> String {
    match action {
        "assigned" => "Assigned".into(),
        "mentioned" => "Mentioned".into(),
        "directly_addressed" => "Addressed you".into(),
        "build_failed" => "Pipeline failed".into(),
        "marked" => "Added to do".into(),
        "approval_required" => "Approval required".into(),
        "unmergeable" => "Can't merge".into(),
        "merge_train_removed" => "Removed from merge train".into(),
        "review_requested" => "Review requested".into(),
        "review_submitted" => "Review submitted".into(),
        "member_access_requested" => "Access requested".into(),
        other => other
            .split('_')
            .filter(|w| !w.is_empty())
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn project(reference: Option<String>, separator: char, url: &str) -> String {
    if let Some((project, _)) = reference.as_deref().and_then(|r| r.rsplit_once(separator)) {
        if !project.is_empty() {
            return project.to_string();
        }
    }
    project_from_url(url)
}

fn project_from_url(url: &str) -> String {
    let path = url::Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    let project = path.split("/-/").next().unwrap_or("").trim_start_matches('/');
    if project.is_empty() {
        "?".into()
    } else {
        project.to_string()
    }
}

fn item(mr: MergeRequest) -> SearchItem {
    let state = match mr.state.as_str() {
        "merged" => "MERGED",
        "opened" | "locked" => "OPEN",
        _ => "CLOSED",
    };
    let repo = project(mr.references.and_then(|r| r.full), '!', &mr.web_url);
    ForgeItem {
        number: mr.iid,
        title: mr.title,
        url: mr.web_url,
        state,
        draft: mr.draft.or(mr.work_in_progress).unwrap_or(false),
        created: mr.created_at,
        updated: mr.updated_at,
        base: mr.target_branch,
        author: mr.author.as_ref().map(|a| a.username.clone()),
        avatar: mr.author.and_then(|a| a.avatar_url),
        repo,
        comments: mr.user_notes_count,
        labels: mr.labels.unwrap_or_default().into_iter().map(FlexLabel::info).collect(),
        review: None,
    }
    .into()
}

fn issue_item(i: Issue) -> SearchItem {
    let repo = project(i.references.and_then(|r| r.full), '#', &i.web_url);
    issue(ForgeItem {
        number: i.iid,
        title: i.title,
        url: i.web_url,
        state: if i.state == "opened" { "OPEN" } else { "CLOSED" },
        draft: false,
        created: i.created_at,
        updated: i.updated_at,
        base: None,
        author: i.author.as_ref().map(|a| a.username.clone()),
        avatar: i.author.and_then(|a| a.avatar_url),
        repo,
        comments: i.user_notes_count,
        labels: i.labels.unwrap_or_default().into_iter().map(FlexLabel::info).collect(),
        review: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MRS: &str = r##"[
      {
        "id": 901, "iid": 12, "project_id": 3,
        "title": "Add caching",
        "state": "opened",
        "created_at": "2026-03-01T10:00:00.123Z",
        "updated_at": "2026-03-04T08:30:00.000Z",
        "target_branch": "main",
        "author": { "id": 7, "username": "alice", "name": "Alice", "avatar_url": "https://gitlab.com/a.png" },
        "draft": true,
        "work_in_progress": true,
        "user_notes_count": 4,
        "labels": [{ "id": 1, "name": "backend", "color": "#428BCA" }],
        "references": { "short": "!12", "relative": "!12", "full": "group/sub/app!12" },
        "web_url": "https://gitlab.com/group/sub/app/-/merge_requests/12"
      },
      {
        "id": 902, "iid": 3,
        "title": "Fix typo",
        "state": "merged",
        "created_at": "2026-02-01T10:00:00Z",
        "updated_at": "2026-02-02T10:00:00+02:00",
        "author": null,
        "labels": ["docs"],
        "web_url": "https://gitlab.example.com/team/site/-/merge_requests/3"
      }
    ]"##;

    #[test]
    fn maps_merge_requests() {
        let page: Vec<MergeRequest> = serde_json::from_str(MRS).unwrap();
        let items: Vec<SearchItem> = page.into_iter().map(item).collect();
        let a = &items[0];
        assert_eq!(a.number, 12);
        assert_eq!(a.repository.name_with_owner, "group/sub/app");
        assert_eq!(a.pr_state.as_deref(), Some("OPEN"));
        assert_eq!(a.is_draft, Some(true));
        assert_eq!(a.base_ref_name.as_deref(), Some("main"));
        assert_eq!(a.author.as_ref().map(|x| x.login.as_str()), Some("alice"));
        assert_eq!(a.comments.total_count, 4);
        assert_eq!(a.labels()[0].color, "428BCA");
        assert!(a.forge);
        let b = &items[1];
        assert_eq!(b.pr_state.as_deref(), Some("MERGED"));
        assert_eq!(b.repository.name_with_owner, "team/site");
        assert_eq!(b.labels()[0].name, "docs");
        assert!(b.author.is_none());
    }

    #[test]
    fn maps_issues() {
        let page: Vec<Issue> = serde_json::from_str(
            r#"[{ "iid": 8, "title": "Crash", "state": "closed", "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-03T00:00:00Z",
                  "author": { "username": "bob" }, "references": { "full": "g/app#8" }, "user_notes_count": 1, "labels": ["bug"],
                  "web_url": "https://gitlab.com/g/app/-/issues/8" }]"#,
        )
        .unwrap();
        let i = issue_item(page.into_iter().next().unwrap());
        assert!(!i.is_pr());
        assert!(i.forge);
        assert_eq!(i.issue_state.as_deref(), Some("CLOSED"));
        assert_eq!(i.repository.name_with_owner, "g/app");
        assert!(state_matches(StateFilter::Closed, &i));
    }

    #[test]
    fn mention_todos_skip_unreadable_targets() {
        let page: Vec<TodoTarget<MergeRequest>> = serde_json::from_str(
            r#"[{ "id": 1, "target": { "iid": 4, "title": "T", "web_url": "https://gitlab.com/g/a/-/merge_requests/4", "state": "opened",
                   "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z" } },
                 { "id": 2, "target": { "unexpected": true } },
                 { "id": 3 }]"#,
        )
        .unwrap();
        let targets: Vec<MergeRequest> = page.into_iter().filter_map(|t| t.target).collect();
        assert_eq!(targets.len(), 1);
        assert_eq!(item(targets.into_iter().next().unwrap()).number, 4);
    }

    #[test]
    fn maps_todos() {
        let todos: Vec<Todo> = serde_json::from_str(
            r#"[{ "id": 77, "action_name": "directly_addressed", "target_type": "MergeRequest", "target_url": "https://gitlab.com/g/a/-/merge_requests/4",
                  "state": "pending", "created_at": "2026-01-05T00:00:00Z", "author": { "username": "carol" },
                  "project": { "path_with_namespace": "g/a" }, "target": { "title": "Speed up" } },
                { "id": 78, "action_name": "build_failed", "target_type": "Commit", "body": "Pipeline #5", "state": "pending",
                  "created_at": "2026-01-04T00:00:00Z", "target": "not an object" }]"#,
        )
        .unwrap();
        let n: Vec<ForgeNotification> = todos.into_iter().map(|t| notification(t, "gitlab.com")).collect();
        assert_eq!(n[0].id, "77");
        assert_eq!(n[0].title, "Speed up");
        assert_eq!(n[0].kind, "MR");
        assert_eq!(n[0].reason, "Addressed you");
        assert_eq!(n[0].repo, "g/a");
        assert!(n[0].unread);
        assert_eq!(n[1].title, "Pipeline #5");
        assert_eq!(n[1].reason, "Pipeline failed");
        assert_eq!(n[1].url, "https://gitlab.com/dashboard/todos");
        assert_eq!(reason("some_new_action"), "Some New Action");
    }

    #[test]
    fn maps_projects() {
        let projects: Vec<Project> = serde_json::from_str(
            r#"[{ "path_with_namespace": "g/app", "description": "", "web_url": "https://gitlab.com/g/app", "visibility": "public",
                  "last_activity_at": "2026-01-01T00:00:00Z", "default_branch": "main", "star_count": 3 }]"#,
        )
        .unwrap();
        let r = repo(projects.into_iter().next().unwrap());
        assert_eq!(r.full_name, "g/app");
        assert!(!r.is_private);
        assert!(r.description.is_none());
        assert_eq!(r.stars, Some(3));
    }

    #[test]
    fn maps_user() {
        let u: User = serde_json::from_str(r#"{ "id": 5, "username": "alice", "name": "", "avatar_url": null }"#).unwrap();
        assert_eq!(u.username, "alice");
        assert_eq!(u.id, 5);
    }

    #[test]
    fn project_path() {
        assert_eq!(project(Some("a/b!3".into()), '!', ""), "a/b");
        assert_eq!(project(None, '#', "https://gitlab.com/a/b/-/issues/1"), "a/b");
        assert_eq!(project_from_url("not a url"), "?");
    }
}
