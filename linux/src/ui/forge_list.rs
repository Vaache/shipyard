use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};
use gtk::glib;

use super::item_row::{item_menu, item_row};
use super::lists::{state_toggles, StateFilter};
use super::menu::context_menu;
use super::widgets::*;
use crate::api::models::{Route, SearchItem};
use crate::app::{self, PrFilter};

thread_local! {
    static CACHE: RefCell<HashMap<String, (Vec<SearchItem>, DateTime<Utc>)>> = RefCell::new(HashMap::new());
}

pub fn reset_cache() {
    CACHE.with(|c| c.borrow_mut().clear());
}

fn cache_key(filter: PrFilter, state: StateFilter) -> String {
    format!("{}\n{}\n{}", app::cache_account(), filter.key(), state.title())
}

fn matches(item: &SearchItem, text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let text = text.to_lowercase();
    let author = item.author.as_ref().map(|a| a.login.as_str()).unwrap_or("");
    text.split_whitespace().all(|word| {
        if let Some(repo) = word.strip_prefix("repo:") {
            return item.repository.name_with_owner.to_lowercase().contains(repo);
        }
        if let Some(label) = word.strip_prefix("label:") {
            return item.labels().iter().any(|l| l.name.to_lowercase().contains(label));
        }
        item.title.to_lowercase().contains(word) || item.repository.name_with_owner.to_lowercase().contains(word) || author.to_lowercase().contains(word) || item.number.to_string() == word.trim_start_matches(['#', '!'])
    })
}

struct ForgeList {
    filter: PrFilter,
    state: Rc<Cell<StateFilter>>,
    items: RefCell<Vec<SearchItem>>,
    loading: Cell<bool>,
    refreshing: Cell<bool>,
    error: RefCell<Option<String>>,
    updated: Cell<Option<DateTime<Utc>>>,
    generation: Cell<u64>,
    text: RefCell<String>,
    list: gtk::ListBox,
    keys: RefCell<Vec<Route>>,
    title: adw::WindowTitle,
    spinner: gtk::Spinner,
    status: Rc<RefreshStatus>,
    stack: gtk::Stack,
    rendered: RefCell<String>,
}

impl ForgeList {
    fn reload(self: &Rc<Self>, silent: bool) {
        let Some(forge) = app::forge() else { return };
        let filter = self.filter;
        let state = self.state.get();
        let key = cache_key(filter, state);
        let gen = self.generation.get() + 1;
        self.generation.set(gen);
        if !silent {
            match CACHE.with(|c| c.borrow().get(&key).cloned()) {
                Some((items, at)) => {
                    *self.items.borrow_mut() = items;
                    self.updated.set(Some(at));
                }
                None => {
                    self.items.borrow_mut().clear();
                    self.updated.set(None);
                    self.loading.set(true);
                }
            }
            *self.error.borrow_mut() = None;
        }
        self.refreshing.set(true);
        self.render();
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let result = crate::rt::run(async move { forge.pull_requests(filter, state).await }).await;
            if gen != me.generation.get() {
                return;
            }
            match result {
                Ok(items) => {
                    super::forge_view::remember(&items);
                    let now = Utc::now();
                    CACHE.with(|c| c.borrow_mut().insert(key, (items.clone(), now)));
                    *me.items.borrow_mut() = items;
                    me.updated.set(Some(now));
                    *me.error.borrow_mut() = None;
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.loading.set(false);
            me.refreshing.set(false);
            me.render();
        });
    }

    fn render(&self) {
        let text = self.text.borrow().clone();
        let items: Vec<SearchItem> = self.items.borrow().iter().filter(|i| matches(i, &text)).cloned().collect();
        self.title.set_subtitle(&if items.is_empty() { String::new() } else { items.len().to_string() });
        let loading = self.loading.get();
        self.spinner.set_visible(self.refreshing.get() && !items.is_empty());
        let err = self.error.borrow().clone();
        self.status.update(self.updated.get(), self.refreshing.get(), if items.is_empty() { None } else { err.clone() });
        if items.is_empty() {
            if loading {
                self.stack.set_visible_child_name("loading");
            } else if let Some(e) = err {
                if let Some(page) = self.stack.child_by_name("error").and_downcast::<gtk::Box>() {
                    clear(&page);
                    let l = wrap_label(&e, &["fg-red", "sy-small"]);
                    pad(&l, 12);
                    page.append(&l);
                }
                self.stack.set_visible_child_name("error");
            } else {
                self.stack.set_visible_child_name("empty");
            }
            self.list.remove_all();
            self.keys.borrow_mut().clear();
            self.rendered.borrow_mut().clear();
            return;
        }
        self.stack.set_visible_child_name("list");
        let mut sig = serde_json::to_string(&items).unwrap_or_default();
        for item in &items {
            sig.push('|');
            sig.push_str(&crate::util::rel(Some(item.updated_at)));
        }
        if *self.rendered.borrow() != sig {
            *self.rendered.borrow_mut() = sig;
            rebuild_rows(&self.list, || {
                let mut keys = vec![];
                for item in &items {
                    let row = gtk::ListBoxRow::new();
                    row.set_child(Some(&item_row(item, true)));
                    let it = item.clone();
                    context_menu(&row, move || item_menu(&it));
                    self.list.append(&row);
                    keys.push(item.route());
                }
                *self.keys.borrow_mut() = keys;
            });
        }
        self.sync_selection();
    }

