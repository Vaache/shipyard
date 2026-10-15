use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib};
use serde_json::{Map, Value};

use crate::claude::accounts;
use crate::claude::chat::{self, ChatImage, ChatItem, ChatSession, Event, PermissionRequest, ToolCall, ToolStatus};
use crate::claude::workspace::Tab;
use crate::claude::ClaudeMode;
use crate::diff::{DiffLine, LineKind};
use crate::ui::markdown;
use crate::ui::menu::Menu;
use crate::ui::pr::files::line_widget;
use crate::ui::widgets::*;
use crate::{app, util};

pub struct ChatView {
    pub widget: gtk::Box,
    tab: Rc<Tab>,
    chat: Rc<ChatSession>,
    items_box: gtk::Box,
    rendered: RefCell<Vec<(String, gtk::Widget)>>,
    welcome: gtk::Box,
    working: gtk::Box,
    working_label: gtk::Label,
    working_time: gtk::Label,
    error_box: gtk::Box,
    sleeping: gtk::Box,
    permission_bar: gtk::Box,
    scroller: gtk::ScrolledWindow,
    stick: Cell<bool>,
    dirty_texts: RefCell<Vec<String>>,
    flush_scheduled: Cell<bool>,
    open_tools: RefCell<std::collections::HashMap<String, bool>>,
    composer: Rc<ChatComposer>,
}

fn tool_icon(name: &str) -> &'static str {
    match name {
        "Read" => "sy-file-symbolic",
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => "document-edit-symbolic",
        "Bash" => "sy-terminal-symbolic",
        "Grep" | "WebSearch" => "system-search-symbolic",
        "Glob" => "sy-folder-symbolic",
        "WebFetch" => "sy-globe-symbolic",
        "Task" | "Agent" => "sy-person-symbolic",
        "TodoWrite" => "sy-checklist-symbolic",
        "AskUserQuestion" => "sy-comment-symbolic",
        "ExitPlanMode" => "sy-checklist-symbolic",
        "Skill" => "sy-sparkles-symbolic",
        n if n.starts_with("mcp__") => "sy-project-symbolic",
        _ => "emblem-system-symbolic",
    }
}

fn tool_title(c: &ToolCall) -> String {
    if c.name.starts_with("mcp__") {
        return c.name.split('_').filter(|s| !s.is_empty()).last().unwrap_or(&c.name).to_string();
    }
    c.name.clone()
}

fn relative(path: &str, folder: &str) -> String {
    match path.strip_prefix(&format!("{folder}/")) {
        Some(r) => r.to_string(),
        None => util::tilde(path),
    }
}

fn tool_summary(c: &ToolCall, folder: &str) -> String {
    let i = c.input.borrow();
    let g = |k: &str| i.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    match c.name.as_str() {
        "Read" | "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => relative(&g("file_path").or_else(|| g("notebook_path")).unwrap_or_default(), folder),
        "Bash" => g("description").or_else(|| g("command")).unwrap_or_default(),
        "Grep" => format!("\"{}\"{}", g("pattern").unwrap_or_default(), g("path").map(|p| format!(" · {}", relative(&p, folder))).unwrap_or_default()),
        "Glob" => g("pattern").unwrap_or_default(),
        "WebFetch" => g("url").unwrap_or_default(),
        "WebSearch" => g("query").unwrap_or_default(),
        "Task" | "Agent" => g("description").unwrap_or_default(),
        "Skill" => g("skill").unwrap_or_default(),
        _ => match i.values().find_map(|v| v.as_str()) {
            Some(s) => s.chars().take(120).collect(),
            None => {
                if c.partial_input.borrow().is_empty() {
                    String::new()
                } else {
                    "…".into()
                }
            }
        },
    }
}

fn diff_lines(c: &ToolCall) -> Vec<DiffLine> {
    let mut out = vec![];
    let mut idx = 0;
    if let Some(patch) = c.structured.borrow().as_ref().and_then(|s| s.get("structuredPatch")).and_then(|p| p.as_array()).filter(|p| !p.is_empty()) {
        for hunk in patch {
            let mut old = hunk.get("oldStart").and_then(|v| v.as_i64()).unwrap_or(1);
            let mut new = hunk.get("newStart").and_then(|v| v.as_i64()).unwrap_or(1);
            for l in hunk.get("lines").and_then(|v| v.as_array()).cloned().unwrap_or_default() {
                let l = l.as_str().unwrap_or("").to_string();
                let body: String = l.chars().skip(1).collect();
                match l.chars().next() {
                    Some('+') => {
                        out.push(DiffLine { id: idx, kind: LineKind::Add, text: body, old_line: None, new_line: Some(new) });
                        new += 1;
                    }
                    Some('-') => {
                        out.push(DiffLine { id: idx, kind: LineKind::Del, text: body, old_line: Some(old), new_line: None });
                        old += 1;
                    }
                    _ => {
                        out.push(DiffLine { id: idx, kind: LineKind::Context, text: body, old_line: Some(old), new_line: Some(new) });
                        old += 1;
                        new += 1;
                    }
                }
                idx += 1;
            }
        }
        return out;
    }
    if c.name == "Write" {
        for (n, l) in c.string("content").unwrap_or_default().split('\n').enumerate() {
            out.push(DiffLine { id: idx, kind: LineKind::Add, text: l.to_string(), old_line: None, new_line: Some(n as i64 + 1) });
            idx += 1;
        }
        return out;
    }
    let input = c.input.borrow();
    let edits: Vec<Map<String, Value>> = if c.name == "MultiEdit" {
        input.get("edits").and_then(|e| e.as_array()).map(|a| a.iter().filter_map(|x| x.as_object().cloned()).collect()).unwrap_or_default()
    } else {
        vec![input.clone()]
    };
    for e in edits {
        for l in e.get("old_string").and_then(|v| v.as_str()).unwrap_or("").split('\n') {
            out.push(DiffLine { id: idx, kind: LineKind::Del, text: l.to_string(), old_line: None, new_line: None });
            idx += 1;
        }
        for l in e.get("new_string").and_then(|v| v.as_str()).unwrap_or("").split('\n') {
            out.push(DiffLine { id: idx, kind: LineKind::Add, text: l.to_string(), old_line: None, new_line: None });
            idx += 1;
        }
    }
    out
}

fn tool_diff_stat(c: &ToolCall) -> Option<(i64, i64)> {
    if let Some(patch) = c.structured.borrow().as_ref().and_then(|s| s.get("structuredPatch")).and_then(|p| p.as_array()).filter(|p| !p.is_empty()) {
        let lines: Vec<String> = patch.iter().flat_map(|h| h.get("lines").and_then(|l| l.as_array()).cloned().unwrap_or_default()).filter_map(|l| l.as_str().map(|s| s.to_string())).collect();
        return Some((lines.iter().filter(|l| l.starts_with('+')).count() as i64, lines.iter().filter(|l| l.starts_with('-')).count() as i64));
    }
    if c.name == "Write" {
        if let Some(content) = c.string("content") {
            return Some((content.split('\n').count() as i64, 0));
        }
    }
    None
}

