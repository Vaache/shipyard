use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};

use crate::api::models::*;
use crate::claude::workspace::Tab;
use crate::fix_prompt::{self, FixOptions};
use crate::ui::code_view::CodeView;
use crate::ui::commit::PlainDiff;
use crate::ui::item_row::item_row;
use crate::ui::menu::{context_menu, Menu};
use crate::ui::pr::model::{Change, PrModel};
use crate::ui::repos::entry_row;
use crate::ui::tabstrip::{TabItem, TabStrip};
use crate::ui::widgets::*;
use crate::{app, util};

pub struct Inspector {
    pub widget: gtk::Box,
    tab: Rc<Tab>,
    changes_header: gtk::Box,
    unpushed: gtk::Box,
    changes_stack: gtk::Stack,
    diff: Rc<PlainDiff>,
    files: Rc<LocalFiles>,
    pr: Rc<BranchPrs>,
}

impl Inspector {
    pub fn new(tab: Rc<Tab>) -> Rc<Self> {
        let strip = TabStrip::new(vec![
            TabItem { id: "changes", title: "Changes".into(), icon: "sy-plusminus-symbolic" },
            TabItem { id: "files", title: "Files".into(), icon: "sy-folder-symbolic" },
            TabItem { id: "pr", title: "PR".into(), icon: "sy-pr-symbolic" },
        ]);
        let stack = gtk::Stack::new();
        stack.set_vexpand(true);
        let changes = vbox(0);
        let changes_header = hbox(8);
        margins(&changes_header, 6, 10, 6, 10);
        let unpushed = vbox(3);
        margins(&unpushed, 0, 10, 6, 10);
        let changes_stack = gtk::Stack::new();
        changes_stack.set_vexpand(true);
        let empty = vbox(8);
        pad(&empty, 24);
        empty.set_valign(gtk::Align::Center);
        empty.append(&colored_icon("sy-check-symbolic", 32, "green"));
        let el = wrap_label("Changes Claude makes will show up here automatically", &["sy-muted"]);
        el.set_justify(gtk::Justification::Center);
        el.set_xalign(0.5);
        empty.append(&el);
        changes_stack.add_named(&empty, Some("empty"));
        let diff = PlainDiff::new();
        let folder = tab.folder.clone();
        diff.on_open(move |p| util::open_path(&format!("{folder}/{p}")));
        changes_stack.add_named(&diff.widget, Some("diff"));
        changes.append(&changes_header);
        changes.append(&unpushed);
        changes.append(&line());
        changes.append(&changes_stack);
        stack.add_named(&changes, Some("changes"));
        let files = LocalFiles::new(&tab.folder);
        stack.add_named(&files.widget, Some("files"));
        let pr = BranchPrs::new(tab.clone());
        stack.add_named(&pr.widget, Some("pr"));
        let widget = vbox(0);
        widget.add_css_class("sy-panel-pane");
        strip.widget.set_margin_top(4);
        widget.append(&strip.widget);
        widget.append(&line());
        widget.append(&stack);
        let me = Rc::new(Inspector { widget, tab: tab.clone(), changes_header, unpushed, changes_stack, diff, files, pr });
        let st = stack.clone();
        let w = Rc::downgrade(&me);
        strip.on_change(move |id| {
            st.set_visible_child_name(id);
            if let Some(me) = w.upgrade() {
                if id == "pr" {
                    me.pr.load();
                }
                if id == "files" {
                    me.files.load_dir();
                }
            }
        });
        let w = Rc::downgrade(&me);
        let s2 = strip.clone();
        tab.git.changed.connect(move |_| match w.upgrade() {
            Some(me) => {
                me.render_changes();
                let n = me.tab.git.changes.borrow().len();
                s2.set_badge("changes", if n > 0 { Some(n.to_string()) } else { None }.as_deref(), Some("amber"));
                me.pr.on_git_change();
                true
            }
            None => false,
        });
        me.render_changes();
        keep_alive(&me.widget, strip);
        tab.git.refresh();
        me
    }

