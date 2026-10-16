use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};
use gtk::glib;

use super::code_view::CodeView;
use super::dialogs::Sheet;
use super::html::HtmlView;
use super::item_row::{item_menu, item_row};
use super::lists::{state_toggles, SearchModel, StateFilter};
use super::menu::{context_menu, Menu};
use super::tabstrip::{TabItem, TabStrip};
use super::widgets::*;
use crate::api::models::*;
use crate::api::GitHubApi;
use crate::conversation_html::{markdown_page, Look};
use crate::{app, util};

struct ReposList {
    repos: RefCell<Vec<RepoSummary>>,
    cursor: RefCell<Option<String>>,
    has_more: Cell<bool>,
    loading: Cell<bool>,
    error: RefCell<Option<String>>,
    text: RefCell<String>,
    hide_archived: Cell<bool>,
    updated: Cell<Option<DateTime<Utc>>>,
    list: gtk::ListBox,
    stack: gtk::Stack,
    keys: RefCell<Vec<RepoRef>>,
    status: Rc<RefreshStatus>,
}

fn repo_row(r: &RepoSummary) -> gtk::Box {
    let b = hbox(9);
    margins(&b, 4, 0, 4, 0);
    let tile = icon_tile(if r.is_private { "sy-lock-symbolic" } else { "sy-repo-symbolic" }, if r.is_archived { "slate" } else { "teal" }, 22);
    tile.set_valign(gtk::Align::Start);
    b.append(&tile);
    let col = vbox(3);
    col.set_hexpand(true);
    let top = hbox(5);
    top.append(&ellipsized(&r.name_with_owner, &["sy-bold", "sy-text"]));
    if r.is_archived {
        top.append(&chip("archived", "amber", None, false));
    }
    if r.is_fork {
        top.append(&tooltip(colored_icon("sy-branch-symbolic", 11, "dim"), "Fork"));
    }
    col.append(&top);
    if let Some(d) = r.description.as_deref().filter(|d| !d.is_empty()) {
        let l = wrap_label(d, &["sy-muted", "sy-small"]);
        l.set_lines(2);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        col.append(&l);
    }
    let meta = hbox(10);
    if let Some(lang) = &r.primary_language {
        let lb = hbox(3);
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("dot");
        dot.add_css_class(&crate::theme::hex_class(lang.color.as_deref().unwrap_or("#888888")));
        dot.set_valign(gtk::Align::Center);
        lb.append(&dot);
        lb.append(&label_with(&lang.name, &["sy-muted", "sy-small"]));
        meta.append(&lb);
    }
    let stat = |icon_name: &str, n: i64, color: &str| {
        let s = hbox(3);
        s.append(&colored_icon(icon_name, 11, color));
        s.append(&label_with(&n.to_string(), &[&format!("fg-{color}"), "sy-small"]));
        s
    };
    if r.pull_requests.total_count > 0 {
        meta.append(&stat("sy-pr-symbolic", r.pull_requests.total_count, "violet"));
    }
    if r.issues.total_count > 0 {
        meta.append(&stat("sy-issue-symbolic", r.issues.total_count, "green"));
    }
    if r.stargazer_count > 0 {
        meta.append(&stat("sy-star-symbolic", r.stargazer_count, "amber"));
    }
    meta.append(&spacer());
    meta.append(&label_with(&util::rel(r.pushed_at), &["sy-dim", "sy-small"]));
    col.append(&meta);
    b.append(&col);
    b
}

impl ReposList {
    fn visible(&self) -> Vec<RepoSummary> {
        let text = self.text.borrow().clone();
        self.repos
            .borrow()
            .iter()
            .filter(|r| (!self.hide_archived.get() || !r.is_archived) && (text.is_empty() || util::contains_ci(&r.name_with_owner, &text) || util::contains_ci(r.description.as_deref().unwrap_or(""), &text)))
            .cloned()
            .collect()
    }

    fn render(self: &Rc<Self>) {
        let _span = crate::diagnostics::ui_span("repos.render");
        let items = self.visible();
        self.status.update(self.updated.get(), self.loading.get(), if self.repos.borrow().is_empty() { None } else { self.error.borrow().clone() });
        self.list.remove_all();
        let mut keys = vec![];
        if items.is_empty() {
            if self.loading.get() {
                self.stack.set_visible_child_name("loading");
            } else {
                if let Some(e) = self.error.borrow().clone() {
                    if let Some(l) = self.stack.child_by_name("empty").and_downcast::<gtk::Label>() {
                        l.set_text(&e);
                    }
                } else if let Some(l) = self.stack.child_by_name("empty").and_downcast::<gtk::Label>() {
                    l.set_text("Nothing here");
                }
                self.stack.set_visible_child_name("empty");
            }
            return;
        }
        self.stack.set_visible_child_name("list");
        for r in items {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&repo_row(&r)));
            let rr = r.clone();
            context_menu(&row, move || {
                let m = Menu::ctx();
                let u = rr.url.clone();
                m.item("Open in browser", move || util::open_url(&u));
                let u = rr.url.clone();
                m.item("Copy link", move || util::copy(&u));
                let c = format!("gh repo clone {}", rr.name_with_owner);
                m.item("Copy “gh repo clone”", move || util::copy(&c));
                m
            });
            self.list.append(&row);
            keys.push(r.repo().unwrap_or_else(|| RepoRef::new("", "")));
        }
        *self.keys.borrow_mut() = keys;
        let route = app::get().state.borrow().route.clone();
        if let Some(Route::Repo(r)) = route {
            if let Some(i) = self.keys.borrow().iter().position(|k| *k == r) {
                if let Some(row) = self.list.row_at_index(i as i32) {
                    self.list.select_row(Some(&row));
                }
            }
        }
    }

    fn load(self: &Rc<Self>) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        me.loading.set(true);
        me.render();
        crate::rt::spawn_local(async move {
            let res = crate::rt::run(async move { api.repos(None).await }).await;
            me.loading.set(false);
            match res {
                Ok(r) => {
                    let current = me.repos.borrow().clone();
                    let keep: Vec<RepoSummary> = if current.len() > r.nodes.len() {
                        current.into_iter().skip(r.nodes.len()).filter(|n| !r.nodes.iter().any(|x| x.name_with_owner == n.name_with_owner)).collect()
                    } else {
                        vec![]
                    };
                    if keep.is_empty() {
                        *me.cursor.borrow_mut() = r.page_info.end_cursor.clone();
                        me.has_more.set(r.page_info.has_next_page);
                    }
                    let mut all = r.nodes;
                    all.extend(keep);
                    *me.repos.borrow_mut() = all;
                    me.updated.set(Some(Utc::now()));
                    *me.error.borrow_mut() = None;
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.render();
        });
    }

    async fn load_more(self: Rc<Self>) -> bool {
        let Some(api) = app::api() else { return false };
        if !self.has_more.get() || self.loading.get() {
            return false;
        }
        self.loading.set(true);
        let cursor = self.cursor.borrow().clone();
        let res = crate::rt::run(async move { api.repos(cursor).await }).await;
        self.loading.set(false);
        let Ok(r) = res else { return false };
        let known: std::collections::HashSet<String> = self.repos.borrow().iter().map(|r| r.name_with_owner.clone()).collect();
        let added: Vec<RepoSummary> = r.nodes.into_iter().filter(|n| !known.contains(&n.name_with_owner)).collect();
        self.repos.borrow_mut().extend(added);
        *self.cursor.borrow_mut() = r.page_info.end_cursor;
        self.has_more.set(r.page_info.has_next_page);
        self.render();
        true
    }
}

