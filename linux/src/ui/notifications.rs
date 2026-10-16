use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};

use super::menu::{context_menu, Menu};
use super::widgets::*;
use crate::api::models::NotificationThread;
use crate::{app, util};

struct View {
    items: RefCell<Vec<NotificationThread>>,
    show_all: Cell<bool>,
    loading: Cell<bool>,
    error: RefCell<Option<String>>,
    updated: Cell<Option<DateTime<Utc>>>,
    list: gtk::ListBox,
    stack: gtk::Stack,
    status: Rc<RefreshStatus>,
    mark_all: gtk::Button,
    empty_label: gtk::Label,
    empty_icon: gtk::Box,
    rendered: RefCell<String>,
}

fn icon_for(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "PullRequest" => ("sy-pr-symbolic", "violet"),
        "Issue" => ("sy-issue-symbolic", "green"),
        "Release" => ("sy-tag-symbolic", "teal"),
        "Discussion" => ("sy-comment-symbolic", "indigo"),
        "CheckSuite" => ("sy-x-symbolic", "red"),
        "Commit" => ("sy-commit-symbolic", "sky"),
        _ => ("sy-bell-symbolic", "amber"),
    }
}

fn reason_text(r: &str) -> String {
    match r {
        "review_requested" => "review requested",
        "mention" => "mention",
        "team_mention" => "team mention",
        "author" => "author",
        "comment" => "comment",
        "assign" => "assigned",
        "state_change" => "state change",
        "subscribed" | "manual" => "subscribed",
        "ci_activity" => "CI",
        "security_alert" => "security alert",
        other => other,
    }
    .to_string()
}

fn reason_color(r: &str) -> &'static str {
    match r {
        "review_requested" => "fg-sky",
        "mention" | "team_mention" => "fg-amber",
        "security_alert" => "fg-red",
        _ => "sy-muted",
    }
}

fn row(n: &NotificationThread) -> gtk::Box {
    let b = hbox(8);
    margins(&b, 4, 0, 4, 0);
    let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    dot.set_size_request(7, 7);
    dot.set_valign(gtk::Align::Start);
    dot.set_margin_top(7);
    if n.unread {
        dot.add_css_class("hex-5bb8ec");
        dot.add_css_class("dot");
        crate::theme::hex_class("5bb8ec");
    }
    b.append(&dot);
    let (ic, color) = icon_for(&n.subject.kind);
    let tile = icon_tile(ic, color, 22);
    tile.set_valign(gtk::Align::Start);
    b.append(&tile);
    let col = vbox(3);
    col.set_hexpand(true);
    let title = wrap_label(&n.subject.title, &[if n.unread { "sy-text" } else { "sy-muted" }]);
    if n.unread {
        title.add_css_class("sy-bold");
    }
    title.set_lines(2);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    col.append(&title);
    let meta = hbox(5);
    meta.append(&avatar(n.repository.owner.as_ref().and_then(|o| o.avatar_url.as_deref()), 13, None));
    meta.append(&ellipsized(&n.repository.full_name, &["sy-muted", "sy-small"]));
    meta.append(&label_with("·", &["sy-dim", "sy-small"]));
    meta.append(&label_with(&reason_text(&n.reason), &[reason_color(&n.reason), "sy-small"]));
    meta.append(&label_with("·", &["sy-dim", "sy-small"]));
    meta.append(&label_with(&util::rel(Some(n.updated_at)), &["sy-dim", "sy-small"]));
    col.append(&meta);
    b.append(&col);
    b
}

fn signature(items: &[NotificationThread]) -> String {
    items
        .iter()
        .map(|n| format!("{}|{}|{}|{}|{}|{}|{}|{}", n.id, n.unread, n.reason, n.updated_at, n.subject.kind, n.subject.title, n.repository.full_name, util::rel(Some(n.updated_at))))
        .collect::<Vec<_>>()
        .join("\n")
}

