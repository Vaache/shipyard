use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::api::models::{CheckOutcome, LabelInfo, VisualState};
use crate::util;

pub fn label(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l
}

pub fn label_with(text: &str, classes: &[&str]) -> gtk::Label {
    let l = label(text);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

pub fn wrap_label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = label_with(text, classes);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l
}

pub fn ellipsized(text: &str, classes: &[&str]) -> gtk::Label {
    let l = label_with(text, classes);
    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
    l
}

pub fn selectable(l: gtk::Label) -> gtk::Label {
    l.set_selectable(true);
    l
}

pub fn hbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn vbox(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn spacer() -> gtk::Box {
    let b = hbox(0);
    b.set_hexpand(true);
    b
}

pub fn line() -> gtk::Separator {
    let s = gtk::Separator::new(gtk::Orientation::Horizontal);
    s.add_css_class("sy-line");
    s
}

pub fn icon(name: &str, size: i32) -> gtk::Image {
    crate::icons::image(name, size)
}

pub fn icon_label_content(icon_name: &str, text: &str) -> gtk::Widget {
    if crate::icons::glyph(icon_name).is_some() {
        let b = hbox(6);
        b.append(&icon(icon_name, 16));
        b.append(&gtk::Label::new(Some(text)));
        b.set_halign(gtk::Align::Center);
        b.upcast()
    } else {
        let content = adw::ButtonContent::new();
        content.set_icon_name(icon_name);
        content.set_label(text);
        content.upcast()
    }
}

pub fn fixed_box(css: &[&str], width: i32, height: i32) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    for c in css {
        b.add_css_class(c);
    }
    b.set_size_request(width, height);
    b.set_hexpand(false);
    b.set_vexpand(false);
    b.set_halign(gtk::Align::Center);
    b.set_valign(gtk::Align::Center);
    b
}

pub fn centered_in<W: IsA<gtk::Widget>>(b: &gtk::Box, child: &W, width: i32, height: i32) {
    let c = child.as_ref();
    c.set_halign(gtk::Align::Center);
    c.set_valign(gtk::Align::Center);
    c.set_hexpand(true);
    c.set_vexpand(true);
    b.set_hexpand(false);
    b.set_vexpand(false);
    b.set_size_request(width, height);
    b.append(c);
}

pub fn colored_icon(name: &str, size: i32, color: &str) -> gtk::Image {
    let i = icon(name, size);
    i.add_css_class(&format!("fg-{color}"));
    i
}

pub fn margins<W: IsA<gtk::Widget>>(w: &W, top: i32, end: i32, bottom: i32, start: i32) {
    w.set_margin_top(top);
    w.set_margin_end(end);
    w.set_margin_bottom(bottom);
    w.set_margin_start(start);
}

pub fn pad<W: IsA<gtk::Widget>>(w: &W, all: i32) {
    margins(w, all, all, all, all);
}

pub fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

pub fn button(label: &str) -> gtk::Button {
    gtk::Button::with_label(label)
}

pub fn suggested(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class("suggested-action");
    b
}

pub fn destructive(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class("destructive-action");
    b
}

pub fn flat(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class("flat");
    b
}

pub fn link_button(label: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class("flat");
    b.add_css_class("fg-accent");
    b
}

pub fn icon_button(icon_name: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::new();
    crate::icons::set_button_icon(&b, icon_name);
    b.set_tooltip_text(Some(tooltip));
    b.add_css_class("flat");
    b
}

pub fn tinted(label: &str, color: &str) -> gtk::Button {
    let b = gtk::Button::with_label(label);
    b.add_css_class(&format!("tint-{color}"));
    b
}

pub fn icon_label_button(icon_name: &str, text: &str) -> gtk::Button {
    let b = gtk::Button::new();
    b.set_child(Some(&icon_label_content(icon_name, text)));
    b
}