pub fn list() -> gtk::Widget {
    let lb = gtk::ListBox::new();
    lb.add_css_class("sy-list");
    let stack = gtk::Stack::new();
    let scroll = scrolled(&lb);
    stack.add_named(&scroll, Some("list"));
    stack.add_named(&loading(), Some("loading"));
    let empty = label_with("Nothing here", &["sy-muted"]);
    empty.set_valign(gtk::Align::Start);
    empty.set_halign(gtk::Align::Center);
    empty.set_margin_top(30);
    empty.set_wrap(true);
    empty.set_natural_wrap_mode(gtk::NaturalWrapMode::None);
    empty.set_justify(gtk::Justification::Center);
    stack.add_named(&empty, Some("empty"));
    let holder: Rc<RefCell<Option<std::rc::Weak<ReposList>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(v) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            v.load();
        }
    });
    let me = Rc::new(ReposList {
        repos: RefCell::new(vec![]),
        cursor: RefCell::new(None),
        has_more: Cell::new(false),
        loading: Cell::new(false),
        error: RefCell::new(None),
        text: RefCell::new(String::new()),
        hide_archived: Cell::new(true),
        updated: Cell::new(None),
        list: lb.clone(),
        stack: stack.clone(),
        keys: RefCell::new(vec![]),
        status,
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let w = Rc::downgrade(&me);
    lb.connect_row_activated(move |_, row| {
        if let Some(me) = w.upgrade() {
            if let Some(r) = me.keys.borrow().get(row.index().max(0) as usize).cloned() {
                app::select(Some(Route::Repo(r)));
            }
        }
    });
    let w = Rc::downgrade(&me);
    scroll.connect_edge_reached(move |_, pos| {
        if pos == gtk::PositionType::Bottom {
            if let Some(me) = w.upgrade() {
                crate::rt::spawn_local(async move {
                    me.load_more().await;
                });
            }
        }
    });

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Filter repositories"));
    let w = Rc::downgrade(&me);
    search.connect_search_changed(move |e| {
        if let Some(me) = w.upgrade() {
            *me.text.borrow_mut() = e.text().trim().to_string();
            me.render();
            if !me.text.borrow().is_empty() && me.has_more.get() {
                let me = me.clone();
                crate::rt::spawn_local(async move {
                    for _ in 0..20 {
                        if !me.clone().load_more().await {
                            break;
                        }
                    }
                });
            }
        }
    });
    let search_bar = gtk::SearchBar::new();
    search_bar.set_child(Some(&search));
    search_bar.connect_entry(&search);
    let search_btn = gtk::ToggleButton::new();
    search_btn.set_icon_name("system-search-symbolic");
    search_btn.set_tooltip_text(Some("Filter"));
    search_btn.bind_property("active", &search_bar, "search-mode-enabled").bidirectional().build();
    let archived = gtk::ToggleButton::new();
    crate::icons::set_button_icon(&archived, "sy-archive-symbolic");
    archived.set_tooltip_text(Some("Hide archived"));
    archived.set_active(true);
    let w = Rc::downgrade(&me);
    archived.connect_toggled(move |t| {
        if let Some(me) = w.upgrade() {
            me.hide_archived.set(t.is_active());
            me.render();
        }
    });

    let content = vbox(0);
    content.append(&stack);
    content.append(&footer(&me.status));
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_end_title_buttons(false);
    hb.set_title_widget(Some(&adw::WindowTitle::new("Repositories", "")));
    hb.pack_start(&search_btn);
    hb.pack_end(&archived);
    tv.add_top_bar(&hb);
    tv.add_top_bar(&search_bar);
    tv.set_content(Some(&content));
    let w = Rc::downgrade(&me);
    app::on_refresh(move |_| match w.upgrade() {
        Some(me) => {
            me.load();
            true
        }
        None => false,
    });
    me.load();
    keep_alive(&tv, me);
    tv.upcast()
}

#[derive(Clone)]
pub enum FileContent {
    Text(String, bool),
    Markdown(String, String),
    Image(gtk::gdk::Texture),
    Binary(i64),
}

pub struct CodeModel {
    pub repo: RepoRef,
    api: GitHubApi,
    pub branches: RefCell<Vec<BranchInfo>>,
    pub git_ref: RefCell<String>,
    pub dir: RefCell<String>,
    pub entries: RefCell<Vec<TreeEntry>>,
    pub loading_dir: Cell<bool>,
    pub dir_error: RefCell<Option<String>>,
    pub readme: RefCell<Option<String>>,
    pub readme_state: Cell<ReadmeState>,
    pub selected_file: RefCell<Option<String>>,
    pub file: RefCell<Option<FileContent>>,
    pub file_size: Cell<Option<i64>>,
    pub file_error: RefCell<Option<String>>,
    pub all_paths: RefCell<Option<Vec<String>>>,
    pub paths_truncated: Cell<bool>,
    loading_paths: Cell<bool>,
    dir_cache: RefCell<HashMap<String, Vec<TreeEntry>>>,
    readme_cache: RefCell<HashMap<String, Option<String>>>,
    pub changed: crate::signal::Signal<()>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadmeState {
    None,
    Pending,
    Loading,
    Loaded,
}

fn has_readme(entries: &[TreeEntry]) -> bool {
    entries.iter().any(|e| !e.is_dir() && e.name.to_lowercase().starts_with("readme"))
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "svg"];
const MARKDOWN_EXTS: &[&str] = &["md", "markdown", "mdx"];

impl CodeModel {
    pub fn new(repo: RepoRef, api: GitHubApi) -> Rc<Self> {
        Rc::new(CodeModel {
            repo,
            api,
            branches: RefCell::new(vec![]),
            git_ref: RefCell::new(String::new()),
            dir: RefCell::new(String::new()),
            entries: RefCell::new(vec![]),
            loading_dir: Cell::new(false),
            dir_error: RefCell::new(None),
            readme: RefCell::new(None),
            readme_state: Cell::new(ReadmeState::None),
            selected_file: RefCell::new(None),
            file: RefCell::new(None),
            file_size: Cell::new(None),
            file_error: RefCell::new(None),
            all_paths: RefCell::new(None),
            paths_truncated: Cell::new(false),
            loading_paths: Cell::new(false),
            dir_cache: RefCell::new(HashMap::new()),
            readme_cache: RefCell::new(HashMap::new()),
            changed: crate::signal::Signal::new(),
        })
    }

    pub fn crumbs(&self) -> Vec<(String, String)> {
        let mut out = vec![(self.repo.name.clone(), String::new())];
        let mut acc = String::new();
        for part in self.dir.borrow().split('/').filter(|p| !p.is_empty()) {
            acc = if acc.is_empty() { part.to_string() } else { format!("{acc}/{part}") };
            out.push((part.to_string(), acc.clone()));
        }
        out
    }

    pub async fn start(self: Rc<Self>, default_branch: String) {
        if !self.git_ref.borrow().is_empty() {
            return;
        }
        *self.git_ref.borrow_mut() = default_branch;
        let me = self.clone();
        crate::rt::spawn_local(async move { me.load_branches().await });
        self.open_dir(String::new()).await;
    }

    async fn load_branches(self: Rc<Self>) {
        let api = self.api.clone();
        let repo = self.repo.clone();
        if let Ok(mut list) = crate::rt::run(async move { api.branches(&repo).await }).await {
            let current = self.git_ref.borrow().clone();
            list.sort_by(|a, b| {
                if a.name == current {
                    return std::cmp::Ordering::Less;
                }
                if b.name == current {
                    return std::cmp::Ordering::Greater;
                }
                util::natural_cmp(&a.name, &b.name)
            });
            *self.branches.borrow_mut() = list;
            self.changed.emit(&());
        }
    }

    pub async fn switch_ref(self: Rc<Self>, name: String) {
        if *self.git_ref.borrow() == name {
            return;
        }
        *self.git_ref.borrow_mut() = name;
        *self.all_paths.borrow_mut() = None;
        *self.selected_file.borrow_mut() = None;
        *self.file.borrow_mut() = None;
        *self.readme.borrow_mut() = None;
        self.readme_state.set(ReadmeState::None);
        let keep = self.dir.borrow().clone();
        self.clone().open_dir(keep.clone()).await;
        if self.dir_error.borrow().is_some() && !keep.is_empty() {
            self.open_dir(String::new()).await;
        }
    }

    pub async fn open_dir(self: Rc<Self>, path: String) {
        let git_ref = self.git_ref.borrow().clone();
        let key = format!("{git_ref}|{path}");
        let switching = *self.dir.borrow() != path;
        *self.dir.borrow_mut() = path.clone();
        let cached = self.dir_cache.borrow().get(&key).cloned();
        if let Some(entries) = cached {
            let readme = self.readme_cache.borrow().get(&key).cloned();
            self.readme_state.set(match (&readme, has_readme(&entries)) {
                (_, false) => ReadmeState::None,
                (Some(_), true) => ReadmeState::Loaded,
                (None, true) => ReadmeState::Pending,
            });
            *self.readme.borrow_mut() = readme.flatten();
            *self.entries.borrow_mut() = entries;
            *self.dir_error.borrow_mut() = None;
        } else if switching {
            self.entries.borrow_mut().clear();
            *self.readme.borrow_mut() = None;
            self.readme_state.set(ReadmeState::None);
            *self.dir_error.borrow_mut() = None;
        }
        self.loading_dir.set(true);
        self.changed.emit(&());
        let api = self.api.clone();
        let repo = self.repo.clone();
        let (r2, p2) = (git_ref.clone(), path.clone());
        let res = crate::rt::run(async move { api.git_object(&repo, &r2, &p2).await }).await;
        let still = || *self.dir.borrow() == path && *self.git_ref.borrow() == git_ref;
        if !still() {
            return;
        }
        match res {
            Ok(obj) => match obj.and_then(|o| o.entries) {
                None => {
                    self.entries.borrow_mut().clear();
                    *self.readme.borrow_mut() = None;
                    self.readme_state.set(ReadmeState::None);
                    *self.dir_error.borrow_mut() = Some("Folder not found on this branch".into());
                }
                Some(mut list) => {
                    list.sort_by(|a, b| {
                        if a.is_dir() != b.is_dir() {
                            return if a.is_dir() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
                        }
                        util::natural_cmp(&a.name, &b.name)
                    });
                    *self.dir_error.borrow_mut() = None;
                    if has_readme(&list) {
                        if self.readme_state.get() != ReadmeState::Loading {
                            self.readme_state.set(ReadmeState::Pending);
                        }
                    } else {
                        *self.readme.borrow_mut() = None;
                        self.readme_state.set(ReadmeState::None);
                        self.readme_cache.borrow_mut().remove(&key);
                    }
                    *self.entries.borrow_mut() = list.clone();
                    self.dir_cache.borrow_mut().insert(key, list);
                }
            },
            Err(e) => *self.dir_error.borrow_mut() = Some(e.message),
        }
        self.loading_dir.set(false);
        self.changed.emit(&());
    }

    pub fn ensure_readme(self: &Rc<Self>) {
        if self.readme_state.get() != ReadmeState::Pending {
            return;
        }
        self.readme_state.set(ReadmeState::Loading);
        let me = self.clone();
        crate::rt::spawn_local(async move { me.load_readme().await });
    }

    async fn load_readme(self: Rc<Self>) {
        let git_ref = self.git_ref.borrow().clone();
        let path = self.dir.borrow().clone();
        let api = self.api.clone();
        let repo = self.repo.clone();
        let (r, p) = (git_ref.clone(), path.clone());
        let html = crate::rt::run(async move { api.readme_html(&repo, &r, &p).await }).await.ok().flatten();
        if *self.dir.borrow() != path || *self.git_ref.borrow() != git_ref {
            return;
        }
        self.readme_cache.borrow_mut().insert(format!("{git_ref}|{path}"), html.clone());
        *self.readme.borrow_mut() = html;
        self.readme_state.set(ReadmeState::Loaded);
        self.changed.emit(&());
    }

    pub async fn select(self: Rc<Self>, entry: TreeEntry) {
        if entry.is_dir() {
            *self.selected_file.borrow_mut() = None;
            *self.file.borrow_mut() = None;
            self.open_dir(entry.path).await;
        } else if !entry.is_submodule() {
            self.open_file(entry.path).await;
        }
    }

    pub async fn reveal(self: Rc<Self>, path: String) {
        let parent = util::parent_path(&path);
        if parent != *self.dir.borrow() {
            self.clone().open_dir(parent).await;
        }
        self.open_file(path).await;
    }

    pub async fn open_file(self: Rc<Self>, path: String) {
        *self.selected_file.borrow_mut() = Some(path.clone());
        *self.file.borrow_mut() = None;
        *self.file_error.borrow_mut() = None;
        self.changed.emit(&());
        let ext = util::extension(&path);
        let api = self.api.clone();
        let repo = self.repo.clone();
        let git_ref = self.git_ref.borrow().clone();
        let current = || self.selected_file.borrow().as_deref() == Some(path.as_str());
        if IMAGE_EXTS.contains(&ext.as_str()) {
            let (r, p) = (git_ref.clone(), path.clone());
            let res = crate::rt::run(async move { api.raw_file(&repo, &r, &p).await }).await;
            if !current() {
                return;
            }
            match res {
                Ok(Some(data)) => {
                    self.file_size.set(Some(data.len() as i64));
                    let len = data.len() as i64;
                    let tex = gtk::gdk::Texture::from_bytes(&glib::Bytes::from_owned(data)).ok();
                    *self.file.borrow_mut() = Some(match tex {
                        Some(t) => FileContent::Image(t),
                        None => FileContent::Binary(len),
                    });
                }
                Ok(None) => *self.file_error.borrow_mut() = Some("File not found".into()),
                Err(e) => *self.file_error.borrow_mut() = Some(e.message),
            }
            self.changed.emit(&());
            return;
        }
        let (r, p) = (git_ref.clone(), path.clone());
        let api2 = api.clone();
        let repo2 = repo.clone();
        let res = crate::rt::run(async move { api2.git_object(&repo2, &r, &p).await }).await;
        if !current() {
            return;
        }
        match res {
            Ok(Some(obj)) => {
                self.file_size.set(obj.byte_size);
                if obj.is_binary == Some(true) {
                    *self.file.borrow_mut() = Some(FileContent::Binary(obj.byte_size.unwrap_or(0)));
                } else if MARKDOWN_EXTS.contains(&ext.as_str()) {
                    let (r, p) = (git_ref.clone(), path.clone());
                    let html = crate::rt::run(async move { api.rendered_file(&repo, &r, &p).await }).await.ok().flatten();
                    if !current() {
                        return;
                    }
                    *self.file.borrow_mut() = Some(match html {
                        Some(h) => FileContent::Markdown(h, obj.text.clone().unwrap_or_default()),
                        None => FileContent::Text(obj.text.clone().unwrap_or_default(), obj.is_truncated.unwrap_or(false)),
                    });
                } else {
                    *self.file.borrow_mut() = Some(FileContent::Text(obj.text.clone().unwrap_or_default(), obj.is_truncated.unwrap_or(false)));
                }
            }
            Ok(None) => *self.file_error.borrow_mut() = Some("File not found".into()),
            Err(e) => *self.file_error.borrow_mut() = Some(e.message),
        }
        self.changed.emit(&());
    }

    pub async fn load_all_paths(self: Rc<Self>) {
        if self.all_paths.borrow().is_some() || self.loading_paths.get() {
            return;
        }
        self.loading_paths.set(true);
        let current = self.git_ref.borrow().clone();
        let mut sha = self.branches.borrow().iter().find(|b| b.name == current).map(|b| b.commit.sha.clone());
        if sha.is_none() {
            self.clone().load_branches().await;
            sha = self.branches.borrow().iter().find(|b| b.name == current).map(|b| b.commit.sha.clone());
        }
        let target = sha.unwrap_or(current);
        let api = self.api.clone();
        let repo = self.repo.clone();
        if let Ok((paths, truncated)) = crate::rt::run(async move { api.all_paths(&repo, &target).await }).await {
            *self.all_paths.borrow_mut() = Some(paths);
            self.paths_truncated.set(truncated);
        } else {
            *self.all_paths.borrow_mut() = Some(vec![]);
        }
        self.loading_paths.set(false);
        self.changed.emit(&());
    }

    pub fn web_url(&self) -> String {
        let base = format!("https://github.com/{}", self.repo.full());
        let r = self.git_ref.borrow().clone();
        if let Some(f) = self.selected_file.borrow().as_ref() {
            return format!("{base}/blob/{r}/{f}");
        }
        let d = self.dir.borrow().clone();
        if d.is_empty() {
            format!("{base}/tree/{r}")
        } else {
            format!("{base}/tree/{r}/{d}")
        }
    }
}

fn entry_icon(e: &TreeEntry) -> (&'static str, &'static str) {
    if e.is_dir() {
        return ("sy-folder-symbolic", "sky");
    }
    if e.is_submodule() {
        return ("sy-archive-symbolic", "indigo");
    }
    let ext = util::extension(&e.name);
    let icon_name = match ext.as_str() {
        "md" | "markdown" | "txt" => "sy-file-symbolic",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" => "sy-photo-symbolic",
        "sh" | "bash" | "zsh" => "sy-terminal-symbolic",
        "lock" => "sy-lock-symbolic",
        "json" | "yaml" | "yml" | "toml" | "xml" | "plist" | "swift" | "dart" | "js" | "ts" | "tsx" | "jsx" | "kt" | "java" | "go" | "rs" | "py" | "rb" | "c" | "cpp" | "h" | "m" => "sy-code-symbolic",
        _ => "sy-file-symbolic",
    };
    (icon_name, "muted")
}

pub fn entry_row(e: &TreeEntry) -> gtk::Box {
    let b = hbox(7);
    let (i, c) = entry_icon(e);
    b.append(&colored_icon(i, 14, c));
    let n = ellipsized(&e.name, &["sy-text"]);
    if e.is_dir() {
        n.add_css_class("sy-bold");
    }
    n.set_hexpand(true);
    b.append(&n);
    if let Some(s) = e.size().filter(|_| !e.is_dir()) {
        b.append(&label_with(&util::byte_count(s), &["sy-dim", "sy-tiny"]));
    }
    b
}

pub struct CodeBrowser {
    pub widget: gtk::Paned,
    model: Rc<CodeModel>,
    branch_button: gtk::MenuButton,
    crumbs: gtk::Box,
    entries: gtk::ListBox,
    entry_keys: RefCell<Vec<Option<TreeEntry>>>,
    side_stack: gtk::Stack,
    content: adw::Bin,
    markdown_source: Cell<bool>,
    wrap: Cell<bool>,
    html: RefCell<Option<Rc<HtmlView>>>,
    code: CodeView,
    rendered_key: RefCell<String>,
}

impl CodeBrowser {
    pub fn new(model: Rc<CodeModel>) -> Rc<Self> {
        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        let side = vbox(0);
        side.add_css_class("sy-list-pane");
        side.set_size_request(260, -1);
        let top = hbox(6);
        margins(&top, 8, 10, 8, 10);
        let branch_button = gtk::MenuButton::new();
        branch_button.set_label("…");
        branch_button.add_css_class("flat");
        branch_button.set_tooltip_text(Some("Branch"));
        top.append(&branch_button);
        top.append(&spacer());
        let qo = icon_button("system-search-symbolic", "Go to file (Ctrl+P)");
        top.append(&qo);
        side.append(&top);
        let crumbs = hbox(2);
        let cs = gtk::ScrolledWindow::new();
        cs.set_vscrollbar_policy(gtk::PolicyType::Never);
        cs.set_child(Some(&crumbs));
        margins(&cs, 0, 10, 6, 10);
        side.append(&cs);
        side.append(&line());
        let entries = gtk::ListBox::new();
        entries.add_css_class("sy-list");
        entries.add_css_class("sy-plain");
        let side_stack = gtk::Stack::new();
        side_stack.add_named(&scrolled(&entries), Some("list"));
        side_stack.add_named(&loading(), Some("loading"));
        side_stack.set_vexpand(true);
        side.append(&side_stack);
        let content = adw::Bin::new();
        content.set_hexpand(true);
        paned.set_start_child(Some(&side));
        paned.set_end_child(Some(&content));
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(false);
        paned.set_position(300);
        let me = Rc::new(CodeBrowser {
            widget: paned,
            model: model.clone(),
            branch_button,
            crumbs,
            entries: entries.clone(),
            entry_keys: RefCell::new(vec![]),
            side_stack,
            content,
            markdown_source: Cell::new(false),
            wrap: Cell::new(false),
            html: RefCell::new(None),
            code: CodeView::new(),
            rendered_key: RefCell::new(String::new()),
        });
        let w = Rc::downgrade(&me);
        me.content.connect_map(move |_| {
            if let Some(me) = w.upgrade() {
                me.render_content();
            }
        });
        let w = Rc::downgrade(&me);
        qo.connect_clicked(move |_| {
            if let Some(me) = w.upgrade() {
                quick_open(me.model.clone());
            }
        });
        let w = Rc::downgrade(&me);
        entries.connect_row_activated(move |_, row| {
            let Some(me) = w.upgrade() else { return };
            let key = me.entry_keys.borrow().get(row.index().max(0) as usize).cloned();
            let m = me.model.clone();
            match key {
                Some(Some(e)) => crate::rt::spawn_local(async move { m.select(e).await }),
                Some(None) => {
                    let parent = util::parent_path(&m.dir.borrow());
                    *m.selected_file.borrow_mut() = None;
                    *m.file.borrow_mut() = None;
                    crate::rt::spawn_local(async move { m.open_dir(parent).await })
                }
                None => {}
            }
        });
        let shortcut = gtk::ShortcutController::new();
        shortcut.set_scope(gtk::ShortcutScope::Managed);
        let w = Rc::downgrade(&me);
        let action = gtk::CallbackAction::new(move |_, _| {
            if let Some(me) = w.upgrade() {
                quick_open(me.model.clone());
            }
            glib::Propagation::Stop
        });
        shortcut.add_shortcut(gtk::Shortcut::new(gtk::ShortcutTrigger::parse_string("<Control>p"), Some(action)));
        me.widget.add_controller(shortcut);
        let w = Rc::downgrade(&me);
        model.changed.connect(move |_| match w.upgrade() {
            Some(me) => {
                me.render();
                true
            }
            None => false,
        });
        let w = Rc::downgrade(&me);
        crate::theme::on_change(move |_| match w.upgrade() {
            Some(me) => {
                me.code.apply_scheme();
                me.rendered_key.borrow_mut().clear();
                me.render();
                true
            }
            None => false,
        });
        me.render();
        me
    }

    fn html(self: &Rc<Self>) -> Rc<HtmlView> {
        if let Some(h) = self.html.borrow().as_ref() {
            return h.clone();
        }
        let h = HtmlView::new();
        let w = Rc::downgrade(self);
        h.on_link(move |url| {
            if let Some(me) = w.upgrade() {
                me.open_link(&url);
            }
        });
        *self.html.borrow_mut() = Some(h.clone());
        h
    }

    fn open_link(&self, url: &str) {
        match util::parse_github_url(url, false) {
            Some(Route::Repo(_)) => util::open_url(url),
            Some(r) => app::open(r, true),
            None => {
                if url == "https://github.com/" {
                    return;
                }
                util::open_url(url)
            }
        }
    }

    fn render(self: &Rc<Self>) {
        let m = &self.model;
        let git_ref = m.git_ref.borrow().clone();
        self.branch_button.set_label(if git_ref.is_empty() { "…" } else { &git_ref });
        let menu = Menu::new();
        for b in m.branches.borrow().iter() {
            let name = b.name.clone();
            let mm = m.clone();
            let label = if b.name == git_ref { format!("✓ {}", b.name) } else { b.name.clone() };
            menu.item(&label, move || {
                let mm = mm.clone();
                let n = name.clone();
                crate::rt::spawn_local(async move { mm.switch_ref(n).await });
            });
        }
        menu.apply_to(&self.branch_button);

        clear(&self.crumbs);
        let crumbs = m.crumbs();
        let n = crumbs.len();
        for (i, (name, path)) in crumbs.into_iter().enumerate() {
            if i > 0 {
                self.crumbs.append(&label_with("/", &["sy-dim"]));
            }
            let b = flat(&name);
            if i == n - 1 {
                b.add_css_class("sy-text");
            } else {
                b.add_css_class("fg-accent");
            }
            if i == 0 {
                b.add_css_class("sy-bold");
            }
            let mm = m.clone();
            b.connect_clicked(move |_| {
                *mm.selected_file.borrow_mut() = None;
                *mm.file.borrow_mut() = None;
                let mm = mm.clone();
                let p = path.clone();
                crate::rt::spawn_local(async move { mm.open_dir(p).await });
            });
            self.crumbs.append(&b);
        }

        self.entries.remove_all();
        let mut keys = vec![];
        if !m.dir.borrow().is_empty() {
            let row = gtk::ListBoxRow::new();
            let b = hbox(7);
            b.append(&icon("go-up-symbolic", 14));
            b.append(&label_with("..", &["sy-muted"]));
            row.set_child(Some(&b));
            self.entries.append(&row);
            keys.push(None);
        }
        if let Some(e) = m.dir_error.borrow().as_ref() {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&wrap_label(e, &["fg-red", "sy-small"])));
            row.set_activatable(false);
            self.entries.append(&row);
            keys.push(None);
        }
        let selected = m.selected_file.borrow().clone();
        for e in m.entries.borrow().iter() {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&entry_row(e)));
            let ee = e.clone();
            let repo = m.repo.clone();
            let mm = m.clone();
            context_menu(&row, move || {
                let menu = Menu::ctx();
                let p = ee.path.clone();
                menu.item("Copy path", move || util::copy(&p));
                let url = format!("https://github.com/{}/{}/{}/{}", repo.full(), if ee.is_dir() { "tree" } else { "blob" }, mm.git_ref.borrow(), ee.path);
                menu.item("Open in browser", move || util::open_url(&url));
                menu
            });
            self.entries.append(&row);
            if selected.as_deref() == Some(e.path.as_str()) {
                self.entries.select_row(Some(&row));
            }
            keys.push(Some(e.clone()));
        }
        *self.entry_keys.borrow_mut() = keys;
        self.side_stack.set_visible_child_name(if m.loading_dir.get() && m.entries.borrow().is_empty() { "loading" } else { "list" });
        self.render_content();
    }

    fn render_content(self: &Rc<Self>) {
        let m = &self.model;
        let selected = m.selected_file.borrow().clone();
        if selected.is_none() && self.content.is_mapped() {
            m.ensure_readme();
        }
        let file = m.file.borrow().clone();
        let error = m.file_error.borrow().clone();
        let readme_waiting = matches!(m.readme_state.get(), ReadmeState::Pending | ReadmeState::Loading);
        let key = format!(
            "{:?}|{}|{}|{}|{}|{}|{}|{}",
            selected,
            match &file {
                Some(FileContent::Text(t, _)) => format!("t{}", t.len()),
                Some(FileContent::Markdown(h, _)) => format!("m{}", h.len()),
                Some(FileContent::Image(_)) => "i".into(),
                Some(FileContent::Binary(n)) => format!("b{n}"),
                None => "-".into(),
            },
            error.clone().unwrap_or_default(),
            m.readme.borrow().as_ref().map(|r| r.len()).unwrap_or(0),
            readme_waiting,
            m.loading_dir.get(),
            self.markdown_source.get(),
            self.wrap.get()
        );
        if *self.rendered_key.borrow() == key {
            return;
        }
        *self.rendered_key.borrow_mut() = key;
        if let Some(path) = selected {
            let col = vbox(0);
            col.append(&self.file_header(&path, file.as_ref()));
            match (&error, &file) {
                (Some(e), _) => {
                    let mm = m.clone();
                    let p = path.clone();
                    col.append(&error_state(
                        e,
                        Some(Box::new(move || {
                            let mm = mm.clone();
                            let p = p.clone();
                            crate::rt::spawn_local(async move { mm.open_file(p).await });
                        })),
                    ));
                }
                (None, None) => col.append(&loading()),
                (None, Some(f)) => col.append(&self.file_body(f, &path)),
            }
            self.content.set_child(Some(&col));
            return;
        }
        if let Some(readme) = m.readme.borrow().clone() {
            let html = self.html();
            html.set_html(&markdown_page(&readme, Look::current()));
            let holder = vbox(0);
            holder.add_css_class("sy-card-flat");
            pad(&holder, 10);
            if let Some(p) = html.view.parent() {
                if let Ok(b) = p.downcast::<gtk::Box>() {
                    b.remove(&html.view);
                }
            }
            holder.append(&html.view);
            self.content.set_child(Some(&holder));
            return;
        }
        if readme_waiting {
            let holder = vbox(8);
            holder.add_css_class("sy-card-flat");
            pad(&holder, 10);
            holder.set_vexpand(true);
            let row = hbox(8);
            row.set_halign(gtk::Align::Center);
            row.set_valign(gtk::Align::Start);
            row.set_margin_top(24);
            row.append(&spinner(14));
            row.append(&label_with("Loading README…", &["sy-dim", "sy-small"]));
            holder.append(&row);
            self.content.set_child(Some(&holder));
            return;
        }
        if m.loading_dir.get() {
            self.content.set_child(Some(&loading()));
            return;
        }
        self.content.set_child(Some(&empty_state("sy-file-symbolic", "teal", "Pick a file on the left or press Ctrl+P", None)));
    }

    fn file_header(self: &Rc<Self>, path: &str, file: Option<&FileContent>) -> gtk::Box {
        let b = hbox(10);
        margins(&b, 8, 12, 8, 12);
        b.append(&icon_tile("sy-file-symbolic", "sky", 20));
        let l = label_with(path, &["sy-mono", "sy-text", "sy-bold"]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        l.set_selectable(true);
        b.append(&l);
        if let Some(size) = self.model.file_size.get() {
            b.append(&chip(&util::byte_count(size), "muted", None, false));
        }
        if let Some(FileContent::Text(s, _)) = file {
            let lines = s.matches('\n').count() as i64 + 1;
            b.append(&chip(&util::plural(lines, "line", "lines"), "muted", None, false));
        }
        b.append(&spacer());
        if let Some(FileContent::Markdown(..)) = file {
            let toggles = hbox(0);
            toggles.add_css_class("linked");
            let preview = gtk::ToggleButton::with_label("Preview");
            let source = gtk::ToggleButton::with_label("Source");
            source.set_group(Some(&preview));
            if self.markdown_source.get() {
                source.set_active(true);
            } else {
                preview.set_active(true);
            }
            let w = Rc::downgrade(self);
            source.connect_toggled(move |t| {
                if let Some(me) = w.upgrade() {
                    me.markdown_source.set(t.is_active());
                    me.render_content();
                }
            });
            toggles.append(&preview);
            toggles.append(&source);
            b.append(&toggles);
        }
        if let Some(FileContent::Text(..)) = file {
            let wrap = gtk::CheckButton::with_label("Wrap");
            wrap.set_active(self.wrap.get());
            let w = Rc::downgrade(self);
            wrap.connect_toggled(move |c| {
                if let Some(me) = w.upgrade() {
                    me.wrap.set(c.is_active());
                    me.code.set_wrap(c.is_active());
                }
            });
            b.append(&wrap);
        }
        let menu = Menu::new();
        let p = path.to_string();
        menu.item("Copy path", move || util::copy(&p));
        match file {
            Some(FileContent::Text(s, _)) | Some(FileContent::Markdown(_, s)) => {
                let s = s.clone();
                menu.item("Copy contents", move || util::copy(&s));
            }
            _ => {}
        }
        let m = self.model.clone();
        menu.item("Copy link", move || util::copy(&m.web_url()));
        let m = self.model.clone();
        menu.item("Open in browser", move || util::open_url(&m.web_url()));
        b.append(&menu.button("view-more-symbolic", "More"));
        b
    }

    fn file_body(self: &Rc<Self>, f: &FileContent, path: &str) -> gtk::Widget {
        let wrapper = vbox(0);
        margins(&wrapper, 0, 10, 10, 10);
        wrapper.set_vexpand(true);
        match f {
            FileContent::Text(s, truncated) => {
                if *truncated {
                    let warn = hbox(6);
                    warn.append(&colored_icon("sy-attention-symbolic", 12, "amber"));
                    warn.append(&label_with("File is too large — showing only part of it", &["fg-amber", "sy-small"]));
                    pad(&warn, 6);
                    wrapper.append(&warn);
                }
                self.code.set_text(s, path);
                self.code.set_wrap(self.wrap.get());
                detach(&self.code.widget);
                wrapper.append(&frame(&self.code.widget, "sy-sunken"));
            }
            FileContent::Markdown(html, source) => {
                if self.markdown_source.get() {
                    self.code.set_text(source, path);
                    self.code.set_wrap(true);
                    detach(&self.code.widget);
                    wrapper.append(&frame(&self.code.widget, "sy-sunken"));
                } else {
                    let view = self.html();
                    view.set_html(&markdown_page(html, Look::current()));
                    detach(&view.view);
                    wrapper.append(&frame(&view.view, "sy-card-flat"));
                }
            }
            FileContent::Image(tex) => {
                let pic = gtk::Picture::for_paintable(tex);
                pic.set_can_shrink(true);
                pic.set_content_fit(gtk::ContentFit::ScaleDown);
                pad(&pic, 20);
                let s = scrolled_both(&pic);
                wrapper.append(&frame(&s, "sy-sunken"));
            }
            FileContent::Binary(size) => {
                let b = vbox(8);
                b.set_valign(gtk::Align::Center);
                b.set_halign(gtk::Align::Center);
                b.set_vexpand(true);
                b.append(&icon_tile("sy-archive-symbolic", "slate", 48));
                b.append(&label_with(&format!("Binary file · {}", util::byte_count(*size)), &["sy-muted"]));
                let open = button("Open in browser");
                let m = self.model.clone();
                open.connect_clicked(move |_| util::open_url(&m.web_url()));
                b.append(&open);
                wrapper.append(&b);
            }
        }
        wrapper.upcast()
    }
}

