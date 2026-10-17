use serde::Deserialize;

use super::{label, newest_first, ForgeClient, ForgeItem, ForgeUser, Query};
use crate::api::models::{Date, SearchItem};
use crate::api::ApiResult;
use crate::app::PrFilter;
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
#[serde(untagged)]
enum Label {
    Detailed {
        name: String,
        #[serde(default)]
        color: Option<String>,
    },
    Name(String),
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
    labels: Option<Vec<Label>>,
}

pub(super) async fn current_user(c: &ForgeClient) -> ApiResult<ForgeUser> {
    let u: User = c.http().get("/user", &[]).await?;
    Ok(ForgeUser { login: u.username, name: u.name.filter(|n| !n.is_empty()), avatar_url: u.avatar_url, id: Some(u.id.to_string()) })
}

fn states(state: StateFilter) -> &'static [&'static str] {
    match state {
        StateFilter::Open => &["opened"],
        StateFilter::Closed => &["merged", "closed"],
        StateFilter::All => &["all"],
    }
}

pub(super) async fn pull_requests(c: &ForgeClient, filter: PrFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    let mut base: Query = vec![("order_by", "updated_at".into()), ("sort", "desc".into()), ("per_page", "50".into()), ("with_labels_details", "true".into())];
    match filter {
        PrFilter::ReviewRequested => {
            let me = c.login().await?;
            base.push(("scope", "all".into()));
            base.push(("reviewer_username", me));
        }
        PrFilter::Mine => base.push(("scope", "created_by_me".into())),
        PrFilter::Assigned => base.push(("scope", "assigned_to_me".into())),
        PrFilter::Mentioned | PrFilter::Reviewed => return Ok(vec![]),
    }
    let mut items = vec![];
    for s in states(state) {
        let mut query = base.clone();
        query.push(("state", s.to_string()));
        let page: Vec<MergeRequest> = c.http().get("/merge_requests", &query).await?;
        items.extend(page.into_iter().map(item));
    }
    Ok(newest_first(items))
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
    let repo = mr.references.and_then(|r| r.full).and_then(|f| f.split('!').next().map(|s| s.to_string())).filter(|s| !s.is_empty()).unwrap_or_else(|| project_from_url(&mr.web_url));
    let labels = mr
        .labels
        .unwrap_or_default()
        .into_iter()
        .map(|l| match l {
            Label::Detailed { name, color } => label(name, color),
            Label::Name(name) => label(name, None),
        })
        .collect();
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
        labels,
        review: None,
    }
    .into()
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
    fn maps_user() {
        let u: User = serde_json::from_str(r#"{ "id": 5, "username": "alice", "name": "", "avatar_url": null }"#).unwrap();
        assert_eq!(u.username, "alice");
        assert_eq!(u.id, 5);
    }

    #[test]
    fn project_path_from_web_url() {
        assert_eq!(project_from_url("https://gitlab.com/a/b/-/merge_requests/1"), "a/b");
        assert_eq!(project_from_url("not a url"), "?");
    }
}
