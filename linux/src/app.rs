use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use gtk::{gio, glib};

use crate::api::models::*;
use crate::api::GitHubApi;
use crate::brand;
use crate::claude::workspace::Workspace;
use crate::demo;
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
    LoggedIn,
}

pub struct State {
    pub phase: Phase,
    pub api: Option<GitHubApi>,
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
    if a.state.borrow().section == s {
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
    let section = a.state.borrow().section;
    if section == Section::Claude {
        a.state.borrow_mut().section = Section::Prs(PrFilter::Mine);
        a.section_changed.emit(&Section::Prs(PrFilter::Mine));
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
        match phase {
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
        let token = match brand::env_var("TOKEN") {
            Some(t) => Some(t),
            None => crate::rt::run(secret::load(settings::config_dir())).await,
        };
        let Some(token) = token else {
            self.set_phase(Phase::LoggedOut);
            return;
        };
        match self.clone().login(token.clone(), false).await {
            Ok(()) => {}
            Err(e) if e.status == 401 => {
                crate::rt::run(secret::delete(settings::config_dir())).await;
                self.set_phase(Phase::LoggedOut);
            }
            Err(_) => {
                {
                    let mut s = self.state.borrow_mut();
                    s.api = Some(GitHubApi::new(&token));
                    if demo::is_on() {
                        s.user = Some(demo::user());
                    }
                }
                self.set_phase(Phase::LoggedIn);
                self.start_polling();
            }
        }
    }

    pub async fn login(self: Rc<Self>, token: String, persist: bool) -> Result<(), crate::api::ApiError> {
        let api = GitHubApi::new(&token);
        let a2 = api.clone();
        let (user, scopes) = crate::rt::run(async move { a2.current_user().await }).await?;
        if persist {
            match crate::rt::run(secret::save(settings::config_dir(), token.clone())).await {
                secret::Saved::Keyring => {}
                secret::Saved::File => toast_with("Keyring unavailable. Token saved to a private file in the config folder.", "OK", || {}),
                secret::Saved::Failed => toast_with("Could not save the token. You will need to sign in again next time.", "OK", || {}),
            }
        }
        {
            let mut s = self.state.borrow_mut();
            s.api = Some(api);
            s.user = Some(if demo::is_on() { demo::user() } else { user });
            s.scopes = scopes;
            s.known_review_requests = None;
        }
        self.pr_models.borrow_mut().clear();
        let was_logged_in = self.state.borrow().phase == Phase::LoggedIn;
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
        Ok(())
    }

    pub fn logout(self: &Rc<Self>) {
        if let Some(id) = self.poll.borrow_mut().take() {
            id.remove();
        }
        {
            let mut s = self.state.borrow_mut();
            s.api = None;
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
        crate::rt::spawn_local(async { crate::rt::run(secret::delete(settings::config_dir())).await });
        self.set_phase(Phase::LoggedOut);
        self.badges.emit(&());
    }

    pub fn user_refresh(self: &Rc<Self>) {
        self.state.borrow_mut().last_live = Instant::now();
        self.refresh.emit(&());
        let me = self.clone();
        crate::rt::spawn_local(async move { me.refresh_badges().await });
    }

    pub fn start_polling(self: &Rc<Self>) {
        if let Some(id) = self.poll.borrow_mut().take() {
            id.remove();
        }
        self.state.borrow_mut().last_live = Instant::now();
        let me = self.clone();
        crate::rt::spawn_local(async move { me.refresh_badges().await });
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
        crate::diagnostics::tick_begin();
        self.state.borrow_mut().last_live = Instant::now();
        self.live.emit(&());
        let me = self.clone();
        crate::rt::spawn_local(async move { me.refresh_badges().await });
    }

    pub async fn refresh_badges(self: Rc<Self>) {
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
            let new_items: Vec<SearchItem> = {
                let mut s = self.state.borrow_mut();
                let ids: HashSet<String> = b.rr.nodes.iter().map(|i| i.url.clone()).collect();
                let fresh = match &s.known_review_requests {
                    Some(known) => b.rr.nodes.iter().filter(|i| !known.contains(&i.url)).cloned().collect(),
                    None => vec![],
                };
                s.known_review_requests = Some(ids);
                s.review_requests = b.rr.nodes.clone();
                s.review_request_count = b.rr.issue_count;
                s.my_prs = b.mine.nodes.clone();
                s.my_pr_count = b.mine.issue_count;
                fresh
            };
            if !new_items.is_empty() && settings::read(|s| s.notify_review_requests) && !is_active() {
                for item in new_items.iter().take(3) {
                    let n = gio::Notification::new(&format!("Review requested: {}", item.title));
                    n.set_body(Some(&format!("{}#{} by {}", item.repository.name_with_owner, item.number, item.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost"))));
                    n.set_default_action_and_target_value("app.open-url", Some(&item.url.to_variant()));
                    self.gtk_app.send_notification(Some(&format!("rr-{}", item.url)), &n);
                }
            }
        }
        let a2 = api.clone();
        if let Ok(n) = crate::rt::run(async move { a2.unread_notification_count().await }).await {
            self.state.borrow_mut().unread_count = n;
        }
        self.state.borrow_mut().last_badge_refresh = Some(chrono::Utc::now());
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
            if s.chars().count() >= 300 || !s.contains("github.com") {
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
        let Ok(url) = url::Url::parse(uri) else {
            if let Some(route) = util::parse_github_url(uri, true) {
                open(route, false);
            }
            return;
        };
        let scheme = url.scheme().to_string();
        if scheme == "http" || scheme == "https" {
            if let Some(route) = util::parse_github_url(uri, false) {
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
