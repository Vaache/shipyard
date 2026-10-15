use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::pr::files::line_widget;
use super::widgets::*;
use crate::api::models::*;
use crate::diff::DiffFile;
use crate::{app, util};

#[derive(Clone)]
enum Row {
    Header(usize),
    Line(usize, usize),
    Note(String),
}

pub struct PlainDiff {
    pub widget: gtk::ScrolledWindow,
    files: RefCell<Vec<DiffFile>>,
    collapsed: RefCell<HashSet<String>>,
    store: gio::ListStore,
    rows: RefCell<Vec<Row>>,
    view: gtk::ListView,
    on_open: RefCell<Option<Rc<dyn Fn(String)>>>,
}

impl PlainDiff {
    pub fn new() -> Rc<Self> {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let factory = gtk::SignalListItemFactory::new();
        let view = gtk::ListView::new(Some(gtk::NoSelection::new(Some(store.clone()))), Some(factory.clone()));
        view.add_css_class("diff-view");
        let widget = gtk::ScrolledWindow::new();
        widget.set_hscrollbar_policy(gtk::PolicyType::Never);
        widget.set_child(Some(&view));
        widget.set_vexpand(true);
        widget.set_hexpand(true);
        let me = Rc::new(PlainDiff {
            widget,
            files: RefCell::new(vec![]),
            collapsed: RefCell::new(HashSet::new()),
            store,
            rows: RefCell::new(vec![]),
            view,
            on_open: RefCell::new(None),
        });
        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_activatable(false);
                item.set_selectable(false);
                item.set_child(Some(&adw::Bin::new()));
            }
        });
        let w = Rc::downgrade(&me);
        factory.connect_bind(move |_, item| {
            let Some(me) = w.upgrade() else { return };
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else { return };
            let row = obj.borrow::<Row>().clone();
            let widget = me.row_widget(&row);
            if let Some(bin) = item.child().and_downcast::<adw::Bin>() {
                bin.set_child(Some(&widget));
            }
        });
        me
    }

    pub fn on_open(&self, f: impl Fn(String) + 'static) {
        *self.on_open.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_files(self: &Rc<Self>, files: Vec<DiffFile>, collapse_large: bool) {
        if collapse_large {
            *self.collapsed.borrow_mut() = files.iter().filter(|f| f.lines.len() > 1500).map(|f| f.id.clone()).collect();
        }
        *self.files.borrow_mut() = files;
        self.rebuild();
    }

    fn rebuild(&self) {
        let files = self.files.borrow();
        let collapsed = self.collapsed.borrow();
        let mut rows = vec![];
        for (fi, f) in files.iter().enumerate() {
            rows.push(Row::Header(fi));
            if collapsed.contains(&f.id) {
                continue;
            }
            if !f.has_patch {
                rows.push(Row::Note("Diff unavailable (binary or too large)".into()));
                continue;
            }
            for li in 0..f.lines.len() {
                rows.push(Row::Line(fi, li));
            }
        }
        let objs: Vec<glib::BoxedAnyObject> = rows.iter().map(|r| glib::BoxedAnyObject::new(r.clone())).collect();
        let n = self.store.n_items();
        self.store.splice(0, n, &objs);
        *self.rows.borrow_mut() = rows;
    }

    fn row_widget(self: &Rc<Self>, row: &Row) -> gtk::Widget {
        match row {
            Row::Header(fi) => {
                let Some(f) = self.files.borrow().get(*fi).cloned() else { return vbox(0).upcast() };
                let collapsed = self.collapsed.borrow().contains(&f.id);
                let b = hbox(8);
                b.add_css_class("diff-file-header");
                let t = icon_button(if collapsed { "pan-end-symbolic" } else { "pan-down-symbolic" }, "Toggle");
                let w = Rc::downgrade(self);
                let id = f.id.clone();
                t.connect_clicked(move |_| {
                    if let Some(me) = w.upgrade() {
                        {
                            let mut c = me.collapsed.borrow_mut();
                            if !c.remove(&id) {
                                c.insert(id.clone());
                            }
                        }
                        me.rebuild();
                    }
                });
                b.append(&t);
                b.append(&label_with(f.status_letter(), &["sy-mono", "sy-bold", "sy-tiny", &format!("fg-{}", f.status_css())]));
                let name = match &f.previous_filename {
                    Some(p) => format!("{p} → {}", f.id),
                    None => f.id.clone(),
                };
                let l = label_with(&name, &["sy-mono", "sy-bold", "sy-text"]);
                l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
                b.append(&l);
                b.append(&diff_stat(f.additions, f.deletions));
                b.append(&spacer());
                if let Some(open) = self.on_open.borrow().clone() {
                    let o = icon_button("sy-external-symbolic", "Open file");
                    let id = f.id.clone();
                    o.connect_clicked(move |_| open(id.clone()));
                    b.append(&o);
                }
                let c = icon_button("edit-copy-symbolic", "Copy path");
                let id = f.id.clone();
                c.connect_clicked(move |_| util::copy(&id));
                b.append(&c);
                b.upcast()
            }
            Row::Line(fi, li) => {
                let files = self.files.borrow();
                match files.get(*fi).and_then(|f| f.lines.get(*li)) {
                    Some(l) => line_widget(l, false).upcast(),
                    None => vbox(0).upcast(),
                }
            }
            Row::Note(t) => label_with(t, &["diff-note"]).upcast(),
        }
    }
}