pub fn detach<W: IsA<gtk::Widget>>(w: &W) {
    if let Some(parent) = w.parent() {
        if let Ok(b) = parent.clone().downcast::<gtk::Box>() {
            b.remove(w);
        } else if let Ok(b) = parent.clone().downcast::<adw::Bin>() {
            b.set_child(None::<&gtk::Widget>);
        } else {
            w.unparent();
        }
    }
}

pub fn quick_open(model: Rc<CodeModel>) {
    let sheet = Sheet::new("Go to file", 620, 520);
    let entry = gtk::SearchEntry::new();
    entry.set_placeholder_text(Some("Go to file…"));
    sheet.body.append(&entry);
    let list = gtk::ListBox::new();
    list.add_css_class("sy-list");
    let stack = gtk::Stack::new();
    stack.add_named(&scrolled(&list), Some("list"));
    stack.add_named(&loading(), Some("loading"));
    stack.set_size_request(-1, 360);
    sheet.body.append(&stack);
    let trunc = label_with("Large repository — file list is incomplete", &["fg-amber", "sy-small"]);
    trunc.set_visible(false);
    sheet.body.append(&trunc);
    sheet.footer.append(&label_with("Enter open · Esc close", &["sy-dim", "sy-small"]));
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let open = suggested("Open");
    sheet.footer.append(&open);
    let results: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(vec![]));
    let results_query: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let show = {
        let list = list.clone();
        let results = results.clone();
        let results_query = results_query.clone();
        Rc::new(move |query: String, r: Vec<String>| {
            let _span = crate::diagnostics::ui_span("quickopen.render");
            list.remove_all();
            for path in &r {
                let row = gtk::ListBoxRow::new();
                let col = vbox(1);
                col.append(&label_with(&util::last_component(path), &["sy-text"]));
                let full = label_with(path, &["sy-muted", "sy-small", "sy-mono"]);
                full.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                col.append(&full);
                row.set_child(Some(&col));
                list.append(&row);
            }
            if let Some(first) = list.row_at_index(0) {
                list.select_row(Some(&first));
            }
            *results.borrow_mut() = r;
            *results_query.borrow_mut() = Some(query);
        })
    };
    let search: Rc<RefCell<Option<Rc<util::FuzzySearch>>>> = Rc::new(RefCell::new(None));
    let refresh = {
        let search = search.clone();
        let entry = entry.clone();
        let show = show.clone();
        Rc::new(move || {
            let Some(s) = search.borrow().clone() else { return };
            let query = entry.text().to_string();
            let show = show.clone();
            let search = search.clone();
            crate::rt::spawn_local(async move {
                let Some(r) = s.run(&query, 200, "quickopen.filter").await else { return };
                if search.borrow().as_ref().is_some_and(|current| Rc::ptr_eq(current, &s)) {
                    show(query, r);
                }
            });
        })
    };
    let flush = {
        let search = search.clone();
        let entry = entry.clone();
        let results_query = results_query.clone();
        move || {
            let Some(s) = search.borrow().clone() else { return };
            let query = entry.text().to_string();
            if results_query.borrow().as_deref() == Some(query.as_str()) {
                return;
            }
            let r = s.run_now(&query, 200, "quickopen.filter");
            show(query, r);
        }
    };
    let epoch = Rc::new(Cell::new(0u64));
    let building = Rc::new(Cell::new(false));
    let release = {
        let epoch = epoch.clone();
        let building = building.clone();
        let search = search.clone();
        move || {
            epoch.set(epoch.get() + 1);
            building.set(false);
            search.borrow_mut().take();
        }
    };
    let sync = {
        let model = model.clone();
        let stack = stack.clone();
        let trunc = trunc.clone();
        let refresh = refresh.clone();
        Rc::new(move || {
            let paths = match model.all_paths.borrow().as_ref() {
                None => None,
                Some(_) if search.borrow().is_some() || building.get() => return,
                Some(p) => Some(p.clone()),
            };
            match paths {
                None => {
                    epoch.set(epoch.get() + 1);
                    building.set(false);
                    search.borrow_mut().take();
                    stack.set_visible_child_name("loading");
                }
                Some(p) => {
                    stack.set_visible_child_name("loading");
                    trunc.set_visible(model.paths_truncated.get());
                    building.set(true);
                    let current = epoch.get();
                    let epoch = epoch.clone();
                    let building = building.clone();
                    let search = search.clone();
                    let stack = stack.clone();
                    let refresh = refresh.clone();
                    crate::rt::spawn_local(async move {
                        let s = util::FuzzySearch::build(p).await;
                        if epoch.get() != current {
                            return;
                        }
                        building.set(false);
                        *search.borrow_mut() = Some(s);
                        stack.set_visible_child_name("list");
                        refresh();
                    });
                }
            }
        })
    };
    let d = sheet.dialog.clone();
    let go = {
        let model = model.clone();
        let results = results.clone();
        let list = list.clone();
        Rc::new(move |idx: Option<i32>| {
            if idx.is_none() {
                flush();
            }
            let i = idx.or_else(|| list.selected_row().map(|r| r.index())).unwrap_or(0).max(0) as usize;
            if let Some(path) = results.borrow().get(i).cloned() {
                d.close();
                let m = model.clone();
                crate::rt::spawn_local(async move { m.reveal(path).await });
            }
        })
    };
    entry.set_search_delay(100);
    let r = refresh.clone();
    entry.connect_search_changed(move |_| r());
    let g = go.clone();
    entry.connect_activate(move |_| g(None));
    let g = go.clone();
    list.connect_row_activated(move |_, row| g(Some(row.index())));
    let g = go.clone();
    open.connect_clicked(move |_| g(None));
    let r = sync.clone();
    let closed = Rc::new(Cell::new(false));
    let c = closed.clone();
    sheet.dialog.connect_closed(move |_| {
        c.set(true);
        release();
    });
    model.changed.connect(move |_| {
        if closed.get() {
            return false;
        }
        r();
        true
    });
    sync();
    let m = model.clone();
    crate::rt::spawn_local(async move { m.load_all_paths().await });
    sheet.present();
    entry.grab_focus();
}

