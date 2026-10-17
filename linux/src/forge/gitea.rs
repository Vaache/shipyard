use std::collections::HashSet;

use serde::Deserialize;

use super::{issue, label, newest_first, ForgeCapabilities, ForgeClient, ForgeItem, ForgeNotification, ForgeRepo, ForgeUser, Query};
use crate::api::models::{Date, LabelInfo, SearchItem};
use crate::api::ApiResult;
use crate::app::{IssueFilter, PrFilter};
use crate::ui::lists::StateFilter;

#[derive(Deserialize)]
struct User {
    #[serde(default)]
    id: Option<i64>,
    login: String,
    #[serde(default)]
    full_name: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct Pull {
    #[serde(default)]
    merged: Option<bool>,
    #[serde(default)]
    draft: Option<bool>,
}

#[derive(Deserialize)]
struct Repo {
    full_name: String,
}

#[derive(Deserialize)]
struct Label {
    name: String,
    #[serde(default)]
    color: Option<String>,
}

#[derive(Deserialize)]
struct Issue {
    number: i64,
    title: String,
    html_url: String,
    state: String,
    created_at: Date,
    updated_at: Date,
    #[serde(default)]
    user: Option<User>,
    #[serde(default)]
    repository: Option<Repo>,
    #[serde(default)]
    comments: Option<i64>,
    #[serde(default)]
    labels: Option<Vec<Label>>,
    #[serde(default)]
    pull_request: Option<Pull>,
}

#[derive(Deserialize)]
struct Subject {
    title: String,
    #[serde(default, rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    latest_comment_html_url: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
struct ThreadRepo {
    full_name: String,
    #[serde(default)]
    html_url: Option<String>,
}

#[derive(Deserialize)]
struct Thread {
    id: i64,
    unread: bool,
    #[serde(default)]
    pinned: Option<bool>,
    updated_at: Date,
    subject: Subject,
    #[serde(default)]
    repository: Option<ThreadRepo>,
}

#[derive(Deserialize)]
struct Repository {
    full_name: String,
    #[serde(default)]
    description: Option<String>,
    html_url: String,
    #[serde(default)]
    private: Option<bool>,
    #[serde(default)]
    archived: Option<bool>,
    #[serde(default)]
    updated_at: Option<Date>,
    #[serde(default)]
    default_branch: Option<String>,
    #[serde(default)]
    stars_count: Option<i64>,
}

pub(super) async fn current_user(c: &ForgeClient) -> ApiResult<ForgeUser> {
    let u: User = c.http().get("/user", &[]).await?;
    Ok(ForgeUser { login: u.login, name: u.full_name.filter(|n| !n.is_empty()), avatar_url: u.avatar_url, id: u.id.map(|id| id.to_string()) })
}

pub(super) fn capabilities() -> ForgeCapabilities {
    ForgeCapabilities { pull_filters: PrFilter::ALL.to_vec(), issue_filters: IssueFilter::ALL.to_vec(), inbox_title: Some("Notifications"), repositories: true }
}

fn flag(filter: PrFilter) -> &'static str {
    match filter {
        PrFilter::ReviewRequested => "review_requested",
        PrFilter::Mine => "created",
        PrFilter::Assigned => "assigned",
        PrFilter::Mentioned => "mentioned",
        PrFilter::Reviewed => "reviewed",
    }
}

fn issue_flag(filter: IssueFilter) -> &'static str {
    match filter {
        IssueFilter::Assigned => "assigned",
        IssueFilter::Mine => "created",
        IssueFilter::Mentioned => "mentioned",
    }
}

async fn search(c: &ForgeClient, kind: &str, flag: &'static str, state: StateFilter) -> ApiResult<Vec<Issue>> {
    let s = match state {
        StateFilter::Open => "open",
        StateFilter::Closed => "closed",
        StateFilter::All => "all",
    };
    let query: Query = vec![("type", kind.into()), ("state", s.into()), (flag, "true".into()), ("sort", "recentupdate".into()), ("limit", "50".into())];
    c.http().get("/repos/issues/search", &query).await
}

pub(super) async fn pull_requests(c: &ForgeClient, filter: PrFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    let found = search(c, "pulls", flag(filter), state).await?;
    Ok(newest_first(found.into_iter().map(item).collect()))
}

pub(super) async fn issues(c: &ForgeClient, filter: IssueFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    let found = search(c, "issues", issue_flag(filter), state).await?;
    Ok(newest_first(found.into_iter().map(issue_item).collect()))
}

pub(super) async fn notifications(c: &ForgeClient) -> ApiResult<Vec<ForgeNotification>> {
    let query: Query = vec![("limit", "50".into())];
    let threads: Vec<Thread> = c.http().get("/notifications", &query).await?;
    let host = c.http().base.trim_start_matches("https://").trim_end_matches("/api/v1").to_string();
    let mut out: Vec<ForgeNotification> = threads.into_iter().map(|t| notification(t, &host)).collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(out)
}

