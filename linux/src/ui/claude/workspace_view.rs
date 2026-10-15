use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;

use super::chat_view::ChatView;
use super::inspector::Inspector;
use crate::claude::accounts::{self, Status};
use crate::claude::workspace::{apply_terminal_colors, Tab, WsEvent};
use crate::claude::{launcher, ClaudeMode, ModelOption, TabKind, TabState, EFFORTS};
use crate::ui::menu::Menu;
use crate::ui::widgets::*;
use crate::{app, util};

struct View {
    outer: gtk::Stack,
    title: adw::WindowTitle,
    inspector_btn: gtk::ToggleButton,
    tab_bar: gtk::Box,
    control_bar: gtk::Box,
    content: gtk::Stack,
    inspector_bin: adw::Bin,
    paned: gtk::Paned,
    empty: gtk::Box,
    tab_widgets: RefCell<HashMap<u64, (gtk::Widget, Option<Rc<ChatView>>)>>,
    inspectors: RefCell<HashMap<u64, Rc<Inspector>>>,
    terminal_overlays: RefCell<HashMap<u64, gtk::Box>>,
}

fn control_menu(icon_name: &str, title: &str, help: &str, menu: &Menu) -> gtk::MenuButton {
    let b = crate::ui::menu::dropdown_button(title, Some(icon_name), menu);
    b.set_tooltip_text(Some(&format!("{help} — applies to new and restarted sessions")));
    b
}

impl View {
    fn render_all(self: &Rc<Self>) {
        let store = accounts::store();
        let status = store.status.borrow().clone();
        if matches!(status, Status::LoggedOut | Status::NotInstalled) {
            self.outer.set_visible_child_name("auth");
            self.title.set_title("Claude Code");
            self.title.set_subtitle("");
            self.inspector_btn.set_visible(false);
            return;
        }
        let ws = app::get().claude.clone();
        let Some(tab) = ws.selected_tab() else {
            self.render_empty();
            self.outer.set_visible_child_name("empty");
            self.title.set_title("Claude Code");
            self.title.set_subtitle("");
            self.inspector_btn.set_visible(false);
            return;
        };
        self.outer.set_visible_child_name("tabs");
        self.inspector_btn.set_visible(true);
        self.title.set_title(&tab.title.borrow());
        self.title.set_subtitle(&util::tilde(&tab.folder));
        self.render_tab_bar();
        self.render_control_bar(&tab);
        self.show_tab_content(&tab);
        self.prune();
    }

    fn prune(&self) {
        let ws = app::get().claude.clone();
        let live: Vec<u64> = ws.tabs.borrow().iter().map(|t| t.id).collect();
        let dead: Vec<u64> = self.tab_widgets.borrow().keys().filter(|k| !live.contains(k)).copied().collect();
        for id in dead {
            if let Some((w, _)) = self.tab_widgets.borrow_mut().remove(&id) {
                self.content.remove(&w);
            }
            self.inspectors.borrow_mut().remove(&id);
            self.terminal_overlays.borrow_mut().remove(&id);
        }
    }

    fn render_empty(self: &Rc<Self>) {
        clear(&self.empty);
        self.empty.append(&icon_tile("sy-sparkles-symbolic", "coral", 56));
        let t = label_with("Claude Code", &["title-2", "sy-text"]);
        t.set_xalign(0.5);
        self.empty.append(&t);
        let s = wrap_label("Start a new session in a project or resume one from the history on the left.", &["sy-muted"]);
        s.set_justify(gtk::Justification::Center);
        s.set_xalign(0.5);
        s.set_max_width_chars(50);
        self.empty.append(&s);
        let list = vbox(6);
        list.set_size_request(380, -1);
        let ws = app::get().claude.clone();
        for p in ws.projects.borrow().iter().filter(|p| p.exists()).take(6) {
            let b = gtk::Button::new();
            b.add_css_class("sy-card-flat");
            let row = hbox(10);
            pad(&row, 8);
            row.append(&icon_tile("sy-folder-symbolic", crate::theme::palette_for(&p.path), 22));
            let n = label_with(&p.name(), &["sy-bold", "sy-text"]);
            n.set_hexpand(true);
            row.append(&n);
            row.append(&label_with(&util::rel(Some(p.last_activity())), &["sy-dim", "sy-small"]));
            row.append(&colored_icon("list-add-symbolic", 14, "coral"));
            b.set_child(Some(&row));
            let path = p.path.clone();
            b.connect_clicked(move |_| {
                app::get().claude.new_session(&path, None, None, None, None);
            });
            list.append(&b);
        }
        let open = button("Open folder…");
        open.connect_clicked(|_| super::column::open_folder());
        list.append(&open);
        self.empty.append(&list);
    }

