use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

use super::models::*;
use super::queries as q;
use super::{ApiError, ApiResult, ErrorKind, GitHubApi};

const STACK_TTL: Duration = Duration::from_secs(600);
const STACK_CACHE_LIMIT: usize = 200;

static STACK_CACHE: LazyLock<Mutex<HashMap<String, (Instant, Vec<StackPr>)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static STACK_FIELD_MISSING: AtomicBool = AtomicBool::new(false);

pub struct StackRefs {
    pub head_owner: String,
    pub head: String,
    pub base: String,
}

fn stack_key(r: &ItemRef, refs: &StackRefs) -> String {
    format!("{}#{}|{}:{}|{}", r.repo.full(), r.number, refs.head_owner, refs.head, refs.base)
}

pub fn reset_caches() {
    STACK_CACHE.lock().unwrap_or_else(|e| e.into_inner()).clear();
    STACK_FIELD_MISSING.store(false, Ordering::Relaxed);
}

const PATH_SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC.remove(b'-').remove(b'_').remove(b'.').remove(b'~');

pub fn encode_segment(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, PATH_SET).to_string()
}

pub fn encode_path(path: &str) -> String {
    path.split('/').map(encode_segment).collect::<Vec<_>>().join("/")
}

pub fn encode_query(s: &str) -> String {
    encode_segment(s)
}

impl GitHubApi {
    pub async fn current_user(&self) -> ApiResult<(RestUser, Vec<String>)> {
        let (bytes, headers) = self.rest_raw(Method::GET, "/user", None).await?;
        let user: RestUser = serde_json::from_slice(&bytes).map_err(|e| ApiError::new(format!("Failed to parse response: {e}")))?;
        let scopes = headers
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        Ok((user, scopes))
    }

    pub async fn search(&self, query: &str, after: Option<String>) -> ApiResult<SearchConnection> {
        let r: SearchResponse = self.graphql("search", &q::search(), json!({ "q": query, "after": after }), false).await?;
        Ok(r.search)
    }