    fn render_changes(&self) {
        let git = &self.tab.git;
        clear(&self.changes_header);
        if git.loading.get() {
            self.changes_header.append(&spinner(12));
        }
        let changes = git.changes.borrow().clone();
        self.changes_header.append(&label_with(&if changes.is_empty() { "No uncommitted changes".to_string() } else { util::plural(changes.len() as i64, "file changed", "files changed") }, &["sy-muted", "sy-small"]));
        if !changes.is_empty() {
            self.changes_header.append(&diff_stat(changes.iter().map(|c| c.additions).sum(), changes.iter().map(|c| c.deletions).sum()));
        }
        self.changes_header.append(&spacer());
        let refresh = icon_button("view-refresh-symbolic", "Refresh");
        let g = git.clone();
        refresh.connect_clicked(move |_| g.refresh());
        self.changes_header.append(&refresh);
        clear(&self.unpushed);
        let unpushed = git.unpushed.borrow().clone();
        self.unpushed.set_visible(!unpushed.is_empty());
        if !unpushed.is_empty() {
            self.unpushed.append(&label_with(&format!("Unpushed commits · {}", unpushed.len()), &["sy-muted", "sy-small", "sy-bold"]));
            for c in unpushed.iter().take(6) {
                let row = hbox(6);
                row.append(&label_with(&c.short, &["sy-mono", "sy-muted", "sy-small"]));
                row.append(&ellipsized(&c.subject, &["sy-small", "sy-text"]));
                self.unpushed.append(&row);
            }
        }
        if changes.is_empty() {
            self.changes_stack.set_visible_child_name("empty");
        } else {
            self.diff.set_files(changes, true);
            self.changes_stack.set_visible_child_name("diff");
        }
    }
}

struct LocalFiles {
    widget: gtk::Paned,
    root: String,
    dir: RefCell<String>,
    path_label: gtk::Label,
    back: gtk::Button,
    list: gtk::ListBox,
    entries: RefCell<Vec<TreeEntry>>,
    viewer: adw::Bin,
    code: CodeView,
    loaded: Cell<bool>,
}

impl LocalFiles {
    fn new(root: &str) -> Rc<Self> {
        let top = vbox(0);
        let bar = hbox(4);
        margins(&bar, 5, 8, 5, 8);
        let back = icon_button("go-previous-symbolic", "Up");
        let path_label = label_with(&util::last_component(root), &["sy-mono", "sy-muted", "sy-small"]);
        path_label.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        bar.append(&back);
        bar.append(&path_label);
        top.append(&bar);
        top.append(&line());
        let list = gtk::ListBox::new();
        list.add_css_class("sy-list");
        list.add_css_class("sy-plain");
        top.append(&scrolled(&list));
        top.set_size_request(-1, 160);
        let viewer = adw::Bin::new();
        viewer.set_size_request(-1, 160);
        let widget = gtk::Paned::new(gtk::Orientation::Vertical);
        widget.set_start_child(Some(&top));
        widget.set_end_child(Some(&viewer));
        widget.set_position(240);
        let me = Rc::new(LocalFiles {
            widget,
            root: root.to_string(),
            dir: RefCell::new(String::new()),
            path_label,
            back: back.clone(),
            list: list.clone(),
            entries: RefCell::new(vec![]),
            viewer,
            code: CodeView::new(),
            loaded: Cell::new(false),
        });
        me.viewer.set_child(Some(&empty_state("sy-file-symbolic", "slate", "Pick a file", None)));
        let w = Rc::downgrade(&me);
        back.connect_clicked(move |_| {
            if let Some(me) = w.upgrade() {
                let parent = util::parent_path(&me.dir.borrow());
                *me.dir.borrow_mut() = parent;
                me.render();
            }
        });
        let w = Rc::downgrade(&me);
        list.connect_row_activated(move |_, row| {
            let Some(me) = w.upgrade() else { return };
            let e = me.entries.borrow().get(row.index().max(0) as usize).cloned();
            if let Some(e) = e {
                if e.is_dir() {
                    *me.dir.borrow_mut() = e.path.clone();
                    me.render();
                } else {
                    me.open(&e.path);
                }
            }
        });
        me
    }