    fn render_tab_bar(self: &Rc<Self>) {
        clear(&self.tab_bar);
        let ws = app::get().claude.clone();
        let selected = ws.selected.get();
        for t in ws.tabs.borrow().iter() {
            let chip_box = hbox(6);
            chip_box.add_css_class("claude-tab");
            if selected == Some(t.id) {
                chip_box.add_css_class("selected");
            }
            let edge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            edge.add_css_class(&format!("edge-{}", crate::theme::palette_for(&t.folder)));
            edge.set_size_request(3, 14);
            edge.set_valign(gtk::Align::Center);
            chip_box.append(&edge);
            chip_box.append(&super::state_badge(t.state(), None, true));
            chip_box.append(&colored_icon(if t.kind == TabKind::Chat { "sy-comment-symbolic" } else { "sy-terminal-symbolic" }, 11, "muted"));
            let title = ellipsized(&t.title.borrow(), &[if selected == Some(t.id) { "sy-text" } else { "sy-muted" }]);
            title.set_max_width_chars(22);
            chip_box.append(&title);
            chip_box.append(&label_with(&t.project_name(), &["sy-dim", "sy-small"]));
            let close = icon_button("window-close-symbolic", "Close");
            close.add_css_class("circular");
            let tt = t.clone();
            close.connect_clicked(move |_| app::get().claude.close(&tt));
            chip_box.append(&close);
            let id = t.id;
            on_click_widget(&chip_box, move || app::get().claude.select(id));
            self.tab_bar.append(&chip_box);
        }
        let mut menu = Menu::new();
        if let Some(current) = ws.selected_tab() {
            let f = current.folder.clone();
            menu.item(&format!("Chat in {}", current.project_name()), move || {
                app::get().claude.new_session(&f, None, None, None, Some(TabKind::Chat));
            });
            let f = current.folder.clone();
            menu.item(&format!("Terminal in {}", current.project_name()), move || {
                app::get().claude.new_session(&f, None, None, None, Some(TabKind::Terminal));
            });
            menu.section(None);
        }
        for p in ws.projects.borrow().iter().filter(|p| p.exists()).take(12) {
            let path = p.path.clone();
            menu.item(&p.name(), move || {
                app::get().claude.new_session(&path, None, None, None, None);
            });
        }
        menu.item("Open folder…", super::column::open_folder);
        let b = menu.button("list-add-symbolic", "New session");
        b.add_css_class("flat");
        self.tab_bar.append(&b);
    }

