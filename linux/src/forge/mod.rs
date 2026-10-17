mod bitbucket;
mod gitea;
mod gitlab;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use reqwest::header::{ACCEPT, AUTHORIZATION};
use reqwest::{Method, StatusCode};
use serde::de::{DeserializeOwned, Deserializer};
use serde::Deserialize;
use serde_json::Value;

use crate::api::models::{Actor, Count, Date, LabelInfo, Nodes, RepoName, SearchItem};
use crate::api::{http_client, ApiError, ApiResult};
use crate::app::{IssueFilter, PrFilter};
use crate::auth::{self, Endpoint};
use crate::git_accounts::{GitAccount, GitCredential, GitProvider};
use crate::secret;
use crate::ui::lists::StateFilter;

pub type Query = Vec<(&'static str, String)>;

#[derive(Debug, Clone)]
pub struct ForgeUser {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForgeCapabilities {
    pub pull_filters: Vec<PrFilter>,
    pub issue_filters: Vec<IssueFilter>,
    pub inbox_title: Option<&'static str>,
    pub repositories: bool,
}

impl Default for ForgeCapabilities {
    fn default() -> Self {
        ForgeCapabilities { pull_filters: vec![PrFilter::ReviewRequested, PrFilter::Mine], issue_filters: vec![], inbox_title: None, repositories: false }
    }
}

impl ForgeCapabilities {
    pub fn has_inbox(&self) -> bool {
        self.inbox_title.is_some()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForgeNotification {
    pub id: String,
    pub title: String,
    pub repo: String,
    pub kind: String,
    pub reason: String,
    pub url: String,
    pub updated_at: Date,
    pub author: Option<Actor>,
    pub unread: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForgeRepo {
    pub full_name: String,
    pub description: Option<String>,
    pub url: String,
    pub is_private: bool,
    pub updated_at: Option<Date>,
    pub default_branch: Option<String>,
    pub stars: Option<i64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum FlexLabel {
    Detailed {
        name: String,
        #[serde(default)]
        color: Option<String>,
    },
    Name(String),
}

impl FlexLabel {
    pub fn info(self) -> LabelInfo {
        match self {
            FlexLabel::Detailed { name, color } => label(name, color),
            FlexLabel::Name(name) => label(name, None),
        }
    }
}

pub fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    let v = Option::<Value>::deserialize(d)?;
    Ok(v.and_then(|v| serde_json::from_value(v).ok()))
}

#[derive(Clone)]
struct Refresher {
    client_id: String,
    refresh_token: String,
    endpoint: Endpoint,
    secret_key: String,
    config_dir: PathBuf,
}

pub struct ForgeHttp {
    base: String,
    service: String,
    authorization: Mutex<String>,
    refresher: Mutex<Option<Refresher>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn error_message(body: &[u8]) -> Option<String> {
    let obj: Value = serde_json::from_slice(body).ok()?;
    let s = |v: Option<&Value>| v.and_then(|v| v.as_str()).map(|s| s.to_string());
    s(obj.get("message"))
        .or_else(|| s(obj.get("error").and_then(|e| e.get("message"))))
        .or_else(|| s(obj.get("error_description")))
        .or_else(|| s(obj.get("error")))
}

impl ForgeHttp {
    fn new(base: String, service: String, authorization: String) -> Self {
        ForgeHttp { base, service, authorization: Mutex::new(authorization), refresher: Mutex::new(None) }
    }

    pub fn url(&self, path: &str, query: &[(&'static str, String)]) -> ApiResult<url::Url> {
        let raw = if path.starts_with("http") { path.to_string() } else { format!("{}{path}", self.base) };
        let mut u = url::Url::parse(&raw).map_err(|_| ApiError::new(format!("Invalid address: {raw}")))?;
        if !query.is_empty() {
            let mut pairs = u.query_pairs_mut();
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        Ok(u)
    }

    async fn send(&self, method: &Method, url: &url::Url) -> ApiResult<(StatusCode, Vec<u8>)> {
        let authorization = lock(&self.authorization).clone();
        let resp = http_client().request(method.clone(), url.clone()).header(AUTHORIZATION, authorization).header(ACCEPT, "application/json").send().await.map_err(|e| self.network_error(e))?;
        let status = resp.status();
        let bytes = resp.bytes().await.map_err(|e| self.network_error(e))?;
        Ok((status, bytes.to_vec()))
    }

    fn network_error(&self, e: reqwest::Error) -> ApiError {
        if e.is_connect() {
            return ApiError::new(format!("Could not connect to {}", self.service)).with_kind(crate::api::ErrorKind::Network);
        }
        ApiError::from(crate::api::scrub(e))
    }

    async fn refresh(&self) -> ApiResult<bool> {
        let Some(r) = lock(&self.refresher).clone() else { return Ok(false) };
        let fresh = auth::refresh(&r.client_id, &r.refresh_token, &r.endpoint).await?;
        *lock(&self.authorization) = format!("Bearer {}", fresh.access_token);
        if let Some(stored) = lock(&self.refresher).as_mut() {
            stored.refresh_token = fresh.refresh_token.clone().unwrap_or_else(|| r.refresh_token.clone());
        }
        let credential = fresh.credential(Some(r.client_id.clone()));
        secret::save(r.config_dir.clone(), r.secret_key.clone(), credential.encode()).await;
        Ok(true)
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&'static str, String)]) -> ApiResult<T> {
        let body = self.call(Method::GET, path, query).await?;
        serde_json::from_slice(&body).map_err(|e| ApiError::new(format!("Failed to parse response from {}: {e}", self.service)))
    }

    pub async fn post(&self, method: Method, path: &str, query: &[(&'static str, String)]) -> ApiResult<()> {
        self.call(method, path, query).await.map(|_| ())
    }

    async fn call(&self, method: Method, path: &str, query: &[(&'static str, String)]) -> ApiResult<Vec<u8>> {
        let target = self.url(path, query)?;
        let (mut status, mut body) = self.send(&method, &target).await?;
        if status == StatusCode::UNAUTHORIZED && self.refresh().await? {
            (status, body) = self.send(&method, &target).await?;
        }
        if !status.is_success() {
            if status == StatusCode::UNAUTHORIZED {
                return Err(ApiError::with_status("Token is invalid or revoked (401)", 401));
            }
            let reason = error_message(&body).unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_string());
            return Err(ApiError::with_status(format!("{} {}: {reason}", self.service, status.as_u16()), status.as_u16()));
        }
        Ok(body)
    }
}

#[derive(Default)]
struct ClientState {
    login: Option<String>,
    user_id: Option<String>,
    workspaces: Option<Vec<String>>,
    cache: HashMap<String, (Instant, Vec<SearchItem>)>,
    approval_filter: Option<bool>,
    participant_filter_broken: bool,
    review_cache: HashMap<String, (Instant, Vec<bitbucket::PullRequest>)>,
    repo_cache: Option<(Instant, Vec<bitbucket::Repo>)>,
}

struct Inner {
    provider: GitProvider,
    http: ForgeHttp,
    state: Mutex<ClientState>,
}

#[derive(Clone)]
pub struct ForgeClient {
    inner: Arc<Inner>,
}

impl ForgeClient {
    pub fn new(account: &GitAccount, credential: &GitCredential, config_dir: PathBuf) -> Option<ForgeClient> {
        let (base, service, authorization) = match account.provider {
            GitProvider::Github | GitProvider::GithubEnterprise => return None,
            GitProvider::Gitlab => (format!("https://{}/api/v4", account.host), "GitLab".to_string(), format!("Bearer {}", credential.token)),
            GitProvider::Bitbucket => ("https://api.bitbucket.org/2.0".to_string(), "Bitbucket".to_string(), bitbucket::authorization(account.email.as_deref(), &credential.token)),
            GitProvider::Gitea => (format!("https://{}/api/v1", account.host), account.host.clone(), format!("token {}", credential.token)),
        };
        let http = ForgeHttp::new(base, service, authorization);
        if account.provider == GitProvider::Gitlab {
            if let (Some(refresh_token), Some(client_id)) = (credential.refresh_token.clone(), credential.client_id.clone()) {
                *lock(&http.refresher) = Some(Refresher { client_id, refresh_token, endpoint: Endpoint::gitlab(&account.host), secret_key: account.secret_key.clone(), config_dir });
            }
        }
        let state = ClientState { login: Some(account.login.clone()).filter(|l| !l.is_empty()), user_id: account.user_id.clone(), ..Default::default() };
        Some(ForgeClient { inner: Arc::new(Inner { provider: account.provider, http, state: Mutex::new(state) }) })
    }

    pub fn provider(&self) -> GitProvider {
        self.inner.provider
    }

    pub fn same(&self, other: &ForgeClient) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    fn http(&self) -> &ForgeHttp {
        &self.inner.http
    }

    fn state(&self) -> MutexGuard<'_, ClientState> {
        lock(&self.inner.state)
    }

    pub async fn current_user(&self) -> ApiResult<ForgeUser> {
        let user = match self.inner.provider {
            GitProvider::Gitlab => gitlab::current_user(self).await?,
            GitProvider::Bitbucket => bitbucket::current_user(self).await?,
            GitProvider::Gitea => gitea::current_user(self).await?,
            GitProvider::Github | GitProvider::GithubEnterprise => return Err(ApiError::new("Not a forge account")),
        };
        let mut s = self.state();
        s.login = Some(user.login.clone());
        if user.id.is_some() {
            s.user_id = user.id.clone();
        }
        Ok(user)
    }

    async fn login(&self) -> ApiResult<String> {
        let known = self.state().login.clone();
        match known {
            Some(l) => Ok(l),
            None => Ok(self.current_user().await?.login),
        }
    }

    async fn user_id(&self) -> ApiResult<String> {
        let known = self.state().user_id.clone();
        match known {
            Some(id) => Ok(id),
            None => Ok(self.current_user().await?.id.unwrap_or_default()),
        }
    }

    pub async fn capabilities(&self) -> ForgeCapabilities {
        match self.inner.provider {
            GitProvider::Gitlab => gitlab::capabilities(self).await,
            GitProvider::Bitbucket => bitbucket::capabilities(),
            GitProvider::Gitea => gitea::capabilities(),
            GitProvider::Github | GitProvider::GithubEnterprise => ForgeCapabilities::default(),
        }
    }

    pub async fn issues(&self, filter: IssueFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
        match self.inner.provider {
            GitProvider::Gitlab => gitlab::issues(self, filter, state).await,
            GitProvider::Gitea => gitea::issues(self, filter, state).await,
            _ => Ok(vec![]),
        }
    }

    pub async fn notifications(&self) -> ApiResult<Vec<ForgeNotification>> {
        match self.inner.provider {
            GitProvider::Gitlab => gitlab::notifications(self).await,
            GitProvider::Gitea => gitea::notifications(self).await,
            _ => Ok(vec![]),
        }
    }

    pub async fn mark_done(&self, n: &ForgeNotification) -> ApiResult<()> {
        match self.inner.provider {
            GitProvider::Gitlab => self.http().post(Method::POST, &format!("/todos/{}/mark_as_done", n.id), &[]).await,
            GitProvider::Gitea => self.http().post(Method::PATCH, &format!("/notifications/threads/{}", n.id), &[("to-status", "read".into())]).await,
            _ => Ok(()),
        }
    }

    pub async fn repositories(&self) -> ApiResult<Vec<ForgeRepo>> {
        match self.inner.provider {
            GitProvider::Gitlab => gitlab::repositories(self).await,
            GitProvider::Bitbucket => bitbucket::repositories(self).await,
            GitProvider::Gitea => gitea::repositories(self).await,
            _ => Ok(vec![]),
        }
    }

    pub async fn pull_requests(&self, filter: PrFilter, state: StateFilter) -> ApiResult<Vec<SearchItem>> {
        match self.inner.provider {
            GitProvider::Gitlab => gitlab::pull_requests(self, filter, state).await,
            GitProvider::Bitbucket => bitbucket::pull_requests(self, filter, state).await,
            GitProvider::Gitea => gitea::pull_requests(self, filter, state).await,
            GitProvider::Github | GitProvider::GithubEnterprise => Ok(vec![]),
        }
    }
}

pub struct ForgeItem {
    pub number: i64,
    pub title: String,
    pub url: String,
    pub state: &'static str,
    pub draft: bool,
    pub created: Date,
    pub updated: Date,
    pub base: Option<String>,
    pub author: Option<String>,
    pub avatar: Option<String>,
    pub repo: String,
    pub comments: Option<i64>,
    pub labels: Vec<LabelInfo>,
    pub review: Option<String>,
}

impl From<ForgeItem> for SearchItem {
    fn from(f: ForgeItem) -> SearchItem {
        SearchItem {
            typename: "PullRequest".into(),
            number: f.number,
            title: f.title,
            url: f.url,
            pr_state: Some(f.state.into()),
            issue_state: None,
            state_reason: None,
            is_draft: Some(f.draft),
            created_at: f.created,
            updated_at: f.updated,
            base_ref_name: f.base,
            author: f.author.map(|login| Actor { login, avatar_url: f.avatar }),
            repository: RepoName { name_with_owner: f.repo, default_branch_ref: None },
            review_decision: f.review,
            additions: None,
            deletions: None,
            comments: Count { total_count: f.comments.unwrap_or(0) },
            labels: if f.labels.is_empty() { None } else { Some(Nodes { nodes: f.labels, total_count: None }) },
            commits: None,
            forge: true,
        }
    }
}

pub fn issue(f: ForgeItem) -> SearchItem {
    let open = f.state == "OPEN";
    let completed = f.state != "NOT_PLANNED";
    let mut item: SearchItem = f.into();
    item.typename = "Issue".into();
    item.pr_state = None;
    item.is_draft = None;
    item.base_ref_name = None;
    item.review_decision = None;
    item.issue_state = Some(if open { "OPEN" } else { "CLOSED" }.into());
    item.state_reason = if open { None } else { Some(if completed { "COMPLETED" } else { "NOT_PLANNED" }.into()) };
    item
}

pub fn state_matches(state: StateFilter, item: &SearchItem) -> bool {
    let open = if item.is_pr() { item.pr_state.as_deref() == Some("OPEN") } else { item.issue_state.as_deref() == Some("OPEN") };
    match state {
        StateFilter::Open => open,
        StateFilter::Closed => !open,
        StateFilter::All => true,
    }
}

pub fn label(name: String, color: Option<String>) -> LabelInfo {
    LabelInfo { name, color: color.unwrap_or_else(|| "888888".into()).trim_start_matches('#').to_string() }
}

pub fn newest_first(items: Vec<SearchItem>) -> Vec<SearchItem> {
    let mut seen = HashSet::new();
    let mut unique: Vec<SearchItem> = items.into_iter().filter(|i| seen.insert(i.url.clone())).collect();
    unique.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    unique.truncate(100);
    unique
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(url: &str, updated: &str) -> SearchItem {
        ForgeItem {
            number: 1,
            title: "t".into(),
            url: url.into(),
            state: "OPEN",
            draft: false,
            created: "2026-01-01T00:00:00Z".parse().unwrap(),
            updated: updated.parse().unwrap(),
            base: None,
            author: None,
            avatar: None,
            repo: "a/b".into(),
            comments: None,
            labels: vec![],
            review: None,
        }
        .into()
    }

    #[test]
    fn newest_first_dedupes_and_sorts() {
        let out = newest_first(vec![item("u1", "2026-01-02T00:00:00Z"), item("u2", "2026-01-05T00:00:00Z"), item("u1", "2026-01-09T00:00:00Z")]);
        assert_eq!(out.iter().map(|i| i.url.as_str()).collect::<Vec<_>>(), ["u2", "u1"]);
        assert!(out.iter().all(|i| i.forge && i.is_pr()));
    }

    #[test]
    fn issues_and_state_filter() {
        let mut f = ForgeItem {
            number: 3,
            title: "bug".into(),
            url: "u3".into(),
            state: "CLOSED",
            draft: false,
            created: "2026-01-01T00:00:00Z".parse().unwrap(),
            updated: "2026-01-01T00:00:00Z".parse().unwrap(),
            base: None,
            author: None,
            avatar: None,
            repo: "a/b".into(),
            comments: None,
            labels: vec![],
            review: None,
        };
        let closed = issue(ForgeItem { url: "u4".into(), ..clone_item(&f) });
        assert!(!closed.is_pr());
        assert_eq!(closed.issue_state.as_deref(), Some("CLOSED"));
        assert_eq!(closed.state_reason.as_deref(), Some("COMPLETED"));
        assert!(state_matches(StateFilter::Closed, &closed));
        f.state = "OPEN";
        let open = issue(f);
        assert!(state_matches(StateFilter::Open, &open));
        assert!(!state_matches(StateFilter::Closed, &open));
        assert!(state_matches(StateFilter::Open, &item("u5", "2026-01-01T00:00:00Z")));
    }

    fn clone_item(f: &ForgeItem) -> ForgeItem {
        ForgeItem { number: f.number, title: f.title.clone(), url: f.url.clone(), state: f.state, draft: f.draft, created: f.created, updated: f.updated, base: None, author: None, avatar: None, repo: f.repo.clone(), comments: None, labels: vec![], review: None }
    }

    #[test]
    fn flexible_labels() {
        let labels: Vec<FlexLabel> = serde_json::from_str(r##"["docs", {"name": "bug", "color": "#ff0000"}]"##).unwrap();
        let infos: Vec<LabelInfo> = labels.into_iter().map(FlexLabel::info).collect();
        assert_eq!(infos[0].color, "888888");
        assert_eq!(infos[1].color, "ff0000");
    }

    #[test]
    fn labels_drop_hash() {
        assert_eq!(label("bug".into(), Some("#ff0000".into())).color, "ff0000");
        assert_eq!(label("x".into(), None).color, "888888");
    }

    #[test]
    fn query_is_encoded() {
        let http = ForgeHttp::new("https://api.bitbucket.org/2.0".into(), "Bitbucket".into(), String::new());
        let u = http.url("/repositories/ws/repo/pullrequests", &[("q", "reviewers.uuid=\"{abc}\"".into()), ("state", "OPEN".into()), ("state", "MERGED".into())]).unwrap();
        assert_eq!(u.path(), "/2.0/repositories/ws/repo/pullrequests");
        let pairs: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (k.into(), v.into())).collect();
        assert_eq!(pairs[0], ("q".to_string(), "reviewers.uuid=\"{abc}\"".to_string()));
        assert_eq!(pairs.iter().filter(|(k, _)| k == "state").count(), 2);
    }
}
