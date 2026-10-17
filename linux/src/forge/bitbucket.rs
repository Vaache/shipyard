use std::time::{Duration, Instant};

use base64::Engine;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;

use super::{newest_first, ForgeClient, ForgeItem, ForgeUser, Query};
use crate::api::models::{Date, SearchItem};
use crate::api::{ApiError, ApiResult};
use crate::app::PrFilter;
use crate::ui::lists::StateFilter;

const MAX_REPOS: usize = 20;
const CACHE_TTL: Duration = Duration::from_secs(180);
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[derive(Deserialize)]
struct Href {
    #[serde(default)]
    href: Option<String>,
}

#[derive(Deserialize)]
struct UserLinks {
    #[serde(default)]
    avatar: Option<Href>,
}

#[derive(Deserialize)]
struct User {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    links: Option<UserLinks>,
}

impl User {
    fn login(&self) -> Option<String> {
        self.nickname.clone().or_else(|| self.username.clone()).or_else(|| self.display_name.clone())
    }

    fn avatar(&self) -> Option<String> {
        self.links.as_ref().and_then(|l| l.avatar.as_ref()).and_then(|a| a.href.clone())
    }
}

#[derive(Deserialize)]
struct Page<T> {
    values: Vec<T>,
}

#[derive(Deserialize)]
struct Slug {
    slug: String,
}

#[derive(Deserialize)]
struct WorkspaceRef {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    workspace: Option<Slug>,
}

#[derive(Deserialize)]
struct Repo {
    full_name: String,
}

#[derive(Deserialize)]
struct Links {
    #[serde(default)]
    html: Option<Href>,
}

#[derive(Deserialize)]
struct Branch {
    name: String,
}

#[derive(Deserialize)]
struct Endpoint {
    #[serde(default)]
    branch: Option<Branch>,
    #[serde(default)]
    repository: Option<Repo>,
}