    fn load_dir(self: &Rc<Self>) {
        if !self.loaded.replace(true) {
            self.render();
        }
    }

    fn render(self: &Rc<Self>) {
        let dir = self.dir.borrow().clone();
        self.back.set_sensitive(!dir.is_empty());
        self.path_label.set_text(&if dir.is_empty() { util::last_component(&self.root) } else { dir.clone() });
        let base = if dir.is_empty() { self.root.clone() } else { format!("{}/{dir}", self.root) };
        let mut entries: Vec<TreeEntry> = std::fs::read_dir(&base)
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name == ".git" || name == ".DS_Store" {
                            return None;
                        }
                        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                        let size = e.metadata().map(|m| m.len() as i64).ok();
                        Some(TreeEntry {
                            path: if dir.is_empty() { name.clone() } else { format!("{dir}/{name}") },
                            name,
                            kind: if is_dir { "tree".into() } else { "blob".into() },
                            object: Some(BlobMeta { byte_size: if is_dir { None } else { size }, is_binary: None }),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by(|a, b| {
            if a.is_dir() != b.is_dir() {
                return if a.is_dir() { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater };
            }
            util::natural_cmp(&a.name, &b.name)
        });
        self.list.remove_all();
        for e in &entries {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&entry_row(e)));
            self.list.append(&row);
        }
        *self.entries.borrow_mut() = entries;
    }

    fn open(self: &Rc<Self>, rel: &str) {
        let full = format!("{}/{rel}", self.root);
        let size = std::fs::metadata(&full).map(|m| m.len()).unwrap_or(u64::MAX);
        let content = if size < 2_000_000 { std::fs::read_to_string(&full).ok() } else { None };
        match content {
            Some(text) => {
                self.code.set_text(&text, rel);
                crate::ui::repos::detach(&self.code.widget);
                self.viewer.set_child(Some(&self.code.widget));
            }
            None => self.viewer.set_child(Some(&empty_state("sy-file-symbolic", "slate", "Binary or large file", None))),
        }
    }
}

struct BranchPrs {
    widget: gtk::Stack,
    tab: Rc<Tab>,
    branch_box: gtk::Box,
    list_box: gtk::ListBox,
    list_items: RefCell<Vec<SearchItem>>,
    branch_items: RefCell<Vec<SearchItem>>,
    linked: RefCell<Option<SearchItem>>,
    scope: Cell<u8>,
    status: Rc<RefreshStatus>,
    updated: Cell<Option<DateTime<Utc>>>,
    branch_loading: Cell<bool>,
    list_loading: Cell<bool>,
    error: RefCell<Option<String>>,
    branch_error: RefCell<Option<String>>,
    detail: adw::Bin,
    last_key: RefCell<String>,
    repo_label: gtk::Label,
}

