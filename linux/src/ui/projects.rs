use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};
use gtk::{gdk, glib};

use super::menu::{context_menu, Menu};
use super::widgets::*;
use crate::api::models::*;
use crate::{app, auth, util};

struct ProjectsList {
    projects: RefCell<Vec<ProjectSummary>>,
    loading: Cell<bool>,
    error: RefCell<Option<String>>,
    show_closed: Cell<bool>,
    updated: Cell<Option<DateTime<Utc>>>,
    list: gtk::ListBox,
    keys: RefCell<Vec<ProjectRef>>,
    hint: gtk::Box,
    status: Rc<RefreshStatus>,
    stack: gtk::Stack,
}

fn scope_hint() -> gtk::Box {
    let b = vbox(8);
    b.add_css_class("banner-box");
    b.add_css_class("amber");
    margins(&b, 8, 8, 8, 8);
    let top = hbox(8);
    top.append(&icon_tile("sy-key-symbolic", "amber", 22));
    top.append(&label_with("Token is missing Projects scopes", &["sy-bold", "sy-text"]));
    b.append(&top);
    b.append(&wrap_label("Run this in a terminal, then click “Refresh Token from gh”:", &["sy-muted", "sy-small"]));
    let cmd = selectable(label_with("gh auth refresh -s read:project,project", &["sy-mono", "sy-text"]));
    let cmdbox = frame(&cmd, "sy-sunken");
    pad(&cmd, 6);
    b.append(&cmdbox);
    let row = hbox(6);
    let copy = button("Copy Command");
    copy.connect_clicked(|_| util::copy("gh auth refresh -s read:project,project"));
    let refresh = button("Refresh Token from gh");
    refresh.connect_clicked(|b| {
        b.set_sensitive(false);
        let b = b.clone();
        crate::rt::spawn_local(async move {
            if let Ok(t) = auth::gh_token().await {
                let _ = app::get().login(t, true).await;
            }
            b.set_sensitive(true);
            app::get().user_refresh();
        });
    });
    row.append(&copy);
    row.append(&refresh);
    b.append(&row);
    b
}

fn project_row(p: &ProjectSummary) -> gtk::Box {
    let b = hbox(9);
    margins(&b, 4, 0, 4, 0);
    let tile = icon_tile("sy-project-symbolic", if p.closed { "slate" } else { "indigo" }, 22);
    tile.set_valign(gtk::Align::Start);
    b.append(&tile);
    let col = vbox(3);
    col.set_hexpand(true);
    let top = hbox(5);
    top.append(&ellipsized(&p.title, &["sy-bold", "sy-text"]));
    if p.closed {
        top.append(&chip("closed", "slate", None, false));
    }
    col.append(&top);
    if let Some(d) = p.short_description.as_deref().filter(|d| !d.is_empty()) {
        let l = wrap_label(d, &["sy-muted", "sy-small"]);
        l.set_lines(2);
        l.set_ellipsize(gtk::pango::EllipsizeMode::End);
        col.append(&l);
    }
    let meta = hbox(6);
    meta.append(&label_with(&format!("{} · #{}", p.owner.as_ref().and_then(|o| o.login.clone()).unwrap_or_default(), p.number), &["sy-muted", "sy-small"]));
    meta.append(&label_with("·", &["sy-dim", "sy-small"]));
    meta.append(&label_with(&util::plural(p.items.total_count, "item", "items"), &["fg-indigo", "sy-small"]));
    meta.append(&spacer());
    meta.append(&label_with(&util::rel(Some(p.updated_at)), &["sy-dim", "sy-small"]));
    col.append(&meta);
    b.append(&col);
    b
}

impl ProjectsList {
    fn render(self: &Rc<Self>) {
        let err = self.error.borrow().clone();
        self.hint.set_visible(!app::has_project_scope() || err.as_deref().map(|e| e.contains("scope")).unwrap_or(false));
        let visible: Vec<ProjectSummary> = self.projects.borrow().iter().filter(|p| self.show_closed.get() || !p.closed).cloned().collect();
        self.status.update(self.updated.get(), self.loading.get(), if visible.is_empty() { None } else { err.clone() });
        self.list.remove_all();
        let mut keys = vec![];
        if visible.is_empty() {
            if self.loading.get() {
                self.stack.set_visible_child_name("loading");
            } else {
                if let Some(l) = self.stack.child_by_name("empty").and_downcast::<gtk::Label>() {
                    l.set_text(match &err {
                        Some(e) if !self.hint.is_visible() => e,
                        _ => "No projects",
                    });
                }
                self.stack.set_visible_child_name("empty");
            }
            return;
        }
        self.stack.set_visible_child_name("list");
        for p in visible {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&project_row(&p)));
            let url = p.url.clone();
            context_menu(&row, move || {
                let m = Menu::ctx();
                let u = url.clone();
                m.item("Open in Browser", move || util::open_url(&u));
                m
            });
            self.list.append(&row);
            keys.push(p.project_ref());
        }
        *self.keys.borrow_mut() = keys;
    }

    fn load(self: &Rc<Self>) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        me.loading.set(true);
        me.render();
        crate::rt::spawn_local(async move {
            let res = crate::rt::run(async move { api.projects().await }).await;
            me.loading.set(false);
            match res {
                Ok(p) => {
                    *me.projects.borrow_mut() = p;
                    me.updated.set(Some(Utc::now()));
                    *me.error.borrow_mut() = None;
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.render();
        });
    }
}