#[derive(Deserialize)]
struct Participant {
    #[serde(default)]
    approved: Option<bool>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
struct PullRequest {
    id: i64,
    title: String,
    state: String,
    #[serde(default)]
    draft: Option<bool>,
    created_on: Date,
    updated_on: Date,
    #[serde(default)]
    author: Option<User>,
    #[serde(default)]
    destination: Option<Endpoint>,
    #[serde(default)]
    links: Option<Links>,
    #[serde(default)]
    comment_count: Option<i64>,
    #[serde(default)]
    participants: Option<Vec<Participant>>,
}

pub(super) fn authorization(email: Option<&str>, token: &str) -> String {
    match email.filter(|e| !e.is_empty()) {
        Some(email) => format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{email}:{token}"))),
        None => format!("Bearer {token}"),
    }
}

fn segment(s: &str) -> String {
    utf8_percent_encode(s, SEGMENT).to_string()
}

pub(super) async fn current_user(c: &ForgeClient) -> ApiResult<ForgeUser> {
    let u: User = c.http().get("/user", &[]).await?;
    let id = u.uuid.clone();
    let login = u.login().or_else(|| id.clone()).unwrap_or_default();
    Ok(ForgeUser { avatar_url: u.avatar(), name: u.display_name.clone(), login, id })
}

fn state_query(state: StateFilter) -> Query {
    let states: &[&str] = match state {
        StateFilter::Open => &["OPEN"],
        StateFilter::Closed => &["MERGED", "DECLINED", "SUPERSEDED"],
        StateFilter::All => &["OPEN", "MERGED", "DECLINED", "SUPERSEDED"],
    };
    states.iter().map(|s| ("state", s.to_string())).collect()
}

pub(super) async fn pull_requests(c: &ForgeClient, filter: PrFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    let key = format!("{filter:?}|{}", state.title());
    let hit = c.state().cache.get(&key).filter(|(at, _)| at.elapsed() < CACHE_TTL).map(|(_, items)| items.clone());
    if let Some(items) = hit {
        return Ok(items);
    }
    let items = fetch(c, filter, state).await?;
    c.state().cache.insert(key, (Instant::now(), items.clone()));
    Ok(items)
}

async fn fetch(c: &ForgeClient, filter: PrFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
    let me = c.user_id().await?;
    let spaces = workspace_slugs(c).await?;
    let mut common = state_query(state);
    common.push(("pagelen", "50".into()));
    common.push(("sort", "-updated_on".into()));
    common.push(("fields", "+values.participants".into()));
    match filter {
        PrFilter::Mine => {
            let mut items = vec![];
            for ws in &spaces {
                let page: Page<PullRequest> = c.http().get(&format!("/workspaces/{}/pullrequests/{}", segment(ws), segment(&me)), &common).await?;
                items.extend(page.values.into_iter().map(item));
            }
            Ok(newest_first(items))
        }
        PrFilter::ReviewRequested => {
            let mut repos: Vec<String> = vec![];
            for ws in &spaces {
                if repos.len() >= MAX_REPOS {
                    break;
                }
                let query: Query = vec![("role", "member".into()), ("sort", "-updated_on".into()), ("pagelen", MAX_REPOS.to_string())];
                let page: Page<Repo> = c.http().get(&format!("/repositories/{}", segment(ws)), &query).await?;
                repos.extend(page.values.into_iter().map(|r| r.full_name));
            }
            let mut items = vec![];
            let mut last_error: Option<ApiError> = None;
            for repo in repos.iter().take(MAX_REPOS) {
                let mut query = common.clone();
                query.push(("q", format!("reviewers.uuid=\"{me}\"")));
                let path = format!("/repositories/{}/pullrequests", repo.split('/').map(segment).collect::<Vec<_>>().join("/"));
                match c.http().get::<Page<PullRequest>>(&path, &query).await {
                    Ok(page) => items.extend(page.values.into_iter().map(item)),
                    Err(e) => last_error = Some(e),
                }
            }
            if items.is_empty() {
                if let Some(e) = last_error.filter(|e| e.status == 401) {
                    return Err(e);
                }
            }
            Ok(newest_first(items))
        }
        _ => Ok(vec![]),
    }
}

async fn workspace_slugs(c: &ForgeClient) -> ApiResult<Vec<String>> {
    let known = c.state().workspaces.clone();
    if let Some(spaces) = known {
        return Ok(spaces);
    }
    let query: Query = vec![("pagelen", "100".into())];
    let page: Page<WorkspaceRef> = match c.http().get("/user/workspaces", &query).await {
        Ok(p) => p,
        Err(e) if e.status == 404 || e.status == 410 => c.http().get("/workspaces", &query).await?,
        Err(e) => return Err(e),
    };
    let slugs = slugs(page);
    c.state().workspaces = Some(slugs.clone());
    Ok(slugs)
}

fn slugs(page: Page<WorkspaceRef>) -> Vec<String> {
    page.values.into_iter().filter_map(|w| w.workspace.map(|s| s.slug).or(w.slug)).collect()
}

fn item(pr: PullRequest) -> SearchItem {
    let state = match pr.state.as_str() {
        "OPEN" => "OPEN",
        "MERGED" => "MERGED",
        _ => "CLOSED",
    };
    let participants = pr.participants.unwrap_or_default();
    let review = if participants.iter().any(|p| p.state.as_deref() == Some("changes_requested")) {
        Some("CHANGES_REQUESTED".to_string())
    } else if participants.iter().any(|p| p.approved == Some(true)) {
        Some("APPROVED".to_string())
    } else {
        None
    };
    let (base, repo) = match pr.destination {
        Some(d) => (d.branch.map(|b| b.name), d.repository.map(|r| r.full_name)),
        None => (None, None),
    };
    let repo = repo.unwrap_or_else(|| "?".into());
    let url = pr.links.and_then(|l| l.html).and_then(|h| h.href).unwrap_or_else(|| format!("https://bitbucket.org/{repo}/pull-requests/{}", pr.id));
    ForgeItem {
        number: pr.id,
        title: pr.title,
        url,
        state,
        draft: pr.draft.unwrap_or(false),
        created: pr.created_on,
        updated: pr.updated_on,
        base,
        author: pr.author.as_ref().and_then(|a| a.nickname.clone().or_else(|| a.display_name.clone())),
        avatar: pr.author.as_ref().and_then(|a| a.avatar()),
        repo,
        comments: pr.comment_count,
        labels: vec![],
        review,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRS: &str = r#"{
      "pagelen": 50,
      "values": [
        {
          "id": 42,
          "title": "Speed up builds",
          "state": "OPEN",
          "draft": false,
          "created_on": "2026-03-01T10:00:00.123456+00:00",
          "updated_on": "2026-03-03T11:22:33.000001+00:00",
          "comment_count": 2,
          "author": { "uuid": "{u-1}", "display_name": "Bob B", "nickname": "bob", "links": { "avatar": { "href": "https://avatar/bob" } } },
          "destination": { "branch": { "name": "develop" }, "repository": { "full_name": "acme/api" } },
          "links": { "html": { "href": "https://bitbucket.org/acme/api/pull-requests/42" } },
          "participants": [{ "approved": true, "state": "approved" }, { "approved": false, "state": null }]
        },
        {
          "id": 7,
          "title": "Old change",
          "state": "DECLINED",
          "created_on": "2026-01-01T10:00:00+00:00",
          "updated_on": "2026-01-02T10:00:00+00:00",
          "destination": { "repository": { "full_name": "acme/web" } },
          "participants": [{ "approved": true }, { "approved": false, "state": "changes_requested" }]
        }
      ]
    }"#;

    #[test]
    fn maps_pull_requests() {
        let page: Page<PullRequest> = serde_json::from_str(PRS).unwrap();
        let items: Vec<SearchItem> = page.values.into_iter().map(item).collect();
        let a = &items[0];
        assert_eq!(a.number, 42);
        assert_eq!(a.repository.name_with_owner, "acme/api");
        assert_eq!(a.base_ref_name.as_deref(), Some("develop"));
        assert_eq!(a.review_decision.as_deref(), Some("APPROVED"));
        assert_eq!(a.author.as_ref().map(|x| x.login.as_str()), Some("bob"));
        assert_eq!(a.author.as_ref().and_then(|x| x.avatar_url.as_deref()), Some("https://avatar/bob"));
        assert_eq!(a.comments.total_count, 2);
        let b = &items[1];
        assert_eq!(b.pr_state.as_deref(), Some("CLOSED"));
        assert_eq!(b.review_decision.as_deref(), Some("CHANGES_REQUESTED"));
        assert_eq!(b.url, "https://bitbucket.org/acme/web/pull-requests/7");
    }

    #[test]
    fn workspace_slugs_from_either_shape() {
        let page: Page<WorkspaceRef> = serde_json::from_str(r#"{ "values": [{ "workspace": { "slug": "acme" } }, { "slug": "solo" }, {}] }"#).unwrap();
        assert_eq!(slugs(page), ["acme", "solo"]);
    }

    #[test]
    fn user_login_prefers_nickname() {
        let u: User = serde_json::from_str(r#"{ "uuid": "{u}", "display_name": "Bob", "nickname": "bob" }"#).unwrap();
        assert_eq!(u.login().as_deref(), Some("bob"));
        let u: User = serde_json::from_str(r#"{ "uuid": "{u}", "display_name": "Bob" }"#).unwrap();
        assert_eq!(u.login().as_deref(), Some("Bob"));
    }

    #[test]
    fn auth_header_and_segments() {
        assert_eq!(authorization(Some("a@b.c"), "tok"), "Basic YUBiLmM6dG9r");
        assert_eq!(authorization(None, "tok"), "Bearer tok");
        assert_eq!(segment("{abc-1}"), "%7Babc-1%7D");
        assert_eq!(segment("my.ws_1"), "my.ws_1");
    }
}