    fn sync_selection(&self) {
        let route = app::get().state.borrow().route.clone();
        let keys = self.keys.borrow();
        match route.and_then(|r| keys.iter().position(|k| *k == r)) {
            Some(i) => {
                if let Some(row) = self.list.row_at_index(i as i32) {
                    self.list.select_row(Some(&row));
                }
            }
            None => self.list.unselect_all(),
        }
    }
}

pub fn build(filter: PrFilter) -> gtk::Widget {
    let list = gtk::ListBox::new();
    list.add_css_class("sy-list");
    list.set_selection_mode(gtk::SelectionMode::Single);
    let title = adw::WindowTitle::new(filter.title(), "");
    let spinner = spinner(14);
    spinner.set_visible(false);
    let stack = gtk::Stack::new();
    let (toggles, state) = state_toggles(|_| {});
    let holder: Rc<RefCell<Option<std::rc::Weak<ForgeList>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(false, move || {
        if let Some(me) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            me.reload(false);
        }
    });
    let me = Rc::new(ForgeList {
        filter,
        state,
        items: RefCell::new(vec![]),
        loading: Cell::new(false),
        refreshing: Cell::new(false),
        error: RefCell::new(None),
        updated: Cell::new(None),
        generation: Cell::new(0),
        text: RefCell::new(String::new()),
        list: list.clone(),
        keys: RefCell::new(vec![]),
        title: title.clone(),
        spinner: spinner.clone(),
        status,
        stack: stack.clone(),
        rendered: RefCell::new(String::new()),
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));

    let mut child = toggles.first_child();
    while let Some(w) = child {
        if let Ok(t) = w.clone().downcast::<gtk::ToggleButton>() {
            let weak = Rc::downgrade(&me);
            t.connect_toggled(move |t| {
                if t.is_active() {
                    if let Some(me) = weak.upgrade() {
                        glib::idle_add_local_once(move || me.reload(false));
                    }
                }
            });
        }
        child = w.next_sibling();
    }

    let search = gtk::SearchEntry::new();
    search.set_placeholder_text(Some("Filter: text, repo:name, label:bug"));
    search.set_hexpand(true);
    let weak = Rc::downgrade(&me);
    search.connect_search_changed(move |e| {
        if let Some(me) = weak.upgrade() {
            *me.text.borrow_mut() = e.text().trim().to_string();
            me.render();
        }
    });

    let weak = Rc::downgrade(&me);
    list.connect_row_activated(move |_, row| {
        let Some(me) = weak.upgrade() else { return };
        let route = me.keys.borrow().get(row.index().max(0) as usize).cloned();
        if let Some(route) = route {
            app::select(Some(route));
        }
    });

    stack.add_named(&scrolled(&list), Some("list"));
    stack.add_named(&loading(), Some("loading"));
    let empty = label_with("Nothing here", &["sy-muted"]);
    empty.set_valign(gtk::Align::Start);
    empty.set_halign(gtk::Align::Center);
    empty.set_margin_top(30);
    stack.add_named(&empty, Some("empty"));
    stack.add_named(&vbox(8), Some("error"));

    let toggles_bar = hbox(0);
    margins(&toggles_bar, 6, 10, 6, 10);
    toggles.set_hexpand(true);
    toggles_bar.append(&toggles);

    let content = vbox(0);
    content.append(&toggles_bar);
    content.append(&line());
    content.append(&stack);
    content.append(&footer(&me.status));

    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_end_title_buttons(false);
    hb.set_title_widget(Some(&title));
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
    app::on_refresh(move |_| match weak.upgrade() {
        Some(me) => {
            me.reload(true);
            true
        }
        None => false,
    });
    let weak = Rc::downgrade(&me);
    app::on_live_visible(&tv, move |_| match weak.upgrade() {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> SearchItem {
        serde_json::from_value(serde_json::json!({
            "__typename": "PullRequest",
            "number": 12,
            "title": "Add caching layer",
            "url": "https://gitlab.com/g/app/-/merge_requests/12",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z",
            "author": { "login": "alice" },
            "repository": { "nameWithOwner": "g/app" },
            "labels": { "nodes": [{ "name": "backend", "color": "ff0000" }] },
            "forge": true
        }))
        .unwrap()
    }

    #[test]
    fn local_filter() {
        let i = item();
        assert!(i.forge);
        assert!(matches(&i, ""));
        assert!(matches(&i, "caching"));
        assert!(matches(&i, "repo:app label:back"));
        assert!(matches(&i, "!12"));
        assert!(matches(&i, "alice"));
        assert!(!matches(&i, "label:docs"));
    }
}
