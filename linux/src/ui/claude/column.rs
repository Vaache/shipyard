use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;

use super::auth_view;
use crate::claude::accounts::{self, Status};
use crate::claude::sessions::{self, Project, SessionInfo};
use crate::claude::workspace::{Tab, WsEvent};
use crate::claude::{memory, TabKind};
use crate::ui::dialogs;
use crate::ui::menu::{context_menu, Menu};
use crate::ui::widgets::*;
use crate::{app, util};

struct Column {
    list: gtk::Box,
    tabs_box: gtk::Box,
    query: RefCell<String>,
    expanded: RefCell<HashSet<String>>,
    status: Rc<RefreshStatus>,
    open_folder: gtk::Button,
    initialized: std::cell::Cell<bool>,
}

fn session_row(s: &SessionInfo, open: bool) -> gtk::Box {
    let b = hbox(8);
    margins(&b, 3, 8, 3, 8);
    let edge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    edge.add_css_class(&format!("edge-{}", crate::theme::palette_for(&sessions::project_root(&s.cwd))));
    if !open {
        edge.set_opacity(0.45);
    }
    b.append(&edge);
    let col = vbox(4);
    col.set_hexpand(true);
    let top = hbox(6);
    if open {
        top.append(&tooltip(colored_icon("sy-comment-symbolic", 11, "green"), "Open"));
    }
    let t = wrap_label(&s.title, &["sy-text"]);
    t.set_lines(2);
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    if open {
        t.add_css_class("sy-bold");
    }
    top.append(&t);
    col.append(&top);
    let meta = hbox(5);
    if let Some(br) = &s.branch {
        meta.append(&branch_tag(br));
    }
    if let Some(w) = s.worktree_name() {
        let wt = hbox(3);
        wt.append(&colored_icon("sy-stack-symbolic", 11, "teal"));
        wt.append(&ellipsized(&w, &["fg-teal", "sy-small"]));
        meta.append(&wt);
    }
    if let Some(n) = s.pr_number {
        meta.append(&pr_tag(n));
    }
    meta.append(&spacer());
    if let (Some(a), Some(r)) = (s.lines_added, s.lines_removed) {
        if a + r > 0 {
            meta.append(&diff_stat(a, r));
        }
    }
    meta.append(&label_with(&util::rel(Some(s.modified)), &["sy-dim", "sy-small"]));
    col.append(&meta);
    b.append(&col);
    b
}

fn tab_row(t: &Tab, selected: bool) -> gtk::Box {
    let b = hbox(8);
    margins(&b, 3, 8, 3, 8);
    if selected {
        b.add_css_class("bg-sky");
    }
    let edge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    edge.add_css_class(&format!("edge-{}", crate::theme::palette_for(&t.folder)));
    b.append(&edge);
    let col = vbox(3);
    col.set_hexpand(true);
    let top = hbox(6);
    top.append(&colored_icon(if t.kind == TabKind::Chat { "sy-comment-symbolic" } else { "sy-terminal-symbolic" }, 11, "muted"));
    let title = ellipsized(&t.title.borrow(), &["sy-text"]);
    if selected {
        title.add_css_class("sy-bold");
    }
    top.append(&title);
    col.append(&top);
    let meta = hbox(6);
    meta.append(&super::state_badge(t.state(), Some(t.memory_bytes()), false));
    meta.append(&spacer());
    meta.append(&ellipsized(&t.project_name(), &[&format!("fg-{}", crate::theme::palette_for(&t.folder)), "sy-small"]));
    col.append(&meta);
    b.append(&col);
    b
}