pub fn chip(text: &str, color: &str, icon_name: Option<&str>, mono: bool) -> gtk::Box {
    let b = hbox(4);
    b.add_css_class("chip");
    b.add_css_class(color);
    if mono {
        b.add_css_class("mono");
    }
    if let Some(i) = icon_name {
        b.append(&icon(i, 11));
    }
    let l = gtk::Label::new(Some(text));
    l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
    l.set_max_width_chars(40);
    b.append(&l);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Start);
    b
}

pub fn label_chip(l: &LabelInfo) -> gtk::Label {
    let w = gtk::Label::new(Some(&l.name));
    w.add_css_class("label-chip");
    w.add_css_class(&crate::theme::hex_class(&l.color));
    w.set_valign(gtk::Align::Center);
    w
}

pub fn icon_tile(icon_name: &str, color: &str, size: i32) -> gtk::Box {
    let tile_class = format!("tile-{color}");
    let b = fixed_box(&["icon-tile", tile_class.as_str()], size, size);
    centered_in(&b, &icon(icon_name, ((size as f64) * 0.55).round() as i32), size, size);
    b
}

pub fn state_badge(state: VisualState, compact: bool) -> gtk::Widget {
    if compact {
        let b = fixed_box(&["state-dot", state.css()], 22, 22);
        b.set_valign(gtk::Align::Start);
        centered_in(&b, &icon(state.icon(), 12), 22, 22);
        b.set_tooltip_text(Some(state.title()));
        b.upcast()
    } else {
        let b = hbox(5);
        b.add_css_class("state-pill");
        b.add_css_class(state.css());
        b.append(&icon(state.icon(), 13));
        b.append(&gtk::Label::new(Some(state.title())));
        b.set_valign(gtk::Align::Center);
        b.upcast()
    }
}

pub fn diff_stat(additions: i64, deletions: i64) -> gtk::Box {
    let b = hbox(4);
    let a = label_with(&format!("+{}", util::thousands(additions)), &["fg-green", "sy-small", "sy-mono"]);
    let d = label_with(&format!("−{}", util::thousands(deletions)), &["fg-red", "sy-small", "sy-mono"]);
    b.append(&a);
    b.append(&d);
    b.set_valign(gtk::Align::Center);
    b
}

pub fn ci_icon(state: Option<&str>) -> Option<gtk::Image> {
    let (name, color) = match state? {
        "SUCCESS" => ("sy-check-symbolic", "green"),
        "FAILURE" | "ERROR" => ("sy-x-symbolic", "red"),
        "PENDING" | "EXPECTED" => ("sy-clock-symbolic", "amber"),
        _ => return None,
    };
    let i = colored_icon(name, 12, color);
    i.set_tooltip_text(Some(match color {
        "green" => "Checks passed",
        "red" => "Checks failed",
        _ => "Checks running",
    }));
    Some(i)
}

pub fn outcome_icon(o: CheckOutcome) -> gtk::Image {
    match o {
        CheckOutcome::Success => colored_icon("sy-check-symbolic", 14, "green"),
        CheckOutcome::Failure => colored_icon("sy-x-symbolic", 14, "red"),
        CheckOutcome::Pending => colored_icon("sy-clock-symbolic", 14, "amber"),
        CheckOutcome::Neutral => colored_icon("sy-dot-symbolic", 14, "muted"),
        CheckOutcome::Skipped => colored_icon("sy-issue-skip-symbolic", 14, "muted"),
    }
}

pub fn rebuild_rows(list: &gtk::ListBox, build: impl FnOnce()) {
    let scroller = list.ancestor(gtk::ScrolledWindow::static_type()).and_downcast::<gtk::ScrolledWindow>();
    let pos = scroller.as_ref().map(|s| s.vadjustment().value());
    let focused = list.root().and_then(|r| r.focus()).is_some_and(|f| f.is_ancestor(list));
    let focus = list.focus_child().and_downcast::<gtk::ListBoxRow>().map(|r| r.index()).filter(|_| focused);
    list.remove_all();
    build();
    if let (Some(s), Some(v)) = (scroller, pos) {
        let adj = s.vadjustment();
        adj.set_value(v);
        glib::idle_add_local_once(move || adj.set_value(v));
    }
    if let Some(row) = focus.and_then(|i| list.row_at_index(i)) {
        row.grab_focus();
    }
}