    fn render_control_bar(self: &Rc<Self>, tab: &Rc<Tab>) {
        clear(&self.control_bar);
        let ws = app::get().claude.clone();
        self.control_bar.append(&icon_tile("sy-folder-symbolic", crate::theme::palette_for(&tab.folder), 18));
        self.control_bar.append(&ellipsized(&tab.project_name(), &["sy-bold", "sy-text"]));
        if let Some(b) = tab.git.branch.borrow().as_ref().filter(|b| *b != "HEAD") {
            self.control_bar.append(&branch_tag(b));
        }
        let badge = super::state_badge(tab.state(), Some(tab.memory_bytes()), false);
        badge.set_margin_start(4);
        self.control_bar.append(&badge);
        self.control_bar.append(&spacer());
        if tab.kind == TabKind::Terminal {
            let model_menu = Menu::new();
            for m in ModelOption::ALL {
                let w = ws.clone();
                model_menu.item(&format!("{}{}", if ws.model() == m { "✓ " } else { "" }, m.title()), move || w.set_model(m));
            }
            let mut model_menu = model_menu;
            model_menu.section(None);
            let w = ws.clone();
            if tab.running() && ws.model() != ModelOption::Default {
                model_menu.item("Apply to current session (/model)", move || w.apply_model_to_current());
            } else {
                model_menu.disabled("Apply to current session (/model)");
            }
            self.control_bar.append(&control_menu("sy-memory-symbolic", ws.model().title(), "Model", &model_menu));
            let effort_menu = Menu::new();
            let current = ws.effort();
            for e in EFFORTS {
                let w = ws.clone();
                let label = if e.is_empty() { "Default" } else { e };
                effort_menu.item(&format!("{}{label}", if current == e { "✓ " } else { "" }), move || w.set_effort(e));
            }
            self.control_bar.append(&control_menu("sy-gauge-symbolic", if current.is_empty() { "Effort" } else { &current }, "Effort", &effort_menu));
            let mode_menu = Menu::new();
            for m in ClaudeMode::ALL {
                let w = ws.clone();
                mode_menu.item(&format!("{}{}", if ws.mode() == m { "✓ " } else { "" }, m.title()), move || w.set_mode(m));
            }
            self.control_bar.append(&control_menu("sy-attention-symbolic", ws.mode().title(), "Permission mode", &mode_menu));
        } else if let Some(chat) = &tab.chat {
            if chat.total_cost.get() > 0.0 || chat.session_id.borrow().is_some() {
                self.control_bar.append(&label_with(&chat.model.borrow().replace("claude-", ""), &["sy-muted", "sy-small"]));
            }
        }
        let kind = hbox(0);
        kind.add_css_class("linked");
        let chat_btn = gtk::ToggleButton::new();
        chat_btn.set_icon_name("sy-comment-symbolic");
        chat_btn.set_tooltip_text(Some("Chat"));
        let term_btn = gtk::ToggleButton::new();
        term_btn.set_icon_name("sy-terminal-symbolic");
        term_btn.set_tooltip_text(Some("Terminal"));
        term_btn.set_group(Some(&chat_btn));
        if tab.kind == TabKind::Chat {
            chat_btn.set_active(true);
        } else {
            term_btn.set_active(true);
        }
        let has_session = !tab.session_id.borrow().is_empty();
        chat_btn.set_sensitive(has_session);
        term_btn.set_sensitive(has_session);
        let tt = tab.clone();
        term_btn.connect_toggled(move |b| {
            if b.is_active() && tt.kind == TabKind::Chat {
                let tt = tt.clone();
                gtk::glib::idle_add_local_once(move || app::get().claude.open_in_terminal(&tt));
            }
        });
        let tt = tab.clone();
        chat_btn.connect_toggled(move |b| {
            if b.is_active() && tt.kind == TabKind::Terminal {
                let tt = tt.clone();
                gtk::glib::idle_add_local_once(move || app::get().claude.open_in_chat(&tt));
            }
        });
        kind.append(&chat_btn);
        kind.append(&term_btn);
        kind.set_tooltip_text(Some("Chat or terminal (same session)"));
        self.control_bar.append(&kind);
        let mut more = Menu::new();
        let tt = tab.clone();
        more.item("Restart with new settings", move || {
            let tt = tt.clone();
            if let Some(c) = &tt.chat {
                c.stop();
            } else {
                tt.kill_terminal();
            }
            gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(600), move || app::get().claude.restart(&tt));
        });
        let tt = tab.clone();
        if tab.running() && !matches!(tab.state(), TabState::Working | TabState::Waiting) {
            more.item("Free memory now", move || tt.sleep_now());
        } else {
            more.disabled("Free memory now");
        }
        let w = ws.clone();
        let idle = ws.idle_minutes();
        more.submenu("Sleep after idle", move |sub| {
            for m in [0i64, 5, 15, 30, 60] {
                let w2 = w.clone();
                let label = if m == 0 { "Never".to_string() } else { format!("{m} min") };
                sub.item(&format!("{}{label}", if idle == m { "✓ " } else { "" }), move || w2.set_idle_minutes(m));
            }
        });
        let w = ws.clone();
        let pref = ws.preferred_kind();
        more.submenu("Open new sessions as", move |sub| {
            let w2 = w.clone();
            sub.item(&format!("{}Chat", if pref == TabKind::Chat { "✓ " } else { "" }), move || w2.set_preferred_kind(TabKind::Chat));
            let w3 = w.clone();
            sub.item(&format!("{}Terminal", if pref == TabKind::Terminal { "✓ " } else { "" }), move || w3.set_preferred_kind(TabKind::Terminal));
        });
        more.section(None);
        let tt = tab.clone();
        more.item("Continue in system terminal", move || {
            let sid = tt.session_id.borrow().clone();
            let title = tt.title.borrow().clone();
            if let Err(e) = launcher::launch("", &tt.folder, launcher::Target::Terminal, launcher::PermissionMode::Standard, &title, if sid.is_empty() { None } else { Some(&sid) }, tt.config_dir.as_deref()) {
                crate::ui::dialogs::error(&e);
            }
        });
        let f = tab.folder.clone();
        more.item("Open folder", move || util::open_path(&f));
        let tt = tab.clone();
        more.item("Copy session ID", move || util::copy(&tt.session_id.borrow()));
        self.control_bar.append(&more.button("view-more-symbolic", "More"));
    }

    fn terminal_widget(self: &Rc<Self>, tab: &Rc<Tab>) -> gtk::Widget {
        let overlay = gtk::Overlay::new();
        let term = tab.terminal();
        overlay.set_child(Some(&term));
        let card = vbox(10);
        card.add_css_class("sy-card");
        card.set_halign(gtk::Align::Center);
        card.set_valign(gtk::Align::Center);
        overlay.add_overlay(&card);
        self.terminal_overlays.borrow_mut().insert(tab.id, card);
        overlay.upcast()
    }

    fn update_terminal_card(&self, tab: &Rc<Tab>) {
        let Some(card) = self.terminal_overlays.borrow().get(&tab.id).cloned() else { return };
        clear(&card);
        if tab.running() {
            card.set_visible(false);
            return;
        }
        card.set_visible(true);
        if tab.terminal_sleeping.get() {
            let r = hbox(6);
            r.append(&colored_icon("sy-moon-symbolic", 14, "indigo"));
            r.append(&label_with("Session is sleeping — memory freed", &["fg-indigo"]));
            card.append(&r);
        } else {
            let code = tab.exit_code.get().map(|c| format!(" ({c})")).unwrap_or_default();
            let text = if tab.exit_code.get() == Some(127) { "claude CLI not found".to_string() } else { format!("Session ended{code}") };
            card.append(&label_with(&text, &["sy-muted"]));
        }
        let row = hbox(8);
        let resume = tinted("Resume session", "coral");
        let tt = tab.clone();
        resume.connect_clicked(move |_| app::get().claude.restart(&tt));
        let close = button("Close tab");
        let tt = tab.clone();
        close.connect_clicked(move |_| app::get().claude.close(&tt));
        row.append(&resume);
        row.append(&close);
        card.append(&row);
    }

    fn show_tab_content(self: &Rc<Self>, tab: &Rc<Tab>) {
        let existing = self.tab_widgets.borrow().get(&tab.id).map(|(w, _)| w.clone());
        let widget = match existing {
            Some(w) => w,
            None => {
                let (w, chat) = match &tab.chat {
                    Some(_) => {
                        let v = ChatView::new(tab.clone());
                        (v.widget.clone().upcast::<gtk::Widget>(), Some(v))
                    }
                    None => (self.terminal_widget(tab), None),
                };
                self.content.add_named(&w, Some(&format!("tab-{}", tab.id)));
                self.tab_widgets.borrow_mut().insert(tab.id, (w.clone(), chat));
                w
            }
        };
        self.content.set_visible_child(&widget);
        if tab.chat.is_none() {
            self.update_terminal_card(tab);
            let term = tab.terminal();
            gtk::glib::idle_add_local_once(move || {
                term.grab_focus();
            });
        }
        let insp = self.inspectors.borrow().get(&tab.id).cloned();
        let insp = match insp {
            Some(i) => i,
            None => {
                let i = Inspector::new(tab.clone());
                self.inspectors.borrow_mut().insert(tab.id, i.clone());
                i
            }
        };
        self.inspector_bin.set_child(Some(&insp.widget));
        self.inspector_bin.set_visible(app::get().claude.show_inspector.get());
    }
}