impl BranchPrs {
    fn new(tab: Rc<Tab>) -> Rc<Self> {
        let widget = gtk::Stack::new();
        let none = empty_state("sy-repo-symbolic", "slate", "Folder isn't linked to a GitHub repository", None);
        widget.add_named(&none, Some("none"));
        let col = vbox(6);
        pad(&col, 10);
        let branch_box = vbox(6);
        col.append(&branch_box);
        col.append(&line());
        let head = hbox(6);
        let repo_label = label_with("", &["sy-muted", "sy-small", "sy-bold"]);
        repo_label.set_hexpand(true);
        head.append(&repo_label);
        col.append(&head);
        let toggles = hbox(0);
        toggles.add_css_class("linked");
        let mut first: Option<gtk::ToggleButton> = None;
        let scopes = ["Open", "Merged", "Mine"];
        let mut buttons = vec![];
        for s in scopes {
            let t = gtk::ToggleButton::with_label(s);
            t.set_hexpand(true);
            if let Some(f) = &first {
                t.set_group(Some(f));
            } else {
                t.set_active(true);
                first = Some(t.clone());
            }
            toggles.append(&t);
            buttons.push(t);
        }
        col.append(&toggles);
        let list_box = gtk::ListBox::new();
        list_box.add_css_class("sy-list");
        col.append(&list_box);
        let holder: Rc<RefCell<Option<std::rc::Weak<BranchPrs>>>> = Rc::new(RefCell::new(None));
        let h = holder.clone();
        let status = RefreshStatus::new(true, move || {
            if let Some(me) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
                me.load_all();
            }
        });
        head.append(&status.widget);
        let page = vbox(0);
        page.append(&scrolled(&col));
        widget.add_named(&page, Some("list"));
        let detail = adw::Bin::new();
        widget.add_named(&detail, Some("detail"));
        let me = Rc::new(BranchPrs {
            widget,
            tab,
            branch_box,
            list_box: list_box.clone(),
            list_items: RefCell::new(vec![]),
            branch_items: RefCell::new(vec![]),
            linked: RefCell::new(None),
            scope: Cell::new(0),
            status,
            updated: Cell::new(None),
            branch_loading: Cell::new(false),
            list_loading: Cell::new(false),
            error: RefCell::new(None),
            branch_error: RefCell::new(None),
            detail,
            last_key: RefCell::new(String::new()),
            repo_label,
        });
        *holder.borrow_mut() = Some(Rc::downgrade(&me));
        for (i, b) in buttons.iter().enumerate() {
            let w = Rc::downgrade(&me);
            b.connect_toggled(move |t| {
                if t.is_active() {
                    if let Some(me) = w.upgrade() {
                        me.scope.set(i as u8);
                        me.load_list();
                    }
                }
            });
        }
        let w = Rc::downgrade(&me);
        list_box.connect_row_activated(move |_, row| {
            if let Some(me) = w.upgrade() {
                let item = me.list_items.borrow().get(row.index().max(0) as usize).cloned();
                if let Some(it) = item {
                    me.show_detail(it.item_ref());
                }
            }
        });
        for sig in [0, 1] {
            let w = Rc::downgrade(&me);
            let f = move |_: &()| match w.upgrade() {
                Some(me) => {
                    if me.widget.visible_child_name().as_deref() == Some("list") {
                        me.load_all();
                    }
                    true
                }
                None => false,
            };
            if sig == 0 {
                app::on_refresh(f);
            } else {
                app::on_live_visible(&page, f);
            }
        }
        if let Some(chat) = &me.tab.chat {
            let w = Rc::downgrade(&me);
            let was_busy = Cell::new(false);
            chat.changed.connect(move |e| match w.upgrade() {
                Some(me) => {
                    if matches!(e, crate::claude::chat::Event::State) {
                        let busy = me.tab.busy();
                        if was_busy.replace(busy) && !busy {
                            me.load_branch();
                        }
                    }
                    true
                }
                None => false,
            });
        }
        me
    }

    fn repo(&self) -> Option<RepoRef> {
        self.tab.git.repo.borrow().clone()
    }

    fn on_git_change(self: &Rc<Self>) {
        let key = format!("{}|{}|{}", self.repo().map(|r| r.full()).unwrap_or_default(), self.tab.git.branch.borrow().clone().unwrap_or_default(), self.tab.git.ahead());
        if *self.last_key.borrow() != key {
            *self.last_key.borrow_mut() = key;
            self.load_all();
        }
    }

    fn load(self: &Rc<Self>) {
        if self.updated.get().is_none() {
            self.load_all();
        }
    }

    fn load_all(self: &Rc<Self>) {
        self.load_branch();
        self.load_list();
    }

    fn render(self: &Rc<Self>) {
        if self.widget.visible_child_name().as_deref() == Some("detail") {
            return;
        }
        let Some(repo) = self.repo() else {
            self.widget.set_visible_child_name("none");
            return;
        };
        self.widget.set_visible_child_name("list");
        self.repo_label.set_text(&repo.full());
        let err = self.branch_error.borrow().clone().or_else(|| self.error.borrow().clone());
        self.status.update(self.updated.get(), self.branch_loading.get() || self.list_loading.get(), err);
        clear(&self.branch_box);
        let head = hbox(6);
        head.append(&label_with("This branch", &["sy-muted", "sy-small", "sy-bold"]));
        let branch = self.tab.git.branch.borrow().clone();
        if let Some(b) = &branch {
            head.append(&branch_tag(b));
        }
        self.branch_box.append(&head);
        let items = self.branch_items.borrow().clone();
        let linked = self.linked.borrow().clone();
        if let (Some(e), true) = (self.branch_error.borrow().clone(), items.is_empty()) {
            self.branch_box.append(&wrap_label(&e, &["fg-red", "sy-small"]));
        } else if items.is_empty() && linked.is_none() && !self.branch_loading.get() {
            self.branch_box.append(&label_with(&format!("No PR for branch {}", branch.clone().unwrap_or_else(|| "?".into())), &["sy-muted"]));
            if let Some(b) = branch.filter(|b| b != "HEAD") {
                let create = button("Create PR in browser");
                create.set_halign(gtk::Align::Start);
                let url = format!("https://github.com/{}/pull/new/{b}", repo.full());
                create.connect_clicked(move |_| util::open_url(&url));
                self.branch_box.append(&create);
            }
        }
        let lb = gtk::ListBox::new();
        lb.add_css_class("sy-list");
        let mut all = items.clone();
        if let Some(l) = linked.filter(|l| !items.iter().any(|i| i.number == l.number)) {
            all.push(l);
        }
        for it in &all {
            lb.append(&self.row(it));
        }
        let w = Rc::downgrade(self);
        let all2 = all.clone();
        lb.connect_row_activated(move |_, row| {
            if let Some(me) = w.upgrade() {
                if let Some(it) = all2.get(row.index().max(0) as usize) {
                    me.show_detail(it.item_ref());
                }
            }
        });
        self.branch_box.append(&lb);
        self.list_box.remove_all();
        let list = self.list_items.borrow().clone();
        if list.is_empty() && !self.list_loading.get() {
            let r = gtk::ListBoxRow::new();
            r.set_child(Some(&label_with(self.error.borrow().as_deref().unwrap_or("Nothing here"), &["sy-dim"])));
            r.set_activatable(false);
            self.list_box.append(&r);
        }
        for it in &list {
            self.list_box.append(&self.row(it));
        }
    }

    fn row(&self, it: &SearchItem) -> gtk::ListBoxRow {
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&item_row(it, false)));
        let item = it.clone();
        context_menu(&row, move || {
            let m = Menu::ctx();
            let r = item.route();
            m.item("Open in PR section", move || app::open(r.clone(), false));
            let u = item.url.clone();
            m.item("Open in browser", move || util::open_url(&u));
            let u = item.url.clone();
            m.item("Copy link", move || util::copy(&u));
            m
        });
        row
    }

    fn load_branch(self: &Rc<Self>) {
        let (Some(api), Some(repo)) = (app::api(), self.repo()) else {
            self.render();
            return;
        };
        let branch = self.tab.git.branch.borrow().clone();
        let linked_number = self.tab.session_id.borrow().clone();
        let pr_number = app::get().claude.find_session(&linked_number).and_then(|s| s.pr_number);
        self.branch_loading.set(true);
        self.render();
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let (r2, b2, a2) = (repo.clone(), branch.clone(), api.clone());
            let res = crate::rt::run(async move {
                let found = match b2.filter(|b| b != "HEAD") {
                    Some(b) => a2.repo_pull_requests(&r2, Some(b), &["OPEN", "MERGED", "CLOSED"], 5).await?,
                    None => vec![],
                };
                let mut linked = None;
                if let Some(n) = pr_number {
                    if !found.iter().any(|f| f.number == n) {
                        linked = a2.pull_request_item(&ItemRef { repo: r2.clone(), number: n }).await.ok().flatten();
                    }
                }
                Ok::<_, crate::api::ApiError>((found, linked))
            })
            .await;
            me.branch_loading.set(false);
            if branch != *me.tab.git.branch.borrow() || Some(&repo) != me.repo().as_ref() {
                return;
            }
            match res {
                Ok((found, linked)) => {
                    *me.branch_items.borrow_mut() = found;
                    *me.linked.borrow_mut() = linked;
                    *me.branch_error.borrow_mut() = None;
                    me.updated.set(Some(Utc::now()));
                }
                Err(e) => *me.branch_error.borrow_mut() = Some(e.message),
            }
            me.render();
        });
    }

    fn load_list(self: &Rc<Self>) {
        let (Some(api), Some(repo)) = (app::api(), self.repo()) else {
            self.render();
            return;
        };
        let scope = self.scope.get();
        self.list_loading.set(true);
        self.render();
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let r2 = repo.clone();
            let res = crate::rt::run(async move {
                match scope {
                    0 => api.repo_pull_requests(&r2, None, &["OPEN"], 30).await,
                    1 => api.repo_pull_requests(&r2, None, &["MERGED"], 30).await,
                    _ => api.search(&format!("repo:{} is:pr author:@me sort:updated-desc", r2.full()), None).await.map(|c| c.nodes),
                }
            })
            .await;
            me.list_loading.set(false);
            if scope != me.scope.get() || Some(&repo) != me.repo().as_ref() {
                return;
            }
            match res {
                Ok(items) => {
                    *me.list_items.borrow_mut() = items;
                    *me.error.borrow_mut() = None;
                    me.updated.set(Some(Utc::now()));
                }
                Err(e) => *me.error.borrow_mut() = Some(e.message),
            }
            me.render();
        });
    }

    fn show_detail(self: &Rc<Self>, r: ItemRef) {
        let Some(model) = app::pr_model(&r) else { return };
        let (widget, view) = PrDetail::new(self.clone(), model, r);
        self.detail.set_child(Some(&widget));
        self.widget.set_visible_child_name("detail");
        keep_alive(&widget, view);
    }

    fn back(self: &Rc<Self>) {
        self.detail.set_child(None::<&gtk::Widget>);
        self.widget.set_visible_child_name("list");
        self.render();
    }
}