pub fn scrolled<W: IsA<gtk::Widget>>(child: &W) -> gtk::ScrolledWindow {
    let s = gtk::ScrolledWindow::new();
    s.set_hscrollbar_policy(gtk::PolicyType::Never);
    s.set_child(Some(child));
    s.set_vexpand(true);
    s.set_hexpand(true);
    s
}

pub fn scrolled_both<W: IsA<gtk::Widget>>(child: &W) -> gtk::ScrolledWindow {
    let s = gtk::ScrolledWindow::new();
    s.set_child(Some(child));
    s.set_vexpand(true);
    s.set_hexpand(true);
    s
}

pub fn loading() -> gtk::Widget {
    let s = gtk::Spinner::new();
    s.set_spinning(true);
    s.set_size_request(24, 24);
    s.set_halign(gtk::Align::Center);
    s.set_valign(gtk::Align::Center);
    s.set_vexpand(true);
    s.set_hexpand(true);
    s.upcast()
}

pub fn after_next_paint<W: IsA<gtk::Widget>>(w: &W, f: impl FnOnce() + 'static) {
    let Some(clock) = w.frame_clock() else { return };
    let pending = RefCell::new(Some(f));
    let handler: Rc<RefCell<Option<glib::SignalHandlerId>>> = Rc::new(RefCell::new(None));
    let h = handler.clone();
    let id = clock.connect_after_paint(move |c| {
        if let Some(f) = pending.borrow_mut().take() {
            f();
        }
        if let Some(id) = h.borrow_mut().take() {
            c.disconnect(id);
        }
    });
    *handler.borrow_mut() = Some(id);
    clock.request_phase(gdk::FrameClockPhase::PAINT);
}

pub fn spinner(size: i32) -> gtk::Spinner {
    let s = gtk::Spinner::new();
    s.set_spinning(true);
    s.set_size_request(size, size);
    s
}

pub fn error_state(message: &str, retry: Option<Box<dyn Fn()>>) -> gtk::Widget {
    let b = vbox(10);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.set_vexpand(true);
    b.set_hexpand(true);
    pad(&b, 30);
    b.append(&icon_tile("sy-attention-symbolic", "amber", 44));
    let l = wrap_label(message, &["sy-muted"]);
    l.set_justify(gtk::Justification::Center);
    l.set_xalign(0.5);
    l.set_selectable(true);
    l.set_max_width_chars(60);
    b.append(&l);
    if let Some(retry) = retry {
        let btn = button("Retry");
        btn.set_halign(gtk::Align::Center);
        btn.connect_clicked(move |_| retry());
        b.append(&btn);
    }
    b.upcast()
}

pub fn empty_state(icon_name: &str, color: &str, title: &str, subtitle: Option<&str>) -> gtk::Widget {
    let b = vbox(10);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.set_vexpand(true);
    b.set_hexpand(true);
    pad(&b, 24);
    b.append(&icon_tile(icon_name, color, 52));
    let t = label_with(title, &["sy-muted"]);
    t.set_xalign(0.5);
    t.set_justify(gtk::Justification::Center);
    t.set_wrap(true);
    t.set_natural_wrap_mode(gtk::NaturalWrapMode::None);
    t.set_max_width_chars(48);
    b.append(&t);
    if let Some(s) = subtitle {
        let l = label_with(s, &["sy-dim", "sy-small"]);
        l.set_xalign(0.5);
        b.append(&l);
    }
    b.upcast()
}

