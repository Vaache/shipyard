use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};

use super::menu::{context_menu, Menu};
use super::widgets::*;
use crate::app;
use crate::forge::{ForgeNotification, ForgeRepo};
use crate::util;

struct Inbox {
    items: RefCell<Vec<ForgeNotification>>,
    loading: Cell<bool>,
    refreshing: Cell<bool>,
    error: RefCell<Option<String>>,
    updated: Cell<Option<DateTime<Utc>>>,
    list: gtk::ListBox,
    title: adw::WindowTitle,
    stack: gtk::Stack,
    status: Rc<RefreshStatus>,
}

fn shell(title: &adw::WindowTitle, list: &gtk::ListBox, stack: &gtk::Stack, status: &RefreshStatus, empty_text: &str, search: Option<&gtk::SearchEntry>) -> gtk::Widget {
    stack.add_named(&scrolled(list), Some("list"));
    stack.add_named(&loading(), Some("loading"));
    let empty = label_with(empty_text, &["sy-muted"]);
    empty.set_valign(gtk::Align::Start);
    empty.set_halign(gtk::Align::Center);
    empty.set_margin_top(30);
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&vbox(8), Some("error"));
    let content = vbox(0);
    content.append(stack);
    content.append(&footer(status));
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_end_title_buttons(false);
    hb.set_title_widget(Some(title));
    tv.add_top_bar(&hb);
    if let Some(search) = search {
        let search_bar = gtk::SearchBar::new();
        search_bar.set_child(Some(search));
        search_bar.connect_entry(search);
        let search_btn = gtk::ToggleButton::new();
        search_btn.set_icon_name("system-search-symbolic");
        search_btn.set_tooltip_text(Some("Filter"));
        search_btn.bind_property("active", &search_bar, "search-mode-enabled").bidirectional().build();
        hb.pack_start(&search_btn);
        tv.add_top_bar(&search_bar);
    }
    tv.set_content(Some(&content));
    tv.upcast()
}

fn show_state(stack: &gtk::Stack, empty: bool, loading: bool, error: Option<&str>) {
    if !empty {
        stack.set_visible_child_name("list");
    } else if loading {
        stack.set_visible_child_name("loading");
    } else if let Some(e) = error {
        if let Some(page) = stack.child_by_name("error").and_downcast::<gtk::Box>() {
            clear(&page);
            let l = wrap_label(e, &["fg-red", "sy-small"]);
            pad(&l, 12);
            page.append(&l);
        }
        stack.set_visible_child_name("error");
    } else {
        stack.set_visible_child_name("empty");
    }
}

fn notification_row(n: &ForgeNotification, on_done: impl Fn() + 'static) -> gtk::Box {
    let row = hbox(10);
    margins(&row, 5, 0, 5, 0);
    let a = avatar(n.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 22, n.author.as_ref().map(|a| a.login.as_str()));
    a.set_valign(gtk::Align::Start);
    row.append(&a);
    let col = vbox(3);
    col.set_hexpand(true);
    let title = wrap_label(&n.title, &["sy-bold", "sy-text"]);
    title.set_lines(2);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    col.append(&title);
    let meta = hbox(5);
    meta.append(&label_with(&n.reason, &["fg-accent", "sy-small"]));
    if !n.repo.is_empty() {
        meta.append(&label_with("·", &["sy-muted", "sy-small"]));
        meta.append(&ellipsized(&n.repo, &["sy-muted", "sy-small"]));
    }
    meta.append(&label_with("·", &["sy-muted", "sy-small"]));
    meta.append(&label_with(&util::rel(Some(n.updated_at)), &["sy-muted", "sy-small"]));
    col.append(&meta);
    if let Some(author) = &n.author {
        col.append(&label_with(&format!("{} · @{}", n.kind, author.login), &["sy-dim", "sy-small"]));
    }
    row.append(&col);
    let done = icon_button("sy-check-symbolic", "Mark as done");
    done.set_valign(gtk::Align::Start);
    done.connect_clicked(move |_| on_done());
    row.append(&done);
    if n.unread {
        row.add_css_class("unread");
    } else {
        row.set_opacity(0.7);
    }
    row
}

impl Inbox {
    fn render(self: &Rc<Self>) {
        let items = self.items.borrow().clone();
        self.title.set_subtitle(&if items.is_empty() { String::new() } else { items.len().to_string() });
        let err = self.error.borrow().clone();
        self.status.update(self.updated.get(), self.refreshing.get(), if items.is_empty() { None } else { err.clone() });
        show_state(&self.stack, items.is_empty(), self.loading.get(), err.as_deref());
        rebuild_rows(&self.list, || {
            for n in &items {
                let row = gtk::ListBoxRow::new();
                let weak = Rc::downgrade(self);
                let target = n.clone();
                row.set_child(Some(&notification_row(n, move || {
                    if let Some(me) = weak.upgrade() {
                        me.mark_done(target.clone());
                    }
                })));
                let weak = Rc::downgrade(self);
                let target = n.clone();
                context_menu(&row, move || {
                    let menu = Menu::ctx();
                    let url = target.url.clone();
                    menu.item("Open in Browser", move || util::open_url(&url));
                    let url = target.url.clone();
                    menu.item("Copy Link", move || util::copy(&url));
                    let weak = weak.clone();
                    let t = target.clone();
                    menu.item("Mark as Done", move || {
                        if let Some(me) = weak.upgrade() {
                            me.mark_done(t.clone());
                        }
                    });
                    menu
                });
                self.list.append(&row);
            }
        });
    }