fn output_view(text: Option<&str>, title: Option<&str>) -> Option<gtk::Widget> {
    let text = text?;
    if text.trim().is_empty() {
        return None;
    }
    let b = vbox(4);
    if let Some(t) = title {
        b.append(&label_with(t, &["sy-muted", "sy-small", "sy-bold"]));
    }
    let shown: String = text.chars().take(20_000).collect();
    let l = gtk::Label::new(Some(&shown));
    l.set_xalign(0.0);
    l.set_yalign(0.0);
    l.set_selectable(true);
    l.add_css_class("sy-mono");
    l.add_css_class("sy-small");
    let s = gtk::ScrolledWindow::new();
    s.set_child(Some(&l));
    s.set_max_content_height(220);
    s.set_propagate_natural_height(true);
    let f = frame(&s, "chat-code");
    b.append(&f);
    Some(b.upcast())
}

fn todo_list(todos: &[Value]) -> gtk::Box {
    let b = vbox(4);
    for t in todos {
        let status = t.get("status").and_then(|s| s.as_str()).unwrap_or("pending");
        let row = hbox(8);
        let (icon_name, color) = match status {
            "completed" => ("sy-check-symbolic", "green"),
            "in_progress" => ("sy-clock-symbolic", "sky"),
            _ => ("sy-dot-symbolic", "dim"),
        };
        row.append(&colored_icon(icon_name, 12, color));
        let text = t.get("content").or_else(|| t.get("subject")).and_then(|c| c.as_str()).unwrap_or("");
        let l = if status == "completed" { markdown_strike(text) } else { label_with(text, &["sy-text"]) };
        l.set_wrap(true);
        row.append(&l);
        b.append(&row);
    }
    b
}

fn markdown_strike(text: &str) -> gtk::Label {
    let l = gtk::Label::new(None);
    l.set_markup(&format!("<s>{}</s>", util::markup_esc(text)));
    l.add_css_class("sy-muted");
    l.set_xalign(0.0);
    l
}

