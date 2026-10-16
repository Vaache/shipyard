use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};
use gtk::glib;

use super::item_row::{item_menu, item_row};
use super::menu::context_menu;
use super::widgets::*;
use crate::api::models::{Route, SearchItem};
use crate::app;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StateFilter {
    Open,
    Closed,
    All,
}

impl StateFilter {
    pub const ALL: [StateFilter; 3] = [StateFilter::Open, StateFilter::Closed, StateFilter::All];

    pub fn title(&self) -> &'static str {
        match self {
            StateFilter::Open => "Open",
            StateFilter::Closed => "Closed",
            StateFilter::All => "All",
        }
    }

    pub fn query(&self) -> &'static str {
        match self {
            StateFilter::Open => "is:open",
            StateFilter::Closed => "is:closed",
            StateFilter::All => "",
        }
    }
}

#[derive(Clone)]
struct Snapshot {
    items: Vec<SearchItem>,
    total: i64,
    cursor: Option<String>,
    has_more: bool,
    updated: DateTime<Utc>,
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Snapshot>> = RefCell::new(HashMap::new());
}

pub struct SearchModel {
    pub items: RefCell<Vec<SearchItem>>,
    pub total: Cell<i64>,
    pub loading: Cell<bool>,
    pub refreshing: Cell<bool>,
    pub last_updated: Cell<Option<DateTime<Utc>>>,
    pub error: RefCell<Option<String>>,
    cursor: RefCell<Option<String>>,
    has_more: Cell<bool>,
    loading_more: Cell<bool>,
    current_query: RefCell<String>,
    generation: Cell<u64>,
    pub changed: crate::signal::Signal<()>,
}

impl SearchModel {
    pub fn new() -> Rc<Self> {
        Rc::new(SearchModel {
            items: RefCell::new(vec![]),
            total: Cell::new(0),
            loading: Cell::new(false),
            refreshing: Cell::new(false),
            last_updated: Cell::new(None),
            error: RefCell::new(None),
            cursor: RefCell::new(None),
            has_more: Cell::new(false),
            loading_more: Cell::new(false),
            current_query: RefCell::new(String::new()),
            generation: Cell::new(0),
            changed: crate::signal::Signal::new(),
        })
    }

    fn apply(&self, snap: &Snapshot) {
        *self.items.borrow_mut() = snap.items.clone();
        self.total.set(snap.total);
        *self.cursor.borrow_mut() = snap.cursor.clone();
        self.has_more.set(snap.has_more);
        self.last_updated.set(Some(snap.updated));
    }

    pub async fn load(self: Rc<Self>, query: String, silent: bool) {
        let Some(api) = app::api() else { return };
        let mut switched = false;
        if *self.current_query.borrow() != query {
            switched = true;
            *self.current_query.borrow_mut() = query.clone();
            *self.error.borrow_mut() = None;
            match CACHE.with(|c| c.borrow().get(&query).cloned()) {
                Some(snap) => self.apply(&snap),
                None => {
                    self.items.borrow_mut().clear();
                    self.total.set(0);
                    self.last_updated.set(None);
                }
            }
        }
        let gen = self.generation.get() + 1;
        self.generation.set(gen);
        if !silent || switched {
            self.loading.set(true);
        }
        self.refreshing.set(true);
        self.changed.emit(&());
        let q = query.clone();
        let result = crate::rt::run(async move { api.search(&q, None).await }).await;
        if gen != self.generation.get() {
            return;
        }
        match result {
            Ok(r) => {
                let mut merged = r.nodes.clone();
                let mut next_cursor = r.page_info.as_ref().and_then(|p| p.end_cursor.clone());
                let mut more = r.page_info.as_ref().map(|p| p.has_next_page).unwrap_or(false);
                let current_len = self.items.borrow().len();
                if !switched && current_len > r.nodes.len() {
                    let fresh: HashSet<String> = r.nodes.iter().map(|n| n.url.clone()).collect();
                    let rest: Vec<SearchItem> = self.items.borrow().iter().skip(r.nodes.len()).filter(|i| !fresh.contains(&i.url)).cloned().collect();
                    merged.extend(rest);
                    next_cursor = self.cursor.borrow().clone();
                    more = self.has_more.get();
                }
                let snap = Snapshot { items: merged, total: r.issue_count, cursor: next_cursor, has_more: more, updated: Utc::now() };
                CACHE.with(|c| c.borrow_mut().insert(query, snap.clone()));
                self.apply(&snap);
                *self.error.borrow_mut() = None;
            }
            Err(e) => {
                *self.error.borrow_mut() = Some(e.message);
            }
        }
        self.loading.set(false);
        self.refreshing.set(false);
        self.changed.emit(&());
    }