    fn load(self: &Rc<Self>) {
        let Some(forge) = app::forge() else { return };
        self.refreshing.set(true);
        self.render();
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let f = forge.clone();
            let result = crate::rt::run(async move { f.notifications().await }).await;
            if !app::forge().is_some_and(|c| c.same(&forge)) {
                return;
            }
            match result {
                Ok(items) => {
                    app::get().set_unread(items.iter().filter(|n| n.unread).count());
                    *me.items.borrow_mut() = items;
                    *me.error.borrow_mut() = None;
                    me.updated.set(Some(Utc::now()));
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.loading.set(false);
            me.refreshing.set(false);
            me.render();
        });
    }

    fn mark_done(self: &Rc<Self>, n: ForgeNotification) {
        let Some(forge) = app::forge() else { return };
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let target = n.clone();
            match crate::rt::run(async move { forge.mark_done(&target).await }).await {
                Ok(()) => {
                    me.items.borrow_mut().retain(|x| x.id != n.id);
                    let unread = me.items.borrow().iter().filter(|x| x.unread).count();
                    app::get().set_unread(unread);
                    me.render();
                }
                Err(e) => app::toast(&e.message),
            }
        });
    }
}

pub fn inbox() -> gtk::Widget {
    let caps = app::forge_caps();
    let list = gtk::ListBox::new();
    list.add_css_class("sy-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    list.set_activate_on_single_click(true);
    let title = adw::WindowTitle::new(caps.inbox_title.unwrap_or("Inbox"), "");
    let stack = gtk::Stack::new();
    let holder: Rc<RefCell<Option<std::rc::Weak<Inbox>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(me) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            me.load();
        }
    });
    let widget = shell(&title, &list, &stack, &status, "All done", None);
    let me = Rc::new(Inbox { items: RefCell::new(vec![]), loading: Cell::new(true), refreshing: Cell::new(false), error: RefCell::new(None), updated: Cell::new(None), list: list.clone(), title, stack, status });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let weak = Rc::downgrade(&me);
    list.connect_row_activated(move |_, row| {
        let Some(me) = weak.upgrade() else { return };
        let url = me.items.borrow().get(row.index().max(0) as usize).map(|n| n.url.clone());
        if let Some(url) = url {
            util::open_url(&url);
        }
    });
    let weak = Rc::downgrade(&me);
    app::on_refresh(move |_| match weak.upgrade() {
        Some(me) => {
            me.load();
            true
        }
        None => false,
    });
    let weak = Rc::downgrade(&me);
    app::on_live_visible(&widget, move |_| match weak.upgrade() {
        Some(me) => {
            me.load();
            true
        }
        None => false,
    });
    me.load();
    keep_alive(&widget, me);
    widget
}

struct Repos {
    repos: RefCell<Vec<ForgeRepo>>,
    visible: RefCell<Vec<ForgeRepo>>,
    text: RefCell<String>,
    loading: Cell<bool>,
    error: RefCell<Option<String>>,
    updated: Cell<Option<DateTime<Utc>>>,
    list: gtk::ListBox,
    title: adw::WindowTitle,
    stack: gtk::Stack,
    status: Rc<RefreshStatus>,
}

pub fn repo_matches(r: &ForgeRepo, text: &str) -> bool {
    let q = text.trim().to_lowercase();
    q.is_empty() || r.full_name.to_lowercase().contains(&q) || r.description.as_deref().is_some_and(|d| d.to_lowercase().contains(&q))
}

fn repo_row(r: &ForgeRepo) -> gtk::Box {
    let row = hbox(10);
    margins(&row, 5, 0, 5, 0);
    let tile = icon_tile(if r.is_private { "sy-lock-symbolic" } else { "sy-repo-symbolic" }, crate::theme::palette_for(&r.full_name), 24);
    tile.set_valign(gtk::Align::Start);
    row.append(&tile);
    let col = vbox(3);
    col.set_hexpand(true);
    col.append(&ellipsized(&r.full_name, &["sy-bold", "sy-text"]));
    if let Some(d) = &r.description {
        let l = wrap_label(d, &["sy-muted", "sy-small"]);
        l.set_lines(2);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        col.append(&l);
    }
    let meta = hbox(8);
    if let Some(b) = &r.default_branch {
        let bb = hbox(3);
        bb.append(&colored_icon("sy-branch-symbolic", 11, "muted"));
        bb.append(&label_with(b, &["sy-dim", "sy-small"]));
        meta.append(&bb);
    }
    if let Some(stars) = r.stars.filter(|s| *s > 0) {
        let sb = hbox(3);
        sb.append(&colored_icon("sy-star-symbolic", 11, "muted"));
        sb.append(&label_with(&stars.to_string(), &["sy-dim", "sy-small"]));
        meta.append(&sb);
    }
    if let Some(updated) = r.updated_at {
        meta.append(&label_with(&format!("updated {}", util::rel(Some(updated))), &["sy-dim", "sy-small"]));
    }
    col.append(&meta);
    row.append(&col);
    row
}