impl View {
    fn render(self: &Rc<Self>) {
        let _span = crate::diagnostics::ui_span("notifications.render");
        let items = self.items.borrow().clone();
        self.status.update(self.updated.get(), self.loading.get(), if items.is_empty() { None } else { self.error.borrow().clone() });
        self.mark_all.set_sensitive(items.iter().any(|n| n.unread));
        if items.is_empty() {
            self.list.remove_all();
            self.rendered.borrow_mut().clear();
            if self.loading.get() {
                self.stack.set_visible_child_name("loading");
            } else if let Some(e) = self.error.borrow().clone() {
                self.empty_label.set_text(&e);
                self.empty_label.add_css_class("fg-red");
                self.stack.set_visible_child_name("empty");
            } else {
                self.empty_label.remove_css_class("fg-red");
                self.empty_label.set_text(if self.show_all.get() { "No notifications" } else { "All caught up" });
                self.empty_icon.set_visible(true);
                self.stack.set_visible_child_name("empty");
            }
            return;
        }
        self.stack.set_visible_child_name("list");
        let sig = signature(&items);
        if *self.rendered.borrow() == sig {
            return;
        }
        *self.rendered.borrow_mut() = sig;
        rebuild_rows(&self.list, || self.build_rows(items));
    }

    fn build_rows(self: &Rc<Self>, items: Vec<NotificationThread>) {
        for n in items {
            let r = gtk::ListBoxRow::new();
            r.set_child(Some(&row(&n)));
            let weak = Rc::downgrade(self);
            let nn = n.clone();
            context_menu(&r, move || {
                let menu = Menu::ctx();
                if nn.unread {
                    let w = weak.clone();
                    let n2 = nn.clone();
                    menu.item("Mark as read", move || {
                        if let Some(me) = w.upgrade() {
                            me.mark_read(n2.clone());
                        }
                    });
                }
                let w = weak.clone();
                let n2 = nn.clone();
                menu.item("Done (remove from inbox)", move || {
                    if let Some(me) = w.upgrade() {
                        me.mark_done(n2.clone());
                    }
                });
                let url = nn.web_url();
                menu.item("Open in browser", move || util::open_url(&url));
                menu
            });
            self.list.append(&r);
        }
    }

    fn load(self: &Rc<Self>, force: bool) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        me.loading.set(true);
        me.render();
        crate::rt::spawn_local(async move {
            let all = me.show_all.get();
            let res = crate::rt::run(async move { api.notifications(all, force).await }).await;
            me.loading.set(false);
            if all != me.show_all.get() {
                return;
            }
            match res {
                Ok(list) => {
                    if !all {
                        app::get().set_unread(list.len());
                    }
                    *me.items.borrow_mut() = list;
                    me.updated.set(Some(Utc::now()));
                    *me.error.borrow_mut() = None;
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.render();
        });
    }

    fn mark_read(self: &Rc<Self>, n: NotificationThread) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let id = n.id.clone();
            let _ = crate::rt::run(async move { api.mark_notification_read(&id).await }).await;
            let mut changed = false;
            for i in me.items.borrow_mut().iter_mut() {
                if i.id == n.id && i.unread {
                    i.unread = false;
                    changed = true;
                }
            }
            if changed {
                let a = app::get();
                let u = a.state.borrow().unread_count.saturating_sub(1);
                a.set_unread(u);
            }
            me.render();
        });
    }

    fn mark_done(self: &Rc<Self>, n: NotificationThread) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let id = n.id.clone();
            let _ = crate::rt::run(async move { api.mark_notification_done(&id).await }).await;
            me.items.borrow_mut().retain(|i| i.id != n.id);
            if n.unread {
                let a = app::get();
                let u = a.state.borrow().unread_count.saturating_sub(1);
                a.set_unread(u);
            }
            me.render();
        });
    }

    fn mark_all(self: &Rc<Self>) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let _ = crate::rt::run(async move { api.mark_all_notifications_read().await }).await;
            for i in me.items.borrow_mut().iter_mut() {
                i.unread = false;
            }
            app::get().set_unread(0);
            me.render();
        });
    }
}