    pub async fn load_more(self: Rc<Self>) -> bool {
        let Some(api) = app::api() else { return false };
        let cursor = self.cursor.borrow().clone();
        if !self.has_more.get() || self.loading_more.get() || cursor.is_none() {
            return false;
        }
        let query = self.current_query.borrow().clone();
        self.loading_more.set(true);
        let q = query.clone();
        let result = crate::rt::run(async move { api.search(&q, cursor).await }).await;
        self.loading_more.set(false);
        let Ok(r) = result else { return false };
        if *self.current_query.borrow() != query {
            return false;
        }
        let known: HashSet<String> = self.items.borrow().iter().map(|i| i.url.clone()).collect();
        let added: Vec<SearchItem> = r.nodes.into_iter().filter(|n| !known.contains(&n.url)).collect();
        let any = !added.is_empty();
        self.items.borrow_mut().extend(added);
        *self.cursor.borrow_mut() = r.page_info.as_ref().and_then(|p| p.end_cursor.clone());
        self.has_more.set(r.page_info.as_ref().map(|p| p.has_next_page).unwrap_or(false));
        let snap = Snapshot {
            items: self.items.borrow().clone(),
            total: self.total.get(),
            cursor: self.cursor.borrow().clone(),
            has_more: self.has_more.get(),
            updated: self.last_updated.get().unwrap_or_else(Utc::now),
        };
        CACHE.with(|c| c.borrow_mut().insert(query, snap));
        self.changed.emit(&());
        any
    }

    pub async fn load_all(self: Rc<Self>, limit: usize) {
        while self.has_more.get() && self.items.borrow().len() < limit {
            if !self.clone().load_more().await {
                return;
            }
        }
    }
}

pub fn state_toggles(on_change: impl Fn(StateFilter) + 'static) -> (gtk::Box, Rc<Cell<StateFilter>>) {
    let b = hbox(0);
    b.add_css_class("linked");
    let current = Rc::new(Cell::new(StateFilter::Open));
    let on_change = Rc::new(on_change);
    let mut first: Option<gtk::ToggleButton> = None;
    for f in StateFilter::ALL {
        let t = gtk::ToggleButton::with_label(f.title());
        t.set_hexpand(true);
        if let Some(g) = &first {
            t.set_group(Some(g));
        } else {
            t.set_active(true);
            first = Some(t.clone());
        }
        let cur = current.clone();
        let cb = on_change.clone();
        t.connect_toggled(move |t| {
            if t.is_active() && cur.get() != f {
                cur.set(f);
                cb(f);
            }
        });
        b.append(&t);
    }
    (b, current)
}

pub struct SearchList {
    kind: &'static str,
    base_query: String,
    grouped: bool,
    model: Rc<SearchModel>,
    list: gtk::ListBox,
    keys: RefCell<Vec<Option<Route>>>,
    collapsed: RefCell<HashSet<String>>,
    text: RefCell<String>,
    state: Rc<Cell<StateFilter>>,
    title: adw::WindowTitle,
    spinner: gtk::Spinner,
    status: Rc<RefreshStatus>,
    overlay_stack: gtk::Stack,
    debounce: RefCell<Option<glib::SourceId>>,
}

