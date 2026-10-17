use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};

use crate::api::models::*;
use crate::api::{ApiError, GitHubApi};
use crate::brand;
use crate::claude::workspace::Workspace;
use crate::demo;
use crate::forge::{ForgeCapabilities, ForgeClient};
use crate::git_accounts::{self, GitAccount, GitCredential, GitProvider};
use crate::secret;
use crate::settings;
use crate::signal::Signal;
use crate::ui;
use crate::ui::pr::model::PrModel;
use crate::util;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PrFilter {
    ReviewRequested,
    Mine,
    Assigned,
    Mentioned,
    Reviewed,
}

impl PrFilter {
    pub const ALL: [PrFilter; 5] = [PrFilter::ReviewRequested, PrFilter::Mine, PrFilter::Assigned, PrFilter::Mentioned, PrFilter::Reviewed];

    pub fn key(&self) -> &'static str {
        match self {
            PrFilter::ReviewRequested => "reviewRequested",
            PrFilter::Mine => "mine",
            PrFilter::Assigned => "assigned",
            PrFilter::Mentioned => "mentioned",
            PrFilter::Reviewed => "reviewed",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == k)
    }

    pub fn title(&self) -> &'static str {
        match self {
            PrFilter::ReviewRequested => "Review requests",
            PrFilter::Mine => "Created by me",
            PrFilter::Assigned => "Assigned to me",
            PrFilter::Mentioned => "Mentioned",
            PrFilter::Reviewed => "Reviewed by me",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            PrFilter::ReviewRequested => "sy-eye-symbolic",
            PrFilter::Mine => "sy-person-symbolic",
            PrFilter::Assigned => "sy-clock-symbolic",
            PrFilter::Mentioned => "sy-at-symbolic",
            PrFilter::Reviewed => "sy-check-symbolic",
        }
    }

    pub fn tint(&self) -> &'static str {
        match self {
            PrFilter::ReviewRequested => "sky",
            PrFilter::Mine => "violet",
            PrFilter::Assigned => "teal",
            PrFilter::Mentioned => "indigo",
            PrFilter::Reviewed => "green",
        }
    }

    pub fn query(&self) -> String {
        demo::query(match self {
            PrFilter::ReviewRequested => "review-requested:@me",
            PrFilter::Mine => "author:@me",
            PrFilter::Assigned => "assignee:@me",
            PrFilter::Mentioned => "mentions:@me",
            PrFilter::Reviewed => "reviewed-by:@me -author:@me",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum IssueFilter {
    Assigned,
    Mine,
    Mentioned,
}

impl IssueFilter {
    pub const ALL: [IssueFilter; 3] = [IssueFilter::Assigned, IssueFilter::Mine, IssueFilter::Mentioned];

    pub fn key(&self) -> &'static str {
        match self {
            IssueFilter::Assigned => "assigned",
            IssueFilter::Mine => "mine",
            IssueFilter::Mentioned => "mentioned",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.key() == k)
    }

    pub fn title(&self) -> &'static str {
        match self {
            IssueFilter::Assigned => "Assigned to me",
            IssueFilter::Mine => "Created by me",
            IssueFilter::Mentioned => "Mentioned",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            IssueFilter::Assigned => "sy-clock-symbolic",
            IssueFilter::Mine => "sy-issue-symbolic",
            IssueFilter::Mentioned => "sy-at-symbolic",
        }
    }

    pub fn tint(&self) -> &'static str {
        match self {
            IssueFilter::Assigned => "teal",
            IssueFilter::Mine => "green",
            IssueFilter::Mentioned => "indigo",
        }
    }

    pub fn query(&self) -> String {
        demo::query(match self {
            IssueFilter::Assigned => "assignee:@me",
            IssueFilter::Mine => "author:@me",
            IssueFilter::Mentioned => "mentions:@me",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Section {
    Notifications,
    Prs(PrFilter),
    MyPrsByRepo,
    Issues(IssueFilter),
    Repos,
    Projects,
    Claude,
}

impl Section {
    pub fn key(&self) -> String {
        match self {
            Section::Notifications => "notifications".into(),
            Section::Prs(f) => format!("prs/{}", f.key()),
            Section::MyPrsByRepo => "prs/byProject".into(),
            Section::Issues(f) => format!("issues/{}", f.key()),
            Section::Repos => "repos".into(),
            Section::Projects => "projects".into(),
            Section::Claude => "claude".into(),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Section::Notifications => "Notifications".into(),
            Section::Prs(f) => f.title().into(),
            Section::MyPrsByRepo => "My PRs by project".into(),
            Section::Issues(f) => format!("Issues: {}", f.title()),
            Section::Repos => "Repositories".into(),
            Section::Projects => "Projects".into(),
            Section::Claude => "Claude Code".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Checking,
    LoggedOut,
    Locked,
    LoggedIn,
}

pub struct State {
    pub phase: Phase,
    pub api: Option<GitHubApi>,
    pub account: Option<GitAccount>,
    pub forge: Option<ForgeClient>,
    pub forge_caps: ForgeCapabilities,
    pub locked: Option<GitAccount>,
    pub user: Option<RestUser>,
    pub scopes: Vec<String>,
    pub section: Section,
    pub route: Option<Route>,
    pub review_requests: Vec<SearchItem>,
    pub my_prs: Vec<SearchItem>,
    pub review_request_count: i64,
    pub my_pr_count: i64,
    pub unread_count: usize,
    pub last_badge_refresh: Option<chrono::DateTime<chrono::Utc>>,
    pub known_review_requests: Option<HashSet<String>>,
    last_clipboard: Option<String>,
    last_live: Instant,
}

pub struct App {
    pub gtk_app: adw::Application,
    pub state: RefCell<State>,
    pub refresh: Signal<()>,
    pub live: Signal<()>,
    pub badges: Signal<()>,
    pub section_changed: Signal<Section>,
    pub route_changed: Signal<Option<Route>>,
    pub phase_changed: Signal<Phase>,
    pub account_changed: Signal<()>,
    window: RefCell<Option<adw::ApplicationWindow>>,
    root: RefCell<Option<gtk::Stack>>,
    toasts: RefCell<Option<adw::ToastOverlay>>,
    main: RefCell<Option<Rc<ui::main_view::MainView>>>,
    poll: RefCell<Option<glib::SourceId>>,
    pr_models: RefCell<Vec<(ItemRef, Rc<PrModel>)>>,
    splash_done: Cell<bool>,
    held: Cell<bool>,
    smoke: Cell<bool>,
    pub claude: Rc<Workspace>,
}

thread_local! {
    static APP: RefCell<Option<Rc<App>>> = const { RefCell::new(None) };
}

pub fn get() -> Rc<App> {
    APP.with(|a| a.borrow().clone()).expect("app not started")
}

pub fn try_get() -> Option<Rc<App>> {
    APP.with(|a| a.borrow().clone())
}

pub fn main_window() -> Option<gtk::Window> {
    try_get().and_then(|a| a.window.borrow().clone()).map(|w| w.upcast())
}

pub fn api() -> Option<GitHubApi> {
    try_get().and_then(|a| a.state.borrow().api.clone())
}

pub fn forge() -> Option<ForgeClient> {
    try_get().and_then(|a| a.state.borrow().forge.clone())
}

pub fn account() -> Option<GitAccount> {
    try_get().and_then(|a| a.state.borrow().account.clone())
}

pub fn provider() -> GitProvider {
    account().map(|a| a.provider).unwrap_or(GitProvider::Github)
}

pub fn number_prefix() -> &'static str {
    provider().number_prefix()
}

pub fn cache_account() -> String {
    try_get()
        .and_then(|a| {
            let s = a.state.borrow();
            s.account.as_ref().filter(|x| !x.login.is_empty()).map(|x| x.cache_key()).or_else(|| s.user.as_ref().map(|u| u.login.clone()))
        })
        .unwrap_or_default()
}

pub fn forge_caps() -> ForgeCapabilities {
    try_get().map(|a| a.state.borrow().forge_caps.clone()).unwrap_or_default()
}

pub fn caps_allow(caps: &ForgeCapabilities, s: Section) -> bool {
    match s {
        Section::Prs(f) => caps.pull_filters.contains(&f),
        Section::MyPrsByRepo => caps.pull_filters.contains(&PrFilter::Mine),
        Section::Issues(f) => caps.issue_filters.contains(&f),
        Section::Notifications => caps.has_inbox(),
        Section::Repos => caps.repositories,
        Section::Claude => true,
        Section::Projects => false,
    }
}

fn first_section(caps: &ForgeCapabilities) -> Section {
    Section::Prs(caps.pull_filters.first().copied().unwrap_or(PrFilter::Mine))
}

pub fn section_allowed(s: Section) -> bool {
    if provider().is_github() {
        return true;
    }
    caps_allow(&forge_caps(), s)
}

fn github_links() -> bool {
    try_get().is_some_and(|a| {
        let s = a.state.borrow();
        s.api.is_some() && s.account.as_ref().is_none_or(|x| x.provider == GitProvider::Github)
    })
}

pub const CLAUDE_SWITCH_NOTE: &str = "Running Claude sessions stop. Their history stays, and this account's Claude Code sign-in is kept for when you switch back.";

pub const CLAUDE_SIGN_OUT_NOTE: &str = "The Claude Code accounts linked to it are signed out. Other accounts and their Claude Code sign-ins are not affected.";

pub fn claude_running() -> bool {
    try_get().is_some_and(|a| a.claude.running_count() > 0)
}

pub fn is_active() -> bool {
    try_get().and_then(|a| a.window.borrow().clone()).map(|w| w.is_active() && w.is_visible()).unwrap_or(false)
}

pub fn toast(msg: &str) {
    if let Some(t) = try_get().and_then(|a| a.toasts.borrow().clone()) {
        let toast = adw::Toast::new(msg);
        toast.set_timeout(2);
        t.add_toast(toast);
    }
}

pub fn toast_with(msg: &str, button: &str, f: impl Fn() + 'static) {
    if let Some(t) = try_get().and_then(|a| a.toasts.borrow().clone()) {
        let toast = adw::Toast::new(msg);
        toast.set_timeout(8);
        toast.set_button_label(Some(button));
        toast.connect_button_clicked(move |_| f());
        t.add_toast(toast);
    }
}

pub fn quit() {
    if let Some(a) = try_get() {
        a.claude.stop_all();
        a.gtk_app.quit();
    }
}

pub fn notify_update(version: &str) {
    let Some(a) = try_get() else { return };
    let n = gio::Notification::new(&format!("{} {version} is available", brand::APP_NAME));
    n.set_body(Some("Click to see what's new and update."));
    n.set_default_action("app.show-update");
    a.gtk_app.send_notification(Some(&format!("update-{version}")), &n);
}

pub fn claude_attention(tab: &crate::claude::workspace::Tab) {
    let Some(a) = try_get() else { return };
    a.claude.emit(crate::claude::workspace::WsEvent::TabState);
    if is_active() {
        return;
    }
    let n = gio::Notification::new("Claude Code needs your permission");
    n.set_body(Some(&format!("{} · {}", tab.title.borrow(), tab.project_name())));
    n.set_default_action_and_target_value("app.open-url", Some(&format!("{}://section/claude", brand::URL_SCHEME).to_variant()));
    a.gtk_app.send_notification(Some(&format!("claude-{}", tab.id)), &n);
}

pub fn section() -> Section {
    get().state.borrow().section
}

pub fn set_section(s: Section) {
    let a = get();
    if a.state.borrow().section == s || !section_allowed(s) {
        return;
    }
    a.state.borrow_mut().section = s;
    a.section_changed.emit(&s);
}

pub fn select(route: Option<Route>) {
    let a = get();
    a.state.borrow_mut().route = route.clone();
    if let Some(m) = a.main.borrow().clone() {
        m.show_route(route.clone(), false);
    }
    a.route_changed.emit(&route);
}

pub fn open(route: Route, push: bool) {
    let a = get();
    if !matches!(route, Route::Forge(_)) && !github_links() {
        return;
    }
    let section = a.state.borrow().section;
    if section == Section::Claude {
        let target = if section_allowed(Section::Prs(PrFilter::Mine)) { Section::Prs(PrFilter::Mine) } else { first_section(&forge_caps()) };
        a.state.borrow_mut().section = target;
        a.section_changed.emit(&target);
        select(Some(route));
        a.present();
        return;
    }
    let has_root = a.state.borrow().route.is_some();
    if push && has_root {
        if let Some(m) = a.main.borrow().clone() {
            m.show_route(Some(route), true);
        }
    } else {
        select(Some(route));
    }
    a.present();
}

pub fn visible_route() -> Option<Route> {
    let a = try_get()?;
    if a.state.try_borrow().ok()?.section == Section::Claude {
        return None;
    }
    let main = a.main.try_borrow().ok()?.clone()?;
    main.visible_route()
}

pub fn sync_actions() {
    let Some(a) = try_get() else { return };
    if let Some(action) = a.gtk_app.lookup_action("copy-short-link").and_downcast::<gio::SimpleAction>() {
        action.set_enabled(visible_route().and_then(|r| r.short_link()).is_some());
    }
}

pub fn back() {
    if let Some(m) = try_get().and_then(|a| a.main.borrow().clone()) {
        m.back();
    }
}

pub fn on_refresh(f: impl Fn(&()) -> bool + 'static) {
    get().refresh.connect(f);
}

pub fn on_live(f: impl Fn(&()) -> bool + 'static) {
    get().live.connect(f);
}

pub fn on_live_visible(widget: &impl IsA<gtk::Widget>, f: impl Fn(&()) -> bool + 'static) {
    let f = Rc::new(f);
    let stale = Rc::new(Cell::new(false));
    let widget = widget.upcast_ref::<gtk::Widget>();
    let weak = widget.downgrade();
    let (live, pending) = (f.clone(), stale.clone());
    on_live(move |_| {
        let Some(w) = weak.upgrade() else { return false };
        if w.is_mapped() {
            pending.set(false);
            live(&())
        } else {
            pending.set(true);
            true
        }
    });
    widget.connect_map(move |_| {
        if stale.get() && crate::api::rate_limited_until().is_none() {
            stale.set(false);
            f(&());
        }
    });
}

fn report_saved(saved: secret::Saved) {
    if saved == secret::Saved::File {
        toast_with("Keyring unavailable. Token saved to a private file in the config folder.", "OK", || {});
    }
}

fn bind_claude(account: &GitAccount) {
    use crate::claude::accounts as profiles;
    if profiles::owned(&account.id).is_empty() && !profiles::claim_unowned(&account.id) {
        let _ = profiles::add_owned(&format!("{} @{}", account.provider.short_title(), account.login), false, Some(account.id.clone()));
    }
    profiles::set_scope(Some(account.id.clone()));
    if let Some(target) = profiles::preferred(&account.id).filter(|p| p.id != profiles::active_id()) {
        ui::claude::auth_view::switch_profile(&target.id);
    }
    profiles::store().changed.emit(&());
}

fn forget_account(account: Option<&GitAccount>) {
    match account {
        Some(a) => {
            git_accounts::remove(&a.id);
        }
        None if brand::env_var("TOKEN").is_none() => crate::rt::spawn_local(async { crate::rt::run(secret::delete(settings::config_dir(), secret::DEFAULT_KEY.to_string())).await }),
        None => {}
    }
}

fn reset_session_caches() {
    crate::api::reset_rate_limit();
    crate::api::endpoints::reset_caches();
    ui::lists::reset_cache();
    ui::forge_list::reset_cache();
}

pub fn on_badges(f: impl Fn(&()) -> bool + 'static) {
    get().badges.connect(f);
}

pub fn has_project_scope() -> bool {
    let a = get();
    let s = a.state.borrow();
    s.scopes.is_empty() || s.scopes.iter().any(|x| x == "read:project" || x == "project")
}

pub fn pr_model(r: &ItemRef) -> Option<Rc<PrModel>> {
    let a = get();
    let api = a.state.borrow().api.clone()?;
    let mut models = a.pr_models.borrow_mut();
    if let Some(pos) = models.iter().position(|(k, _)| k == r) {
        let entry = models.remove(pos);
        let m = entry.1.clone();
        models.push(entry);
        return Some(m);
    }
    let m = PrModel::new(r.clone(), api);
    models.push((r.clone(), m.clone()));
    if models.len() > 15 {
        models.remove(0);
    }
    Some(m)
}

pub fn insert_pr_model(m: Rc<PrModel>) {
    let a = get();
    let mut models = a.pr_models.borrow_mut();
    models.retain(|(k, _)| *k != m.r);
    models.push((m.r.clone(), m));
}

impl App {
    pub fn present(&self) {
        if let Some(w) = self.window.borrow().clone() {
            w.present();
        }
    }

    fn set_phase(self: &Rc<Self>, phase: Phase) {
        self.state.borrow_mut().phase = phase;
        self.render_phase();
        self.phase_changed.emit(&phase);
    }

    fn render_phase(self: &Rc<Self>) {
        let Some(root) = self.root.borrow().clone() else { return };
        let phase = self.state.borrow().phase;
        if !self.splash_done.get() || phase == Phase::Checking {
            root.set_visible_child_name("splash");
            return;
        }
        if phase != Phase::Locked {
            if let Some(old) = root.child_by_name("lock") {
                root.remove(&old);
            }
        }
        match phase {
            Phase::Locked => {
                let Some(account) = self.state.borrow().locked.clone() else { return };
                if let Some(old) = root.child_by_name("lock") {
                    root.remove(&old);
                }
                root.add_named(&ui::unlock::lock_page(&account), Some("lock"));
                root.set_visible_child_name("lock");
                for name in ["main", "login"] {
                    if let Some(old) = root.child_by_name(name) {
                        root.remove(&old);
                    }
                }
                *self.main.borrow_mut() = None;
            }
            Phase::LoggedOut => {
                if root.child_by_name("login").is_none() {
                    root.add_named(&ui::login::build(), Some("login"));
                }
                root.set_visible_child_name("login");
                if let Some(old) = root.child_by_name("main") {
                    root.remove(&old);
                }
                *self.main.borrow_mut() = None;
            }
            Phase::LoggedIn => {
                if root.child_by_name("main").is_none() {
                    let main = ui::main_view::MainView::new();
                    root.add_named(&main.widget, Some("main"));
                    *self.main.borrow_mut() = Some(main);
                }
                root.set_visible_child_name("main");
                if let Some(old) = root.child_by_name("login") {
                    root.remove(&old);
                }
            }
            Phase::Checking => {}
        }
    }

    async fn restore(self: Rc<Self>) {
        if let Some(token) = brand::env_var("TOKEN") {
            self.resume_github(token, None).await;
            return;
        }
        git_accounts::migrate().await;
        let Some(account) = git_accounts::active() else {
            self.set_phase(Phase::LoggedOut);
            return;
        };
        let Some(credential) = git_accounts::credential(&account).await else {
            self.set_phase(Phase::LoggedOut);
            return;
        };
        if crate::account_lock::is_protected(&account.id).await {
            self.lock(account);
            return;
        }
        self.resume(account, credential).await;
    }

    fn lock(self: &Rc<Self>, account: GitAccount) {
        crate::claude::accounts::set_scope(Some(String::new()));
        self.state.borrow_mut().locked = Some(account);
        self.set_phase(Phase::Locked);
    }

    pub fn leave_lock(self: &Rc<Self>) {
        self.state.borrow_mut().locked = None;
        crate::claude::accounts::set_scope(None);
        self.set_phase(Phase::LoggedOut);
    }

    pub fn unlocked(self: &Rc<Self>, account: GitAccount) {
        let was_locked = self.state.borrow().locked.as_ref().is_some_and(|a| a.id == account.id);
        let me = self.clone();
        if !was_locked {
            crate::rt::spawn_local(async move { me.switch_account(account.id).await });
            return;
        }
        self.state.borrow_mut().locked = None;
        self.set_phase(Phase::Checking);
        crate::rt::spawn_local(async move {
            match git_accounts::credential(&account).await {
                Some(credential) => me.resume(account, credential).await,
                None => {
                    crate::claude::accounts::set_scope(None);
                    me.set_phase(Phase::LoggedOut);
                }
            }
        });
    }

    pub fn request_switch(self: &Rc<Self>, id: String) {
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let Some(target) = git_accounts::accounts().into_iter().find(|a| a.id == id) else { return };
            let current = me.state.borrow().account.as_ref().map(|a| a.id.clone());
            if current.as_deref() != Some(target.id.as_str()) && crate::account_lock::is_protected(&target.id).await {
                let unlocked = target.clone();
                let app = me.clone();
                ui::unlock::unlock_sheet(&target, "Enter PIN", Rc::new(move || app.unlocked(unlocked.clone())));
            } else {
                me.switch_account(id).await;
            }
        });
    }

    pub async fn reset_lock(self: Rc<Self>, account: GitAccount) {
        use crate::claude::accounts as profiles;
        crate::account_lock::remove(&account.id).await;
        if profiles::scope().as_deref() == Some(account.id.as_str()) {
            self.claude.close_all_for_profile_switch();
        }
        profiles::store().logout_profiles(profiles::owned(&account.id)).await;
        git_accounts::forget_credential(&account);
        let locked_here = self.state.borrow().locked.as_ref().is_some_and(|a| a.id == account.id);
        let current = self.state.borrow().account.as_ref().is_some_and(|a| a.id == account.id);
        if locked_here {
            self.leave_lock();
        } else if current {
            profiles::set_scope(None);
            self.reset_session(false);
        }
        toast_with(&format!("PIN removed. Sign in to {} again to use {}.", account.provider.title(), account.subtitle()), "OK", || {});
    }

    async fn resume(self: Rc<Self>, account: GitAccount, credential: GitCredential) {
        if account.provider.is_github() {
            self.resume_github(credential.token, Some(account)).await;
            return;
        }
        let Some(client) = ForgeClient::new(&account, &credential, settings::config_dir()) else { return };
        self.enter_forge(account, client.clone());
        self.verify_forge(client).await;
    }

    async fn resume_github(self: Rc<Self>, token: String, account: Option<GitAccount>) {
        let host = account.as_ref().map(|a| a.host.clone()).unwrap_or_else(|| crate::api::GITHUB_HOST.to_string());
        let cached = match &account {
            Some(a) if !a.login.is_empty() => Some(a.user()),
            Some(_) => None,
            None => settings::read(|s| s.last_account.clone()).filter(|a| a.fingerprint == crate::list_cache::fingerprint(&token)).map(|a| RestUser { login: a.login, name: a.name, avatar_url: a.avatar_url }),
        }
        .filter(|_| !demo::is_on());
        if let Some(user) = &cached {
            self.drop_main_if_switching(account.as_ref());
            {
                let mut s = self.state.borrow_mut();
                s.api = Some(GitHubApi::with_host(&token, &host));
                s.account = account.clone();
                s.forge = None;
                s.user = Some(user.clone());
            }
            if let Some(a) = &account {
                bind_claude(a);
            }
            self.set_phase(Phase::LoggedIn);
            self.start_polling();
        }
        let api = GitHubApi::with_host(&token, &host);
        let a2 = api.clone();
        match crate::rt::run(async move { a2.current_user().await }).await {
            Ok((user, scopes)) => self.enter_github(account.map(|a| a.with_user(&user)), api, user, scopes, &token),
            Err(e) if e.status == 401 => self.revoked(account),
            Err(_) if cached.is_some() => {}
            Err(_) => {
                self.drop_main_if_switching(account.as_ref());
                if let Some(a) = &account {
                    bind_claude(a);
                }
                {
                    let mut s = self.state.borrow_mut();
                    s.api = Some(api);
                    s.account = account;
                    s.forge = None;
                    if demo::is_on() {
                        s.user = Some(demo::user());
                    }
                }
                self.set_phase(Phase::LoggedIn);
                self.start_polling();
            }
        }
    }

    fn drop_main_if_switching(self: &Rc<Self>, next: Option<&GitAccount>) {
        let switching = {
            let s = self.state.borrow();
            s.phase == Phase::LoggedIn && s.account.as_ref().map(|a| a.id.as_str()) != next.map(|a| a.id.as_str())
        };
        if !switching {
            return;
        }
        reset_session_caches();
        self.pr_models.borrow_mut().clear();
        self.state.borrow_mut().route = None;
        if let Some(root) = self.root.borrow().clone() {
            if let Some(old) = root.child_by_name("main") {
                root.remove(&old);
            }
        }
        *self.main.borrow_mut() = None;
    }

    fn enter_github(self: &Rc<Self>, account: Option<GitAccount>, api: GitHubApi, user: RestUser, scopes: Vec<String>, token: &str) {
        let was_logged_in = self.state.borrow().phase == Phase::LoggedIn;
        let same = was_logged_in && {
            let s = self.state.borrow();
            s.account.as_ref().map(|a| a.id.as_str()) == account.as_ref().map(|a| a.id.as_str()) && s.user.as_ref().is_some_and(|u| u.login == user.login)
        };
        if !same {
            reset_session_caches();
        }
        if let Some(a) = &account {
            git_accounts::update(a);
            bind_claude(a);
        } else if !demo::is_on() {
            let last = settings::LastAccount { login: user.login.clone(), name: user.name.clone(), avatar_url: user.avatar_url.clone(), fingerprint: crate::list_cache::fingerprint(token) };
            if settings::read(|s| s.last_account.as_ref() != Some(&last)) {
                settings::update(|s| s.last_account = Some(last));
            }
        }
        {
            let mut s = self.state.borrow_mut();
            s.api = Some(api);
            s.account = account;
            s.forge = None;
            s.user = Some(if demo::is_on() { demo::user() } else { user });
            s.scopes = scopes;
            s.known_review_requests = None;
        }
        if same {
            return;
        }
        self.pr_models.borrow_mut().clear();
        self.state.borrow_mut().route = None;
        if was_logged_in {
            if let Some(root) = self.root.borrow().clone() {
                if let Some(old) = root.child_by_name("main") {
                    root.remove(&old);
                }
            }
            *self.main.borrow_mut() = None;
        }
        self.set_phase(Phase::LoggedIn);
        self.start_polling();
    }

    fn enter_forge(self: &Rc<Self>, account: GitAccount, client: ForgeClient) {
        self.drop_main_if_switching(Some(&account));
        bind_claude(&account);
        let caps = ForgeCapabilities::default();
        {
            let mut s = self.state.borrow_mut();
            s.user = Some(account.user());
            s.account = Some(account);
            s.forge = Some(client.clone());
            s.forge_caps = caps.clone();
            s.api = None;
            s.scopes.clear();
            s.route = None;
            s.known_review_requests = None;
            if !caps_allow(&caps, s.section) {
                s.section = first_section(&caps);
            }
        }
        self.pr_models.borrow_mut().clear();
        self.set_phase(Phase::LoggedIn);
        self.start_polling();
        let me = self.clone();
        crate::rt::spawn_local(async move { me.load_forge_caps(client).await });
    }

    async fn load_forge_caps(self: Rc<Self>, client: ForgeClient) {
        let c = client.clone();
        let caps = crate::rt::run(async move { c.capabilities().await }).await;
        if !self.state.borrow().forge.as_ref().is_some_and(|f| f.same(&client)) {
            return;
        }
        let section = {
            let mut s = self.state.borrow_mut();
            if s.forge_caps == caps {
                return;
            }
            s.forge_caps = caps.clone();
            if caps_allow(&caps, s.section) {
                None
            } else {
                s.section = first_section(&caps);
                Some(s.section)
            }
        };
        self.account_changed.emit(&());
        if let Some(section) = section {
            self.section_changed.emit(&section);
        }
        self.refresh_badges(true).await;
    }

    async fn verify_forge(self: &Rc<Self>, client: ForgeClient) {
        let c = client.clone();
        match crate::rt::run(async move { c.current_user().await }).await {
            Ok(user) => {
                let current = self.state.borrow().forge.as_ref().is_some_and(|f| f.same(&client));
                let Some(mut account) = self.state.borrow().account.clone().filter(|_| current) else { return };
                account.login = user.login;
                account.name = user.name;
                account.avatar_url = user.avatar_url;
                if user.id.is_some() {
                    account.user_id = user.id;
                }
                git_accounts::update(&account);
                {
                    let mut s = self.state.borrow_mut();
                    s.user = Some(account.user());
                    s.account = Some(account);
                }
                self.account_changed.emit(&());
            }
            Err(e) if e.status == 401 => {
                let target = self.state.borrow().account.clone().filter(|_| self.state.borrow().forge.as_ref().is_some_and(|f| f.same(&client)));
                if let Some(target) = target {
                    self.revoked(Some(target));
                }
            }
            Err(_) => {}
        }
    }

    pub async fn add_account(self: Rc<Self>, provider: GitProvider, raw_host: String, credential: GitCredential, email: Option<String>) -> Result<(), ApiError> {
        let host = provider.effective_host(&raw_host);
        if host.is_empty() {
            return Err(ApiError::new("Enter the server address"));
        }
        let draft = GitAccount::draft(provider, &host, email);
        if provider.is_github() {
            let api = GitHubApi::with_host(&credential.token, &host);
            let a2 = api.clone();
            let (user, scopes) = crate::rt::run(async move { a2.current_user().await }).await?;
            let (saved, stored) = git_accounts::upsert(draft.with_user(&user), &credential).await?;
            report_saved(stored);
            self.leave_current(&saved).await;
            self.enter_github(Some(saved), api, user, scopes, &credential.token);
            return Ok(());
        }
        let probe = ForgeClient::new(&draft, &credential, settings::config_dir()).ok_or_else(|| ApiError::new("Unsupported service"))?;
        let user = crate::rt::run(async move { probe.current_user().await }).await?;
        let mut draft = draft;
        draft.login = user.login;
        draft.name = user.name;
        draft.avatar_url = user.avatar_url;
        draft.user_id = user.id;
        let (saved, stored) = git_accounts::upsert(draft, &credential).await?;
        report_saved(stored);
        self.leave_current(&saved).await;
        if let Some(client) = ForgeClient::new(&saved, &credential, settings::config_dir()) {
            self.enter_forge(saved, client);
        }
        Ok(())
    }

    pub async fn switch_account(self: Rc<Self>, id: String) {
        let (current, phase) = {
            let s = self.state.borrow();
            (s.account.as_ref().map(|a| a.id.clone()), s.phase)
        };
        if current.as_deref() == Some(id.as_str()) && phase == Phase::LoggedIn {
            return;
        }
        let Some(target) = git_accounts::accounts().into_iter().find(|a| a.id == id) else { return };
        let Some(credential) = git_accounts::credential(&target).await else {
            toast_with(&format!("Sign-in for {} has expired. Sign in again to use this account.", target.subtitle()), "OK", || {});
            return;
        };
        self.leave_current(&target).await;
        self.resume(target, credential).await;
    }

    async fn leave_current(self: &Rc<Self>, next: &GitAccount) {
        let current = self.state.borrow().account.as_ref().map(|a| a.id.clone());
        if current.is_some_and(|id| id != next.id) {
            self.claude.close_all_for_profile_switch();
        }
        git_accounts::set_active(&next.id);
    }

    async fn unbind_claude(self: &Rc<Self>, account: &GitAccount) {
        use crate::claude::accounts as profiles;
        let owned = profiles::owned(&account.id);
        self.claude.close_all_for_profile_switch();
        profiles::store().logout_profiles(owned.clone()).await;
        for p in &owned {
            if p.is_default() {
                profiles::release(&p.id);
            } else {
                profiles::remove(&p.id);
            }
        }
        profiles::set_scope(None);
        profiles::store().changed.emit(&());
        self.claude.reload_projects().await;
    }

    pub async fn refresh_token_from_gh(self: Rc<Self>) {
        let account = self.state.borrow().account.clone();
        let provider = account.as_ref().map(|a| a.provider).unwrap_or(GitProvider::Github);
        if !provider.is_github() {
            return;
        }
        let host = account.map(|a| a.host).unwrap_or_else(|| crate::api::GITHUB_HOST.to_string());
        if let Ok(t) = crate::auth::gh_token(host.clone()).await {
            let _ = self.add_account(provider, host, GitCredential::token(t), None).await;
        }
    }

    pub async fn sign_out(self: Rc<Self>) {
        let current = self.state.borrow().account.clone();
        if let Some(a) = &current {
            self.unbind_claude(a).await;
        }
        if let Some(a) = &current {
            crate::account_lock::remove(&a.id).await;
        }
        forget_account(current.as_ref());
        self.reset_session(true);
    }

    fn revoked(self: &Rc<Self>, target: Option<GitAccount>) {
        match &target {
            Some(t) => git_accounts::forget_credential(t),
            None => forget_account(None),
        }
        crate::claude::accounts::set_scope(None);
        self.reset_session(false);
        let who = target.map(|t| t.subtitle()).unwrap_or_else(|| "this account".into());
        toast_with(&format!("Sign-in for {who} is no longer valid. Sign in again."), "OK", || {});
    }

    fn reset_session(self: &Rc<Self>, forget_cache: bool) {
        if let Some(id) = self.poll.borrow_mut().take() {
            id.remove();
        }
        if forget_cache {
            let key = cache_account();
            let key = if key.is_empty() { settings::read(|s| s.last_account.as_ref().map(|a| a.login.clone())).unwrap_or_default() } else { key };
            if !key.is_empty() {
                crate::list_cache::clear(&key);
            }
            if settings::read(|s| s.last_account.is_some()) {
                settings::update(|s| s.last_account = None);
            }
        }
        reset_session_caches();
        {
            let mut s = self.state.borrow_mut();
            s.api = None;
            s.account = None;
            s.forge = None;
            s.forge_caps = ForgeCapabilities::default();
            s.user = None;
            s.scopes.clear();
            s.route = None;
            s.review_requests.clear();
            s.my_prs.clear();
            s.review_request_count = 0;
            s.my_pr_count = 0;
            s.unread_count = 0;
            s.known_review_requests = None;
        }
        self.pr_models.borrow_mut().clear();
        self.set_phase(Phase::LoggedOut);
        self.badges.emit(&());
    }

    pub fn user_refresh(self: &Rc<Self>) {
        self.state.borrow_mut().last_live = Instant::now();
        self.refresh.emit(&());
        let me = self.clone();
        crate::rt::spawn_local(async move { me.refresh_badges(true).await });
    }

    pub fn start_polling(self: &Rc<Self>) {
        if let Some(id) = self.poll.borrow_mut().take() {
            id.remove();
        }
        self.state.borrow_mut().last_live = Instant::now();
        let me = self.clone();
        crate::rt::spawn_local(async move { me.refresh_badges(true).await });
        let interval = settings::refresh_interval();
        let weak = Rc::downgrade(self);
        let cycle = Cell::new(0u64);
        let id = glib::timeout_add_local(Duration::from_secs_f64(interval), move || {
            let Some(me) = weak.upgrade() else { return glib::ControlFlow::Break };
            cycle.set(cycle.get() + 1);
            let due = me.state.borrow().last_live.elapsed().as_secs_f64() >= interval * 0.8;
            if due && (is_active() || cycle.get() % 4 == 0) {
                me.tick_live();
            }
            glib::ControlFlow::Continue
        });
        *self.poll.borrow_mut() = Some(id);
    }

    pub fn did_become_active(self: &Rc<Self>) {
        if self.state.borrow().phase != Phase::LoggedIn {
            return;
        }
        if self.state.borrow().last_live.elapsed().as_secs() > 10 {
            self.tick_live();
        }
        self.check_clipboard();
    }

    fn tick_live(self: &Rc<Self>) {
        if crate::api::rate_limited_until().is_some() {
            return;
        }
        crate::diagnostics::live_tick(|| {
            self.state.borrow_mut().last_live = Instant::now();
            self.live.emit(&());
            let me = self.clone();
            crate::rt::spawn_local(async move { me.refresh_badges(false).await });
        });
    }

    pub async fn refresh_badges(self: Rc<Self>, force: bool) {
        let forge = self.state.borrow().forge.clone();
        if let Some(forge) = forge {
            self.refresh_forge_badges(forge).await;
            return;
        }
        let Some(api) = self.state.borrow().api.clone() else { return };
        if demo::is_on() {
            {
                let mut s = self.state.borrow_mut();
                s.review_request_count = 3;
                s.my_pr_count = 7;
                s.unread_count = 12;
            }
            self.badges.emit(&());
            return;
        }
        let a1 = api.clone();
        if let Ok(b) = crate::rt::run(async move { a1.badges().await }).await {
            self.apply_review_requests(b.rr.nodes, b.rr.issue_count);
            let mut s = self.state.borrow_mut();
            s.my_prs = b.mine.nodes;
            s.my_pr_count = b.mine.issue_count;
        }
        let a2 = api.clone();
        if let Ok(n) = crate::rt::run(async move { a2.unread_notification_count(force).await }).await {
            self.state.borrow_mut().unread_count = n;
        }
        self.state.borrow_mut().last_badge_refresh = Some(chrono::Utc::now());
        self.badges.emit(&());
    }

    fn apply_review_requests(&self, items: Vec<SearchItem>, count: i64) {
        let new_items: Vec<SearchItem> = {
            let mut s = self.state.borrow_mut();
            let ids: HashSet<String> = items.iter().map(|i| i.url.clone()).collect();
            let fresh = match &s.known_review_requests {
                Some(known) => items.iter().filter(|i| !known.contains(&i.url)).cloned().collect(),
                None => vec![],
            };
            s.known_review_requests = Some(ids);
            s.review_requests = items;
            s.review_request_count = count;
            fresh
        };
        if !new_items.is_empty() && settings::read(|s| s.notify_review_requests) && !is_active() {
            let prefix = number_prefix();
            for item in new_items.iter().take(3) {
                let n = gio::Notification::new(&format!("Review requested: {}", item.title));
                n.set_body(Some(&format!("{}{prefix}{} by {}", item.repository.name_with_owner, item.number, item.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost"))));
                n.set_default_action_and_target_value("app.open-url", Some(&item.url.to_variant()));
                self.gtk_app.send_notification(Some(&format!("rr-{}", item.url)), &n);
            }
        }
    }

    async fn refresh_forge_badges(self: Rc<Self>, forge: ForgeClient) {
        let caps = self.state.borrow().forge_caps.clone();
        let supported = caps.pull_filters.clone();
        let current = |me: &Rc<Self>| me.state.borrow().forge.as_ref().is_some_and(|f| f.same(&forge));
        if supported.contains(&PrFilter::ReviewRequested) {
            let f = forge.clone();
            if let Ok(rr) = crate::rt::run(async move { f.pull_requests(PrFilter::ReviewRequested, ui::lists::StateFilter::Open).await }).await {
                if current(&self) {
                    ui::forge_view::remember(&rr);
                    let count = rr.len() as i64;
                    self.apply_review_requests(rr.into_iter().take(10).collect(), count);
                }
            }
        }
        if supported.contains(&PrFilter::Mine) {
            let f = forge.clone();
            if let Ok(mine) = crate::rt::run(async move { f.pull_requests(PrFilter::Mine, ui::lists::StateFilter::Open).await }).await {
                if current(&self) {
                    ui::forge_view::remember(&mine);
                    let mut s = self.state.borrow_mut();
                    s.my_pr_count = mine.len() as i64;
                    s.my_prs = mine.into_iter().take(10).collect();
                }
            }
        }
        let unread = if caps.has_inbox() {
            let f = forge.clone();
            crate::rt::run(async move { f.notifications().await }).await.ok().map(|n| n.iter().filter(|x| x.unread).count())
        } else {
            Some(0)
        };
        if !current(&self) {
            return;
        }
        {
            let mut s = self.state.borrow_mut();
            if let Some(n) = unread {
                s.unread_count = n;
            }
            s.last_badge_refresh = Some(chrono::Utc::now());
        }
        self.badges.emit(&());
    }

    pub fn set_unread(&self, n: usize) {
        self.state.borrow_mut().unread_count = n;
        self.badges.emit(&());
    }

    fn check_clipboard(self: &Rc<Self>) {
        let Some(display) = gtk::gdk::Display::default() else { return };
        let clip = display.clipboard();
        let me = self.clone();
        clip.read_text_async(None::<&gio::Cancellable>, move |res| {
            let Ok(Some(text)) = res else { return };
            let s = text.to_string();
            if me.state.borrow().last_clipboard.as_deref() == Some(s.as_str()) {
                return;
            }
            me.state.borrow_mut().last_clipboard = Some(s.clone());
            if !github_links() || s.chars().count() >= 300 || !s.contains("github.com") {
                return;
            }
            let Some(route) = util::parse_github_url(&s, false) else { return };
            let title = match &route {
                Route::Pr(r) => format!("PR {}", r.title()),
                Route::Issue(r) => format!("Issue {}", r.title()),
                _ => return,
            };
            if me.state.borrow().route.as_ref() == Some(&route) {
                return;
            }
            toast_with(&format!("In clipboard: {title}"), "Open", move || open(route.clone(), false));
        });
    }

    pub fn handle_url(self: &Rc<Self>, uri: &str) {
        if self.state.borrow().phase != Phase::LoggedIn {
            return;
        }
        let Ok(url) = url::Url::parse(uri) else {
            if let Some(route) = util::parse_github_url(uri, true) {
                open(route, false);
            }
            return;
        };
        let scheme = url.scheme().to_string();
        if scheme == "http" || scheme == "https" {
            if let Some(route) = util::parse_github_url(uri, false).filter(|_| github_links()) {
                let files = url.path().ends_with("/files");
                open(route.clone(), false);
                if files {
                    if let Route::Pr(r) = route {
                        if let Some(m) = pr_model(&r) {
                            m.set_tab(crate::ui::pr::model::PrTab::Files);
                        }
                    }
                }
            } else {
                util::open_url(uri);
            }
            return;
        }
        if !scheme.starts_with(brand::URL_SCHEME) {
            return;
        }
        let host = url.host_str().unwrap_or("").to_string();
        let parts: Vec<String> = url.path_segments().map(|p| p.filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()).unwrap_or_default();
        let query = |name: &str| url.query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.to_string());
        if host == "section" {
            let section = match (parts.first().map(|s| s.as_str()), parts.get(1).map(|s| s.as_str())) {
                (Some("notifications"), _) => Some(Section::Notifications),
                (Some("repos"), _) => Some(Section::Repos),
                (Some("projects"), _) => Some(Section::Projects),
                (Some("claude"), _) => Some(Section::Claude),
                (Some("prs"), Some("byProject")) => Some(Section::MyPrsByRepo),
                (Some("prs"), f) => Some(Section::Prs(f.and_then(PrFilter::from_key).unwrap_or(PrFilter::ReviewRequested))),
                (Some("issues"), f) => Some(Section::Issues(f.and_then(IssueFilter::from_key).unwrap_or(IssueFilter::Assigned))),
                _ => None,
            };
            if let Some(s) = section {
                set_section(s);
            }
            self.present();
            return;
        }
        if host == "claude" {
            set_section(Section::Claude);
            let path = url.path().to_string();
            if path == "/new" {
                if let Some(folder) = query("folder").filter(|f| std::path::Path::new(f).exists()) {
                    let mode = query("mode").and_then(|m| crate::claude::ClaudeMode::from_key(&m));
                    self.claude.new_session(&folder, query("prompt"), query("name"), mode, None);
                }
            }
            if path == "/resume" {
                if let Some(id) = query("id") {
                    let kind = query("kind").and_then(|k| crate::claude::TabKind::from_key(&k));
                    let ws = self.claude.clone();
                    crate::rt::spawn_local(async move {
                        if ws.find_session(&id).is_none() {
                            ws.reload_projects().await;
                        }
                        if let Some(s) = ws.find_session(&id) {
                            ws.resume(&s, false, kind);
                        }
                    });
                }
            }
            self.present();
            return;
        }
        let target = query("url").unwrap_or_else(|| format!("https://{}", uri.split("://").skip(1).collect::<Vec<_>>().join("://")));
        if let Some(route) = util::parse_github_url(&target, false) {
            open(route, false);
        }
    }

    fn install_actions(self: &Rc<Self>) {
        let app = &self.gtk_app;
        let add = |name: &str, accels: &[&str], f: Box<dyn Fn()>| {
            let action = gio::SimpleAction::new(name, None);
            action.connect_activate(move |_, _| f());
            app.add_action(&action);
            if !accels.is_empty() {
                app.set_accels_for_action(&format!("app.{name}"), accels);
            }
        };
        let me = self.clone();
        add("refresh", &["<Control>r", "F5"], Box::new(move || me.user_refresh()));
        add("open-link", &["<Control>l"], Box::new(|| ui::main_view::open_link_dialog()));
        add("back", &["<Alt>Left", "<Control>bracketleft"], Box::new(back));
        add("preferences", &["<Control>comma"], Box::new(|| ui::settings::show()));
        add("check-updates", &[], Box::new(|| ui::update::show(true)));
        add("show-update", &[], Box::new(|| ui::update::show(false)));
        add("about", &[], Box::new(|| ui::main_view::about()));
        add("donate", &[], Box::new(|| util::open_in_default_browser(brand::DONATE_URL)));
        add("quit", &["<Control>q"], Box::new(quit));
        add(
            "copy-short-link",
            &["<Control><Shift>c"],
            Box::new(|| {
                if let Some(r) = visible_route() {
                    ui::item_row::copy_short_link(&r);
                }
            }),
        );
        if let Some(action) = app.lookup_action("copy-short-link").and_downcast::<gio::SimpleAction>() {
            action.set_enabled(false);
        }
        let me = self.clone();
        add(
            "show-window",
            &[],
            Box::new(move || {
                me.present();
            }),
        );
        let sections: Vec<(&str, Section)> = vec![
            ("section-1", Section::Prs(PrFilter::ReviewRequested)),
            ("section-2", Section::Prs(PrFilter::Mine)),
            ("section-3", Section::Notifications),
            ("section-4", Section::Repos),
            ("section-5", Section::Projects),
            ("section-6", Section::Claude),
        ];
        for (i, (name, s)) in sections.into_iter().enumerate() {
            add(name, &[&format!("<Control>{}", i + 1)], Box::new(move || set_section(s)));
        }
        let open_url = gio::SimpleAction::new("open-url", Some(glib::VariantTy::STRING));
        let me = self.clone();
        open_url.connect_activate(move |_, v| {
            if let Some(s) = v.and_then(|v| v.get::<String>()) {
                me.present();
                me.handle_url(&s);
            }
        });
        app.add_action(&open_url);
    }

    fn build_window(self: &Rc<Self>) {
        let (w, h, maximized) = settings::read(|s| (s.window_width, s.window_height, s.window_maximized));
        let window = adw::ApplicationWindow::new(&self.gtk_app);
        window.set_title(Some(brand::APP_NAME));
        window.set_icon_name(Some(brand::APP_ID));
        window.set_default_size(w.max(960), h.max(600));
        window.set_size_request(900, 560);
        if crate::tour::active() {
            window.set_default_size(1600, 1000);
        } else if maximized {
            window.maximize();
        }
        let root = gtk::Stack::new();
        root.set_transition_type(gtk::StackTransitionType::Crossfade);
        root.set_transition_duration(220);
        root.add_named(&ui::splash::build(), Some("splash"));
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&root));
        window.set_content(Some(&toasts));
        *self.root.borrow_mut() = Some(root);
        *self.toasts.borrow_mut() = Some(toasts);
        let weak = Rc::downgrade(self);
        window.connect_is_active_notify(move |w| {
            if w.is_active() {
                if let Some(me) = weak.upgrade() {
                    me.did_become_active();
                }
            }
        });
        let weak = Rc::downgrade(self);
        window.connect_close_request(move |w| {
            let (width, height) = w.default_size();
            let max = w.is_maximized();
            settings::update(|s| {
                if !max {
                    s.window_width = width;
                    s.window_height = height;
                }
                s.window_maximized = max;
            });
            if let Some(me) = weak.upgrade() {
                if settings::read(|s| s.run_in_background) && !me.smoke.get() {
                    w.set_visible(false);
                    if !me.held.get() {
                        me.held.set(true);
                        std::mem::forget(me.gtk_app.hold());
                    }
                    return glib::Propagation::Stop;
                }
                me.claude.stop_all();
            }
            glib::Propagation::Proceed
        });
        *self.window.borrow_mut() = Some(window.clone());
        window.present();
    }
}

pub fn run(smoke: bool) -> glib::ExitCode {
    let gtk_app = adw::Application::builder().application_id(brand::APP_ID).flags(gio::ApplicationFlags::HANDLES_OPEN).build();
    let app = Rc::new(App {
        gtk_app: gtk_app.clone(),
        state: RefCell::new(State {
            phase: Phase::Checking,
            api: None,
            account: None,
            forge: None,
            forge_caps: ForgeCapabilities::default(),
            locked: None,
            user: None,
            scopes: vec![],
            section: Section::Prs(PrFilter::ReviewRequested),
            route: None,
            review_requests: vec![],
            my_prs: vec![],
            review_request_count: 0,
            my_pr_count: 0,
            unread_count: 0,
            last_badge_refresh: None,
            known_review_requests: None,
            last_clipboard: None,
            last_live: Instant::now(),
        }),
        refresh: Signal::new(),
        live: Signal::new(),
        badges: Signal::new(),
        section_changed: Signal::new(),
        route_changed: Signal::new(),
        phase_changed: Signal::new(),
        account_changed: Signal::new(),
        window: RefCell::new(None),
        root: RefCell::new(None),
        toasts: RefCell::new(None),
        main: RefCell::new(None),
        poll: RefCell::new(None),
        pr_models: RefCell::new(vec![]),
        splash_done: Cell::new(false),
        held: Cell::new(false),
        smoke: Cell::new(smoke),
        claude: Workspace::new(),
    });
    APP.with(|a| *a.borrow_mut() = Some(app.clone()));

    let a = app.clone();
    gtk_app.connect_startup(move |_| {
        crate::diagnostics::set_enabled(settings::read(|s| s.diagnostics_enabled));
        crate::icons::install();
        crate::theme::install();
        a.install_actions();
    });
    let a = app.clone();
    gtk_app.connect_activate(move |_| {
        if a.window.borrow().is_some() {
            a.present();
            return;
        }
        a.build_window();
        if a.smoke.get() {
            crate::smoke::run(a.clone());
            return;
        }
        let me = a.clone();
        glib::timeout_add_local_once(Duration::from_millis(750), move || {
            me.splash_done.set(true);
            me.render_phase();
        });
        crate::rt::spawn_local(a.clone().restore());
        if crate::tour::active() {
            crate::tour::run(a.clone());
            return;
        }
        crate::rt::spawn_local(crate::updater::check_on_launch());
        crate::updater::on_present(|_| {
            ui::update::show(false);
            true
        });
    });
    let a = app.clone();
    gtk_app.connect_open(move |g, files, _| {
        if a.window.borrow().is_none() {
            g.activate();
        }
        for f in files {
            let uri = f.uri().to_string();
            let me = a.clone();
            glib::timeout_add_local_once(Duration::from_millis(900), move || me.handle_url(&uri));
        }
    });
    let mut args: Vec<String> = vec![std::env::args().next().unwrap_or_else(|| brand::BIN_NAME.to_string())];
    args.extend(std::env::args().skip(1).filter(|a| !a.starts_with("--")));
    gtk_app.run_with_args(&args)
}

pub fn finish_smoke(code: i32) {
    if let Some(a) = try_get() {
        a.gtk_app.quit();
    }
    std::process::exit(code);
}

pub fn set_smoke_logged_in(api: GitHubApi, user: RestUser) {
    let a = get();
    {
        let mut s = a.state.borrow_mut();
        s.api = Some(api);
        s.user = Some(user);
    }
    a.splash_done.set(true);
    a.set_phase(Phase::LoggedIn);
}

pub fn set_smoke_logged_out() {
    let a = get();
    a.splash_done.set(true);
    a.set_phase(Phase::LoggedOut);
}

pub fn set_smoke_forge(provider: GitProvider, caps: ForgeCapabilities) {
    let a = get();
    let mut account = GitAccount::draft(provider, "smoke.invalid", None);
    account.login = "smoke".into();
    account.name = Some("Smoke Test".into());
    let Some(client) = ForgeClient::new(&account, &GitCredential::token("smoke"), settings::config_dir()) else { return };
    a.drop_main_if_switching(Some(&account));
    {
        let mut s = a.state.borrow_mut();
        s.section = first_section(&caps);
        s.forge_caps = caps;
        s.user = Some(account.user());
        s.account = Some(account);
        s.forge = Some(client);
        s.api = None;
        s.route = None;
    }
    a.set_phase(Phase::LoggedIn);
}

pub fn set_smoke_locked(account: GitAccount) {
    let a = get();
    a.splash_done.set(true);
    a.lock(account);
}