pub fn build() -> gtk::Widget {
    let list = gtk::ListBox::new();
    list.add_css_class("sy-list");
    let stack = gtk::Stack::new();
    stack.add_named(&scrolled(&list), Some("list"));
    stack.add_named(&loading(), Some("loading"));
    let empty = vbox(8);
    empty.set_margin_top(30);
    empty.set_halign(gtk::Align::Center);
    let empty_icon = icon_tile("sy-check-symbolic", "green", 40);
    empty.append(&empty_icon);
    let empty_label = label_with("All caught up", &["sy-muted"]);
    empty_label.set_xalign(0.5);
    empty_label.set_wrap(true);
    empty_label.set_natural_wrap_mode(gtk::NaturalWrapMode::None);
    empty.append(&empty_label);
    stack.add_named(&empty, Some("empty"));
    let mark_all = icon_button("sy-check-symbolic", "Mark all as read");
    let holder: Rc<RefCell<Option<std::rc::Weak<View>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(v) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            v.load(true);
        }
    });
    let view = Rc::new(View {
        items: RefCell::new(vec![]),
        show_all: Cell::new(false),
        loading: Cell::new(false),
        error: RefCell::new(None),
        updated: Cell::new(None),
        list: list.clone(),
        stack,
        status,
        mark_all: mark_all.clone(),
        empty_label,
        empty_icon,
        rendered: RefCell::new(String::new()),
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&view));

    let toggles = hbox(0);
    toggles.add_css_class("linked");
    let unread = gtk::ToggleButton::with_label("Unread");
    unread.set_active(true);
    unread.set_hexpand(true);
    let all = gtk::ToggleButton::with_label("All");
    all.set_group(Some(&unread));
    all.set_hexpand(true);
    toggles.append(&unread);
    toggles.append(&all);
    let v = Rc::downgrade(&view);
    all.connect_toggled(move |t| {
        if let Some(v) = v.upgrade() {
            v.show_all.set(t.is_active());
            v.items.borrow_mut().clear();
            v.load(true);
        }
    });
    let bar = hbox(0);
    margins(&bar, 8, 10, 8, 10);
    toggles.set_hexpand(true);
    bar.append(&toggles);

    let v = Rc::downgrade(&view);
    list.connect_row_activated(move |_, row| {
        let Some(v) = v.upgrade() else { return };
        let idx = row.index();
        let n = v.items.borrow().get(idx.max(0) as usize).cloned();
        if let Some(n) = n {
            match n.route() {
                Some(r) => app::select(Some(r)),
                None => util::open_url(&n.web_url()),
            }
            if n.unread {
                v.mark_read(n);
            }
        }
    });
    let v = Rc::downgrade(&view);
    mark_all.connect_clicked(move |_| {
        if let Some(v) = v.upgrade() {
            v.mark_all();
        }
    });

    let content = vbox(0);
    content.append(&bar);
    content.append(&line());
    content.append(&view.stack);
    content.append(&footer(&view.status));
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_end_title_buttons(false);
    hb.set_title_widget(Some(&adw::WindowTitle::new("Notifications", "")));
    hb.pack_end(&mark_all);
    tv.add_top_bar(&hb);
    tv.set_content(Some(&content));

    let v = Rc::downgrade(&view);
    app::on_refresh(move |_| match v.upgrade() {
        Some(v) => {
            v.load(true);
            true
        }
        None => false,
    });
    let v = Rc::downgrade(&view);
    app::on_live_visible(&tv, move |_| match v.upgrade() {
        Some(v) => {
            v.load(false);
            true
        }
        None => false,
    });
    view.load(true);
    keep_alive(&tv, view);
    tv.upcast()
}
