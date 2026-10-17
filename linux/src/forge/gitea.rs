use serde::Deserialize;

use super::{label, newest_first, ForgeClient, ForgeItem, ForgeUser, Query};
use crate::api::models::{Date, SearchItem};
use crate::api::ApiResult;
use crate::app::PrFilter;
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

pub(super) async fn current_user(c: &ForgeClient) -> ApiResult<ForgeUser> {
    let u: User = c.http().get("/user", &[]).await?;
    Ok(ForgeUser { login: u.login, name: u.full_name.filter(|n| !n.is_empty()), avatar_url: u.avatar_url, id: u.id.map(|id| id.to_string()) })
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

pub(super) async fn pull_requests(c: &ForgeClient, filter: PrFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    let s = match state {
        StateFilter::Open => "open",
        StateFilter::Closed => "closed",
        StateFilter::All => "all",
    };
    let query: Query = vec![("type", "pulls".into()), ("state", s.into()), (flag(filter), "true".into()), ("limit", "50".into())];
    let issues: Vec<Issue> = c.http().get("/repos/issues/search", &query).await?;
    Ok(newest_first(issues.into_iter().map(item).collect()))
}

fn item(i: Issue) -> SearchItem {
    let merged = i.pull_request.as_ref().and_then(|p| p.merged) == Some(true);
    let state = if i.state == "open" {
        "OPEN"
    } else if merged {
        "MERGED"
    } else {
        "CLOSED"
    };
    ForgeItem {
        number: i.number,
        title: i.title,
        url: i.html_url,
        state,
        draft: i.pull_request.and_then(|p| p.draft).unwrap_or(false),
        created: i.created_at,
        updated: i.updated_at,
        base: None,
        author: i.user.as_ref().map(|u| u.login.clone()),
        avatar: i.user.and_then(|u| u.avatar_url),
        repo: i.repository.map(|r| r.full_name).unwrap_or_else(|| "?".into()),
        comments: i.comments,
        labels: i.labels.unwrap_or_default().into_iter().map(|l| label(l.name, l.color)).collect(),
        review: None,
    }
    .into()
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
        let issues: Vec<Issue> = serde_json::from_str(ISSUES).unwrap();
        let items: Vec<SearchItem> = issues.into_iter().map(item).collect();
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
    fn filters_map_to_search_flags() {
        assert_eq!(flag(PrFilter::ReviewRequested), "review_requested");
        assert_eq!(flag(PrFilter::Mine), "created");
    }
}