pub(super) async fn repositories(c: &ForgeClient) -> ApiResult<Vec<ForgeRepo>> {
    let limit = 50;
    let mut all: Vec<Repository> = vec![];
    for page in 1..=3 {
        let query: Query = vec![("order_by", "recentupdate".into()), ("limit", limit.to_string()), ("page", page.to_string())];
        let batch: Vec<Repository> = c.http().get("/user/repos", &query).await?;
        let done = batch.len() < limit;
        all.extend(batch);
        if done {
            break;
        }
    }
    Ok(repos(all))
}

fn repos(all: Vec<Repository>) -> Vec<ForgeRepo> {
    let mut seen = HashSet::new();
    let mut out: Vec<ForgeRepo> = all
        .into_iter()
        .filter(|r| r.archived != Some(true) && seen.insert(r.full_name.clone()))
        .map(|r| ForgeRepo {
            full_name: r.full_name,
            description: r.description.filter(|d| !d.is_empty()),
            url: r.html_url,
            is_private: r.private.unwrap_or(false),
            updated_at: r.updated_at,
            default_branch: r.default_branch,
            stars: r.stars_count,
        })
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

fn web_url(t: &Thread, host: &str) -> String {
    for candidate in [&t.subject.latest_comment_html_url, &t.subject.html_url].into_iter().flatten() {
        if !candidate.is_empty() {
            return candidate.clone();
        }
    }
    if let Some((root, rest)) = t.subject.url.as_deref().and_then(|api| api.split_once("/api/v1/repos/")) {
        return format!("{root}/{rest}").replace("/git/commits/", "/commit/");
    }
    t.repository.as_ref().and_then(|r| r.html_url.clone()).unwrap_or_else(|| format!("https://{host}/notifications"))
}

fn kind(subject_type: Option<&str>) -> String {
    match subject_type.map(|s| s.to_lowercase()).as_deref() {
        Some("pull") => "PR".into(),
        Some("issue") => "Issue".into(),
        Some("commit") => "Commit".into(),
        Some("repository") => "Repository".into(),
        _ => subject_type.unwrap_or("Notification").to_string(),
    }
}

fn reason(t: &Thread) -> String {
    if t.pinned == Some(true) {
        return "Pinned".into();
    }
    let subject = match t.subject.kind.as_deref().map(|s| s.to_lowercase()).as_deref() {
        Some("pull") => "Pull request",
        Some("issue") => "Issue",
        Some("commit") => return "Commit".into(),
        Some("repository") => return "Repository".into(),
        _ => return t.subject.kind.clone().unwrap_or_else(|| "Notification".into()),
    };
    match t.subject.state.as_deref().map(|s| s.to_lowercase()).as_deref() {
        Some("merged") => format!("{subject} merged"),
        Some("closed") => format!("{subject} closed"),
        _ => subject.to_string(),
    }
}

fn notification(t: Thread, host: &str) -> ForgeNotification {
    ForgeNotification {
        id: t.id.to_string(),
        title: t.subject.title.clone(),
        repo: t.repository.as_ref().map(|r| r.full_name.clone()).unwrap_or_default(),
        kind: kind(t.subject.kind.as_deref()),
        reason: reason(&t),
        url: web_url(&t, host),
        updated_at: t.updated_at,
        author: None,
        unread: t.unread,
    }
}

fn labels(l: Option<Vec<Label>>) -> Vec<LabelInfo> {
    l.unwrap_or_default().into_iter().map(|l| label(l.name, l.color)).collect()
}

fn parts(i: Issue, state: &'static str, draft: bool) -> ForgeItem {
    ForgeItem {
        number: i.number,
        title: i.title,
        url: i.html_url,
        state,
        draft,
        created: i.created_at,
        updated: i.updated_at,
        base: None,
        author: i.user.as_ref().map(|u| u.login.clone()),
        avatar: i.user.and_then(|u| u.avatar_url),
        repo: i.repository.map(|r| r.full_name).unwrap_or_else(|| "?".into()),
        comments: i.comments,
        labels: labels(i.labels),
        review: None,
    }
}

fn item(i: Issue) -> SearchItem {
    let merged = i.pull_request.as_ref().and_then(|p| p.merged) == Some(true);
    let draft = i.pull_request.as_ref().and_then(|p| p.draft).unwrap_or(false);
    let state = if i.state == "open" {
        "OPEN"
    } else if merged {
        "MERGED"
    } else {
        "CLOSED"
    };
    parts(i, state, draft).into()
}

fn issue_item(i: Issue) -> SearchItem {
    let state = if i.state == "open" { "OPEN" } else { "CLOSED" };
    issue(parts(i, state, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISSUES: &str = r#"[
      {
        "id": 1001, "number": 5,
        "title": "Dark mode",
        "html_url": "https://codeberg.org/team/app/pulls/5",
        "state": "open",
        "created_at": "2026-04-01T09:00:00+02:00",
        "updated_at": "2026-04-02T09:00:00+02:00",
        "user": { "id": 3, "login": "carol", "full_name": "Carol", "avatar_url": "https://codeberg.org/avatars/c" },
        "repository": { "id": 9, "name": "app", "owner": "team", "full_name": "team/app" },
        "comments": 1,
        "labels": [{ "id": 2, "name": "ui", "color": "e11d21" }],
        "pull_request": { "merged": false, "merged_at": null, "draft": true }
      },
      {
        "id": 1002, "number": 4,
        "title": "Ship it",
        "html_url": "https://codeberg.org/team/app/pulls/4",
        "state": "closed",
        "created_at": "2026-03-01T09:00:00Z",
        "updated_at": "2026-03-02T09:00:00Z",
        "user": { "login": "dave" },
        "repository": { "full_name": "team/app" },
        "labels": null,
        "pull_request": { "merged": true }
      }
    ]"#;

    #[test]
    fn maps_pulls() {
        let found: Vec<Issue> = serde_json::from_str(ISSUES).unwrap();
        let items: Vec<SearchItem> = found.into_iter().map(item).collect();
        assert_eq!(items[0].number, 5);
        assert_eq!(items[0].pr_state.as_deref(), Some("OPEN"));
        assert_eq!(items[0].is_draft, Some(true));
        assert_eq!(items[0].labels()[0].color, "e11d21");
        assert_eq!(items[0].author.as_ref().map(|a| a.login.as_str()), Some("carol"));
        assert_eq!(items[1].pr_state.as_deref(), Some("MERGED"));
        assert!(items[1].labels().is_empty());
        assert_eq!(items[1].repository.name_with_owner, "team/app");
    }

    #[test]
    fn maps_issues() {
        let found: Vec<Issue> = serde_json::from_str(ISSUES).unwrap();
        let i = issue_item(found.into_iter().nth(1).unwrap());
        assert!(!i.is_pr());
        assert_eq!(i.issue_state.as_deref(), Some("CLOSED"));
        assert_eq!(i.state_reason.as_deref(), Some("COMPLETED"));
    }

    #[test]
    fn maps_notifications() {
        let threads: Vec<Thread> = serde_json::from_str(
            r#"[
              { "id": 11, "unread": true, "pinned": false, "updated_at": "2026-04-03T10:00:00Z",
                "subject": { "title": "Fix login", "type": "Pull", "url": "https://codeberg.org/api/v1/repos/team/app/pulls/7", "html_url": "", "latest_comment_html_url": "", "state": "merged" },
                "repository": { "full_name": "team/app", "html_url": "https://codeberg.org/team/app" } },
              { "id": 12, "unread": false, "pinned": true, "updated_at": "2026-04-04T10:00:00Z",
                "subject": { "title": "Bump", "type": "Commit", "url": "https://codeberg.org/api/v1/repos/team/app/git/commits/abc" } },
              { "id": 13, "unread": true, "updated_at": "2026-04-01T10:00:00Z",
                "subject": { "title": "Bug", "type": "Issue", "html_url": "https://codeberg.org/team/app/issues/2", "state": "open" } }
            ]"#,
        )
        .unwrap();
        let n: Vec<ForgeNotification> = threads.into_iter().map(|t| notification(t, "codeberg.org")).collect();
        assert_eq!(n[0].kind, "PR");
        assert_eq!(n[0].reason, "Pull request merged");
        assert_eq!(n[0].url, "https://codeberg.org/team/app/pulls/7");
        assert_eq!(n[0].repo, "team/app");
        assert_eq!(n[1].reason, "Pinned");
        assert_eq!(n[1].url, "https://codeberg.org/team/app/commit/abc");
        assert_eq!(n[2].reason, "Issue");
        assert_eq!(n[2].url, "https://codeberg.org/team/app/issues/2");
    }

    #[test]
    fn maps_repositories() {
        let all: Vec<Repository> = serde_json::from_str(
            r#"[
              { "full_name": "team/old", "html_url": "https://codeberg.org/team/old", "updated_at": "2026-01-01T00:00:00Z" },
              { "full_name": "team/app", "description": "App", "html_url": "https://codeberg.org/team/app", "private": true, "updated_at": "2026-03-01T00:00:00Z", "stars_count": 4 },
              { "full_name": "team/app", "html_url": "https://codeberg.org/team/app" },
              { "full_name": "team/gone", "html_url": "https://codeberg.org/team/gone", "archived": true }
            ]"#,
        )
        .unwrap();
        let r = repos(all);
        assert_eq!(r.iter().map(|r| r.full_name.as_str()).collect::<Vec<_>>(), ["team/app", "team/old"]);
        assert!(r[0].is_private);
        assert_eq!(r[0].stars, Some(4));
    }

    #[test]
    fn filters_map_to_search_flags() {
        assert_eq!(flag(PrFilter::ReviewRequested), "review_requested");
        assert_eq!(flag(PrFilter::Mine), "created");
        assert_eq!(issue_flag(IssueFilter::Mine), "created");
        assert_eq!(capabilities().inbox_title, Some("Notifications"));
    }
}