pub fn build() -> gtk::Widget {
    let outer = gtk::Stack::new();
    outer.set_transition_type(gtk::StackTransitionType::Crossfade);
    outer.add_named(&super::auth_view::build(), Some("auth"));
    let empty = vbox(16);
    empty.set_valign(gtk::Align::Center);
    empty.set_halign(gtk::Align::Center);
    empty.set_vexpand(true);
    let empty_wrap = vbox(0);
    empty_wrap.add_css_class("sy-panel-pane");
    empty_wrap.append(&empty);
    outer.add_named(&empty_wrap, Some("empty"));

    let tab_bar = hbox(4);
    margins(&tab_bar, 6, 10, 6, 10);
    let tab_scroll = gtk::ScrolledWindow::new();
    tab_scroll.set_vscrollbar_policy(gtk::PolicyType::Never);
    tab_scroll.set_child(Some(&tab_bar));
    tab_scroll.add_css_class("sy-bar");
    let control_bar = hbox(6);
    control_bar.add_css_class("sy-bar");
    margins(&control_bar, 0, 0, 0, 0);
    let control_wrap = vbox(0);
    control_wrap.add_css_class("sy-bar");
    let cb_inner = control_bar.clone();
    margins(&cb_inner, 6, 12, 6, 12);
    control_wrap.append(&control_bar);
    let content = gtk::Stack::new();
    content.set_vexpand(true);
    content.set_hexpand(true);
    let left = vbox(0);
    left.add_css_class("sy-panel-pane");
    left.append(&tab_scroll);
    left.append(&control_wrap);
    left.append(&content);
    left.set_size_request(520, -1);
    let inspector_bin = adw::Bin::new();
    inspector_bin.set_size_request(300, -1);
    let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
    paned.set_start_child(Some(&left));
    paned.set_end_child(Some(&inspector_bin));
    paned.set_resize_end_child(false);
    paned.set_shrink_end_child(false);
    paned.set_shrink_start_child(false);
    outer.add_named(&paned, Some("tabs"));

    let title = adw::WindowTitle::new("Claude Code", "");
    let inspector_btn = gtk::ToggleButton::new();
    inspector_btn.set_icon_name("sy-panel-right-symbolic");
    inspector_btn.set_tooltip_text(Some("Toggle changes panel"));
    inspector_btn.set_active(true);
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_title_widget(Some(&title));
    hb.pack_end(&inspector_btn);
    tv.add_top_bar(&hb);
    tv.set_content(Some(&outer));
    let view = Rc::new(View {
        outer,
        title,
        inspector_btn: inspector_btn.clone(),
        tab_bar,
        control_bar,
        content,
        inspector_bin: inspector_bin.clone(),
        paned: paned.clone(),
        empty,
        tab_widgets: RefCell::new(HashMap::new()),
        inspectors: RefCell::new(HashMap::new()),
        terminal_overlays: RefCell::new(HashMap::new()),
    });
    let ib = inspector_bin.clone();
    inspector_btn.connect_toggled(move |b| {
        app::get().claude.show_inspector.set(b.is_active());
        ib.set_visible(b.is_active());
    });
    let w = Rc::downgrade(&view);
    paned.connect_realize(move |p| {
        if w.upgrade().is_some() {
            let width = p.width();
            if width > 900 {
                p.set_position(width - 380);
            }
        }
    });
    let w = Rc::downgrade(&view);
    app::get().claude.changed.connect(move |e| match w.upgrade() {
        Some(v) => {
            match e {
                WsEvent::TabState => {
                    let ws = app::get().claude.clone();
                    if let Some(tab) = ws.selected_tab() {
                        v.render_tab_bar();
                        v.render_control_bar(&tab);
                        if tab.chat.is_none() {
                            v.update_terminal_card(&tab);
                        }
                    }
                }
                _ => v.render_all(),
            }
            true
        }
        None => false,
    });
    let w = Rc::downgrade(&view);
    accounts::store().changed.connect(move |_| match w.upgrade() {
        Some(v) => {
            v.render_all();
            true
        }
        None => false,
    });
    let w = Rc::downgrade(&view);
    crate::theme::on_change(move |_| match w.upgrade() {
        Some(_) => {
            for t in app::get().claude.tabs.borrow().iter() {
                if t.has_terminal() {
                    apply_terminal_colors(&t.terminal());
                }
            }
            true
        }
        None => false,
    });
    view.render_all();
    let _ = &view.paned;
    keep_alive(&tv, view);
    tv.upcast()
}