impl ChatView {
    pub fn new(tab: Rc<Tab>) -> Rc<Self> {
        let chat = tab.chat.clone().expect("chat tab");
        let items_box = vbox(14);
        margins(&items_box, 16, 20, 16, 20);
        let clamp = adw::Clamp::new();
        clamp.set_maximum_size(920);
        clamp.set_tightening_threshold(700);
        let column = vbox(14);
        column.append(&items_box);
        clamp.set_child(Some(&column));
        let scroller = gtk::ScrolledWindow::new();
        scroller.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroller.set_child(Some(&clamp));
        scroller.set_vexpand(true);
        scroller.add_css_class("chat-scroller");

        let welcome = vbox(10);
        welcome.add_css_class("chat-welcome");
        let wt = hbox(8);
        wt.append(&icon_tile("sy-sparkles-symbolic", "coral", 24));
        wt.append(&label_with("Claude Code", &["title-4", "sy-text"]));
        welcome.append(&wt);
        welcome.append(&wrap_label("Describe a task — Claude will read the code, propose changes and ask before acting.", &["sy-muted"]));
        let suggestions = hbox(8);
        for s in ["Explain the project structure", "Find bugs in the latest changes", "Write tests for the changed code"] {
            let b = button(s);
            let c = chat.clone();
            let t = tab.clone();
            b.connect_clicked(move |_| {
                if !c.running.get() {
                    app::get().claude.restart(&t);
                }
                c.send(s, vec![]);
            });
            suggestions.append(&b);
        }
        welcome.append(&suggestions);
        welcome.append(&label_with("/ commands · @ files · Shift+Enter new line · paste a screenshot Ctrl+V", &["sy-dim", "sy-small"]));
        column.prepend(&welcome);

        let working = hbox(8);
        working.append(&spinner(14));
        let working_label = label_with("Working…", &["sy-muted"]);
        working.append(&working_label);
        let working_time = label_with("", &["sy-dim", "sy-small", "sy-mono"]);
        working.append(&working_time);
        working.append(&spacer());
        let stop = button("Stop");
        let c = chat.clone();
        stop.connect_clicked(move |_| c.interrupt());
        working.append(&stop);
        working.set_visible(false);
        let error_box = vbox(8);
        error_box.add_css_class("chat-error");
        error_box.set_visible(false);
        column.append(&working);
        column.append(&error_box);
        margins(&working, 0, 20, 0, 20);
        margins(&error_box, 0, 20, 16, 20);

        let sleeping = hbox(8);
        sleeping.add_css_class("banner-box");
        sleeping.add_css_class("indigo");
        margins(&sleeping, 0, 12, 0, 12);
        sleeping.append(&colored_icon("sy-moon-symbolic", 14, "indigo"));
        let sl = wrap_label("Session is sleeping — process stopped, memory freed. A new message resumes it.", &["sy-muted"]);
        sl.set_hexpand(true);
        sleeping.append(&sl);
        let wake = button("Wake up");
        let c = chat.clone();
        wake.connect_clicked(move |_| c.wake());
        sleeping.append(&wake);
        sleeping.set_visible(false);

        let permission_bar = hbox(6);
        permission_bar.add_css_class("banner-box");
        permission_bar.add_css_class("amber");
        margins(&permission_bar, 6, 12, 0, 12);
        permission_bar.set_visible(false);

        let composer = ChatComposer::new(tab.clone(), chat.clone());
        let widget = vbox(0);
        widget.add_css_class("sy-panel-pane");
        widget.append(&scroller);
        widget.append(&sleeping);
        widget.append(&permission_bar);
        widget.append(&composer.widget);

        let me = Rc::new(ChatView {
            widget,
            tab,
            chat: chat.clone(),
            items_box,
            rendered: RefCell::new(vec![]),
            welcome,
            working,
            working_label,
            working_time,
            error_box,
            sleeping,
            permission_bar,
            scroller,
            stick: Cell::new(true),
            dirty_texts: RefCell::new(vec![]),
            flush_scheduled: Cell::new(false),
            open_tools: RefCell::new(std::collections::HashMap::new()),
            composer,
        });
        let w = Rc::downgrade(&me);
        me.scroller.vadjustment().connect_value_changed(move |adj| {
            if let Some(me) = w.upgrade() {
                me.stick.set(adj.value() + adj.page_size() >= adj.upper() - 40.0);
            }
        });
        let w = Rc::downgrade(&me);
        chat.changed.connect(move |e| match w.upgrade() {
            Some(me) => {
                me.on_event(e);
                true
            }
            None => false,
        });
        let w = Rc::downgrade(&me);
        glib::timeout_add_seconds_local(1, move || match w.upgrade() {
            Some(me) => {
                if let Some(start) = me.chat.turn_started.get() {
                    if me.chat.busy.get() {
                        me.working_time.set_text(&format!("{}s", start.elapsed().as_secs()));
                    }
                }
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        me.reset();
        me.render_state();
        me
    }

    fn on_event(self: &Rc<Self>, e: &Event) {
        match e {
            Event::Reset => self.reset(),
            Event::Appended => self.append_new(),
            Event::Updated(id) => {
                self.dirty_texts.borrow_mut().push(id.clone());
                self.schedule_flush();
            }
            Event::State => self.render_state(),
        }
    }

    fn schedule_flush(self: &Rc<Self>) {
        if self.flush_scheduled.replace(true) {
            return;
        }
        let w = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(80), move || {
            if let Some(me) = w.upgrade() {
                me.flush_scheduled.set(false);
                let ids: Vec<String> = std::mem::take(&mut *me.dirty_texts.borrow_mut());
                let mut seen = std::collections::HashSet::new();
                for id in ids {
                    if seen.insert(id.clone()) {
                        me.update_item(&id);
                    }
                }
                me.scroll_down();
            }
        });
    }

    fn reset(self: &Rc<Self>) {
        clear(&self.items_box);
        self.rendered.borrow_mut().clear();
        self.append_new();
        self.jump_to_bottom();
    }

    fn append_new(self: &Rc<Self>) {
        let items: Vec<ChatItem> = self.chat.items.borrow().clone();
        let start = self.rendered.borrow().len();
        for item in items.iter().skip(start) {
            let w = self.item_widget(item);
            self.items_box.append(&w);
            self.rendered.borrow_mut().push((item.id(), w));
        }
        self.welcome.set_visible(items.is_empty());
        self.scroll_down();
    }

    fn update_item(self: &Rc<Self>, key: &str) {
        let candidates = [key.to_string(), format!("t{key}"), format!("k{key}")];
        let items: Vec<ChatItem> = self.chat.items.borrow().clone();
        let pos = self.rendered.borrow().iter().position(|(id, _)| candidates.contains(id));
        let Some(pos) = pos else { return };
        let Some(item) = items.get(pos) else { return };
        let new = self.item_widget(item);
        let old = self.rendered.borrow()[pos].1.clone();
        self.items_box.insert_child_after(&new, Some(&old));
        self.items_box.remove(&old);
        self.rendered.borrow_mut()[pos].1 = new;
    }

    fn scroll_down(&self) {
        if !self.stick.get() {
            return;
        }
        let adj = self.scroller.vadjustment();
        glib::idle_add_local_once(move || adj.set_value(adj.upper()));
    }

    fn jump_to_bottom(&self) {
        let adj = self.scroller.vadjustment();
        for delay in [50u64, 200, 500] {
            let a = adj.clone();
            glib::timeout_add_local_once(Duration::from_millis(delay), move || a.set_value(a.upper()));
        }
    }

    fn render_state(self: &Rc<Self>) {
        let c = &self.chat;
        self.working.set_visible(c.busy.get());
        self.working_label.set_text(&c.status.borrow().clone().unwrap_or_else(|| if c.pending_count() > 0 { "Waiting for you".into() } else { "Working…".into() }));
        if !c.busy.get() {
            self.working_time.set_text("");
        }
        clear(&self.error_box);
        if let Some(e) = c.last_error.borrow().clone() {
            self.error_box.set_visible(true);
            let row = hbox(8);
            row.append(&colored_icon("sy-attention-symbolic", 14, "amber"));
            let l = selectable(wrap_label(&e, &["fg-amber"]));
            row.append(&l);
            self.error_box.append(&row);
            let actions = hbox(6);
            let restart = button("Restart session");
            let t = self.tab.clone();
            restart.connect_clicked(move |_| app::get().claude.restart(&t));
            actions.append(&restart);
            let lower = e.to_lowercase();
            if ["login", "log in", "logged in", "auth", "api key", "credential", "401", "oauth", "token"].iter().any(|k| lower.contains(k)) {
                let check = tinted("Check Claude sign-in", "coral");
                check.connect_clicked(|_| crate::rt::spawn_local(async { accounts::store().refresh().await }));
                actions.append(&check);
            }
            self.error_box.append(&actions);
        } else {
            self.error_box.set_visible(false);
        }
        self.sleeping.set_visible(c.sleeping.get() && !c.running.get());
        clear(&self.permission_bar);
        let pending = c.pending.borrow().iter().find(|p| !p.resolved.get()).cloned();
        match pending {
            Some(p) => {
                self.permission_bar.set_visible(true);
                self.permission_bar.append(&colored_icon("sy-attention-symbolic", 14, "amber"));
                if p.tool_name == "AskUserQuestion" || p.tool_name == "ExitPlanMode" {
                    let l = label_with(if p.tool_name == "AskUserQuestion" { "Claude asked a question — answer above" } else { "Claude is waiting for plan approval above" }, &["sy-text"]);
                    l.set_hexpand(true);
                    self.permission_bar.append(&l);
                } else {
                    let l = ellipsized(&format!("Claude needs permission: {}", p.tool_name), &["sy-text"]);
                    l.set_hexpand(true);
                    self.permission_bar.append(&l);
                    let allow = suggested("Allow");
                    let (c1, p1) = (c.clone(), p.clone());
                    allow.connect_clicked(move |_| c1.respond(&p1, true, None, None, None));
                    let deny = button("Deny");
                    let (c2, p2) = (c.clone(), p.clone());
                    deny.connect_clicked(move |_| c2.respond(&p2, false, None, None, None));
                    self.permission_bar.append(&allow);
                    self.permission_bar.append(&deny);
                }
            }
            None => self.permission_bar.set_visible(false),
        }
        self.composer.render_state();
        self.scroll_down();
    }

    fn item_widget(self: &Rc<Self>, item: &ChatItem) -> gtk::Widget {
        match item {
            ChatItem::User { text, images, .. } => {
                let b = vbox(8);
                b.add_css_class("chat-user");
                if !images.is_empty() {
                    let row = hbox(6);
                    for img in images {
                        if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from(&img.data[..])) {
                            let p = gtk::Picture::for_paintable(&tex);
                            p.set_size_request(64, 64);
                            p.set_content_fit(gtk::ContentFit::Cover);
                            row.append(&p);
                        }
                    }
                    b.append(&row);
                }
                if !text.is_empty() {
                    let lines = text.split('\n').count();
                    let l = selectable(wrap_label(text, &["sy-text"]));
                    if lines > 12 {
                        l.set_lines(12);
                        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                        b.append(&l);
                        let more = link_button("Show all");
                        more.set_halign(gtk::Align::Start);
                        let ll = l.clone();
                        more.connect_clicked(move |btn| {
                            let expanded = ll.lines() < 0;
                            ll.set_lines(if expanded { 12 } else { -1 });
                            ll.set_ellipsize(if expanded { gtk::pango::EllipsizeMode::End } else { gtk::pango::EllipsizeMode::None });
                            btn.set_label(if expanded { "Show all" } else { "Collapse" });
                        });
                        b.append(&more);
                    } else {
                        b.append(&l);
                    }
                }
                b.upcast()
            }
            ChatItem::Text(t) => {
                let text = t.text.borrow().clone();
                if text.is_empty() {
                    return vbox(0).upcast();
                }
                markdown::view(&text).upcast()
            }
            ChatItem::Thinking(t) => {
                let text = t.text.borrow().clone();
                if text.is_empty() {
                    return vbox(0).upcast();
                }
                let e = gtk::Expander::new(None);
                let head = hbox(5);
                head.append(&colored_icon("sy-sparkles-symbolic", 12, "violet"));
                head.append(&label_with("Thinking", &["sy-muted", "sy-small"]));
                e.set_label_widget(Some(&head));
                let l = selectable(wrap_label(&text, &["sy-muted"]));
                l.set_margin_start(18);
                e.set_child(Some(&l));
                e.upcast()
            }
            ChatItem::Tool(c) => self.tool_widget(c),
            ChatItem::Notice { text, is_error, .. } => {
                let b = hbox(6);
                let l1 = line();
                l1.set_hexpand(true);
                l1.set_valign(gtk::Align::Center);
                let l2 = line();
                l2.set_hexpand(true);
                l2.set_valign(gtk::Align::Center);
                b.append(&l1);
                b.append(&label_with(text, &[if *is_error { "fg-red" } else { "chat-notice" }, "sy-small"]));
                b.append(&l2);
                b.upcast()
            }
        }
    }

    fn tool_widget(self: &Rc<Self>, c: &Rc<ToolCall>) -> gtk::Widget {
        let folder = self.tab.folder.clone();
        let permission = c.permission.borrow().clone();
        let pending_permission = permission.as_ref().map(|p| !p.resolved.get()).unwrap_or(false);
        let default_open = pending_permission || matches!(c.name.as_str(), "Edit" | "MultiEdit" | "Write" | "ExitPlanMode" | "TodoWrite");
        let open = self.open_tools.borrow().get(&c.id).copied().unwrap_or(default_open);
        let card = vbox(0);
        card.add_css_class("chat-tool");
        match c.status.get() {
            ToolStatus::Error if !pending_permission => card.add_css_class("error"),
            ToolStatus::Denied if !pending_permission => card.add_css_class("denied"),
            _ => {}
        }
        let head = gtk::Button::new();
        head.add_css_class("flat");
        let hb = hbox(8);
        match c.status.get() {
            ToolStatus::Running => {
                if pending_permission {
                    hb.append(&colored_icon("sy-attention-symbolic", 14, "amber"));
                } else {
                    hb.append(&spinner(12));
                }
            }
            ToolStatus::Done => hb.append(&colored_icon("sy-check-symbolic", 14, "green")),
            ToolStatus::Error => hb.append(&colored_icon("sy-x-symbolic", 14, "red")),
            ToolStatus::Denied => hb.append(&colored_icon("sy-issue-skip-symbolic", 14, "amber")),
        }
        hb.append(&colored_icon(tool_icon(&c.name), 14, "muted"));
        hb.append(&label_with(&tool_title(c), &["sy-bold", "sy-text"]));
        let summary = label_with(&tool_summary(c, &folder), &["sy-muted"]);
        summary.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        summary.set_hexpand(true);
        hb.append(&summary);
        if let Some((a, d)) = tool_diff_stat(c) {
            hb.append(&diff_stat(a, d));
        }
        hb.append(&icon(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }, 12));
        head.set_child(Some(&hb));
        let w = Rc::downgrade(self);
        let id = c.id.clone();
        head.connect_clicked(move |_| {
            if let Some(me) = w.upgrade() {
                me.open_tools.borrow_mut().insert(id.clone(), !open);
                me.update_item(&format!("c{id}"));
            }
        });
        card.append(&head);
        if open {
            card.append(&line());
            let details = self.tool_details(c, &folder);
            pad(&details, 10);
            card.append(&details);
        }
        if let Some(p) = permission.filter(|p| !p.resolved.get()) {
            let pc = self.permission_card(&p, &folder);
            pad(&pc, 10);
            card.append(&pc);
        }
        card.upcast()
    }

