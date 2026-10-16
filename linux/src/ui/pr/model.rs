use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::api::models::*;
use crate::api::queries as q;
use crate::api::endpoints::StackRefs;
use crate::api::{ApiError, GitHubApi};
use crate::conversation_html::{self, Look};
use crate::diff::{DiffAnchor, DiffFile};
use crate::signal::Signal;

const FULL_REFRESH_EVERY: Duration = Duration::from_secs(300);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrTab {
    Conversation,
    Commits,
    Checks,
    Files,
}

impl PrTab {
    pub fn id(&self) -> &'static str {
        match self {
            PrTab::Conversation => "conversation",
            PrTab::Commits => "commits",
            PrTab::Checks => "checks",
            PrTab::Files => "files",
        }
    }

    pub fn from_id(id: &str) -> Self {
        match id {
            "commits" => PrTab::Commits,
            "checks" => PrTab::Checks,
            "files" => PrTab::Files,
            _ => PrTab::Conversation,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Change {
    Pr,
    Files,
    Busy,
    Stack,
    Tab,
    Diff,
    ScrollBottom,
    Reveal(String),
    ActionError(String),
}

#[derive(Clone)]
pub enum FlatRow {
    Header(usize),
    Line(usize, usize),
    Thread(Box<ReviewThread>),
    Pending(PendingComment),
    Composer(DiffAnchor),
    Note(String),
}

#[derive(Clone)]
pub struct ReviewerInfo {
    pub login: String,
    pub avatar: Option<String>,
    pub state: String,
}

pub struct PrModel {
    pub r: ItemRef,
    pub api: GitHubApi,
    pub repo: RefCell<Option<PrRepository>>,
    pub pr: RefCell<Option<PullRequestDetail>>,
    pub viewer_login: RefCell<String>,
    pub not_pr: Cell<bool>,
    pub loading: Cell<bool>,
    pub last_updated: Cell<Option<DateTime<Utc>>>,
    pub refresh_error: RefCell<Option<String>>,
    pub error: RefCell<Option<String>>,
    pub busy: Cell<bool>,
    pub tab: Cell<PrTab>,
    pub stack: RefCell<Vec<StackPr>>,
    pub html: RefCell<String>,
    pub files: RefCell<Vec<DiffFile>>,
    pub files_loaded: Cell<bool>,
    pub files_loading: Cell<bool>,
    pub files_error: RefCell<Option<String>>,
    pub viewed: RefCell<HashSet<String>>,
    pub collapsed: RefCell<HashSet<String>>,
    pub pending: RefCell<Vec<PendingComment>>,
    pub composer: RefCell<Option<DiffAnchor>>,
    pub composer_text: RefCell<String>,
    thread_index: RefCell<HashMap<String, Vec<ReviewThread>>>,
    outdated_by_path: RefCell<HashMap<String, Vec<ReviewThread>>>,
    active: Cell<bool>,
    queued: Cell<bool>,
    want_full: Cell<bool>,
    last_full: Cell<Option<Instant>>,
    stack_fresh: Cell<bool>,
    reload_files: Cell<bool>,
    files_stale: Cell<bool>,
    files_oid: RefCell<Option<String>>,
    merge_rechecks: Cell<u32>,
    recheck_pending: Cell<bool>,
    pub changed: Signal<Change>,
}

impl PrModel {
    pub fn new(r: ItemRef, api: GitHubApi) -> Rc<Self> {
        Rc::new(PrModel {
            r,
            api,
            repo: RefCell::new(None),
            pr: RefCell::new(None),
            viewer_login: RefCell::new(String::new()),
            not_pr: Cell::new(false),
            loading: Cell::new(false),
            last_updated: Cell::new(None),
            refresh_error: RefCell::new(None),
            error: RefCell::new(None),
            busy: Cell::new(false),
            tab: Cell::new(PrTab::Conversation),
            stack: RefCell::new(vec![]),
            html: RefCell::new(String::new()),
            files: RefCell::new(vec![]),
            files_loaded: Cell::new(false),
            files_loading: Cell::new(false),
            files_error: RefCell::new(None),
            viewed: RefCell::new(HashSet::new()),
            collapsed: RefCell::new(HashSet::new()),
            pending: RefCell::new(vec![]),
            composer: RefCell::new(None),
            composer_text: RefCell::new(String::new()),
            thread_index: RefCell::new(HashMap::new()),
            outdated_by_path: RefCell::new(HashMap::new()),
            active: Cell::new(false),
            queued: Cell::new(false),
            want_full: Cell::new(false),
            last_full: Cell::new(None),
            stack_fresh: Cell::new(false),
            reload_files: Cell::new(false),
            files_stale: Cell::new(false),
            files_oid: RefCell::new(None),
            merge_rechecks: Cell::new(0),
            recheck_pending: Cell::new(false),
            changed: Signal::new(),
        })
    }

    pub fn inject(self: &Rc<Self>, resp: PrDetailResponse, files: Vec<PrFile>) {
        *self.viewer_login.borrow_mut() = resp.viewer.login.clone();
        let Some(mut repo) = resp.repository else { return };
        let Some(pr) = repo.pull_request.take() else { return };
        *self.html.borrow_mut() = conversation_html::render_pr(&pr, Look::current());
        *self.viewed.borrow_mut() = pr.files.as_ref().map(|f| f.nodes.iter().filter(|v| v.viewer_viewed_state == "VIEWED").map(|v| v.path.clone()).collect()).unwrap_or_default();
        self.index_threads(&pr.review_threads.nodes);
        *self.files_oid.borrow_mut() = Some(pr.head_ref_oid.clone());
        *self.repo.borrow_mut() = Some(repo);
        *self.pr.borrow_mut() = Some(pr);
        self.last_updated.set(Some(Utc::now()));
        *self.files.borrow_mut() = files.iter().map(DiffFile::new).collect();
        self.files_loaded.set(true);
        self.emit(Change::Pr);
        self.emit(Change::Files);
    }

    pub fn emit(&self, c: Change) {
        self.changed.emit(&c);
    }

    pub fn set_tab(&self, t: PrTab) {
        if self.tab.get() != t {
            self.tab.set(t);
            self.emit(Change::Tab);
        }
    }

    pub fn can_write(&self) -> bool {
        self.repo.borrow().as_ref().map(|r| r.can_write()).unwrap_or(false)
    }

    pub fn reviewers(&self) -> Vec<ReviewerInfo> {
        let pr = self.pr.borrow();
        let Some(pr) = pr.as_ref() else { return vec![] };
        let mut result: Vec<ReviewerInfo> = pr
            .latest_opinionated_reviews
            .nodes
            .iter()
            .filter_map(|r| r.author.as_ref().map(|a| ReviewerInfo { login: a.login.clone(), avatar: a.avatar_url.clone(), state: r.state.clone() }))
            .collect();
        let done: HashSet<String> = result.iter().map(|r| r.login.clone()).collect();
        for req in &pr.review_requests.nodes {
            if let Some(r) = &req.requested_reviewer {
                if !done.contains(&r.display()) {
                    result.push(ReviewerInfo { login: r.display(), avatar: r.avatar_url.clone(), state: "PENDING".into() });
                }
            }
        }
        result
    }

    pub fn stale_reviews(&self) -> Vec<(String, Option<usize>)> {
        let pr = self.pr.borrow();
        let Some(pr) = pr.as_ref() else { return vec![] };
        if !pr.is_open() {
            return vec![];
        }
        let viewer = self.viewer_login.borrow().clone();
        let pending: HashSet<String> = pr.review_requests.nodes.iter().filter_map(|r| r.requested_reviewer.as_ref().map(|x| x.display())).collect();
        let oids: Vec<&str> = pr.all_commits.nodes.iter().map(|c| c.commit.oid.as_str()).collect();
        pr.latest_opinionated_reviews
            .nodes
            .iter()
            .filter_map(|r| {
                let login = r.author.as_ref()?.login.clone();
                if login == viewer || login.ends_with("[bot]") || pending.contains(&login) {
                    return None;
                }
                let oid = r.commit.as_ref()?.oid.clone();
                if oid == pr.head_ref_oid {
                    return None;
                }
                let after = oids.iter().rposition(|o| *o == oid).map(|i| oids.len() - 1 - i).filter(|n| *n > 0);
                Some((login, after))
            })
            .collect()
    }

    pub fn reviewers_to_rerequest(&self) -> Vec<String> {
        self.stale_reviews().into_iter().map(|s| s.0).collect()
    }

    pub fn thread_count(&self, path: &str) -> usize {
        self.pr.borrow().as_ref().map(|p| p.review_threads.nodes.iter().filter(|t| t.path == path).count()).unwrap_or(0)
    }

    pub fn load(self: &Rc<Self>, full: bool) {
        if full {
            self.stack_fresh.set(true);
            self.reload_files.set(true);
        }
        self.want_full.set(true);
        self.start();
    }

    pub fn refresh_live(self: &Rc<Self>) {
        self.start();
    }

    fn light_ready(&self) -> bool {
        let recent = self.last_full.get().is_some_and(|t| t.elapsed() < FULL_REFRESH_EVERY);
        recent && self.pr.borrow().as_ref().is_some_and(|p| p.light.is_known())
    }

    fn start(self: &Rc<Self>) {
        if self.active.get() {
            self.queued.set(true);
            return;
        }
        let me = self.clone();
        crate::rt::spawn_local(async move {
            loop {
                me.active.set(true);
                me.queued.set(false);
                let full = me.want_full.replace(false) || !me.light_ready();
                if full {
                    me.clone().fetch().await;
                } else {
                    me.clone().check().await;
                }
                me.active.set(false);
                if !me.queued.get() || (full && !me.want_full.get()) {
                    me.queued.set(false);
                    break;
                }
            }
        });
    }

    async fn check(self: Rc<Self>) {
        self.loading.set(true);
        self.emit(Change::Busy);
        let api = self.api.clone();
        let r = self.r.clone();
        let res = crate::rt::run(async move { api.pull_request_light(&r).await }).await;
        self.loading.set(false);
        let current = self.pr.borrow().as_ref().map(|p| p.light.clone().normalized());
        match res {
            Ok(Some(light)) if current.as_ref() == Some(&light.clone().normalized()) => {
                *self.refresh_error.borrow_mut() = None;
                self.last_updated.set(Some(Utc::now()));
                self.emit(Change::Busy);
                self.schedule_merge_recheck(light.merge_unknown());
            }
            Ok(_) => self.fetch().await,
            Err(e) => {
                *self.refresh_error.borrow_mut() = Some(e.message);
                self.emit(Change::Busy);
            }
        }
    }

    async fn fetch(self: Rc<Self>) {
        self.loading.set(true);
        self.emit(Change::Busy);
        let api = self.api.clone();
        let r = self.r.clone();
        let res = crate::rt::run(async move { api.pull_request(&r).await }).await;
        self.loading.set(false);
        match res {
            Ok(resp) => {
                *self.viewer_login.borrow_mut() = resp.viewer.login.clone();
                let Some(mut repo) = resp.repository else {
                    self.not_pr.set(true);
                    self.emit(Change::Pr);
                    return;
                };
                let Some(pr) = repo.pull_request.take() else {
                    self.not_pr.set(true);
                    self.emit(Change::Pr);
                    return;
                };
                let html = conversation_html::render_pr(&pr, Look::current());
                *self.html.borrow_mut() = html;
                *self.viewed.borrow_mut() = pr.files.as_ref().map(|f| f.nodes.iter().filter(|v| v.viewer_viewed_state == "VIEWED").map(|v| v.path.clone()).collect()).unwrap_or_default();
                self.index_threads(&pr.review_threads.nodes);
                let head = pr.head_ref_oid.clone();
                let refs = StackRefs {
                    head_owner: pr.head_repository.as_ref().and_then(|h| h.name_with_owner.split('/').next()).unwrap_or("").to_string(),
                    head: pr.head_ref_name.clone(),
                    base: pr.base_ref_name.clone(),
                };
                let recheck = pr.is_open() && (pr.merge_state_status.as_deref() == Some("UNKNOWN") || pr.mergeable == "UNKNOWN");
                let files_stale = self.reload_files.replace(false) || self.files_oid.borrow().as_deref() != Some(head.as_str());
                if files_stale && self.files_loaded.get() {
                    self.files_stale.set(true);
                }
                *self.repo.borrow_mut() = Some(repo);
                *self.pr.borrow_mut() = Some(pr);
                *self.error.borrow_mut() = None;
                *self.refresh_error.borrow_mut() = None;
                self.last_updated.set(Some(Utc::now()));
                self.last_full.set(Some(Instant::now()));
                self.emit(Change::Pr);
                let me = self.clone();
                crate::rt::spawn_local(async move { me.load_stack(refs).await });
                self.schedule_merge_recheck(recheck);
            }
            Err(e) => {
                if self.pr.borrow().is_none() {
                    *self.error.borrow_mut() = Some(e.message);
                } else {
                    *self.refresh_error.borrow_mut() = Some(e.message);
                }
                self.emit(Change::Pr);
            }
        }
    }

    pub fn rerender_html(&self) {
        if let Some(pr) = self.pr.borrow().as_ref() {
            *self.html.borrow_mut() = conversation_html::render_pr(pr, Look::current());
        }
        self.emit(Change::Pr);
    }

    fn schedule_merge_recheck(self: &Rc<Self>, unknown: bool) {
        if !unknown {
            self.merge_rechecks.set(0);
            return;
        }
        if self.merge_rechecks.get() >= 5 || self.recheck_pending.get() {
            return;
        }
        self.merge_rechecks.set(self.merge_rechecks.get() + 1);
        self.recheck_pending.set(true);
        let delay = self.merge_rechecks.get() as u64 * 2;
        let weak = Rc::downgrade(self);
        gtk::glib::timeout_add_local_once(Duration::from_secs(delay), move || {
            if let Some(me) = weak.upgrade() {
                me.recheck_pending.set(false);
                me.refresh_live();
            }
        });
    }

    async fn load_stack(self: Rc<Self>, refs: StackRefs) {
        let fresh = self.stack_fresh.replace(false);
        let api = self.api.clone();
        let r = self.r.clone();
        if let Ok(s) = crate::rt::run(async move { api.pull_request_stack_cached(&r, &refs, fresh).await }).await {
            if *self.stack.borrow() != s {
                *self.stack.borrow_mut() = s;
                self.emit(Change::Stack);
            }
        }
    }

    pub fn ensure_files(self: &Rc<Self>) {
        if self.pr.borrow().is_none() || self.files_loading.get() || (self.files_loaded.get() && !self.files_stale.get()) {
            return;
        }
        let me = self.clone();
        crate::rt::spawn_local(async move { me.load_files(true).await });
    }

    pub fn refresh_stale_files(self: &Rc<Self>) {
        if self.files_loaded.get() && self.files_stale.get() {
            self.ensure_files();
        }
    }

    pub async fn load_files(self: Rc<Self>, force: bool) {
        if !force && (self.files_loaded.get() || self.files_loading.get()) {
            return;
        }
        self.files_loading.set(true);
        self.files_stale.set(false);
        let oid = self.pr.borrow().as_ref().map(|p| p.head_ref_oid.clone());
        let api = self.api.clone();
        let r = self.r.clone();
        let res = crate::rt::run(async move { api.pull_request_files(&r).await.map(|raw| raw.iter().map(DiffFile::new).collect::<Vec<_>>()) }).await;
        self.files_loading.set(false);
        match res {
            Ok(parsed) => {
                if !self.files_loaded.get() {
                    let viewed = self.viewed.borrow().clone();
                    *self.collapsed.borrow_mut() = parsed.iter().filter(|f| viewed.contains(&f.id) || f.lines.len() > 1500).map(|f| f.id.clone()).collect();
                }
                let head = self.pr.borrow().as_ref().map(|p| p.head_ref_oid.clone());
                if head.is_some() && head != oid {
                    self.files_stale.set(true);
                }
                *self.files.borrow_mut() = parsed;
                *self.files_oid.borrow_mut() = oid;
                self.files_loaded.set(true);
                *self.files_error.borrow_mut() = None;
            }
            Err(e) => *self.files_error.borrow_mut() = Some(e.message),
        }
        self.emit(Change::Files);
    }

    fn index_threads(&self, threads: &[ReviewThread]) {
        let mut idx: HashMap<String, Vec<ReviewThread>> = HashMap::new();
        let mut outdated: HashMap<String, Vec<ReviewThread>> = HashMap::new();
        for t in threads {
            match t.line {
                Some(line) => idx.entry(format!("{}|{}|{line}", t.path, t.diff_side)).or_default().push(t.clone()),
                None => outdated.entry(t.path.clone()).or_default().push(t.clone()),
            }
        }
        *self.thread_index.borrow_mut() = idx;
        *self.outdated_by_path.borrow_mut() = outdated;
    }

    pub fn flat_rows(&self, filter: &str) -> Vec<FlatRow> {
        let files = self.files.borrow();
        let collapsed = self.collapsed.borrow();
        let pending = self.pending.borrow();
        let composer = self.composer.borrow().clone();
        let index = self.thread_index.borrow();
        let outdated = self.outdated_by_path.borrow();
        let mut out = vec![];
        for (fi, file) in files.iter().enumerate() {
            if !filter.is_empty() && !crate::util::contains_ci(&file.id, filter) {
                continue;
            }
            out.push(FlatRow::Header(fi));
            if collapsed.contains(&file.id) {
                continue;
            }
            if !file.has_patch {
                out.push(FlatRow::Note("Diff unavailable (binary or too large)".into()));
                continue;
            }
            let file_pending: Vec<&PendingComment> = pending.iter().filter(|p| p.path == file.id).collect();
            for (li, line) in file.lines.iter().enumerate() {
                out.push(FlatRow::Line(fi, li));
                if !line.commentable() {
                    continue;
                }
                let mut keys = vec![];
                if let (Some(n), true) = (line.new_line, line.kind != crate::diff::LineKind::Del) {
                    keys.push(format!("{}|RIGHT|{n}", file.id));
                }
                if let (Some(o), true) = (line.old_line, line.kind != crate::diff::LineKind::Add) {
                    keys.push(format!("{}|LEFT|{o}", file.id));
                }
                for k in keys {
                    if let Some(ts) = index.get(&k) {
                        for t in ts {
                            out.push(FlatRow::Thread(Box::new(t.clone())));
                        }
                    }
                }
                if let Some(anchor) = line.anchor_line() {
                    for p in &file_pending {
                        if p.line == anchor && p.side == line.anchor_side() {
                            out.push(FlatRow::Pending((*p).clone()));
                        }
                    }
                    if let Some(c) = &composer {
                        if c.path == file.id && c.line == anchor && c.side == line.anchor_side() {
                            out.push(FlatRow::Composer(c.clone()));
                        }
                    }
                }
            }
            if let Some(ts) = outdated.get(&file.id) {
                if !ts.is_empty() {
                    out.push(FlatRow::Note("Outdated comments".into()));
                    for t in ts {
                        out.push(FlatRow::Thread(Box::new(t.clone())));
                    }
                }
            }
        }
        out
    }

    pub fn reveal_file(self: &Rc<Self>, path: &str) {
        self.set_tab(PrTab::Files);
        self.collapsed.borrow_mut().remove(path);
        self.emit(Change::Diff);
        self.emit(Change::Reveal(path.to_string()));
        self.ensure_files();
    }

    pub async fn run<F, Fut>(self: &Rc<Self>, reload: bool, op: F) -> bool
    where
        F: FnOnce(GitHubApi) -> Fut,
        Fut: std::future::Future<Output = Result<(), ApiError>> + Send + 'static,
    {
        self.busy.set(true);
        self.emit(Change::Busy);
        let fut = op(self.api.clone());
        let res = crate::rt::run(fut).await;
        self.busy.set(false);
        self.emit(Change::Busy);
        match res {
            Ok(()) => {
                if reload {
                    self.stack_fresh.set(true);
                    self.load(false);
                }
                true
            }
            Err(e) => {
                self.emit(Change::ActionError(e.message));
                false
            }
        }
    }

    fn pr_id(&self) -> Option<String> {
        self.pr.borrow().as_ref().map(|p| p.id.clone())
    }

    pub async fn comment(self: Rc<Self>, body: String) -> bool {
        let Some(id) = self.pr_id() else { return false };
        let ok = self.run(true, move |api| async move { api.add_comment(&id, &body).await }).await;
        if ok {
            self.emit(Change::ScrollBottom);
        }
        ok
    }

    pub async fn submit_review(self: Rc<Self>, event: String, body: String) -> bool {
        let Some(id) = self.pr_id() else { return false };
        let comments = self.pending.borrow().clone();
        let ok = self.run(true, move |api| async move { api.add_review(&id, &event, Some(body), &comments).await }).await;
        if ok {
            self.pending.borrow_mut().clear();
            self.emit(Change::Diff);
            self.emit(Change::ScrollBottom);
        }
        ok
    }

    pub async fn add_inline(self: Rc<Self>, anchor: DiffAnchor, body: String, immediately: bool) -> bool {
        let c = PendingComment { id: uuid::Uuid::new_v4().to_string(), path: anchor.path.clone(), line: anchor.line, side: anchor.side.clone(), body };
        if immediately {
            let Some(id) = self.pr_id() else { return false };
            let ok = self.run(true, move |api| async move { api.add_review(&id, "COMMENT", None, &[c]).await }).await;
            if ok {
                *self.composer.borrow_mut() = None;
                self.composer_text.borrow_mut().clear();
                self.emit(Change::Diff);
            }
            return ok;
        }
        self.pending.borrow_mut().push(c);
        *self.composer.borrow_mut() = None;
        self.composer_text.borrow_mut().clear();
        self.emit(Change::Diff);
        true
    }

    pub async fn reply(self: Rc<Self>, thread_id: String, body: String) -> bool {
        self.run(true, move |api| async move { api.mutate(q::REPLY_THREAD, json!({ "id": thread_id, "b": body })).await }).await
    }

    pub async fn set_resolved(self: Rc<Self>, thread_id: String, resolved: bool) -> bool {
        let query = if resolved { q::RESOLVE_THREAD } else { q::UNRESOLVE_THREAD };
        self.run(true, move |api| async move { api.mutate(query, json!({ "id": thread_id })).await }).await
    }

    pub async fn merge(self: Rc<Self>, method: String, title: Option<String>, body: Option<String>, delete_branch: bool) -> bool {
        let Some(pr) = self.pr.borrow().clone() else { return false };
        self.run(true, move |api| async move {
            api.mutate(q::MERGE, json!({ "id": pr.id, "m": method, "t": title, "b": body })).await?;
            if delete_branch {
                if let Some(head) = pr.head_repository.as_ref() {
                    let _ = api.delete_branch(&head.name_with_owner, &pr.head_ref_name).await;
                }
            }
            Ok(())
        })
        .await
    }

    pub async fn enable_auto_merge(self: Rc<Self>, method: String) -> bool {
        let Some(id) = self.pr_id() else { return false };
        self.run(true, move |api| async move { api.mutate(q::ENABLE_AUTO_MERGE, json!({ "id": id, "m": method })).await }).await
    }

    pub async fn simple(self: Rc<Self>, mutation: &'static str) -> bool {
        let Some(id) = self.pr_id() else { return false };
        self.run(true, move |api| async move { api.mutate(mutation, json!({ "id": id })).await }).await
    }

    pub async fn edit(self: Rc<Self>, title: String, body: String) -> bool {
        let Some(id) = self.pr_id() else { return false };
        self.run(true, move |api| async move { api.mutate(q::EDIT_PR, json!({ "id": id, "t": title, "b": body })).await }).await
    }

    pub async fn request_reviewers(self: Rc<Self>, logins: Vec<String>) -> bool {
        let r = self.r.clone();
        self.run(true, move |api| async move { api.request_reviewers(&r, &logins).await }).await
    }

    pub async fn delete_branch(self: Rc<Self>) -> bool {
        let Some(pr) = self.pr.borrow().clone() else { return false };
        let Some(head) = pr.head_repository.clone() else { return false };
        self.run(true, move |api| async move { api.delete_branch(&head.name_with_owner, &pr.head_ref_name).await }).await
    }

    pub async fn toggle_viewed(self: Rc<Self>, path: String) {
        let Some(id) = self.pr_id() else { return };
        let viewed = self.viewed.borrow().contains(&path);
        if viewed {
            self.viewed.borrow_mut().remove(&path);
        } else {
            self.viewed.borrow_mut().insert(path.clone());
            self.collapsed.borrow_mut().insert(path.clone());
        }
        self.emit(Change::Diff);
        let query = if viewed { q::UNMARK_VIEWED } else { q::MARK_VIEWED };
        self.run(false, move |api| async move { api.mutate(query, json!({ "id": id, "p": path })).await }).await;
    }

    pub async fn rerun_failed(self: Rc<Self>) -> bool {
        let runs: HashSet<i64> = self.pr.borrow().as_ref().map(|p| p.checks().iter().filter(|c| c.outcome() == CheckOutcome::Failure).filter_map(|c| c.run_id()).collect()).unwrap_or_default();
        let repo = self.r.repo.clone();
        self.run(true, move |api| async move {
            for id in runs {
                api.rerun_failed_jobs(&repo, id).await?;
            }
            Ok(())
        })
        .await
    }
}