pub struct RepoDetail {
    repo: RepoRef,
    info: RefCell<Option<RepoInfo>>,
    tabs: Rc<TabStrip>,
    header_box: gtk::Box,
    content: gtk::Stack,
    list_model: Rc<SearchModel>,
    list: gtk::ListBox,
    list_stack: gtk::Stack,
    state: Rc<Cell<StateFilter>>,
    code_model: Rc<CodeModel>,
    code_started: Cell<bool>,
    toggles: gtk::Box,
    status: Rc<RefreshStatus>,
}

impl RepoDetail {
    fn query(&self) -> String {
        let kind = if self.tabs.active() == "issue" { "issue" } else { "pr" };
        [format!("repo:{}", self.repo.full()), format!("is:{kind}"), self.state.get().query().to_string(), "sort:updated-desc".into()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
    }

    fn render_header(&self) {
        clear(&self.header_box);
        let info = self.info.borrow().clone();
        let card = hbox(12);
        card.add_css_class("sy-card");
        let tile = icon_tile(if info.as_ref().map(|i| i.is_private).unwrap_or(false) { "sy-lock-symbolic" } else { "sy-repo-symbolic" }, "teal", 36);
        tile.set_valign(gtk::Align::Start);
        card.append(&tile);
        let col = vbox(4);
        col.set_hexpand(true);
        let top = hbox(8);
        top.append(&selectable(label_with(&self.repo.full(), &["sy-title", "sy-text"])));
        if info.as_ref().map(|i| i.is_private).unwrap_or(false) {
            top.append(&chip("private", "amber", Some("sy-lock-symbolic"), false));
        }
        col.append(&top);
        if let Some(d) = info.as_ref().and_then(|i| i.description.clone()).filter(|d| !d.is_empty()) {
            let l = wrap_label(&d, &["sy-muted"]);
            l.set_lines(2);
            l.set_ellipsize(gtk::pango::EllipsizeMode::End);
            col.append(&l);
        }
        if let Some(i) = &info {
            let chips = hbox(6);
            chips.append(&chip(&i.stargazer_count.to_string(), "amber", Some("sy-star-symbolic"), false));
            chips.append(&chip(&i.fork_count.to_string(), "sky", Some("sy-branch-symbolic"), false));
            if let Some(b) = &i.default_branch_ref {
                chips.append(&chip(&b.name, "teal", Some("sy-branch-symbolic"), true));
            }
            col.append(&chips);
            self.tabs.set_badge("pr", Some(&i.pull_requests.total_count.to_string()), None);
            self.tabs.set_badge("issue", Some(&i.issues.total_count.to_string()), None);
        }
        card.append(&col);
        self.header_box.append(&card);
    }

    fn render_list(&self) {
        let _span = crate::diagnostics::ui_span("repo.list");
        let m = &self.list_model;
        let items = m.items.borrow().clone();
        self.status.update(m.last_updated.get(), m.refreshing.get(), if items.is_empty() { None } else { m.error.borrow().clone() });
        self.list.remove_all();
        if items.is_empty() {
            self.list_stack.set_visible_child_name(if m.loading.get() { "loading" } else { "empty" });
            return;
        }
        self.list_stack.set_visible_child_name("list");
        for item in items {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&item_row(&item, true)));
            let it = item.clone();
            context_menu(&row, move || item_menu(&it));
            self.list.append(&row);
        }
    }

    fn reload_list(self: &Rc<Self>, silent: bool) {
        if self.tabs.active() == "code" {
            return;
        }
        let m = self.list_model.clone();
        let q = self.query();
        crate::rt::spawn_local(async move { m.load(q, silent, true).await });
    }

    fn switch_tab(self: &Rc<Self>, id: &str) {
        if id == "code" {
            self.content.set_visible_child_name("code");
            self.toggles.set_visible(false);
            self.start_code();
        } else {
            self.content.set_visible_child_name("list");
            self.toggles.set_visible(true);
            self.reload_list(false);
        }
    }

    fn start_code(self: &Rc<Self>) {
        if self.code_started.get() {
            return;
        }
        let Some(info) = self.info.borrow().clone() else { return };
        self.code_started.set(true);
        let branch = info.default_branch_ref.map(|b| b.name).unwrap_or_else(|| "HEAD".into());
        let m = self.code_model.clone();
        crate::rt::spawn_local(async move { m.start(branch).await });
    }
}