fn project_header(p: &Project, expanded: bool, on_toggle: impl Fn() + 'static, on_new: impl Fn() + 'static) -> gtk::Box {
    let color = crate::theme::palette_for(&p.path);
    let b = hbox(7);
    margins(&b, 6, 8, 3, 8);
    b.append(&icon(if expanded { "pan-down-symbolic" } else { "pan-end-symbolic" }, 12));
    b.append(&icon_tile(if p.exists() { "sy-folder-symbolic" } else { "sy-attention-symbolic" }, if p.exists() { color } else { "slate" }, 24));
    let col = vbox(0);
    col.set_hexpand(true);
    col.append(&ellipsized(&p.name(), &["sy-bold", "sy-text"]));
    let path = label_with(&util::tilde(&p.path), &["sy-dim", "sy-tiny"]);
    path.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    col.append(&path);
    b.append(&col);
    b.append(&chip(&p.sessions.len().to_string(), color, None, false));
    let new = icon_button("list-add-symbolic", "New session");
    new.add_css_class("fg-coral");
    new.set_sensitive(p.exists());
    new.connect_clicked(move |_| on_new());
    b.append(&new);
    if !p.exists() {
        b.set_opacity(0.6);
    }
    on_click_widget(&b, on_toggle);
    b
}

fn session_menu(s: &SessionInfo) -> Menu {
    let menu = Menu::ctx();
    let ws = app::get().claude.clone();
    let (s1, w1) = (s.clone(), ws.clone());
    menu.item("Resume as chat", move || {
        w1.resume(&s1, false, Some(TabKind::Chat));
    });
    let (s2, w2) = (s.clone(), ws.clone());
    menu.item("Resume in terminal", move || {
        w2.resume(&s2, false, Some(TabKind::Terminal));
    });
    let (s3, w3) = (s.clone(), ws.clone());
    menu.item("Fork session", move || {
        w3.resume(&s3, true, None);
    });
    let mut menu = menu;
    menu.section(None);
    let id = s.id.clone();
    menu.item("Copy session ID", move || util::copy(&id));
    let cmd = format!("cd {} && claude --resume {}", util::shell_quote(&s.cwd), s.id);
    menu.item("Copy resume command", move || util::copy(&cmd));
    let file = s.file.to_string_lossy().to_string();
    menu.item("Show transcript in Files", move || util::reveal_path(&file));
    if let Some(pr) = s.pr_url.clone() {
        menu.item("Open PR", move || {
            if let Some(r) = util::parse_github_url(&pr, true) {
                app::open(r, false);
            }
        });
    }
    menu
}

impl Column {
    fn render(self: &Rc<Self>) {
        clear(&self.list);
        let store = accounts::store();
        let status = store.status.borrow().clone();
        let ws = app::get().claude.clone();
        self.open_folder.set_sensitive(store.is_logged_in());
        self.status.update(ws.projects_updated.get(), ws.loading_projects.get(), None);
        match &status {
            Status::LoggedIn(info) => {
                let c = auth_view::account_card(info, true);
                margins(&c, 6, 8, 4, 8);
                self.list.append(&c);
            }
            Status::LoggedOut => {
                let l = hbox(6);
                margins(&l, 8, 10, 8, 10);
                l.append(&colored_icon("sy-person-symbolic", 14, "amber"));
                l.append(&wrap_label("Not signed in to Claude — sign in on the right", &["fg-amber"]));
                self.list.append(&l);
                if accounts::profiles().len() > 1 {
                    self.list.append(&auth_view::profile_switcher());
                }
            }
            Status::NotInstalled => {
                let l = hbox(6);
                margins(&l, 8, 10, 8, 10);
                l.append(&colored_icon("sy-attention-symbolic", 14, "amber"));
                l.append(&wrap_label("claude CLI not found — see the right side", &["fg-amber"]));
                self.list.append(&l);
            }
            _ => {}
        }
        match status {
            Status::LoggedIn(_) => {}
            Status::Unknown | Status::Checking => {
                let s = spinner(16);
                s.set_halign(gtk::Align::Center);
                s.set_margin_top(20);
                self.list.append(&s);
                return;
            }
            _ => {
                let b = vbox(6);
                margins(&b, 8, 12, 8, 12);
                b.append(&icon("sy-lock-symbolic", 18));
                b.append(&wrap_label("Session history and projects appear after you sign in to Claude.", &["sy-muted"]));
                self.list.append(&b);
                return;
            }
        }
        self.list.append(&self.tabs_box);
        self.render_tabs();
        self.render_projects();
    }