pub fn page(repo: &RepoRef, sha: &str) -> adw::NavigationPage {
    let header = vbox(8);
    pad(&header, 16);
    let diff = PlainDiff::new();
    let main = vbox(0);
    main.append(&header);
    main.append(&line());
    main.append(&diff.widget);
    let body = gtk::Stack::new();
    body.add_named(&loading(), Some("loading"));
    body.add_named(&main, Some("main"));
    let short: String = sha.chars().take(7).collect();
    let (tv, hb, _) = toolbar_page(&short, &repo.full(), &body);
    tv.add_css_class("sy-panel-pane");
    let copy = icon_button("edit-copy-symbolic", "Copy SHA");
    let s = sha.to_string();
    copy.connect_clicked(move |_| {
        util::copy(&s);
        app::toast("Copied SHA");
    });
    let browser = icon_button("sy-external-symbolic", "Open in browser");
    browser.set_sensitive(false);
    hb.pack_end(&browser);
    hb.pack_end(&copy);
    let load: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));
    let (r, s) = (repo.clone(), sha.to_string());
    let d = diff.clone();
    let body_w = body.clone();
    let header_w = header.clone();
    let browser_w = browser.clone();
    let load_holder = load.clone();
    let f: Rc<dyn Fn()> = Rc::new(move || {
        let Some(api) = app::api() else { return };
        let (r, s) = (r.clone(), s.clone());
        let d = d.clone();
        let body = body_w.clone();
        let header = header_w.clone();
        let browser = browser_w.clone();
        let reload = load_holder.clone();
        crate::rt::spawn_local(async move {
            let res = crate::rt::run(async move { api.commit(&r, &s).await }).await;
            match res {
                Ok(c) => {
                    clear(&header);
                    let mut lines = c.commit.message.lines();
                    let t = wrap_label(lines.next().unwrap_or(""), &["sy-title", "sy-text"]);
                    t.set_selectable(true);
                    header.append(&t);
                    let rest = lines.collect::<Vec<_>>().join("\n").trim().to_string();
                    if !rest.is_empty() {
                        let l = wrap_label(&rest, &["sy-mono", "sy-muted"]);
                        l.set_selectable(true);
                        header.append(&l);
                    }
                    let meta = hbox(8);
                    meta.append(&avatar(c.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 18, None));
                    let who = c.author.as_ref().map(|a| a.login.clone()).or_else(|| c.commit.author.as_ref().and_then(|a| a.name.clone())).unwrap_or_default();
                    meta.append(&label_with(&who, &["sy-bold", "sy-text"]));
                    meta.append(&label_with(&util::rel(c.commit.author.as_ref().and_then(|a| a.date)), &["sy-muted"]));
                    meta.append(&selectable(label_with(&c.sha, &["sy-mono", "sy-muted", "sy-small"])));
                    if let Some(st) = &c.stats {
                        meta.append(&diff_stat(st.additions, st.deletions));
                    }
                    header.append(&meta);
                    let url = c.html_url.clone();
                    browser.set_sensitive(true);
                    browser.connect_clicked(move |_| util::open_url(&url));
                    let files: Vec<DiffFile> = c.files.as_deref().unwrap_or(&[]).iter().map(DiffFile::new).collect();
                    d.set_files(files, true);
                    body.set_visible_child_name("main");
                }
                Err(e) => {
                    let again = reload.borrow().clone();
                    if let Some(old) = body.child_by_name("error") {
                        body.remove(&old);
                    }
                    body.add_named(
                        &error_state(
                            &e.message,
                            Some(Box::new(move || {
                                if let Some(f) = again.clone() {
                                    f();
                                }
                            })),
                        ),
                        Some("error"),
                    );
                    body.set_visible_child_name("error");
                }
            }
        });
    });
    *load.borrow_mut() = Some(f.clone());
    f();
    keep_alive(&tv, (diff, load));
    adw::NavigationPage::new(&tv, &short)
}