thread_local! {
    static TEXTURES: RefCell<HashMap<String, Option<gdk::Texture>>> = RefCell::new(HashMap::new());
    static WAITING: RefCell<HashMap<String, Vec<glib::WeakRef<adw::Avatar>>>> = RefCell::new(HashMap::new());
}

pub fn avatar(url: Option<&str>, size: i32, name: Option<&str>) -> adw::Avatar {
    let a = adw::Avatar::new(size, name, name.is_some());
    a.set_valign(gtk::Align::Center);
    if let Some(url) = url.filter(|u| !u.is_empty()) {
        load_avatar(&a, url, size);
    }
    a
}

pub fn load_avatar(a: &adw::Avatar, url: &str, size: i32) {
    let key = format!("{url}|{}", size.max(24));
    let cached = TEXTURES.with(|t| t.borrow().get(&key).cloned());
    match cached {
        Some(Some(tex)) => {
            a.set_custom_image(Some(&tex));
            return;
        }
        Some(None) => return,
        None => {}
    }
    let first = WAITING.with(|w| {
        let mut w = w.borrow_mut();
        let entry = w.entry(key.clone()).or_default();
        entry.push(a.downgrade());
        entry.len() == 1
    });
    if !first {
        return;
    }
    let url = url.to_string();
    let px = size.max(24);
    crate::rt::spawn_local(async move {
        let bytes = crate::rt::run(async move { crate::api::GitHubApi::avatar(&url, px).await }).await;
        let tex = bytes.and_then(|b| gdk::Texture::from_bytes(&glib::Bytes::from_owned(b)).ok());
        TEXTURES.with(|t| t.borrow_mut().insert(key.clone(), tex.clone()));
        let waiters = WAITING.with(|w| w.borrow_mut().remove(&key)).unwrap_or_default();
        if let Some(tex) = tex {
            for w in waiters {
                if let Some(a) = w.upgrade() {
                    a.set_custom_image(Some(&tex));
                }
            }
        }
    });
}

pub struct RefreshStatus {
    pub widget: gtk::Box,
    icon: gtk::Stack,
    text: gtk::Label,
    retry: gtk::Button,
    state: Rc<RefCell<(Option<chrono::DateTime<chrono::Utc>>, bool, Option<String>)>>,
    compact: bool,
}