pub fn list() -> gtk::Widget {
    let lb = gtk::ListBox::new();
    lb.add_css_class("sy-list");
    let hint = scope_hint();
    hint.set_visible(false);
    let stack = gtk::Stack::new();
    stack.add_named(&scrolled(&lb), Some("list"));
    stack.add_named(&loading(), Some("loading"));
    let empty = label_with("No projects", &["sy-muted"]);
    empty.set_valign(gtk::Align::Start);
    empty.set_halign(gtk::Align::Center);
    empty.set_margin_top(30);
    empty.set_wrap(true);
    stack.add_named(&empty, Some("empty"));
    stack.set_vexpand(true);
    let holder: Rc<RefCell<Option<std::rc::Weak<ProjectsList>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(v) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            v.load();
        }
    });
    let me = Rc::new(ProjectsList {
        projects: RefCell::new(vec![]),
        loading: Cell::new(false),
        error: RefCell::new(None),
        show_closed: Cell::new(false),
        updated: Cell::new(None),
        list: lb.clone(),
        keys: RefCell::new(vec![]),
        hint: hint.clone(),
        status,
        stack: stack.clone(),
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let w = Rc::downgrade(&me);
    lb.connect_row_activated(move |_, row| {
        if let Some(me) = w.upgrade() {
            if let Some(p) = me.keys.borrow().get(row.index().max(0) as usize).cloned() {
                app::select(Some(Route::Project(p)));
            }
        }
    });
    let closed = gtk::ToggleButton::new();
    closed.set_icon_name("sy-archive-symbolic");
    closed.set_tooltip_text(Some("Show closed"));
    let w = Rc::downgrade(&me);
    closed.connect_toggled(move |t| {
        if let Some(me) = w.upgrade() {
            me.show_closed.set(t.is_active());
            me.render();
        }
    });
    let content = vbox(0);
    content.append(&hint);
    content.append(&stack);
    content.append(&footer(&me.status));
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_end_title_buttons(false);
    hb.set_title_widget(Some(&adw::WindowTitle::new("Projects", "")));
    hb.pack_end(&closed);
    tv.add_top_bar(&hb);
    tv.set_content(Some(&content));
    for sig in [0, 1] {
        let w = Rc::downgrade(&me);
        let f = move |_: &()| match w.upgrade() {
            Some(me) => {
                me.load();
                true
            }
            None => false,
        };
        if sig == 0 {
            app::on_refresh(f);
        } else {
            app::on_live(f);
        }
    }
    me.load();
    keep_alive(&tv, me);
    tv.upcast()
}

struct Board {
    project: ProjectRef,
    detail: RefCell<Option<ProjectDetail>>,
    items: RefCell<Vec<ProjectItem>>,
    field_id: RefCell<Option<String>>,
    loading: Cell<bool>,
    error: RefCell<Option<String>>,
    updated: Cell<Option<DateTime<Utc>>>,
    filter: RefCell<String>,
    columns: gtk::Box,
    group_picker: gtk::DropDown,
    group_ids: RefCell<Vec<String>>,
    count_chip: adw::Bin,
    stack: gtk::Stack,
    status: Rc<RefreshStatus>,
    syncing: Cell<bool>,
}

struct Column {
    option_id: Option<String>,
    name: String,
    color: String,
    items: Vec<ProjectItem>,
}

impl Board {
    fn group_fields(&self) -> Vec<ProjectField> {
        self.detail.borrow().as_ref().map(|d| d.fields.nodes.iter().filter(|f| f.options.is_some()).cloned().collect()).unwrap_or_default()
    }

    fn field(&self) -> Option<ProjectField> {
        let id = self.field_id.borrow().clone()?;
        self.group_fields().into_iter().find(|f| f.id == id)
    }

    fn columns_for(&self) -> Vec<Column> {
        let filter = self.filter.borrow().clone();
        let items: Vec<ProjectItem> = self
            .items
            .borrow()
            .iter()
            .filter(|i| {
                filter.is_empty()
                    || util::contains_ci(i.content.as_ref().and_then(|c| c.title.as_deref()).unwrap_or(""), &filter)
                    || util::contains_ci(i.content.as_ref().and_then(|c| c.repository.as_ref()).map(|r| r.name_with_owner.as_str()).unwrap_or(""), &filter)
            })
            .cloned()
            .collect();
        let (Some(field), Some(field_id)) = (self.field(), self.field_id.borrow().clone()) else {
            return vec![Column { option_id: None, name: "All".into(), color: "GRAY".into(), items }];
        };
        let options = field.options.clone().unwrap_or_default();
        let known: std::collections::HashSet<String> = options.iter().map(|o| o.id.clone()).collect();
        let mut cols = vec![];
        let none: Vec<ProjectItem> = items.iter().filter(|i| i.option_id(&field_id).map(|o| !known.contains(&o)).unwrap_or(true)).cloned().collect();
        if !none.is_empty() {
            cols.push(Column { option_id: None, name: format!("No {}", field.name), color: "GRAY".into(), items: none });
        }
        for o in options {
            let its = items.iter().filter(|i| i.option_id(&field_id).as_deref() == Some(o.id.as_str())).cloned().collect();
            cols.push(Column { option_id: Some(o.id.clone()), name: o.name.clone(), color: o.color.clone(), items: its });
        }
        cols
    }

    fn render(self: &Rc<Self>) {
        self.status.update(self.updated.get(), self.loading.get(), self.error.borrow().clone());
        if self.detail.borrow().is_none() {
            if let Some(e) = self.error.borrow().clone() {
                let w = Rc::downgrade(self);
                let err = error_state(
                    &e,
                    Some(Box::new(move || {
                        if let Some(me) = w.upgrade() {
                            me.load();
                        }
                    })),
                );
                if let Some(old) = self.stack.child_by_name("error") {
                    self.stack.remove(&old);
                }
                self.stack.add_named(&err, Some("error"));
                self.stack.set_visible_child_name("error");
            } else {
                self.stack.set_visible_child_name("loading");
            }
            return;
        }
        self.stack.set_visible_child_name("board");
        let fields = self.group_fields();
        self.syncing.set(true);
        let names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        self.group_picker.set_model(Some(&gtk::StringList::new(&names)));
        *self.group_ids.borrow_mut() = fields.iter().map(|f| f.id.clone()).collect();
        if let Some(id) = self.field_id.borrow().as_ref() {
            if let Some(i) = fields.iter().position(|f| &f.id == id) {
                self.group_picker.set_selected(i as u32);
            }
        }
        self.group_picker.set_visible(fields.len() > 1);
        self.syncing.set(false);
        self.count_chip.set_child(Some(&chip(&util::plural(self.items.borrow().len() as i64, "item", "items"), "indigo", Some("sy-stack-symbolic"), false)));
        clear(&self.columns);
        let options = self.field().and_then(|f| f.options).unwrap_or_default();
        for col in self.columns_for() {
            self.columns.append(&self.column_widget(col, &options));
        }
    }

    fn column_widget(self: &Rc<Self>, col: Column, options: &[ProjectOption]) -> gtk::Box {
        let color = crate::theme::project_color(&col.color);
        let b = vbox(0);
        b.add_css_class("project-column");
        b.set_size_request(290, -1);
        b.set_vexpand(true);
        let edge = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        edge.set_size_request(-1, 3);
        edge.add_css_class(&format!("edge-{color}"));
        b.append(&edge);
        let head = hbox(6);
        pad(&head, 10);
        head.append(&colored_icon("sy-dot-symbolic", 10, color));
        head.append(&ellipsized(&col.name, &["sy-bold", "sy-text"]));
        head.append(&chip(&col.items.len().to_string(), color, None, false));
        b.append(&head);
        let cards = vbox(8);
        margins(&cards, 0, 8, 8, 8);
        for item in &col.items {
            cards.append(&self.card(item, options));
        }
        let scroll = scrolled(&cards);
        b.append(&scroll);
        let target = gtk::DropTarget::new(glib::Type::STRING, gdk::DragAction::MOVE);
        let w = Rc::downgrade(self);
        let option = col.option_id.clone();
        let bb = b.clone();
        target.connect_drop(move |_, value, _, _| {
            bb.remove_css_class("drop-target");
            let Some(me) = w.upgrade() else { return false };
            if me.field_id.borrow().is_none() {
                return false;
            }
            if let Ok(id) = value.get::<String>() {
                let me = me.clone();
                let opt = option.clone();
                crate::rt::spawn_local(async move { me.move_item(id, opt).await });
                return true;
            }
            false
        });
        let bb = b.clone();
        target.connect_enter(move |_, _, _| {
            bb.add_css_class("drop-target");
            gdk::DragAction::MOVE
        });
        let bb = b.clone();
        target.connect_leave(move |_| bb.remove_css_class("drop-target"));
        b.add_controller(target);
        b
    }

    fn card(self: &Rc<Self>, item: &ProjectItem, options: &[ProjectOption]) -> gtk::Box {
        let c = item.content.clone();
        let card = vbox(5);
        card.add_css_class("project-card");
        let top = hbox(5);
        match c.as_ref().and_then(|c| c.visual_state()) {
            Some(s) => top.append(&colored_icon(s.icon(), 12, s.css())),
            None => top.append(&colored_icon("sy-file-symbolic", 12, "dim")),
        }
        let where_ = c
            .as_ref()
            .and_then(|c| c.repository.as_ref().map(|r| format!("{}{}", r.name_with_owner, c.number.map(|n| format!("#{n}")).unwrap_or_default())))
            .unwrap_or_else(|| "Draft".into());
        let w = ellipsized(&where_, &["sy-muted", "sy-small"]);
        w.set_hexpand(true);
        top.append(&w);
        for a in c.as_ref().and_then(|c| c.assignees.as_ref()).map(|a| a.nodes.clone()).unwrap_or_default() {
            top.append(&tooltip(avatar(a.avatar_url.as_deref(), 16, None), &a.login));
        }
        card.append(&top);
        let title = wrap_label(c.as_ref().and_then(|c| c.title.as_deref()).unwrap_or("—"), &["sy-text"]);
        title.set_lines(3);
        title.set_ellipsize(gtk::pango::EllipsizeMode::End);
        card.append(&title);
        if let Some(route) = c.as_ref().and_then(|c| c.route()) {
            on_click_widget(&card, move || app::open(route.clone(), true));
        }
        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        let id = item.id.clone();
        drag.connect_prepare(move |_, _, _| Some(gdk::ContentProvider::for_value(&id.to_value())));
        card.add_controller(drag);
        let w = Rc::downgrade(self);
        let opts: Vec<ProjectOption> = options.to_vec();
        let id = item.id.clone();
        let url = c.as_ref().and_then(|c| c.url.clone());
        context_menu(&card, move || {
            let m = Menu::ctx();
            let w2 = w.clone();
            let id2 = id.clone();
            let opts2 = opts.clone();
            m.submenu("Move to", move |sub| {
                for o in &opts2 {
                    let w3 = w2.clone();
                    let id3 = id2.clone();
                    let oid = o.id.clone();
                    sub.item(&o.name, move || {
                        if let Some(me) = w3.upgrade() {
                            let id = id3.clone();
                            let oid = oid.clone();
                            crate::rt::spawn_local(async move { me.move_item(id, Some(oid)).await });
                        }
                    });
                }
                let w3 = w2.clone();
                let id3 = id2.clone();
                sub.item("No Status", move || {
                    if let Some(me) = w3.upgrade() {
                        let id = id3.clone();
                        crate::rt::spawn_local(async move { me.move_item(id, None).await });
                    }
                });
            });
            if let Some(u) = url.clone() {
                let u2 = u.clone();
                m.item("Open in Browser", move || util::open_url(&u2));
                m.item("Copy Link", move || util::copy(&u));
            }
            m
        });
        card
    }

    async fn move_item(self: Rc<Self>, item_id: String, option: Option<String>) {
        let Some(field_id) = self.field_id.borrow().clone() else { return };
        let Some(api) = app::api() else { return };
        let idx = self.items.borrow().iter().position(|i| i.id == item_id);
        let Some(idx) = idx else { return };
        if self.items.borrow()[idx].option_id(&field_id) == option {
            return;
        }
        let backup = self.items.borrow()[idx].clone();
        {
            let mut items = self.items.borrow_mut();
            let it = &mut items[idx];
            let mut nodes: Vec<ProjectFieldValue> = it.field_values.nodes.iter().filter(|v| v.field.as_ref().and_then(|f| f.id.as_deref()) != Some(field_id.as_str())).cloned().collect();
            if let Some(o) = &option {
                nodes.push(ProjectFieldValue { typename: "ProjectV2ItemFieldSingleSelectValue".into(), option_id: Some(o.clone()), field: Some(FieldRef { id: Some(field_id.clone()) }) });
            }
            it.field_values = Nodes { nodes, total_count: None };
        }
        self.render();
        let pid = self.project.id.clone();
        let res = crate::rt::run(async move { api.move_project_item(&pid, &item_id, &field_id, option).await }).await;
        if let Err(e) = res {
            let mut items = self.items.borrow_mut();
            if let Some(i) = items.iter().position(|i| i.id == backup.id) {
                items[i] = backup;
            }
            drop(items);
            self.render();
            super::dialogs::error(&e.message);
        }
    }

    fn load(self: &Rc<Self>) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        me.loading.set(true);
        me.status.update(me.updated.get(), true, None);
        crate::rt::spawn_local(async move {
            let id = me.project.id.clone();
            let res = crate::rt::run(async move { api.project(&id).await }).await;
            me.loading.set(false);
            match res {
                Ok((d, items)) => {
                    *me.detail.borrow_mut() = Some(d);
                    let fields = me.group_fields();
                    let current = me.field_id.borrow().clone();
                    if current.is_none() || !fields.iter().any(|f| Some(&f.id) == current.as_ref()) {
                        *me.field_id.borrow_mut() = fields.iter().find(|f| f.name.to_lowercase() == "status").or(fields.first()).map(|f| f.id.clone());
                    }
                    *me.items.borrow_mut() = items.into_iter().filter(|i| !i.is_archived).collect();
                    me.updated.set(Some(Utc::now()));
                    *me.error.borrow_mut() = None;
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.render();
        });
    }
}

pub fn board_page(project: &ProjectRef) -> adw::NavigationPage {
    let columns = hbox(12);
    pad(&columns, 14);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_vscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_child(Some(&columns));
    scroll.set_vexpand(true);
    let stack = gtk::Stack::new();
    stack.add_named(&loading(), Some("loading"));
    let picker = gtk::DropDown::new(None::<gtk::StringList>, None::<gtk::Expression>);
    picker.set_tooltip_text(Some("Group by"));
    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some("Filter cards"));
    filter.set_width_chars(24);
    let count_chip = adw::Bin::new();
    let bar = hbox(8);
    margins(&bar, 8, 14, 8, 14);
    bar.append(&picker);
    bar.append(&filter);
    bar.append(&spacer());
    bar.append(&count_chip);
    let board_box = vbox(0);
    board_box.append(&bar);
    board_box.append(&line());
    board_box.append(&scroll);
    stack.add_named(&board_box, Some("board"));
    stack.set_vexpand(true);
    let holder: Rc<RefCell<Option<std::rc::Weak<Board>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(v) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            v.load();
        }
    });
    let me = Rc::new(Board {
        project: project.clone(),
        detail: RefCell::new(None),
        items: RefCell::new(vec![]),
        field_id: RefCell::new(None),
        loading: Cell::new(false),
        error: RefCell::new(None),
        updated: Cell::new(None),
        filter: RefCell::new(String::new()),
        columns,
        group_picker: picker.clone(),
        group_ids: RefCell::new(vec![]),
        count_chip,
        stack: stack.clone(),
        status,
        syncing: Cell::new(false),
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let w = Rc::downgrade(&me);
    picker.connect_selected_notify(move |p| {
        let Some(me) = w.upgrade() else { return };
        if me.syncing.get() {
            return;
        }
        let id = me.group_ids.borrow().get(p.selected() as usize).cloned();
        if id.is_some() {
            *me.field_id.borrow_mut() = id;
            me.render();
        }
    });
    let w = Rc::downgrade(&me);
    filter.connect_search_changed(move |e| {
        if let Some(me) = w.upgrade() {
            *me.filter.borrow_mut() = e.text().to_string();
            me.render();
        }
    });
    let content = vbox(0);
    content.append(&stack);
    content.append(&footer(&me.status));
    let (tv, hb, _) = toolbar_page(&project.title, "", &content);
    tv.add_css_class("sy-panel-pane");
    let open = icon_button("sy-external-symbolic", "Open in Browser");
    let url = project.url.clone();
    open.connect_clicked(move |_| util::open_url(&url));
    hb.pack_end(&open);
    for sig in [0, 1] {
        let w = Rc::downgrade(&me);
        let f = move |_: &()| match w.upgrade() {
            Some(me) => {
                me.load();
                true
            }
            None => false,
        };
        if sig == 0 {
            app::on_refresh(f);
        } else {
            app::on_live(f);
        }
    }
    me.load();
    keep_alive(&tv, me);
    adw::NavigationPage::new(&tv, &project.title)
}