impl SearchList {
    pub fn build(title: &str, kind: &'static str, base_query: String, grouped: bool) -> gtk::Widget {
        let model = SearchModel::new();
        let list = gtk::ListBox::new();
        list.add_css_class("sy-list");
        list.set_selection_mode(gtk::SelectionMode::Single);
        let title_w = adw::WindowTitle::new(title, "");
        let spinner = spinner(14);
        spinner.set_visible(false);
        let overlay_stack = gtk::Stack::new();
        let (toggles, state) = state_toggles(|_| {});
        let model_c = model.clone();
        let weak_holder: Rc<RefCell<Option<std::rc::Weak<SearchList>>>> = Rc::new(RefCell::new(None));
        let wh = weak_holder.clone();
        let status = RefreshStatus::new(false, move || {
            if let Some(me) = wh.borrow().as_ref().and_then(|w| w.upgrade()) {
                me.reload(true);
            }
        });
        let me = Rc::new(SearchList {
            kind,
            base_query,
            grouped,
            model: model_c,
            list: list.clone(),
            keys: RefCell::new(vec![]),
            collapsed: RefCell::new(HashSet::new()),
            text: RefCell::new(String::new()),
            state,
            title: title_w.clone(),
            spinner: spinner.clone(),
            status,
            overlay_stack: overlay_stack.clone(),
            debounce: RefCell::new(None),
        });
        *weak_holder.borrow_mut() = Some(Rc::downgrade(&me));

        let weak = Rc::downgrade(&me);
        for child in toggles_children(&toggles) {
            let weak = weak.clone();
            child.connect_toggled(move |t| {
                if t.is_active() {
                    if let Some(me) = weak.upgrade() {
                        glib::idle_add_local_once(move || me.reload(false));
                    }
                }
            });
        }

        let search = gtk::SearchEntry::new();
        search.set_placeholder_text(Some("Filter: text, repo:org/name, label:bug"));
        search.set_hexpand(true);
        let weak = Rc::downgrade(&me);
        search.connect_search_changed(move |e| {
            let Some(me) = weak.upgrade() else { return };
            *me.text.borrow_mut() = e.text().trim().to_string();
            if let Some(id) = me.debounce.borrow_mut().take() {
                id.remove();
            }
            let weak = Rc::downgrade(&me);
            let id = glib::timeout_add_local_once(std::time::Duration::from_millis(350), move || {
                if let Some(me) = weak.upgrade() {
                    *me.debounce.borrow_mut() = None;
                    me.reload(false);
                }
            });
            *me.debounce.borrow_mut() = Some(id);
        });

        let weak = Rc::downgrade(&me);
        list.connect_row_activated(move |_, row| {
            let Some(me) = weak.upgrade() else { return };
            let idx = row.index();
            if idx < 0 {
                return;
            }
            let key = me.keys.borrow().get(idx as usize).cloned().flatten();
            match key {
                Some(route) => app::select(Some(route)),
                None => {
                    let name = row.widget_name().to_string();
                    if let Some(group) = name.strip_prefix("group:") {
                        let g = group.to_string();
                        let mut c = me.collapsed.borrow_mut();
                        if !c.remove(&g) {
                            c.insert(g);
                        }
                        drop(c);
                        me.render();
                    }
                }
            }
        });

        let scroll = scrolled(&list);
        let weak = Rc::downgrade(&me);
        scroll.connect_edge_reached(move |_, pos| {
            if pos == gtk::PositionType::Bottom {
                if let Some(me) = weak.upgrade() {
                    if !me.grouped {
                        let m = me.model.clone();
                        crate::rt::spawn_local(async move {
                            m.load_more().await;
                        });
                    }
                }
            }
        });
        overlay_stack.add_named(&scroll, Some("list"));
        overlay_stack.add_named(&loading(), Some("loading"));
        let empty = label_with("Nothing here", &["sy-muted"]);
        empty.set_valign(gtk::Align::Start);
        empty.set_halign(gtk::Align::Center);
        empty.set_margin_top(30);
        overlay_stack.add_named(&empty, Some("empty"));
        let err_box = vbox(8);
        err_box.set_widget_name("error");
        overlay_stack.add_named(&err_box, Some("error"));

        let toggles_bar = hbox(0);
        margins(&toggles_bar, 6, 10, 6, 10);
        toggles.set_hexpand(true);
        toggles_bar.append(&toggles);

        let content = vbox(0);
        content.append(&toggles_bar);
        content.append(&line());
        content.append(&overlay_stack);
        content.append(&footer(&me.status));

        let tv = adw::ToolbarView::new();
        let hb = adw::HeaderBar::new();
        hb.set_show_end_title_buttons(false);
        hb.set_title_widget(Some(&title_w));
        hb.pack_end(&spinner);
        let search_bar = gtk::SearchBar::new();
        search_bar.set_child(Some(&search));
        search_bar.connect_entry(&search);
        let search_btn = gtk::ToggleButton::new();
        search_btn.set_icon_name("system-search-symbolic");
        search_btn.set_tooltip_text(Some("Filter"));
        search_btn.bind_property("active", &search_bar, "search-mode-enabled").bidirectional().build();
        hb.pack_start(&search_btn);
        tv.add_top_bar(&hb);
        tv.add_top_bar(&search_bar);
        tv.set_content(Some(&content));

        let weak = Rc::downgrade(&me);
        me.model.changed.connect(move |_| match weak.upgrade() {
            Some(me) => {
                me.render();
                true
            }
            None => false,
        });
        let weak = Rc::downgrade(&me);
        app::on_refresh(move |_| match weak.upgrade() {
            Some(me) => {
                me.reload(false);
                true
            }
            None => false,
        });
        let weak = Rc::downgrade(&me);
        app::on_live(move |_| match weak.upgrade() {
            Some(me) => {
                me.reload(true);
                true
            }
            None => false,
        });
        let weak = Rc::downgrade(&me);
        app::get().route_changed.connect(move |_| match weak.upgrade() {
            Some(me) => {
                me.sync_selection();
                true
            }
            None => false,
        });
        me.reload(false);
        keep_alive(&tv, me);
        tv.upcast()
    }