struct PrDetail {
    parent: std::rc::Weak<BranchPrs>,
    model: Rc<PrModel>,
    r: ItemRef,
    body: gtk::Box,
    status: Rc<RefreshStatus>,
    show_files: Cell<bool>,
}

impl PrDetail {
    fn new(parent: Rc<BranchPrs>, model: Rc<PrModel>, r: ItemRef) -> (gtk::Box, Rc<Self>) {
        let widget = vbox(0);
        let bar = hbox(6);
        pad(&bar, 8);
        let back = gtk::Button::new();
        back.add_css_class("flat");
        let bc = adw::ButtonContent::new();
        bc.set_icon_name("go-previous-symbolic");
        bc.set_label("Back");
        back.set_child(Some(&bc));
        bar.append(&back);
        bar.append(&spacer());
        let m = model.clone();
        let status = RefreshStatus::new(true, move || m.load(true));
        bar.append(&status.widget);
        let browser = icon_button("sy-external-symbolic", "Open in browser");
        let url = format!("https://github.com/{}/pull/{}", r.repo.full(), r.number);
        browser.connect_clicked(move |_| util::open_url(&url));
        bar.append(&browser);
        let full = button("Open full");
        let rr = r.clone();
        full.connect_clicked(move |_| app::open(Route::Pr(rr.clone()), false));
        bar.append(&full);
        widget.append(&bar);
        widget.append(&line());
        let body = vbox(12);
        pad(&body, 12);
        widget.append(&scrolled(&body));
        let me = Rc::new(PrDetail { parent: Rc::downgrade(&parent), model: model.clone(), r, body, status, show_files: Cell::new(false) });
        let w = Rc::downgrade(&me);
        back.connect_clicked(move |_| {
            if let Some(me) = w.upgrade() {
                if let Some(p) = me.parent.upgrade() {
                    p.back();
                }
            }
        });
        let w = Rc::downgrade(&me);
        model.changed.connect(move |c| match w.upgrade() {
            Some(me) => {
                if matches!(c, Change::Pr | Change::Files | Change::Busy | Change::Stack) {
                    me.render();
                }
                true
            }
            None => false,
        });
        for sig in [0, 1] {
            let w = Rc::downgrade(&me);
            let f = move |_: &()| match w.upgrade() {
                Some(me) => {
                    me.model.load(sig == 0);
                    true
                }
                None => false,
            };
            if sig == 0 {
                app::on_refresh(f);
            } else {
                app::on_live_visible(&widget, f);
            }
        }
        me.render();
        model.load(false);
        (widget, me)
    }