    fn tool_details(self: &Rc<Self>, c: &Rc<ToolCall>, folder: &str) -> gtk::Box {
        let b = vbox(8);
        match c.name.as_str() {
            "Edit" | "MultiEdit" | "Write" => {
                if let Some(path) = c.string("file_path") {
                    let row = hbox(6);
                    let l = label_with(&relative(&path, folder), &["sy-mono", "sy-muted", "sy-small"]);
                    l.set_hexpand(true);
                    l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                    row.append(&l);
                    let open = link_button("Open");
                    open.connect_clicked(move |_| util::open_path(&path));
                    row.append(&open);
                    b.append(&row);
                }
                let lines = diff_lines(c);
                let diff = vbox(0);
                diff.add_css_class("sy-sunken");
                let total = lines.len();
                for l in lines.iter().take(60) {
                    diff.append(&line_widget(l, false));
                }
                b.append(&diff);
                if total > 60 {
                    let more = link_button(&format!("Show {} more lines", total - 60));
                    more.set_halign(gtk::Align::Start);
                    let d = diff.clone();
                    more.connect_clicked(move |btn| {
                        for l in lines.iter().skip(60) {
                            d.append(&line_widget(l, false));
                        }
                        btn.set_visible(false);
                    });
                    b.append(&more);
                }
            }
            "Bash" => {
                if let Some(cmd) = c.string("command") {
                    let l = selectable(wrap_label(&format!("$ {cmd}"), &["sy-mono", "sy-text"]));
                    b.append(&frame(&l, "chat-code"));
                }
                let out = match c.structured.borrow().as_ref() {
                    Some(s) => {
                        let stdout = s.get("stdout").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let stderr = s.get("stderr").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let joined = if stderr.is_empty() { stdout } else { format!("{stdout}\n{stderr}") };
                        if joined.is_empty() {
                            c.result_text.borrow().clone()
                        } else {
                            Some(joined)
                        }
                    }
                    None => c.result_text.borrow().clone(),
                };
                if let Some(w) = output_view(out.as_deref(), None) {
                    b.append(&w);
                }
            }
            "TodoWrite" => {
                let todos = c.input.borrow().get("todos").and_then(|t| t.as_array()).cloned().unwrap_or_default();
                b.append(&todo_list(&todos));
            }
            "ExitPlanMode" => b.append(&markdown::view(&c.string("plan").unwrap_or_default())),
            "AskUserQuestion" => {
                for q in c.input.borrow().get("questions").and_then(|q| q.as_array()).cloned().unwrap_or_default() {
                    b.append(&wrap_label(q.get("question").and_then(|x| x.as_str()).unwrap_or(""), &["sy-bold", "sy-text"]));
                }
                if let Some(r) = c.result_text.borrow().clone() {
                    b.append(&selectable(wrap_label(&r, &["sy-muted"])));
                }
            }
            "Read" => {
                let content = c.structured.borrow().as_ref().and_then(|s| s.get("file")).and_then(|f| f.get("content")).and_then(|x| x.as_str()).map(|s| s.to_string());
                let text = content.or_else(|| c.result_text.borrow().clone());
                if let Some(w) = output_view(text.as_deref(), None) {
                    b.append(&w);
                }
            }
            _ => {
                let input = c.input.borrow().clone();
                if !input.is_empty() {
                    if let Ok(pretty) = serde_json::to_string_pretty(&Value::Object(input)) {
                        if let Some(w) = output_view(Some(&pretty), Some("Input")) {
                            b.append(&w);
                        }
                    }
                }
                if let Some(w) = output_view(c.result_text.borrow().as_deref(), Some("Result")) {
                    b.append(&w);
                }
            }
        }
        b
    }