    fn query(&self) -> String {
        let state = self.state.get();
        let parts = [self.base_query.clone(), format!("is:{}", self.kind), state.query().to_string(), "archived:false".into(), "sort:updated-desc".into(), self.text.borrow().clone()];
        parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" ")
    }

    fn reload(self: &Rc<Self>, silent: bool) {
        let q = self.query();
        let m = self.model.clone();
        let grouped = self.grouped;
        crate::rt::spawn_local(async move {
            m.clone().load(q, silent).await;
            if grouped {
                m.load_all(400).await;
            }
        });
    }

    fn render(&self) {
        let _span = crate::diagnostics::ui_span("list.render");
        let m = &self.model;
        let total = m.total.get();
        self.title.set_subtitle(&if total > 0 { total.to_string() } else { String::new() });
        let items = m.items.borrow().clone();
        let loading = m.loading.get();
        self.spinner.set_visible(loading && !items.is_empty());
        self.spinner.set_spinning(loading);
        self.list.set_opacity(if loading && !items.is_empty() { 0.55 } else { 1.0 });
        let err = m.error.borrow().clone();
        self.status.update(m.last_updated.get(), m.refreshing.get(), if items.is_empty() { None } else { err.clone() });
        if items.is_empty() {
            if loading {
                self.overlay_stack.set_visible_child_name("loading");
            } else if let Some(e) = err {
                if let Some(page) = self.overlay_stack.child_by_name("error").and_downcast::<gtk::Box>() {
                    clear(&page);
                    let l = wrap_label(&e, &["fg-red", "sy-small"]);
                    pad(&l, 12);
                    page.append(&l);
                }
                self.overlay_stack.set_visible_child_name("error");
            } else {
                self.overlay_stack.set_visible_child_name("empty");
            }
            self.list.remove_all();
            self.keys.borrow_mut().clear();
            return;
        }
        self.overlay_stack.set_visible_child_name("list");
        let scroll_pos = self.list.parent().and_then(|p| p.parent()).and_downcast::<gtk::ScrolledWindow>().map(|s| s.vadjustment().value());
        self.list.remove_all();
        let mut keys = vec![];
        if self.grouped {
            let mut order: Vec<String> = vec![];
            let mut by_repo: HashMap<String, Vec<SearchItem>> = HashMap::new();
            for item in &items {
                let k = item.repository.name_with_owner.clone();
                if !by_repo.contains_key(&k) {
                    order.push(k.clone());
                }
                by_repo.entry(k).or_default().push(item.clone());
            }
            let text_empty = self.text.borrow().is_empty();
            for key in order {
                let group = by_repo.remove(&key).unwrap_or_default();
                let open = !self.collapsed.borrow().contains(&key) || !text_empty;
                let row = gtk::ListBoxRow::new();
                row.set_widget_name(&format!("group:{key}"));
                row.set_child(Some(&group_header(&key, group.len(), open)));
                row.set_selectable(false);
                self.list.append(&row);
                keys.push(None);
                if open {
                    for item in group {
                        self.append_item(&item, false);
                        keys.push(Some(item.route()));
                    }
                }
            }
        } else {
            for item in &items {
                self.append_item(item, true);
                keys.push(Some(item.route()));
            }
        }
        *self.keys.borrow_mut() = keys;
        self.sync_selection();
        if let Some(v) = scroll_pos {
            if let Some(s) = self.list.parent().and_then(|p| p.parent()).and_downcast::<gtk::ScrolledWindow>() {
                let adj = s.vadjustment();
                glib::idle_add_local_once(move || adj.set_value(v));
            }
        }
    }

    fn append_item(&self, item: &SearchItem, show_repo: bool) {
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&item_row(item, show_repo)));
        let it = item.clone();
        context_menu(&row, move || item_menu(&it));
        self.list.append(&row);
    }

    fn sync_selection(&self) {
        let route = app::get().state.borrow().route.clone();
        let keys = self.keys.borrow();
        match route.and_then(|r| keys.iter().position(|k| k.as_ref() == Some(&r))) {
            Some(i) => {
                if let Some(row) = self.list.row_at_index(i as i32) {
                    self.list.select_row(Some(&row));
                }
            }
            None => self.list.unselect_all(),
        }
    }
}

fn toggles_children(b: &gtk::Box) -> Vec<gtk::ToggleButton> {
    let mut out = vec![];
    let mut c = b.first_child();
    while let Some(w) = c {
        if let Ok(t) = w.clone().downcast::<gtk::ToggleButton>() {
            out.push(t);
        }
        c = w.next_sibling();
    }
    out
}

pub fn group_header(full: &str, count: usize, expanded: bool) -> gtk::Box {
    let color = crate::theme::palette_for(full);
    let (owner, name) = full.split_once('/').unwrap_or(("", full));
    let b = hbox(7);
    margins(&b, 3, 0, 3, 0);
    b.append(&icon(if expanded { "pan-down-symbolic" } else { "pan-end-symbolic" }, 12));
    b.append(&icon_tile("sy-repo-symbolic", color, 24));
    let texts = vbox(0);
    texts.set_hexpand(true);
    texts.append(&ellipsized(name, &["sy-bold", "sy-text"]));
    texts.append(&ellipsized(owner, &["sy-dim", "sy-tiny"]));
    b.append(&texts);
    b.append(&chip(&count.to_string(), color, None, false));
    b.set_tooltip_text(Some(full));
    b
}