    fn render(self: &Rc<Self>) {
        let m = &self.model;
        self.status.update(m.last_updated.get(), m.loading.get(), m.refresh_error.borrow().clone());
        clear(&self.body);
        let pr = m.pr.borrow().clone();
        let Some(pr) = pr else {
            if let Some(e) = m.error.borrow().clone() {
                let mm = m.clone();
                self.body.append(&error_state(&e, Some(Box::new(move || mm.load(false)))));
            } else {
                self.body.append(&loading());
            }
            return;
        };
        let head = vbox(6);
        head.append(&selectable(wrap_label(&pr.title, &["heading", "sy-text"])));
        let meta = hbox(6);
        meta.append(&state_badge(pr.visual_state(), false));
        meta.append(&label_with(&format!("#{}", pr.number), &["sy-muted"]));
        meta.append(&diff_stat(pr.additions, pr.deletions));
        head.append(&meta);
        let branches = hbox(4);
        branches.append(&branch_tag(&pr.base_ref_name));
        branches.append(&icon("go-previous-symbolic", 11));
        branches.append(&branch_tag(&pr.head_ref_name));
        head.append(&branches);
        self.body.append(&head);
        if pr.is_open() {
            let rr = self.r.clone();
            if let Some(bar) = crate::ui::pr::merge_status_bar(m, &pr, Rc::new(move || app::open(Route::Pr(rr.clone()), false))) {
                self.body.append(&bar);
            }
            let rr = self.r.clone();
            let mm = m.clone();
            if let Some(b) = crate::ui::pr::rereview_banner(
                m,
                &pr,
                false,
                None,
                false,
                Rc::new(move |logins: Vec<String>| {
                    let mm = mm.clone();
                    let _ = &rr;
                    crate::rt::spawn_local(async move {
                        mm.request_reviewers(logins).await;
                    });
                }),
            ) {
                self.body.append(&b);
            }
        }
        let unresolved: Vec<ReviewThread> = pr.review_threads.nodes.iter().filter(|t| !t.is_resolved).cloned().collect();
        let checks = pr.checks();
        let stats = hbox(14);
        let ok = checks.iter().filter(|c| c.outcome() == CheckOutcome::Success).count();
        let failed = checks.iter().any(|c| c.outcome() == CheckOutcome::Failure);
        let ch = hbox(4);
        ch.append(&colored_icon("sy-checklist-symbolic", 12, if failed { "red" } else { "muted" }));
        ch.append(&label_with(&format!("{ok}/{}", checks.len()), &[if failed { "fg-red" } else { "sy-muted" }, "sy-small"]));
        stats.append(&ch);
        let un = hbox(4);
        un.append(&colored_icon("sy-comment-symbolic", 12, if unresolved.is_empty() { "muted" } else { "amber" }));
        un.append(&label_with(&unresolved.len().to_string(), &[if unresolved.is_empty() { "sy-muted" } else { "fg-amber" }, "sy-small"]));
        un.set_tooltip_text(Some("Unresolved comments"));
        stats.append(&un);
        let reviewers = m.reviewers();
        if !reviewers.is_empty() {
            let rb = hbox(3);
            for r in &reviewers {
                rb.append(&crate::ui::pr::reviewer_badge(r));
            }
            stats.append(&rb);
        }
        self.body.append(&stats);
        let tab = self.parent.upgrade().map(|p| p.tab.clone());
        if let Some(tab) = tab.filter(|t| t.chat.is_some() || t.running()) {
            let send = vbox(6);
            send.append(&label_with("Send to this session", &["sy-muted", "sy-small", "sy-bold"]));
            if let Some(fix) = fix_prompt::latest(&pr) {
                let b = icon_label_button("sy-sparkles-symbolic", &format!("Fix prompt by @{}", fix.author));
                let (t, p) = (tab.clone(), pr.clone());
                b.connect_clicked(move |_| app::get().claude.send(&fix_prompt::wrap(&fix, &p, &FixOptions::default()), Some(t.clone()), false));
                send.append(&b);
            }
            let b = icon_label_button("sy-comment-symbolic", &format!("Unresolved comments ({})", unresolved.len()));
            b.set_sensitive(!unresolved.is_empty());
            let (t, p, r) = (tab.clone(), pr.clone(), self.r.repo.clone());
            b.connect_clicked(move |_| {
                let threads: Vec<&ReviewThread> = p.review_threads.nodes.iter().filter(|t| !t.is_resolved).collect();
                app::get().claude.send(&fix_prompt::build(&p, &r, &threads, &fix_prompt::general_feedback(&p), &FixOptions::default()), Some(t.clone()), false);
            });
            send.append(&b);
            let b = icon_label_button("sy-sparkles-symbolic", "Ask Claude to review this PR");
            b.add_css_class("fg-coral");
            let (t, p) = (tab.clone(), pr.clone());
            b.connect_clicked(move |_| app::get().claude.send(&format!("Review PR #{} ({}) and briefly summarize what it does and any problems.", p.number, p.url), Some(t.clone()), false));
            send.append(&b);
            self.body.append(&send);
        }
        if !unresolved.is_empty() {
            let col = vbox(8);
            col.append(&label_with("Unresolved comments", &["sy-muted", "sy-small", "sy-bold"]));
            for th in &unresolved {
                let c = vbox(3);
                c.add_css_class("sy-sunken");
                pad(&c, 8);
                let p = label_with(&format!("{}{}", th.path, th.line.or(th.original_line).map(|l| format!(":{l}")).unwrap_or_default()), &["sy-mono", "sy-muted", "sy-small"]);
                p.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                c.append(&p);
                if let Some(first) = th.comments.nodes.first() {
                    let row = hbox(5);
                    let av = avatar(first.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 14, None);
                    av.set_valign(gtk::Align::Start);
                    row.append(&av);
                    let l = crate::ui::markdown::inline_label(&first.body);
                    l.set_lines(4);
                    l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    row.append(&l);
                    c.append(&row);
                }
                col.append(&c);
            }
            self.body.append(&col);
        }
        let exp = gtk::Expander::new(Some(&format!("Files changed · {}", pr.changed_files)));
        exp.set_expanded(self.show_files.get());
        let files_box = vbox(3);
        if m.files_loaded.get() {
            for f in m.files.borrow().iter() {
                let row = hbox(6);
                row.append(&label_with(f.status_letter(), &["sy-mono", "sy-bold", "sy-tiny", "sy-muted"]));
                let l = label_with(&f.id, &["sy-mono", "sy-small"]);
                l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                l.set_hexpand(true);
                row.append(&l);
                row.append(&diff_stat(f.additions, f.deletions));
                files_box.append(&row);
            }
        } else {
            files_box.append(&spinner(14));
        }
        exp.set_child(Some(&files_box));
        let w = Rc::downgrade(self);
        exp.connect_expanded_notify(move |e| {
            if let Some(me) = w.upgrade() {
                me.show_files.set(e.is_expanded());
                if e.is_expanded() {
                    let m = me.model.clone();
                    crate::rt::spawn_local(async move { m.load_files(false).await });
                }
            }
        });
        self.body.append(&exp);
    }
}