    pub async fn repo_pull_requests(&self, repo: &RepoRef, head_ref_name: Option<String>, states: &[&str], first: i64) -> ApiResult<Vec<SearchItem>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Repo {
            pull_requests: Nodes<SearchItem>,
        }
        #[derive(Deserialize)]
        struct R {
            repository: Option<Repo>,
        }
        let r: R = self
            .graphql("repoPullRequests", &q::repo_pull_requests(), json!({ "o": repo.owner, "n": repo.name, "b": head_ref_name, "states": states, "first": first }), false)
            .await?;
        Ok(r.repository.map(|r| r.pull_requests.nodes).unwrap_or_default())
    }

    pub async fn pull_request_item(&self, r: &ItemRef) -> ApiResult<Option<SearchItem>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Repo {
            pull_request: Option<SearchItem>,
        }
        #[derive(Deserialize)]
        struct R {
            repository: Option<Repo>,
        }
        let r: R = self.graphql("pullRequestItem", &q::pr_item(), json!({ "o": r.repo.owner, "n": r.repo.name, "num": r.number }), false).await?;
        Ok(r.repository.and_then(|r| r.pull_request))
    }

    pub async fn pull_request_stack(&self, r: &ItemRef) -> ApiResult<Vec<StackPr>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Entry {
            position: i64,
            pull_request: StackPr,
        }
        #[derive(Deserialize)]
        struct Stack {
            entries: Nodes<Entry>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Pr {
            number: i64,
            title: String,
            state: String,
            is_draft: bool,
            head_ref_name: String,
            base_ref_name: String,
            #[serde(default)]
            stack: Option<Stack>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Repo {
            #[serde(default)]
            default_branch_ref: Option<Branch>,
            #[serde(default)]
            pull_request: Option<Pr>,
        }
        #[derive(Deserialize)]
        struct R {
            repository: Option<Repo>,
        }
        let vars = json!({ "o": r.repo.owner, "n": r.repo.name, "num": r.number });
        let resp: R = if STACK_FIELD_MISSING.load(Ordering::Relaxed) {
            self.graphql("pullRequestStackFallback", &q::pr_stack_fallback(), vars, false).await?
        } else {
            match self.graphql("pullRequestStack", &q::pr_stack(), vars.clone(), false).await {
                Ok(v) => v,
                Err(e) if e.kind == ErrorKind::Schema => {
                    STACK_FIELD_MISSING.store(true, Ordering::Relaxed);
                    self.graphql("pullRequestStackFallback", &q::pr_stack_fallback(), vars, false).await?
                }
                Err(e) => return Err(e),
            }
        };
        let Some(repo) = resp.repository else { return Ok(vec![]) };
        let Some(pr) = repo.pull_request else { return Ok(vec![]) };
        if let Some(stack) = &pr.stack {
            if stack.entries.nodes.len() > 1 {
                let mut entries: Vec<&Entry> = stack.entries.nodes.iter().collect();
                entries.sort_by_key(|e| e.position);
                return Ok(entries.into_iter().map(|e| e.pull_request.clone()).collect());
            }
        }
        let main = repo.default_branch_ref.map(|b| b.name);
        let current = StackPr { number: pr.number, title: pr.title, state: pr.state, is_draft: pr.is_draft, head_ref_name: pr.head_ref_name, base_ref_name: pr.base_ref_name };
        let mut below: Vec<StackPr> = vec![];
        let mut above: Vec<StackPr> = vec![];
        let mut seen: HashSet<i64> = HashSet::from([current.number]);
        let mut node = current.clone();
        for _ in 0..8 {
            if Some(&node.base_ref_name) == main.as_ref() {
                break;
            }
            let (parents, _) = self.stack_neighbors(&r.repo, &node).await?;
            let parent = parents.iter().find(|p| p.state == "OPEN").or_else(|| parents.first()).cloned();
            let Some(parent) = parent else { break };
            if !seen.insert(parent.number) {
                break;
            }
            below.insert(0, parent.clone());
            node = parent;
        }
        node = current.clone();
        for _ in 0..8 {
            let (_, children) = self.stack_neighbors(&r.repo, &node).await?;
            let child = children.iter().find(|p| p.state == "OPEN").or_else(|| children.first()).cloned();
            let Some(child) = child else { break };
            if !seen.insert(child.number) {
                break;
            }
            above.push(child.clone());
            node = child;
        }
        let mut chain = below;
        chain.push(current);
        chain.extend(above);
        Ok(if chain.len() > 1 { chain } else { vec![] })
    }

    fn cached_stack(r: &ItemRef, refs: &StackRefs) -> Option<Vec<StackPr>> {
        let cache = STACK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let (at, stack) = cache.get(&stack_key(r, refs))?;
        if at.elapsed() >= STACK_TTL {
            return None;
        }
        Some(stack.clone())
    }

    pub async fn pull_request_stack_cached(&self, r: &ItemRef, refs: &StackRefs, fresh: bool) -> ApiResult<Vec<StackPr>> {
        if !fresh {
            if let Some(stack) = Self::cached_stack(r, refs) {
                return Ok(stack);
            }
        }
        let stack = self.pull_request_stack(r).await?;
        let mut cache = STACK_CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() >= STACK_CACHE_LIMIT {
            cache.retain(|_, (at, _)| at.elapsed() < STACK_TTL);
        }
        cache.insert(stack_key(r, refs), (Instant::now(), stack.clone()));
        Ok(stack)
    }

    async fn stack_neighbors(&self, repo: &RepoRef, pr: &StackPr) -> ApiResult<(Vec<StackPr>, Vec<StackPr>)> {
        #[derive(Deserialize)]
        struct Repo {
            parents: Nodes<StackPr>,
            children: Nodes<StackPr>,
        }
        #[derive(Deserialize)]
        struct R {
            repository: Option<Repo>,
        }
        let r: R = self
            .graphql("pullRequestStackNeighbors", &q::pr_stack_neighbors(), json!({ "o": repo.owner, "n": repo.name, "head": pr.head_ref_name, "base": pr.base_ref_name }), false)
            .await?;
        Ok(r.repository.map(|r| (r.parents.nodes, r.children.nodes)).unwrap_or_default())
    }

    pub async fn badges(&self) -> ApiResult<BadgeResponse> {
        self.graphql("badges", &q::badges(), json!({}), false).await
    }

    pub async fn pull_request(&self, r: &ItemRef) -> ApiResult<PrDetailResponse> {
        self.graphql("pullRequest", &q::pull_request(), json!({ "owner": r.repo.owner, "name": r.repo.name, "number": r.number }), false).await
    }

    pub async fn pull_request_light(&self, r: &ItemRef) -> ApiResult<Option<PrLight>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Repo {
            pull_request: Option<PrLight>,
        }
        #[derive(Deserialize)]
        struct R {
            repository: Option<Repo>,
        }
        let r: R = self.graphql("pullRequestLight", &q::pull_request_light(), json!({ "owner": r.repo.owner, "name": r.repo.name, "number": r.number }), false).await?;
        Ok(r.repository.and_then(|r| r.pull_request))
    }

    pub async fn issue(&self, r: &ItemRef) -> ApiResult<IssueDetailResponse> {
        self.graphql("issue", &q::issue(), json!({ "owner": r.repo.owner, "name": r.repo.name, "number": r.number }), false).await
    }

    pub async fn pull_request_files(&self, r: &ItemRef) -> ApiResult<Vec<PrFile>> {
        let mut all = vec![];
        for page in 1..=30 {
            let batch: Vec<PrFile> = self.rest(Method::GET, &format!("/repos/{}/pulls/{}/files?per_page=100&page={page}", r.repo.full(), r.number), None).await?;
            let n = batch.len();
            all.extend(batch);
            if n < 100 {
                break;
            }
        }
        Ok(all)
    }

    pub async fn commit(&self, repo: &RepoRef, sha: &str) -> ApiResult<CommitDetailRest> {
        self.rest(Method::GET, &format!("/repos/{}/commits/{sha}", repo.full()), None).await
    }

    pub async fn repos(&self, after: Option<String>) -> ApiResult<RepoConnection> {
        let r: ReposResponse = self.graphql("repos", q::REPOS, json!({ "after": after }), false).await?;
        Ok(r.viewer.repositories)
    }

    pub async fn repo_info(&self, repo: &RepoRef) -> ApiResult<Option<RepoInfo>> {
        let r: RepoInfoResponse = self.graphql("repoInfo", q::REPO_INFO, json!({ "owner": repo.owner, "name": repo.name }), false).await?;
        Ok(r.repository)
    }

    pub async fn projects(&self) -> ApiResult<Vec<ProjectSummary>> {
        let r: ProjectsResponse = self.graphql("projects", &q::projects(), json!({}), false).await?;
        let mut seen = HashSet::new();
        let mut all = r.viewer.projects_v2.nodes;
        for org in r.viewer.organizations.nodes {
            all.extend(org.projects_v2.nodes);
        }
        Ok(all.into_iter().filter(|p| seen.insert(p.id.clone())).collect())
    }

    pub async fn project(&self, id: &str) -> ApiResult<(ProjectDetail, Vec<ProjectItem>)> {
        let mut after: Option<String> = None;
        let mut items = vec![];
        let mut detail: Option<ProjectDetail> = None;
        for _ in 0..10 {
            let r: ProjectDetailResponse = self.graphql("project", q::PROJECT, json!({ "id": id, "after": after }), false).await?;
            let node = r.node.ok_or_else(|| ApiError::new("Project not found"))?;
            items.extend(node.items.nodes.clone());
            let more = node.items.page_info.has_next_page;
            let cursor = node.items.page_info.end_cursor.clone();
            if detail.is_none() {
                detail = Some(node);
            }
            if !more {
                break;
            }
            after = cursor;
        }
        Ok((detail.ok_or_else(|| ApiError::new("Project not found"))?, items))
    }

    pub async fn move_project_item(&self, project: &str, item: &str, field: &str, option: Option<String>) -> ApiResult<()> {
        match option {
            Some(o) => self.mutate(q::MOVE_PROJECT_ITEM, json!({ "p": project, "i": item, "f": field, "o": o })).await,
            None => self.mutate(q::CLEAR_PROJECT_ITEM, json!({ "p": project, "i": item, "f": field })).await,
        }
    }

    pub async fn notifications(&self, all: bool, force: bool) -> ApiResult<Vec<NotificationThread>> {
        if crate::demo::is_on() {
            return Ok(vec![]);
        }
        let bytes = self.rest_polled(&format!("/notifications?all={all}&per_page=50"), force).await?;
        serde_json::from_slice(&bytes).map_err(|e| ApiError::new(format!("Failed to parse response: {e}")))
    }

    pub async fn unread_notification_count(&self, force: bool) -> ApiResult<usize> {
        let bytes = self.rest_polled("/notifications?per_page=50", force).await?;
        let list: Vec<Value> = serde_json::from_slice(&bytes).map_err(|e| ApiError::new(format!("Failed to parse response: {e}")))?;
        Ok(list.len())
    }

    pub async fn mark_notification_read(&self, id: &str) -> ApiResult<()> {
        let result = self.send(Method::PATCH, &format!("/notifications/threads/{id}"), None).await;
        self.invalidate_polled();
        result
    }

    pub async fn mark_notification_done(&self, id: &str) -> ApiResult<()> {
        let result = self.send(Method::DELETE, &format!("/notifications/threads/{id}"), None).await;
        self.invalidate_polled();
        result
    }

    pub async fn mark_all_notifications_read(&self) -> ApiResult<()> {
        let result = self.send(Method::PUT, "/notifications", Some(json!({ "read": true }))).await;
        self.invalidate_polled();
        result
    }

    pub async fn add_comment(&self, subject: &str, body: &str) -> ApiResult<()> {
        self.mutate(q::ADD_COMMENT, json!({ "id": subject, "b": body })).await
    }

    pub async fn add_review(&self, pr: &str, event: &str, body: Option<String>, comments: &[PendingComment]) -> ApiResult<()> {
        let threads: Vec<Value> = comments.iter().map(|c| json!({ "path": c.path, "line": c.line, "side": c.side, "body": c.body })).collect();
        let body = body.filter(|b| !b.is_empty());
        let th = if threads.is_empty() { Value::Null } else { Value::Array(threads) };
        self.mutate(q::ADD_REVIEW, json!({ "id": pr, "e": event, "b": body, "th": th })).await
    }

    pub async fn request_reviewers(&self, r: &ItemRef, logins: &[String]) -> ApiResult<()> {
        let teams: Vec<String> = logins.iter().filter(|l| l.contains('/')).filter_map(|l| l.rsplit('/').next().map(|s| s.to_string())).collect();
        let users: Vec<String> = logins.iter().filter(|l| !l.contains('/')).cloned().collect();
        let mut body = serde_json::Map::new();
        if !users.is_empty() {
            body.insert("reviewers".into(), json!(users));
        }
        if !teams.is_empty() {
            body.insert("team_reviewers".into(), json!(teams));
        }
        self.send(Method::POST, &format!("/repos/{}/pulls/{}/requested_reviewers", r.repo.full(), r.number), Some(Value::Object(body))).await
    }

    pub async fn delete_branch(&self, repo: &str, branch: &str) -> ApiResult<()> {
        self.send(Method::DELETE, &format!("/repos/{repo}/git/refs/heads/{}", encode_path(branch)), None).await
    }

    pub async fn rerun_failed_jobs(&self, repo: &RepoRef, run_id: i64) -> ApiResult<()> {
        self.send(Method::POST, &format!("/repos/{}/actions/runs/{run_id}/rerun-failed-jobs", repo.full()), None).await
    }

    pub async fn check_annotations(&self, repo: &RepoRef, check_run_id: i64) -> ApiResult<Vec<CheckAnnotation>> {
        self.rest(Method::GET, &format!("/repos/{}/check-runs/{check_run_id}/annotations?per_page=50", repo.full()), None).await
    }

    pub async fn check_run(&self, repo: &RepoRef, check_run_id: i64) -> ApiResult<CheckRunRest> {
        self.rest(Method::GET, &format!("/repos/{}/check-runs/{check_run_id}", repo.full()), None).await
    }

    pub async fn job_log_tail(&self, repo: &RepoRef, job_id: i64, max_bytes: usize) -> ApiResult<(String, bool)> {
        let target = self.redirect_target(&format!("/repos/{}/actions/jobs/{job_id}/logs", repo.full())).await?;
        self.plain_tail(&target, max_bytes).await
    }

    pub async fn job_log(&self, repo: &RepoRef, job_id: i64) -> ApiResult<String> {
        let target = self.redirect_target(&format!("/repos/{}/actions/jobs/{job_id}/logs", repo.full())).await?;
        self.plain(&target).await
    }

    pub async fn git_object(&self, repo: &RepoRef, git_ref: &str, path: &str) -> ApiResult<Option<GitObject>> {
        let r: GitObjectResponse = self.graphql("gitObject", q::GIT_OBJECT, json!({ "owner": repo.owner, "name": repo.name, "expr": format!("{git_ref}:{path}") }), false).await?;
        Ok(r.repository.and_then(|r| r.object))
    }

    pub async fn blame(&self, repo: &RepoRef, git_ref: &str, path: &str) -> ApiResult<Option<Vec<BlameRange>>> {
        let r: BlameResponse = self.graphql("Blame", q::BLAME, json!({ "owner": repo.owner, "name": repo.name, "ref": git_ref, "path": path }), true).await?;
        Ok(r.repository.and_then(|r| r.object).and_then(|o| o.ranges()))
    }

    pub async fn branches(&self, repo: &RepoRef) -> ApiResult<Vec<BranchInfo>> {
        let mut all = vec![];
        for page in 1..=10 {
            let batch: Vec<BranchInfo> = self.rest(Method::GET, &format!("/repos/{}/branches?per_page=100&page={page}", repo.full()), None).await?;
            let n = batch.len();
            all.extend(batch);
            if n < 100 {
                break;
            }
        }
        Ok(all)
    }

    pub async fn all_paths(&self, repo: &RepoRef, sha: &str) -> ApiResult<(Vec<String>, bool)> {
        let r: GitTreeRest = self.rest(Method::GET, &format!("/repos/{}/git/trees/{}?recursive=1", repo.full(), encode_query(sha)), None).await?;
        Ok((r.tree.into_iter().filter(|t| t.kind == "blob").map(|t| t.path).collect(), r.truncated))
    }

    pub async fn readme_html(&self, repo: &RepoRef, git_ref: &str, dir: &str) -> ApiResult<Option<String>> {
        let suffix = if dir.is_empty() { String::new() } else { format!("/{}", encode_path(dir)) };
        let data = self.fetch(&format!("/repos/{}/readme{suffix}?ref={}", repo.full(), encode_query(git_ref)), "application/vnd.github.html+json").await?;
        Ok(data.map(|d| String::from_utf8_lossy(&d).to_string()))
    }

    pub async fn rendered_file(&self, repo: &RepoRef, git_ref: &str, path: &str) -> ApiResult<Option<String>> {
        let data = self.fetch(&format!("/repos/{}/contents/{}?ref={}", repo.full(), encode_path(path), encode_query(git_ref)), "application/vnd.github.html+json").await?;
        Ok(data.map(|d| String::from_utf8_lossy(&d).to_string()))
    }

    pub async fn raw_file(&self, repo: &RepoRef, git_ref: &str, path: &str) -> ApiResult<Option<Vec<u8>>> {
        self.fetch(&format!("/repos/{}/contents/{}?ref={}", repo.full(), encode_path(path), encode_query(git_ref)), "application/vnd.github.raw").await
    }

    pub async fn avatar(url: &str, size: i32) -> Option<Vec<u8>> {
        let mut u = url::Url::parse(url).ok()?;
        let pairs: Vec<(String, String)> = u.query_pairs().filter(|(k, _)| k != "s").map(|(k, v)| (k.to_string(), v.to_string())).collect();
        u.query_pairs_mut().clear().extend_pairs(pairs).append_pair("s", &(size * 2).to_string());
        let mut probe = crate::diagnostics::begin_rest("GET", u.as_str());
        let (status, bytes) = super::probed_get(&mut probe, super::http_client().get(u.as_str())).await.ok()?;
        if !status.is_success() {
            probe.fail_with(|| format!("HTTP {}", status.as_u16()));
            return None;
        }
        Some(bytes)
    }
}