pub fn detail_page(repo: &RepoRef) -> adw::NavigationPage {
    let Some(api) = app::api() else { return adw::NavigationPage::new(&loading(), &repo.name) };
    let tabs = TabStrip::new(vec![
        TabItem { id: "code", title: "Code".into(), icon: "sy-code-symbolic" },
        TabItem { id: "pr", title: "Pull requests".into(), icon: "sy-pr-symbolic" },
        TabItem { id: "issue", title: "Issues".into(), icon: "sy-issue-symbolic" },
    ]);
    let header_box = vbox(0);
    let list = gtk::ListBox::new();
    list.add_css_class("sy-list");
    let list_stack = gtk::Stack::new();
    list_stack.add_named(&scrolled(&list), Some("list"));
    list_stack.add_named(&loading(), Some("loading"));
    let empty = label_with("Nothing here", &["sy-muted"]);
    empty.set_valign(gtk::Align::Start);
    empty.set_margin_top(30);
    empty.set_halign(gtk::Align::Center);
    list_stack.add_named(&empty, Some("empty"));
    let code_model = CodeModel::new(repo.clone(), api.clone());
    let browser = CodeBrowser::new(code_model.clone());
    let content = gtk::Stack::new();
    content.set_vexpand(true);
    content.add_named(&browser.widget, Some("code"));
    let holder: Rc<RefCell<Option<std::rc::Weak<RepoDetail>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(me) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            me.reload_list(false);
        }
    });
    let list_col = vbox(0);
    list_col.append(&list_stack);
    list_col.append(&footer(&status));
    list_stack.set_vexpand(true);
    content.add_named(&list_col, Some("list"));
    let (toggles, state) = state_toggles(|_| {});
    toggles.set_visible(false);
    let me = Rc::new(RepoDetail {
        repo: repo.clone(),
        info: RefCell::new(None),
        tabs: tabs.clone(),
        header_box: header_box.clone(),
        content: content.clone(),
        list_model: SearchModel::new(),
        list: list.clone(),
        list_stack,
        state,
        code_model,
        code_started: Cell::new(false),
        toggles: toggles.clone(),
        status,
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let mut c = toggles.first_child();
    while let Some(w) = c {
        if let Ok(t) = w.clone().downcast::<gtk::ToggleButton>() {
            let weak = Rc::downgrade(&me);
            t.connect_toggled(move |t| {
                if t.is_active() {
                    if let Some(me) = weak.upgrade() {
                        glib::idle_add_local_once(move || me.reload_list(false));
                    }
                }
            });
        }
        c = w.next_sibling();
    }
    let weak = Rc::downgrade(&me);
    tabs.on_change(move |id| {
        if let Some(me) = weak.upgrade() {
            me.switch_tab(id);
        }
    });
    let weak = Rc::downgrade(&me);
    me.list_model.changed.connect(move |_| match weak.upgrade() {
        Some(me) => {
            me.render_list();
            true
        }
        None => false,
    });
    let weak = Rc::downgrade(&me);
    list.connect_row_activated(move |_, row| {
        if let Some(me) = weak.upgrade() {
            let item = me.list_model.items.borrow().get(row.index().max(0) as usize).cloned();
            if let Some(item) = item {
                app::open(item.route(), true);
            }
        }
    });
    let weak = Rc::downgrade(&me);
    app::on_refresh(move |_| match weak.upgrade() {
        Some(me) => {
            if me.tabs.active() == "code" {
                let m = me.code_model.clone();
                let d = m.dir.borrow().clone();
                crate::rt::spawn_local(async move { m.open_dir(d).await });
            } else {
                me.reload_list(false);
            }
            true
        }
        None => false,
    });
    let weak = Rc::downgrade(&me);
    app::on_live_visible(&list_col, move |_| match weak.upgrade() {
        Some(me) => {
            me.reload_list(true);
            true
        }
        None => false,
    });

    let page = vbox(0);
    margins(&header_box, 12, 16, 4, 16);
    page.append(&header_box);
    let tabs_row = hbox(8);
    tabs_row.append(&tabs.widget);
    tabs_row.append(&spacer());
    margins(&toggles, 0, 16, 0, 0);
    toggles.set_valign(gtk::Align::Center);
    tabs_row.append(&toggles);
    page.append(&tabs_row);
    page.append(&line());
    page.append(&content);
    let (tv, hb, _) = toolbar_page(&repo.name, &repo.owner, &page);
    tv.add_css_class("sy-panel-pane");
    let clone_btn = icon_button("sy-terminal-symbolic", "Copy “gh repo clone”");
    let full = repo.full();
    clone_btn.connect_clicked(move |_| {
        util::copy(&format!("gh repo clone {full}"));
        app::toast("Copied clone command");
    });
    let browser_btn = icon_button("sy-external-symbolic", "Open in browser");
    let cm = me.code_model.clone();
    browser_btn.connect_clicked(move |_| util::open_url(&cm.web_url()));
    hb.pack_end(&browser_btn);
    hb.pack_end(&clone_btn);
    me.render_header();
    let weak = Rc::downgrade(&me);
    let r = repo.clone();
    crate::rt::spawn_local(async move {
        let info = crate::rt::run(async move { api.repo_info(&r).await }).await.ok().flatten();
        if let Some(me) = weak.upgrade() {
            *me.info.borrow_mut() = info.or(Some(RepoInfo {
                name_with_owner: me.repo.full(),
                description: None,
                url: format!("https://github.com/{}", me.repo.full()),
                is_private: false,
                stargazer_count: 0,
                fork_count: 0,
                default_branch_ref: None,
                pull_requests: Count::default(),
                issues: Count::default(),
            }));
            me.render_header();
            if me.tabs.active() == "code" {
                me.start_code();
            }
        }
    });
    keep_alive(&tv, (me, browser));
    adw::NavigationPage::new(&tv, &repo.name)
}