    fn render_tabs(self: &Rc<Self>) {
        clear(&self.tabs_box);
        if !accounts::store().is_logged_in() {
            return;
        }
        let ws = app::get().claude.clone();
        let tabs: Vec<Rc<Tab>> = ws.tabs.borrow().clone();
        let selected = ws.selected.get();
        if !tabs.is_empty() {
            let head = hbox(6);
            margins(&head, 8, 10, 2, 10);
            head.append(&label_with("OPEN", &["sy-section-title"]));
            head.append(&spacer());
            head.append(&icon("sy-memory-symbolic", 12));
            let total = ws.total_memory();
            head.append(&label_with(&if total > 0 { memory::format(total) } else { "0 MB".into() }, &["sy-muted", "sy-small"]));
            if ws.running_count() > 0 {
                let sleep = link_button("Sleep all");
                sleep.set_tooltip_text(Some("Stop idle processes and free memory"));
                let w = ws.clone();
                sleep.connect_clicked(move |_| w.sleep_all());
                head.append(&sleep);
            }
            head.set_tooltip_text(Some("Memory used by running Claude processes"));
            self.tabs_box.append(&head);
            let lb = gtk::ListBox::new();
            lb.add_css_class("sy-sidebar-list");
            for t in &tabs {
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&tab_row(t, selected == Some(t.id))));
                let tt = t.clone();
                context_menu(&row, move || {
                    let m = Menu::ctx();
                    let ws = app::get().claude.clone();
                    let t2 = tt.clone();
                    if tt.running() {
                        let busy = matches!(tt.state(), crate::claude::TabState::Working | crate::claude::TabState::Waiting);
                        if busy {
                            m.disabled("Free memory (sleep)");
                        } else {
                            m.item("Free memory (sleep)", move || t2.sleep_now());
                        }
                    } else {
                        let w = ws.clone();
                        m.item("Wake up", move || w.restart(&t2));
                    }
                    let t3 = tt.clone();
                    m.item("Close", move || ws.close(&t3));
                    m
                });
                lb.append(&row);
            }
            let ids: Vec<u64> = tabs.iter().map(|t| t.id).collect();
            lb.connect_row_activated(move |_, row| {
                if let Some(id) = ids.get(row.index().max(0) as usize) {
                    app::get().claude.select(*id);
                }
            });
            self.tabs_box.append(&lb);
        }
    }

    fn render_projects(self: &Rc<Self>) {
        let ws = app::get().claude.clone();
        let query = self.query.borrow().to_lowercase();
        let projects: Vec<Project> = ws
            .projects
            .borrow()
            .iter()
            .filter_map(|p| {
                if query.is_empty() || p.name().to_lowercase().contains(&query) {
                    return Some(p.clone());
                }
                let s: Vec<SessionInfo> = p.sessions.iter().filter(|s| s.title.to_lowercase().contains(&query) || s.branch.as_deref().unwrap_or("").to_lowercase().contains(&query)).cloned().collect();
                if s.is_empty() {
                    None
                } else {
                    Some(Project { id: p.id.clone(), path: p.path.clone(), sessions: s })
                }
            })
            .collect();
        if !self.initialized.get() {
            if let Some(first) = projects.first() {
                self.expanded.borrow_mut().insert(first.id.clone());
                self.initialized.set(true);
            }
        }
        for p in projects {
            let expanded = self.expanded.borrow().contains(&p.id) || !query.is_empty();
            let w = Rc::downgrade(self);
            let id = p.id.clone();
            let path = p.path.clone();
            self.list.append(&project_header(
                &p,
                expanded,
                move || {
                    if let Some(me) = w.upgrade() {
                        {
                            let mut e = me.expanded.borrow_mut();
                            if !e.remove(&id) {
                                e.insert(id.clone());
                            }
                        }
                        me.render();
                    }
                },
                move || {
                    app::get().claude.new_session(&path, None, None, None, None);
                },
            ));
            if expanded {
                let lb = gtk::ListBox::new();
                lb.add_css_class("sy-sidebar-list");
                let limit = if query.is_empty() { 30 } else { 200 };
                let list: Vec<SessionInfo> = p.sessions.iter().take(limit).cloned().collect();
                for s in &list {
                    let row = gtk::ListBoxRow::new();
                    row.set_child(Some(&session_row(s, ws.tab_for_session(&s.id).is_some())));
                    let ss = s.clone();
                    context_menu(&row, move || session_menu(&ss));
                    lb.append(&row);
                }
                lb.connect_row_activated(move |_, row| {
                    if let Some(s) = list.get(row.index().max(0) as usize) {
                        app::get().claude.resume(s, false, None);
                    }
                });
                self.list.append(&lb);
            }
        }
        if ws.projects.borrow().is_empty() && !ws.loading_projects.get() {
            let l = label_with("No Claude Code sessions yet", &["sy-muted"]);
            margins(&l, 10, 12, 10, 12);
            self.list.append(&l);
        }
    }
}