impl RefreshStatus {
    pub fn new(compact: bool, on_retry: impl Fn() + 'static) -> Rc<Self> {
        let widget = hbox(5);
        widget.add_css_class("sy-muted");
        widget.add_css_class("sy-small");
        widget.set_valign(gtk::Align::Center);
        let icon_stack = gtk::Stack::new();
        icon_stack.add_named(&spinner(12), Some("spin"));
        icon_stack.add_named(&colored_icon("sy-attention-symbolic", 12, "amber"), Some("warn"));
        icon_stack.add_named(&icon("view-refresh-symbolic", 12), Some("idle"));
        icon_stack.set_visible_child_name("idle");
        let text = label("");
        let retry = link_button("Retry");
        retry.set_visible(false);
        retry.connect_clicked(move |_| on_retry());
        widget.append(&icon_stack);
        widget.append(&text);
        widget.append(&retry);
        let me = Rc::new(RefreshStatus { widget, icon: icon_stack, text, retry, state: Rc::new(RefCell::new((None, false, None))), compact });
        let weak = Rc::downgrade(&me);
        glib::timeout_add_seconds_local(5, move || match weak.upgrade() {
            Some(me) => {
                me.render();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        me.render();
        me
    }

    pub fn update(&self, updated: Option<chrono::DateTime<chrono::Utc>>, refreshing: bool, error: Option<String>) {
        *self.state.borrow_mut() = (updated, refreshing, error);
        self.render();
    }

    fn render(&self) {
        let (updated, refreshing, error) = self.state.borrow().clone();
        let paused = crate::api::rate_limited_until().filter(|_| !refreshing);
        self.icon.set_visible_child_name(if refreshing { "spin" } else if error.is_some() || paused.is_some() { "warn" } else { "idle" });
        let resume = paused.map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string());
        let text = if refreshing {
            match updated.filter(|u| !self.compact && (chrono::Utc::now() - *u).num_seconds() >= 60) {
                Some(u) => format!("Updated {} · updating…", util::refresh_ago(u)),
                None => "Updating…".to_string(),
            }
        } else if let Some(at) = &resume {
            if self.compact { "Paused".into() } else { format!("Paused until {at}") }
        } else if error.is_some() {
            if self.compact { "Offline".into() } else { "Couldn't refresh".into() }
        } else if let Some(u) = updated {
            let ago = util::refresh_ago(u);
            if self.compact { ago } else { format!("Updated {ago}") }
        } else if self.compact {
            String::new()
        } else {
            "Not updated yet".into()
        };
        self.text.set_text(&text);
        self.retry.set_visible(error.is_some() && !refreshing && resume.is_none());
        self.widget.set_tooltip_text(Some(&match (&resume, &error) {
            (Some(at), _) => format!("GitHub rate limit reached. Live updates resume at {at}."),
            (None, Some(e)) => format!("Last refresh failed: {e}"),
            (None, None) => "Refresh (Ctrl+R)".to_string(),
        }));
    }
}

pub fn footer(status: &RefreshStatus) -> gtk::Box {
    let b = hbox(0);
    b.add_css_class("refresh-footer");
    status.widget.set_halign(gtk::Align::Center);
    status.widget.set_hexpand(true);
    b.append(&status.widget);
    b
}

pub fn header_title(title: &str, subtitle: &str) -> adw::WindowTitle {
    adw::WindowTitle::new(title, subtitle)
}

pub fn toolbar_page(title: &str, subtitle: &str, content: &impl IsA<gtk::Widget>) -> (adw::ToolbarView, adw::HeaderBar, adw::WindowTitle) {
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    let wt = header_title(title, subtitle);
    hb.set_title_widget(Some(&wt));
    tv.add_top_bar(&hb);
    tv.set_content(Some(content));
    (tv, hb, wt)
}

pub fn frame<W: IsA<gtk::Widget>>(child: &W, class: &str) -> gtk::Box {
    let b = vbox(0);
    b.add_css_class(class);
    b.append(child);
    b
}

pub fn tooltip<W: IsA<gtk::Widget>>(w: W, text: &str) -> W {
    w.set_tooltip_text(Some(text));
    w
}

pub fn on_click_widget<W: IsA<gtk::Widget>>(w: &W, f: impl Fn() + 'static) {
    let g = gtk::GestureClick::new();
    g.set_button(1);
    g.connect_released(move |gesture, n, _, _| {
        if n == 1 {
            gesture.set_state(gtk::EventSequenceState::Claimed);
            f();
        }
    });
    w.add_controller(g);
    w.set_cursor_from_name(Some("pointer"));
}

pub fn branch_chip(name: &str, color: &str) -> gtk::Box {
    let c = chip(name, color, None, true);
    c.set_tooltip_text(Some("Click to copy"));
    let n = name.to_string();
    on_click_widget(&c, move || {
        util::copy(&n);
        crate::app::toast("Copied branch name");
    });
    c
}

pub fn branch_tag(name: &str) -> gtk::Box {
    chip(name, crate::theme::branch_color(name), Some("sy-branch-symbolic"), false)
}

pub fn pr_tag(number: i64) -> gtk::Box {
    chip(&format!("#{number}"), "violet", Some("sy-pr-symbolic"), false)
}

pub fn keep_alive<W: IsA<gtk::Widget>, T: 'static>(widget: &W, value: T) {
    let holder = RefCell::new(Some(value));
    widget.connect_destroy(move |_| {
        holder.borrow_mut().take();
    });
}