impl Repos {
    fn render(&self) {
        let text = self.text.borrow().clone();
        let visible: Vec<ForgeRepo> = self.repos.borrow().iter().filter(|r| repo_matches(r, &text)).cloned().collect();
        let total = self.repos.borrow().len();
        self.title.set_subtitle(&if total == 0 { String::new() } else { total.to_string() });
        let err = self.error.borrow().clone();
        self.status.update(self.updated.get(), self.loading.get(), if visible.is_empty() { None } else { err.clone() });
        show_state(&self.stack, visible.is_empty(), self.loading.get() && total == 0, err.as_deref());
        rebuild_rows(&self.list, || {
            for r in &visible {
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&repo_row(r)));
                let target = r.clone();
                context_menu(&row, move || {
                    let menu = Menu::ctx();
                    let url = target.url.clone();
                    menu.item("Open in Browser", move || util::open_url(&url));
                    let url = target.url.clone();
                    menu.item("Copy Link", move || util::copy(&url));
                    let name = target.full_name.clone();
                    menu.item("Copy Name", move || util::copy(&name));
                    menu
                });
                self.list.append(&row);
            }
        });
        *self.visible.borrow_mut() = visible;
    }

    fn load(self: &Rc<Self>) {
        let Some(forge) = app::forge() else { return };
        self.loading.set(true);
        self.render();
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let f = forge.clone();
            let result = crate::rt::run(async move { f.repositories().await }).await;
            if !app::forge().is_some_and(|c| c.same(&forge)) {
                return;
            }
            match result {
                Ok(repos) => {
                    *me.repos.borrow_mut() = repos;
                    *me.error.borrow_mut() = None;
                    me.updated.set(Some(Utc::now()));
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.loading.set(false);
            me.render();
        });
    }
}

pub fn repos() -> gtk::Widget {
    let provider = app::provider();
    let list = gtk::ListBox::new();
    list.add_css_class("sy-list");
    list.set_selection_mode(gtk::SelectionMode::None);
    let title = adw::WindowTitle::new(provider.repositories_title(), "");
    let stack = gtk::Stack::new();
    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some(&format!("Filter {}", provider.repositories_title().to_lowercase())));
    search.set_hexpand(true);
    let holder: Rc<RefCell<Option<std::rc::Weak<Repos>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(me) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            me.load();
        }
    });
    let widget = shell(&title, &list, &stack, &status, "Nothing here", Some(&search));
    let me = Rc::new(Repos {
        repos: RefCell::new(vec![]),
        visible: RefCell::new(vec![]),
        text: RefCell::new(String::new()),
        loading: Cell::new(false),
        error: RefCell::new(None),
        updated: Cell::new(None),
        list: list.clone(),
        title,
        stack,
        status,
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let weak = Rc::downgrade(&me);
    search.connect_search_changed(move |e| {
        if let Some(me) = weak.upgrade() {
            *me.text.borrow_mut() = e.text().to_string();
            me.render();
        }
    });
    let weak = Rc::downgrade(&me);
    list.connect_row_activated(move |_, row| {
        let Some(me) = weak.upgrade() else { return };
        let url = me.visible.borrow().get(row.index().max(0) as usize).map(|r| r.url.clone());
        if let Some(url) = url {
            util::open_url(&url);
        }
    });
    let weak = Rc::downgrade(&me);
    app::on_refresh(move |_| match weak.upgrade() {
        Some(me) => {
            me.load();
            true
        }
        None => false,
    });
    me.load();
    keep_alive(&widget, me);
    widget
}

pub fn unavailable() -> gtk::Widget {
    let b = vbox(8);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.append(&label_with(&format!("Not available for {}", app::provider().title()), &["sy-muted"]));
    let (tv, _, _) = toolbar_page("", "", &b);
    tv.upcast()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_filter() {
        let r = ForgeRepo { full_name: "team/App".into(), description: Some("Mobile client".into()), url: "u".into(), is_private: false, updated_at: None, default_branch: None, stars: None };
        assert!(repo_matches(&r, ""));
        assert!(repo_matches(&r, "app"));
        assert!(repo_matches(&r, " mobile "));
        assert!(!repo_matches(&r, "web"));
    }
}