    fn permission_card(self: &Rc<Self>, p: &Rc<PermissionRequest>, folder: &str) -> gtk::Box {
        let chat = self.chat.clone();
        let card = vbox(10);
        card.add_css_class("chat-permission");
        let feedback = gtk::Entry::new();
        let deny_row = |placeholder: &str, card: &gtk::Box| {
            let row = hbox(6);
            feedback.set_placeholder_text(Some(placeholder));
            feedback.set_hexpand(true);
            row.append(&feedback);
            let deny = button("Deny");
            let (c, pp, f) = (chat.clone(), p.clone(), feedback.clone());
            deny.connect_clicked(move |_| c.respond(&pp, false, None, Some(f.text().to_string()), None));
            let (c, pp) = (chat.clone(), p.clone());
            feedback.connect_activate(move |f| c.respond(&pp, false, None, Some(f.text().to_string()), None));
            row.append(&deny);
            card.append(&row);
        };
        match p.tool_name.as_str() {
            "AskUserQuestion" => {
                let questions: Vec<Value> = p.input.get("questions").and_then(|q| q.as_array()).cloned().unwrap_or_default();
                let answers: Rc<RefCell<Map<String, Value>>> = Rc::new(RefCell::new(Map::new()));
                let submit = suggested("Submit");
                submit.set_sensitive(questions.is_empty());
                let n = questions.len();
                for q in &questions {
                    let text = q.get("question").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let qb = vbox(6);
                    qb.append(&wrap_label(&text, &["sy-bold", "sy-text"]));
                    let mut group: Option<gtk::CheckButton> = None;
                    for o in q.get("options").and_then(|o| o.as_array()).cloned().unwrap_or_default() {
                        let label = o.get("label").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        let r = gtk::CheckButton::new();
                        let content = vbox(1);
                        content.append(&label_with(&label, &["sy-text"]));
                        if let Some(d) = o.get("description").and_then(|x| x.as_str()) {
                            content.append(&wrap_label(d, &["sy-muted", "sy-small"]));
                        }
                        r.set_child(Some(&content));
                        if let Some(g) = &group {
                            r.set_group(Some(g));
                        } else {
                            group = Some(r.clone());
                        }
                        let (a, t, s) = (answers.clone(), text.clone(), submit.clone());
                        r.connect_toggled(move |b| {
                            if b.is_active() {
                                a.borrow_mut().insert(t.clone(), Value::String(label.clone()));
                                s.set_sensitive(a.borrow().len() >= n);
                            }
                        });
                        qb.append(&r);
                    }
                    let other = gtk::Entry::new();
                    other.set_placeholder_text(Some("Other…"));
                    let (a, t, s) = (answers.clone(), text.clone(), submit.clone());
                    other.connect_changed(move |e| {
                        let v = e.text().to_string();
                        if v.is_empty() {
                            a.borrow_mut().remove(&t);
                        } else {
                            a.borrow_mut().insert(t.clone(), Value::String(v));
                        }
                        s.set_sensitive(a.borrow().len() >= n);
                    });
                    qb.append(&other);
                    card.append(&qb);
                }
                let row = hbox(6);
                let (c, pp, a) = (chat.clone(), p.clone(), answers.clone());
                submit.connect_clicked(move |_| {
                    let mut input = pp.input.clone();
                    input.insert("answers".into(), Value::Object(a.borrow().clone()));
                    c.respond(&pp, true, None, None, Some(input));
                });
                let skip = button("Skip");
                let (c, pp) = (chat.clone(), p.clone());
                skip.connect_clicked(move |_| c.respond(&pp, false, None, Some("The user skipped the question".into()), None));
                row.append(&submit);
                row.append(&skip);
                card.append(&row);
            }
            "ExitPlanMode" => {
                card.append(&label_with("Claude has a plan. Start implementing?", &["sy-bold", "sy-text"]));
                let row = hbox(6);
                let yes_auto = suggested("Yes, auto-accept edits");
                let (c, pp) = (chat.clone(), p.clone());
                yes_auto.connect_clicked(move |_| {
                    c.respond(&pp, true, None, None, None);
                    c.set_permission_mode("acceptEdits");
                });
                let yes_ask = button("Yes, ask before edits");
                let (c, pp) = (chat.clone(), p.clone());
                yes_ask.connect_clicked(move |_| {
                    c.respond(&pp, true, None, None, None);
                    c.set_permission_mode("default");
                });
                row.append(&yes_auto);
                row.append(&yes_ask);
                card.append(&row);
                deny_row("What to change in the plan…", &card);
            }
            _ => {
                let question = match p.tool_name.as_str() {
                    "Bash" => "Claude wants to run a command".to_string(),
                    "Edit" | "MultiEdit" => "Claude wants to edit a file".into(),
                    "Write" => "Claude wants to write a file".into(),
                    "WebFetch" => "Claude wants to fetch a URL".into(),
                    other => format!("Claude wants to use {other}"),
                };
                card.append(&label_with(&question, &["fg-amber", "sy-small"]));
                if let Some(d) = p.description.as_deref().filter(|d| !d.is_empty()) {
                    card.append(&wrap_label(d, &["sy-muted"]));
                }
                if let Some(path) = &p.blocked_path {
                    card.append(&label_with(&relative(path, folder), &["sy-mono", "sy-muted", "sy-small"]));
                }
                let row = hbox(6);
                let allow = suggested("Allow");
                let (c, pp) = (chat.clone(), p.clone());
                allow.connect_clicked(move |_| c.respond(&pp, true, None, None, None));
                row.append(&allow);
                card.append(&row);
                for s in &p.suggestions {
                    let b = link_button(&format!("Allow, and don't ask again for {}", s.label));
                    b.set_halign(gtk::Align::Start);
                    let (c, pp, ss) = (chat.clone(), p.clone(), s.clone());
                    b.connect_clicked(move |_| c.respond(&pp, true, Some(ss.clone()), None, None));
                    card.append(&b);
                }
                deny_row("No — tell Claude what to do instead…", &card);
            }
        }
        card
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Completion {
    Slash,
    File,
}

struct CompletionItem {
    title: String,
    detail: String,
    insert: String,
}

pub struct ChatComposer {
    pub widget: gtk::Box,
    tab: Rc<Tab>,
    chat: Rc<ChatSession>,
    view: gtk::TextView,
    placeholder: gtk::Label,
    images: RefCell<Vec<ChatImage>>,
    images_row: gtk::Box,
    completion_box: gtk::ListBox,
    completion: Cell<Option<Completion>>,
    items: RefCell<Vec<CompletionItem>>,
    files: RefCell<Vec<String>>,
    loading_files: Cell<bool>,
    bottom: gtk::Box,
    stop: gtk::Button,
    send: gtk::Button,
    usage: gtk::MenuButton,
}

pub fn token_short(n: i64) -> String {
    if n >= 1_000_000 {
        if n % 1_000_000 == 0 {
            format!("{}M", n / 1_000_000)
        } else {
            format!("{:.1}M", n as f64 / 1_000_000.0)
        }
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

fn window_title(key: &str) -> String {
    match key {
        "five_hour" => "Session limit".into(),
        "seven_day" => "Weekly · all models".into(),
        k => {
            let model: String = k.replace("seven_day_", "").replace('_', " ");
            if k.starts_with("seven_day") {
                format!("Weekly · {model}")
            } else {
                k.replace('_', " ")
            }
        }
    }
}

fn resets(date: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let Some(d) = date else { return String::new() };
    let secs = (d - chrono::Utc::now()).num_seconds();
    if secs <= 0 {
        return "Resetting".into();
    }
    if secs < 24 * 3600 {
        let (h, m) = (secs / 3600, (secs % 3600) / 60);
        return if h > 0 { format!("Resets in {h} hr {m} min") } else { format!("Resets in {m} min") };
    }
    format!("Resets {}", d.with_timezone(&chrono::Local).format("%a %H:%M"))
}

impl ChatComposer {
    fn new(tab: Rc<Tab>, chat: Rc<ChatSession>) -> Rc<Self> {
        let view = gtk::TextView::new();
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_top_margin(6);
        view.set_bottom_margin(6);
        view.set_left_margin(6);
        view.set_accepts_tab(false);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_propagate_natural_height(true);
        scroll.set_min_content_height(34);
        scroll.set_max_content_height(220);
        scroll.set_child(Some(&view));
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&scroll));
        let placeholder = label_with("Ask Claude or describe a task…", &["sy-dim"]);
        placeholder.set_halign(gtk::Align::Start);
        placeholder.set_valign(gtk::Align::Start);
        placeholder.set_margin_start(8);
        placeholder.set_margin_top(7);
        placeholder.set_can_target(false);
        overlay.add_overlay(&placeholder);
        let completion_box = gtk::ListBox::new();
        completion_box.add_css_class("sy-sunken");
        completion_box.set_visible(false);
        let images_row = hbox(6);
        images_row.set_visible(false);
        let bottom = hbox(6);
        bottom.add_css_class("sy-small");
        let stop = gtk::Button::from_icon_name("sy-stop-symbolic");
        stop.add_css_class("tint-red");
        stop.set_tooltip_text(Some("Stop (Esc)"));
        let send = gtk::Button::from_icon_name("sy-send-symbolic");
        send.add_css_class("tint-coral");
        send.set_tooltip_text(Some("Send (Enter)"));
        let usage = gtk::MenuButton::new();
        usage.add_css_class("flat");
        let widget = vbox(6);
        widget.add_css_class("chat-composer");
        widget.append(&completion_box);
        widget.append(&images_row);
        widget.append(&overlay);
        widget.append(&bottom);
        let me = Rc::new(ChatComposer {
            widget,
            tab,
            chat,
            view: view.clone(),
            placeholder,
            images: RefCell::new(vec![]),
            images_row,
            completion_box: completion_box.clone(),
            completion: Cell::new(None),
            items: RefCell::new(vec![]),
            files: RefCell::new(vec![]),
            loading_files: Cell::new(false),
            bottom,
            stop: stop.clone(),
            send: send.clone(),
            usage,
        });
        let w = Rc::downgrade(&me);
        view.buffer().connect_changed(move |_| {
            if let Some(me) = w.upgrade() {
                me.placeholder.set_visible(me.text().is_empty());
                me.update_completion();
                me.update_send();
            }
        });
        let w = Rc::downgrade(&me);
        view.buffer().connect_cursor_position_notify(move |_| {
            if let Some(me) = w.upgrade() {
                me.update_completion();
            }
        });
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let w = Rc::downgrade(&me);
        keys.connect_key_pressed(move |_, key, _, state| {
            let Some(me) = w.upgrade() else { return glib::Propagation::Proceed };
            me.handle_key(key, state)
        });
        view.add_controller(keys);
        let w = Rc::downgrade(&me);
        view.connect_paste_clipboard(move |v| {
            let Some(me) = w.upgrade() else { return };
            let clip = v.clipboard();
            if clip.formats().contain_mime_type("image/png") || clip.formats().contain_gtype(gdk::Texture::static_type()) {
                v.stop_signal_emission_by_name("paste-clipboard");
                let me2 = me.clone();
                clip.read_texture_async(None::<&gtk::gio::Cancellable>, move |res| {
                    if let Ok(Some(tex)) = res {
                        let bytes = tex.save_to_png_bytes();
                        me2.add_image(ChatImage { data: bytes.to_vec(), media_type: "image/png".into() });
                    }
                });
            }
        });
        let drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
        let w = Rc::downgrade(&me);
        drop.connect_drop(move |_, value, _, _| {
            let Some(me) = w.upgrade() else { return false };
            let Ok(list) = value.get::<gdk::FileList>() else { return false };
            for f in list.files() {
                if let Some(p) = f.path() {
                    me.add_image_path(&p);
                }
            }
            true
        });
        me.widget.add_controller(drop);
        let w = Rc::downgrade(&me);
        completion_box.connect_row_activated(move |_, row| {
            if let Some(me) = w.upgrade() {
                me.accept(row.index().max(0) as usize);
            }
        });
        let w = Rc::downgrade(&me);
        send.connect_clicked(move |_| {
            if let Some(me) = w.upgrade() {
                me.submit();
            }
        });
        let c = me.chat.clone();
        stop.connect_clicked(move |_| c.interrupt());
        let popover = gtk::Popover::new();
        let w = Rc::downgrade(&me);
        popover.connect_show(move |p| {
            if let Some(me) = w.upgrade() {
                me.chat.refresh_context();
                p.set_child(Some(&me.usage_overlay()));
            }
        });
        me.usage.set_popover(Some(&popover));
        me.render_state();
        me
    }

    fn text(&self) -> String {
        let b = self.view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    fn cursor(&self) -> usize {
        self.view.buffer().cursor_position().max(0) as usize
    }

    fn update_send(&self) {
        self.send.set_sensitive(!self.text().trim().is_empty() || !self.images.borrow().is_empty());
    }

    fn add_image(self: &Rc<Self>, img: ChatImage) {
        self.images.borrow_mut().push(img);
        self.render_images();
        self.update_send();
    }

    fn add_image_path(self: &Rc<Self>, p: &std::path::Path) {
        let Ok(data) = std::fs::read(p) else { return };
        let ext = util::extension(&p.to_string_lossy());
        let media = match ext.as_str() {
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "jpg" | "jpeg" => "image/jpeg",
            _ => return,
        };
        self.add_image(ChatImage { data, media_type: media.into() });
    }

    fn render_images(self: &Rc<Self>) {
        clear(&self.images_row);
        let images = self.images.borrow().clone();
        self.images_row.set_visible(!images.is_empty());
        for (i, img) in images.iter().enumerate() {
            let o = gtk::Overlay::new();
            if let Ok(tex) = gdk::Texture::from_bytes(&glib::Bytes::from(&img.data[..])) {
                let p = gtk::Picture::for_paintable(&tex);
                p.set_size_request(52, 52);
                p.set_content_fit(gtk::ContentFit::Cover);
                o.set_child(Some(&p));
            }
            let x = gtk::Button::from_icon_name("window-close-symbolic");
            x.add_css_class("circular");
            x.add_css_class("osd");
            x.set_halign(gtk::Align::End);
            x.set_valign(gtk::Align::Start);
            let w = Rc::downgrade(self);
            x.connect_clicked(move |_| {
                if let Some(me) = w.upgrade() {
                    if i < me.images.borrow().len() {
                        me.images.borrow_mut().remove(i);
                    }
                    me.render_images();
                    me.update_send();
                }
            });
            o.add_overlay(&x);
            self.images_row.append(&o);
        }
    }

    fn handle_key(self: &Rc<Self>, key: gdk::Key, state: gdk::ModifierType) -> glib::Propagation {
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let completing = self.completion.get().is_some() && !self.items.borrow().is_empty();
        match key {
            gdk::Key::Escape => {
                if self.completion.get().is_some() {
                    self.completion.set(None);
                    self.completion_box.set_visible(false);
                    return glib::Propagation::Stop;
                }
                if self.chat.busy.get() {
                    self.chat.interrupt();
                    return glib::Propagation::Stop;
                }
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                if shift {
                    return glib::Propagation::Proceed;
                }
                if completing {
                    let i = self.completion_box.selected_row().map(|r| r.index().max(0) as usize).unwrap_or(0);
                    self.accept(i);
                } else {
                    self.submit();
                }
                return glib::Propagation::Stop;
            }
            gdk::Key::Tab | gdk::Key::ISO_Left_Tab => {
                if shift && !completing {
                    let order = ["default", "acceptEdits", "plan"];
                    let cur = self.chat.permission_mode.borrow().clone();
                    let i = order.iter().position(|m| *m == cur).unwrap_or(0);
                    self.chat.set_permission_mode(order[(i + 1) % order.len()]);
                    return glib::Propagation::Stop;
                }
                if completing {
                    let i = self.completion_box.selected_row().map(|r| r.index().max(0) as usize).unwrap_or(0);
                    self.accept(i);
                    return glib::Propagation::Stop;
                }
            }
            gdk::Key::Down | gdk::Key::Up if completing => {
                let n = self.items.borrow().len() as i32;
                let cur = self.completion_box.selected_row().map(|r| r.index()).unwrap_or(0);
                let next = if key == gdk::Key::Down { (cur + 1).min(n - 1) } else { (cur - 1).max(0) };
                if let Some(row) = self.completion_box.row_at_index(next) {
                    self.completion_box.select_row(Some(&row));
                }
                return glib::Propagation::Stop;
            }
            _ => {}
        }
        glib::Propagation::Proceed
    }

    fn token_range(&self) -> Option<(usize, usize)> {
        let chars: Vec<char> = self.text().chars().collect();
        let pos = self.cursor().min(chars.len());
        let mut start = pos;
        while start > 0 {
            let c = chars[start - 1];
            if c == ' ' || c == '\n' || c == '\t' {
                break;
            }
            start -= 1;
        }
        if start < pos {
            Some((start, pos))
        } else {
            None
        }
    }

    fn update_completion(self: &Rc<Self>) {
        let Some((start, end)) = self.token_range() else {
            self.completion.set(None);
            self.completion_box.set_visible(false);
            return;
        };
        let token: String = self.text().chars().skip(start).take(end - start).collect();
        let (kind, query) = if token.starts_with('/') && start == 0 {
            (Completion::Slash, token[1..].to_string())
        } else if let Some(q) = token.strip_prefix('@') {
            if self.files.borrow().is_empty() {
                self.load_files();
            }
            (Completion::File, q.to_string())
        } else {
            self.completion.set(None);
            self.completion_box.set_visible(false);
            return;
        };
        self.completion.set(Some(kind));
        let items: Vec<CompletionItem> = match kind {
            Completion::Slash => {
                let mut all = vec![("compact".to_string(), "Compact the conversation".to_string()), ("clear".to_string(), "Start a new session".to_string())];
                all.extend(self.chat.commands.borrow().iter().filter(|c| c.name != "compact" && c.name != "clear").map(|c| (c.name.clone(), c.description.clone())));
                let q = query.to_lowercase();
                let mut f: Vec<(String, String)> = all.into_iter().filter(|(n, _)| q.is_empty() || n.to_lowercase().contains(&q)).collect();
                f.sort_by_key(|(n, _)| (if n.to_lowercase().starts_with(&q) { 0 } else { 1 }, n.clone()));
                f.into_iter().take(12).map(|(n, d)| CompletionItem { title: format!("/{n}"), detail: d, insert: format!("/{n} ") }).collect()
            }
            Completion::File => util::fuzzy_match(&query, &self.files.borrow(), 12).into_iter().map(|p| CompletionItem { title: util::last_component(&p), detail: p.clone(), insert: format!("@{p} ") }).collect(),
        };
        self.completion_box.remove_all();
        for it in &items {
            let row = hbox(8);
            pad(&row, 4);
            row.append(&label_with(&it.title, &["sy-text", "sy-bold"]));
            let d = ellipsized(&it.detail, &["sy-muted", "sy-small"]);
            row.append(&d);
            self.completion_box.append(&row);
        }
        if let Some(first) = self.completion_box.row_at_index(0) {
            self.completion_box.select_row(Some(&first));
        }
        self.completion_box.set_visible(!items.is_empty());
        *self.items.borrow_mut() = items;
    }

    fn accept(self: &Rc<Self>, i: usize) {
        let insert = match self.items.borrow().get(i) {
            Some(it) => it.insert.clone(),
            None => return,
        };
        let Some((start, end)) = self.token_range() else { return };
        if insert == "/clear " {
            self.view.buffer().set_text("");
            self.completion.set(None);
            self.completion_box.set_visible(false);
            let f = self.tab.folder.clone();
            app::get().claude.new_session(&f, None, None, None, Some(crate::claude::TabKind::Chat));
            return;
        }
        let buffer = self.view.buffer();
        let mut s = buffer.iter_at_offset(start as i32);
        let mut e = buffer.iter_at_offset(end as i32);
        buffer.delete(&mut s, &mut e);
        let mut at = buffer.iter_at_offset(start as i32);
        buffer.insert(&mut at, &insert);
        self.completion.set(None);
        self.completion_box.set_visible(false);
        self.view.grab_focus();
    }

    fn load_files(self: &Rc<Self>) {
        if self.loading_files.replace(true) {
            return;
        }
        let folder = self.tab.folder.clone();
        let w = Rc::downgrade(self);
        crate::rt::spawn_local(async move {
            let list = crate::rt::blocking(move || {
                let (out, _) = crate::claude::git::run(&["ls-files", "--cached", "--others", "--exclude-standard"], &folder);
                let mut list: Vec<String> = out.lines().map(|s| s.to_string()).collect();
                if list.is_empty() {
                    let mut stack = vec![(std::path::PathBuf::from(&folder), String::new())];
                    while let Some((dir, rel)) = stack.pop() {
                        if list.len() >= 5000 {
                            break;
                        }
                        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
                        for e in rd.flatten() {
                            let name = e.file_name().to_string_lossy().to_string();
                            if name == ".git" || name == "node_modules" || name == "build" || name == "target" {
                                continue;
                            }
                            let r = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                                stack.push((e.path(), r));
                            } else {
                                list.push(r);
                            }
                        }
                    }
                }
                list.truncate(50_000);
                list
            })
            .await;
            if let Some(me) = w.upgrade() {
                *me.files.borrow_mut() = list;
                me.loading_files.set(false);
                me.update_completion();
            }
        });
    }

    fn submit(self: &Rc<Self>) {
        let message = self.text();
        let images = self.images.borrow().clone();
        if message.trim().is_empty() && images.is_empty() {
            return;
        }
        if !self.chat.running.get() {
            app::get().claude.restart(&self.tab);
        }
        self.chat.send(&message, images);
        self.view.buffer().set_text("");
        self.images.borrow_mut().clear();
        self.render_images();
        self.completion.set(None);
        self.completion_box.set_visible(false);
    }

    fn render_state(self: &Rc<Self>) {
        let c = &self.chat;
        self.placeholder.set_text(if c.busy.get() { "Message will be queued…" } else { "Ask Claude or describe a task…" });
        self.stop.set_visible(c.busy.get());
        self.update_send();
        let pct = c.context.borrow().as_ref().map(|x| x.percentage).or(c.context_percent.get()).unwrap_or(0);
        self.usage.set_label(&format!("ctx {pct}%"));
        self.usage.set_tooltip_text(Some(&format!("Context {pct}% — click for usage and limits")));
        let mut c2 = self.bottom.first_child();
        while let Some(w) = c2 {
            c2 = w.next_sibling();
            self.bottom.remove(&w);
        }
        let mode = ClaudeMode::from_key(&c.permission_mode.borrow()).unwrap_or(ClaudeMode::Standard);
        let mode_menu = Menu::new();
        for m in ClaudeMode::ALL {
            let cc = c.clone();
            mode_menu.item(&format!("{}{}", if m == mode { "✓ " } else { "" }, m.title()), move || cc.set_permission_mode(m.key()));
        }
        let mb = crate::ui::menu::dropdown_button(mode.title(), Some(mode.icon()), &mode_menu);
        mb.add_css_class("flat");
        mb.set_tooltip_text(Some("Permission mode (Shift+Tab to cycle)"));
        self.bottom.append(&mb);
        let models = c.models.borrow().clone();
        let current = c.model_value.borrow().clone();
        let model_title = models.iter().find(|m| m.value == current).map(|m| m.display_name.clone()).unwrap_or_else(|| {
            let m = c.model.borrow().replace("claude-", "");
            if m.is_empty() {
                "Model".into()
            } else {
                m
            }
        });
        let model_menu = Menu::new();
        for m in &models {
            let cc = c.clone();
            let v = m.value.clone();
            model_menu.item(&format!("{}{} — {}", if m.value == current { "✓ " } else { "" }, m.display_name, m.description), move || cc.set_model(&v));
        }
        let modb = crate::ui::menu::dropdown_button(&model_title, Some("sy-memory-symbolic"), &model_menu);
        modb.add_css_class("flat");
        modb.set_sensitive(!models.is_empty());
        self.bottom.append(&modb);
        let efforts = models.iter().find(|m| m.value == current).map(|m| m.efforts.clone()).or_else(|| models.first().map(|m| m.efforts.clone())).unwrap_or_default();
        if !efforts.is_empty() {
            let cur = c.effort.borrow().clone();
            let em = Menu::new();
            let cc = c.clone();
            em.item(&format!("{}Default", if cur.is_empty() { "✓ " } else { "" }), move || cc.set_effort(""));
            for e in efforts {
                let cc = c.clone();
                let ee = e.clone();
                em.item(&format!("{}{e}", if cur == e { "✓ " } else { "" }), move || cc.set_effort(&ee));
            }
            let eb = crate::ui::menu::dropdown_button(if cur.is_empty() { "effort" } else { &cur }, Some("sy-gauge-symbolic"), &em);
            eb.add_css_class("flat");
            self.bottom.append(&eb);
        }
        let attach = icon_button("sy-photo-symbolic", "Attach image");
        let w = Rc::downgrade(self);
        attach.connect_clicked(move |_| {
            let w = w.clone();
            crate::ui::dialogs::choose_images(move |paths| {
                if let Some(me) = w.upgrade() {
                    for p in paths {
                        me.add_image_path(&p);
                    }
                }
            });
        });
        self.bottom.append(&attach);
        self.bottom.append(&spacer());
        self.bottom.append(&self.usage);
        self.bottom.append(&self.stop);
        self.bottom.append(&self.send);
    }

    fn usage_overlay(self: &Rc<Self>) -> gtk::Widget {
        let b = vbox(10);
        pad(&b, 12);
        b.set_size_request(320, -1);
        let c = &self.chat;
        b.append(&label_with("Context", &["heading"]));
        match c.context.borrow().clone() {
            Some(ctx) => {
                let bar = gtk::LevelBar::for_interval(0.0, 100.0);
                bar.set_value(ctx.percentage.clamp(0, 100) as f64);
                b.append(&bar);
                b.append(&label_with(&format!("{} / {} tokens · {}%", token_short(ctx.total), token_short(ctx.max), ctx.percentage), &["sy-muted", "sy-small"]));
                if let Some(t) = ctx.auto_compact_threshold.filter(|_| ctx.auto_compact_enabled) {
                    b.append(&label_with(&format!("Auto-compacts at {}", token_short(t)), &["sy-dim", "sy-small"]));
                }
                for cat in ctx.categories.iter().filter(|c| c.kind != "free" && c.tokens > 0) {
                    let row = hbox(6);
                    let n = label_with(&cat.name, &["sy-text", "sy-small"]);
                    n.set_hexpand(true);
                    row.append(&n);
                    row.append(&label_with(&token_short(cat.tokens), &["sy-muted", "sy-small", "sy-mono"]));
                    b.append(&row);
                }
            }
            None => b.append(&label_with("No context data yet", &["sy-muted", "sy-small"])),
        }
        let compact = button("Compact now");
        let cc = c.clone();
        compact.connect_clicked(move |_| cc.compact());
        compact.set_sensitive(c.running.get() && !c.busy.get());
        b.append(&compact);
        b.append(&line());
        let (windows, sub, note) = chat::USAGE.with(|u| {
            let u = u.borrow();
            (u.windows.clone(), u.subscription.clone(), u.overage_note.clone())
        });
        let head = hbox(6);
        head.append(&label_with("Plan usage", &["heading"]));
        if let Some(s) = sub {
            head.append(&chip(&s, "coral", None, false));
        }
        b.append(&head);
        if windows.is_empty() {
            b.append(&wrap_label("Limits appear after the first reply in this session.", &["sy-muted", "sy-small"]));
        }
        for w in windows {
            let row = vbox(3);
            let top = hbox(6);
            let t = label_with(&window_title(&w.key), &["sy-text", "sy-small"]);
            t.set_hexpand(true);
            top.append(&t);
            top.append(&label_with(&format!("{}%", (w.utilization * 100.0).round() as i64), &["sy-muted", "sy-small"]));
            row.append(&top);
            let bar = gtk::LevelBar::for_interval(0.0, 1.0);
            bar.set_value(w.utilization.clamp(0.0, 1.0));
            row.append(&bar);
            row.append(&label_with(&resets(w.resets_at), &["sy-dim", "sy-tiny"]));
            b.append(&row);
        }
        if let Some(n) = note {
            b.append(&label_with(&n, &["fg-amber", "sy-small"]));
        }
        let page = link_button("Usage page");
        page.set_halign(gtk::Align::Start);
        page.connect_clicked(|_| util::open_url("https://claude.ai/settings/usage"));
        b.append(&page);
        b.upcast()
    }
}