pub fn open_folder() {
    dialogs::choose_folder("New session in folder", |path| {
        app::get().claude.new_session(&path, None, None, None, None);
    });
}

pub fn build() -> gtk::Widget {
    let list = vbox(0);
    let scroll = scrolled(&list);
    let open = icon_button("sy-folder-symbolic", "New session in folder…");
    open.connect_clicked(|_| open_folder());
    let holder: Rc<RefCell<Option<std::rc::Weak<Column>>>> = Rc::new(RefCell::new(None));
    let status = RefreshStatus::new(false, || {
        let ws = app::get().claude.clone();
        crate::rt::spawn_local(async move { ws.reload_projects().await });
    });
    let me = Rc::new(Column {
        list,
        tabs_box: vbox(0),
        query: RefCell::new(String::new()),
        expanded: RefCell::new(HashSet::new()),
        status,
        open_folder: open.clone(),
        initialized: std::cell::Cell::new(false),
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Search sessions"));
    let w = Rc::downgrade(&me);
    search.connect_search_changed(move |e| {
        if let Some(me) = w.upgrade() {
            *me.query.borrow_mut() = e.text().trim().to_string();
            me.render();
        }
    });
    let search_bar = gtk::SearchBar::new();
    search_bar.set_child(Some(&search));
    search_bar.connect_entry(&search);
    let search_btn = gtk::ToggleButton::new();
    search_btn.set_icon_name("system-search-symbolic");
    search_btn.set_tooltip_text(Some("Search sessions"));
    search_btn.bind_property("active", &search_bar, "search-mode-enabled").bidirectional().build();
    let content = vbox(0);
    content.append(&scroll);
    content.append(&footer(&me.status));
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_end_title_buttons(false);
    hb.set_title_widget(Some(&adw::WindowTitle::new("Claude Code", "")));
    hb.pack_start(&search_btn);
    hb.pack_end(&open);
    tv.add_top_bar(&hb);
    tv.add_top_bar(&search_bar);
    tv.set_content(Some(&content));
    let w = Rc::downgrade(&me);
    app::get().claude.changed.connect(move |e| match w.upgrade() {
        Some(me) => {
            match e {
                WsEvent::TabState => me.render_tabs(),
                _ => me.render(),
            }
            true
        }
        None => false,
    });
    let w = Rc::downgrade(&me);
    accounts::store().changed.connect(move |_| match w.upgrade() {
        Some(me) => {
            me.render();
            true
        }
        None => false,
    });
    let w = Rc::downgrade(&me);
    app::on_refresh(move |_| match w.upgrade() {
        Some(_) => {
            let ws = app::get().claude.clone();
            crate::rt::spawn_local(async move { ws.reload_projects().await });
            true
        }
        None => false,
    });
    me.render();
    let ws = app::get().claude.clone();
    crate::rt::spawn_local(async move {
        accounts::store().refresh().await;
        ws.reload_projects().await;
    });
    keep_alive(&tv, me);
    tv.upcast()
}
