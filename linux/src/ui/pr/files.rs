use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use super::model::{Change, FlatRow, PrModel};
use crate::api::models::{PendingComment, ReviewThread};
use crate::diff::{DiffAnchor, DiffFile, DiffLine, LineKind};
use crate::ui::composer::Composer;
use crate::ui::menu::Menu;
use crate::ui::widgets::*;
use crate::util;

pub struct FilesTab {
    pub widget: gtk::Stack,
    model: Rc<PrModel>,
    store: gio::ListStore,
    rows: RefCell<Vec<FlatRow>>,
    row_keys: RefCell<Vec<RowKey>>,
    rows_open: Cell<Option<bool>>,
    file_list_sig: Cell<Option<u64>>,
    view: gtk::ListView,
    file_list: gtk::ListBox,
    file_keys: RefCell<Vec<String>>,
    side: gtk::Box,
    filter: RefCell<String>,
    viewed_label: gtk::Label,
    expanded_threads: RefCell<std::collections::HashMap<String, bool>>,
    pending_reveal: RefCell<Option<String>>,
    suppress_select: Cell<bool>,
}

#[derive(PartialEq)]
enum RowKey {
    Header(usize, bool, bool),
    Line(usize, usize),
    Other(String),
}

fn status_label(f: &DiffFile) -> gtk::Label {
    let l = label_with(f.status_letter(), &["sy-mono", "sy-bold", "sy-tiny", &format!("fg-{}", f.status_css())]);
    l.set_width_chars(1);
    l
}

pub fn line_widget(line: &DiffLine, commentable: bool) -> gtk::Box {
    let row = hbox(0);
    row.add_css_class("diff-line");
    match line.kind {
        LineKind::Add => row.add_css_class("add"),
        LineKind::Del => row.add_css_class("del"),
        LineKind::Hunk => row.add_css_class("hunk"),
        LineKind::Meta => row.add_css_class("meta"),
        LineKind::Context => {}
    }
    if line.kind == LineKind::Hunk {
        let t = label_with(&line.text, &["diff-line"]);
        t.set_margin_start(110);
        t.set_margin_top(3);
        t.set_margin_bottom(3);
        t.set_ellipsize(gtk::pango::EllipsizeMode::End);
        t.set_hexpand(true);
        row.append(&t);
        return row;
    }
    let gutter = label_with(&line.gutter(commentable), &["diff-gutter"]);
    gutter.set_width_chars(14);
    gutter.set_xalign(1.0);
    gutter.set_valign(gtk::Align::Start);
    if commentable && line.commentable() {
        gutter.add_css_class("commentable");
        gutter.set_tooltip_text(Some("Comment on this line"));
    }
    row.append(&gutter);
    let text = format!("{}{}", line.sign(), line.text.replace('\t', "    "));
    let t = gtk::Label::new(Some(&text));
    t.add_css_class("diff-line");
    t.set_xalign(0.0);
    t.set_wrap(true);
    t.set_wrap_mode(gtk::pango::WrapMode::Char);
    t.set_hexpand(true);
    t.set_margin_start(6);
    t.set_valign(gtk::Align::Start);
    row.append(&t);
    row
}

impl FilesTab {
    pub fn new(model: Rc<PrModel>) -> Rc<Self> {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let factory = gtk::SignalListItemFactory::new();
        let selection = gtk::NoSelection::new(Some(store.clone()));
        let view = gtk::ListView::new(Some(selection), Some(factory.clone()));
        view.add_css_class("diff-view");
        let diff_scroll = gtk::ScrolledWindow::new();
        diff_scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        diff_scroll.set_child(Some(&view));
        diff_scroll.set_hexpand(true);
        diff_scroll.set_vexpand(true);

        let side = vbox(0);
        side.add_css_class("sy-list-pane");
        side.set_size_request(260, -1);
        let filter_row = hbox(6);
        pad(&filter_row, 8);
        let filter = gtk::SearchEntry::new();
        filter.set_placeholder_text(Some("Filter files"));
        filter.set_hexpand(true);
        let viewed_label = label_with("", &["sy-muted", "sy-small"]);
        viewed_label.set_tooltip_text(Some("Viewed"));
        filter_row.append(&filter);
        filter_row.append(&viewed_label);
        side.append(&filter_row);
        let file_list = gtk::ListBox::new();
        file_list.add_css_class("sy-list");
        file_list.add_css_class("sy-plain");
        side.append(&scrolled(&file_list));

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&side));
        paned.set_end_child(Some(&diff_scroll));
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(false);
        paned.set_position(260);

        let widget = gtk::Stack::new();
        widget.add_named(&loading(), Some("loading"));
        widget.add_named(&paned, Some("files"));
        widget.add_named(&vbox(0), Some("error"));

        let me = Rc::new(FilesTab {
            widget,
            model: model.clone(),
            store,
            rows: RefCell::new(vec![]),
            row_keys: RefCell::new(vec![]),
            rows_open: Cell::new(None),
            file_list_sig: Cell::new(None),
            view,
            file_list: file_list.clone(),
            file_keys: RefCell::new(vec![]),
            side,
            filter: RefCell::new(String::new()),
            viewed_label,
            expanded_threads: RefCell::new(std::collections::HashMap::new()),
            pending_reveal: RefCell::new(None),
            suppress_select: Cell::new(false),
        });

        factory.connect_setup(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                item.set_activatable(false);
                item.set_selectable(false);
                item.set_focusable(false);
                item.set_child(Some(&adw::Bin::new()));
            }
        });
        let w = Rc::downgrade(&me);
        factory.connect_bind(move |_, item| {
            let Some(me) = w.upgrade() else { return };
            let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return };
            let Some(obj) = item.item().and_downcast::<glib::BoxedAnyObject>() else { return };
            let row = obj.borrow::<FlatRow>().clone();
            let widget = me.row_widget(&row);
            if let Some(bin) = item.child().and_downcast::<adw::Bin>() {
                bin.set_child(Some(&widget));
            }
        });
        factory.connect_unbind(|_, item| {
            if let Some(item) = item.downcast_ref::<gtk::ListItem>() {
                if let Some(bin) = item.child().and_downcast::<adw::Bin>() {
                    bin.set_child(None::<&gtk::Widget>);
                }
            }
        });

        let w = Rc::downgrade(&me);
        filter.connect_search_changed(move |e| {
            if let Some(me) = w.upgrade() {
                *me.filter.borrow_mut() = e.text().to_string();
                me.rebuild();
            }
        });
        let w = Rc::downgrade(&me);
        file_list.connect_row_selected(move |_, row| {
            let Some(me) = w.upgrade() else { return };
            if me.suppress_select.get() {
                return;
            }
            let Some(row) = row else { return };
            let path = me.file_keys.borrow().get(row.index().max(0) as usize).cloned();
            if let Some(p) = path {
                let m = me.model.clone();
                glib::idle_add_local_once(move || m.reveal_file(&p));
            }
        });
        let w = Rc::downgrade(&me);
        model.changed.connect(move |c| match w.upgrade() {
            Some(me) => {
                match c {
                    Change::Files => {
                        me.rows_open.set(None);
                        me.file_list_sig.set(None);
                        me.rebuild();
                    }
                    Change::Diff | Change::Pr => me.rebuild(),
                    Change::Reveal(p) => me.reveal(p),
                    _ => {}
                }
                true
            }
            None => false,
        });
        me.rebuild();
        me
    }

    fn rebuild(self: &Rc<Self>) {
        let _span = crate::diagnostics::ui_span("pr.diff");
        let m = &self.model;
        if !m.files_loaded.get() {
            if let Some(e) = m.files_error.borrow().clone() {
                let mm = m.clone();
                let err = error_state(
                    &e,
                    Some(Box::new(move || {
                        let mm = mm.clone();
                        crate::rt::spawn_local(async move { mm.load_files(true).await });
                    })),
                );
                if let Some(old) = self.widget.child_by_name("error") {
                    self.widget.remove(&old);
                }
                self.widget.add_named(&err, Some("error"));
                self.widget.set_visible_child_name("error");
            } else {
                self.widget.set_visible_child_name("loading");
            }
            return;
        }
        self.widget.set_visible_child_name("files");
        let filter = self.filter.borrow().clone();
        let rows = m.flat_rows(&filter);
        let keys: Vec<RowKey> = rows.iter().map(|r| self.row_key(r)).collect();
        let open = Some(m.pr.borrow().as_ref().map(|p| p.is_open()).unwrap_or(false));
        let old = self.row_keys.replace(vec![]);
        let n = self.store.n_items() as usize;
        let (prefix, suffix) = if self.rows_open.get() != open || old.len() != n {
            (0, 0)
        } else {
            let prefix = old.iter().zip(&keys).take_while(|(a, b)| a == b).count();
            let room = old.len().min(keys.len()) - prefix;
            let suffix = old.iter().rev().zip(keys.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
            (prefix, suffix)
        };
        let removed = n - prefix - suffix;
        let objs: Vec<glib::BoxedAnyObject> = rows[prefix..rows.len() - suffix].iter().map(|r| glib::BoxedAnyObject::new(r.clone())).collect();
        if removed > 0 || !objs.is_empty() {
            self.store.splice(prefix as u32, removed as u32, &objs);
        }
        self.rows_open.set(open);
        *self.row_keys.borrow_mut() = keys;
        *self.rows.borrow_mut() = rows;
        self.rebuild_file_list();
        if let Some(p) = self.pending_reveal.borrow_mut().take() {
            let me = self.clone();
            glib::idle_add_local_once(move || me.reveal(&p));
        }
    }

    fn row_key(&self, row: &FlatRow) -> RowKey {
        match row {
            FlatRow::Header(fi) => {
                let files = self.model.files.borrow();
                let id = files.get(*fi).map(|f| f.id.as_str()).unwrap_or("");
                RowKey::Header(*fi, self.model.collapsed.borrow().contains(id), self.model.viewed.borrow().contains(id))
            }
            FlatRow::Line(fi, li) => RowKey::Line(*fi, *li),
            FlatRow::Thread(t) => RowKey::Other(format!("t{:?}{:?}", self.expanded_threads.borrow().get(&t.id), t)),
            FlatRow::Pending(p) => RowKey::Other(format!("p{p:?}")),
            FlatRow::Composer(a) => RowKey::Other(format!("c{a:?}")),
            FlatRow::Note(text) => RowKey::Other(format!("n{text}")),
        }
    }

    fn rebuild_file_list(&self) {
        let m = &self.model;
        let files = m.files.borrow();
        let viewed = m.viewed.borrow();
        let filter = self.filter.borrow().clone();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        filter.hash(&mut hasher);
        for f in files.iter() {
            (&f.id, f.status_letter(), viewed.contains(&f.id), m.thread_count(&f.id) > 0).hash(&mut hasher);
        }
        let sig = Some(hasher.finish());
        if self.file_list_sig.get() == sig {
            return;
        }
        self.file_list_sig.set(sig);
        let viewed_count = files.iter().filter(|f| viewed.contains(&f.id)).count();
        self.viewed_label.set_text(&format!("{viewed_count}/{}", files.len()));
        self.suppress_select.set(true);
        self.file_list.remove_all();
        let mut keys = vec![];
        for f in files.iter().filter(|f| filter.is_empty() || util::contains_ci(&f.id, &filter)) {
            let b = hbox(6);
            b.append(&status_label(f));
            let col = vbox(0);
            col.set_hexpand(true);
            col.append(&ellipsized(&util::last_component(&f.id), &["sy-text"]));
            let dir = label_with(&util::parent_path(&f.id), &["sy-muted", "sy-tiny"]);
            dir.set_ellipsize(gtk::pango::EllipsizeMode::Start);
            col.append(&dir);
            b.append(&col);
            if m.thread_count(&f.id) > 0 {
                b.append(&colored_icon("sy-comment-symbolic", 11, "muted"));
            }
            if viewed.contains(&f.id) {
                b.append(&colored_icon("sy-check-symbolic", 12, "green"));
            }
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&b));
            self.file_list.append(&row);
            keys.push(f.id.clone());
        }
        *self.file_keys.borrow_mut() = keys;
        self.suppress_select.set(false);
    }

    fn reveal(self: &Rc<Self>, path: &str) {
        let files = self.model.files.borrow();
        let idx = self.rows.borrow().iter().position(|r| matches!(r, FlatRow::Header(i) if files.get(*i).map(|f| f.id.as_str()) == Some(path)));
        drop(files);
        match idx {
            Some(i) => self.view.scroll_to(i as u32, gtk::ListScrollFlags::NONE, Some(gtk::ScrollInfo::new())),
            None => *self.pending_reveal.borrow_mut() = Some(path.to_string()),
        }
    }

    fn row_widget(self: &Rc<Self>, row: &FlatRow) -> gtk::Widget {
        match row {
            FlatRow::Header(fi) => {
                let f = self.model.files.borrow().get(*fi).cloned();
                match f {
                    Some(f) => self.header(&f).upcast(),
                    None => vbox(0).upcast(),
                }
            }
            FlatRow::Line(fi, li) => {
                let files = self.model.files.borrow();
                let Some(file) = files.get(*fi) else { return vbox(0).upcast() };
                let Some(line) = file.lines.get(*li) else { return vbox(0).upcast() };
                let open = self.model.pr.borrow().as_ref().map(|p| p.is_open()).unwrap_or(false);
                let w = line_widget(line, open);
                if open && line.commentable() {
                    if let Some(anchor) = line.anchor_line() {
                        let gutter = w.first_child();
                        if let Some(g) = gutter {
                            let m = self.model.clone();
                            let a = DiffAnchor { path: file.id.clone(), line: anchor, side: line.anchor_side().to_string() };
                            on_click_widget(&g, move || {
                                *m.composer.borrow_mut() = Some(a.clone());
                                m.composer_text.borrow_mut().clear();
                                m.emit(Change::Diff);
                            });
                        }
                    }
                }
                w.upcast()
            }
            FlatRow::Thread(t) => self.thread_card(t).upcast(),
            FlatRow::Pending(p) => self.pending_card(p).upcast(),
            FlatRow::Composer(a) => self.composer_card(a).upcast(),
            FlatRow::Note(text) => label_with(text, &["diff-note"]).upcast(),
        }
    }

    fn header(self: &Rc<Self>, f: &DiffFile) -> gtk::Box {
        let m = &self.model;
        let collapsed = m.collapsed.borrow().contains(&f.id);
        let b = hbox(8);
        b.add_css_class("diff-file-header");
        let toggle = icon_button(if collapsed { "pan-end-symbolic" } else { "pan-down-symbolic" }, if collapsed { "Expand" } else { "Collapse" });
        let mm = m.clone();
        let id = f.id.clone();
        toggle.connect_clicked(move |_| {
            {
                let mut c = mm.collapsed.borrow_mut();
                if !c.remove(&id) {
                    c.insert(id.clone());
                }
            }
            mm.emit(Change::Diff);
        });
        b.append(&toggle);
        b.append(&status_label(f));
        let name = match &f.previous_filename {
            Some(p) => format!("{p} → {}", f.id),
            None => f.id.clone(),
        };
        let l = label_with(&name, &["sy-mono", "sy-bold", "sy-text"]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        l.set_selectable(true);
        b.append(&l);
        b.append(&diff_stat(f.additions, f.deletions));
        b.append(&spacer());
        let viewed = gtk::CheckButton::with_label("Viewed");
        viewed.set_active(m.viewed.borrow().contains(&f.id));
        let mm = m.clone();
        let id = f.id.clone();
        viewed.connect_toggled(move |_| {
            let mm = mm.clone();
            let id = id.clone();
            glib::idle_add_local_once(move || {
                crate::rt::spawn_local(async move { mm.toggle_viewed(id).await });
            });
        });
        b.append(&viewed);
        let menu = Menu::new();
        let id = f.id.clone();
        menu.item("Copy path", move || util::copy(&id));
        let side = self.side.clone();
        menu.item(if self.side.is_visible() { "Hide file list" } else { "Show file list" }, move || side.set_visible(!side.is_visible()));
        let mm = m.clone();
        menu.item("Collapse all", move || {
            let all: std::collections::HashSet<String> = mm.files.borrow().iter().map(|f| f.id.clone()).collect();
            *mm.collapsed.borrow_mut() = all;
            mm.emit(Change::Diff);
        });
        let mm = m.clone();
        menu.item("Expand all", move || {
            mm.collapsed.borrow_mut().clear();
            mm.emit(Change::Diff);
        });
        b.append(&menu.button("view-more-symbolic", "File actions"));
        b
    }

    fn thread_card(self: &Rc<Self>, t: &ReviewThread) -> gtk::Box {
        let open = self.expanded_threads.borrow().get(&t.id).copied().unwrap_or(!t.is_resolved);
        let card = vbox(0);
        card.add_css_class("inline-card");
        let head = hbox(6);
        margins(&head, 5, 10, 5, 10);
        let toggle = icon_button(if open { "pan-down-symbolic" } else { "pan-end-symbolic" }, "Toggle thread");
        let w = Rc::downgrade(self);
        let id = t.id.clone();
        toggle.connect_clicked(move |_| {
            if let Some(me) = w.upgrade() {
                me.expanded_threads.borrow_mut().insert(id.clone(), !open);
                me.rebuild();
            }
        });
        head.append(&toggle);
        head.append(&icon("sy-comment-symbolic", 12));
        head.append(&label_with(&util::plural(t.comments.nodes.len() as i64, "comment", "comments"), &["sy-small"]));
        if t.is_outdated {
            head.append(&label_with("outdated", &["sy-muted", "sy-tiny"]));
        }
        if t.is_resolved {
            head.append(&label_with("resolved", &["fg-green", "sy-tiny"]));
        }
        head.append(&spacer());
        let m = self.model.clone();
        if t.is_resolved && t.viewer_can_unresolve {
            let b = button("Unresolve");
            let id = t.id.clone();
            b.connect_clicked(move |_| {
                let m = m.clone();
                let id = id.clone();
                crate::rt::spawn_local(async move {
                    m.set_resolved(id, false).await;
                });
            });
            head.append(&b);
        } else if !t.is_resolved && t.viewer_can_resolve {
            let b = button("Resolve");
            let id = t.id.clone();
            b.connect_clicked(move |_| {
                let m = m.clone();
                let id = id.clone();
                crate::rt::spawn_local(async move {
                    m.set_resolved(id, true).await;
                });
            });
            head.append(&b);
        }
        card.append(&head);
        if open {
            card.append(&line());
            for c in &t.comments.nodes {
                let row = hbox(8);
                margins(&row, 6, 10, 6, 10);
                let av = avatar(c.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 20, None);
                av.set_valign(gtk::Align::Start);
                row.append(&av);
                let col = vbox(2);
                col.set_hexpand(true);
                let meta = hbox(4);
                meta.append(&label_with(c.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost"), &["sy-bold", "sy-small", "sy-text"]));
                meta.append(&label_with(&format!("· {}", util::rel(Some(c.created_at))), &["sy-muted", "sy-small"]));
                col.append(&meta);
                col.append(&crate::ui::markdown::inline_label(&c.body));
                row.append(&col);
                card.append(&row);
            }
            if t.viewer_can_reply {
                card.append(&line());
                let reply_box = vbox(6);
                pad(&reply_box, 8);
                let composer = Composer::new("Reply", 50);
                let actions = hbox(6);
                actions.set_halign(gtk::Align::End);
                let cancel = button("Cancel");
                let send = suggested("Reply");
                actions.append(&cancel);
                actions.append(&send);
                reply_box.append(&composer.widget);
                reply_box.append(&actions);
                reply_box.set_visible(false);
                let start = link_button("Reply…");
                start.set_halign(gtk::Align::Start);
                margins(&start, 4, 6, 4, 6);
                let rb = reply_box.clone();
                let st = start.clone();
                let comp = Rc::downgrade(&composer);
                start.connect_clicked(move |_| {
                    rb.set_visible(true);
                    st.set_visible(false);
                    if let Some(c) = comp.upgrade() {
                        c.grab_focus();
                    }
                });
                let rb = reply_box.clone();
                let st = start.clone();
                cancel.connect_clicked(move |_| {
                    rb.set_visible(false);
                    st.set_visible(true);
                });
                let m = self.model.clone();
                let id = t.id.clone();
                let weak_comp = Rc::downgrade(&composer);
                let go = Rc::new(move || {
                    let Some(comp) = weak_comp.upgrade() else { return };
                    if comp.is_blank() || m.busy.get() {
                        return;
                    }
                    let body = comp.text();
                    let m = m.clone();
                    let id = id.clone();
                    crate::rt::spawn_local(async move {
                        m.reply(id, body).await;
                    });
                });
                let g = go.clone();
                send.connect_clicked(move |_| g());
                let g = go.clone();
                composer.on_submit(move || g());
                card.append(&start);
                card.append(&reply_box);
                keep_alive(&card, composer);
            }
        }
        card.set_size_request(-1, -1);
        card
    }

    fn pending_card(self: &Rc<Self>, p: &PendingComment) -> gtk::Box {
        let card = hbox(8);
        card.add_css_class("inline-card");
        card.add_css_class("pending");
        pad(&card, 10);
        card.append(&chip("pending", "amber", None, false));
        let body = crate::ui::markdown::inline_label(&p.body);
        body.set_hexpand(true);
        card.append(&body);
        let del = icon_button("user-trash-symbolic", "Delete pending comment");
        let m = self.model.clone();
        let id = p.id.clone();
        del.connect_clicked(move |_| {
            m.pending.borrow_mut().retain(|x| x.id != id);
            m.emit(Change::Diff);
        });
        card.append(&del);
        card
    }

    fn composer_card(self: &Rc<Self>, a: &DiffAnchor) -> gtk::Box {
        let card = vbox(6);
        card.add_css_class("inline-card");
        card.add_css_class("composer");
        let composer = Composer::new(&format!("Comment on line {} (Markdown)", a.line), 70);
        composer.set_text(&self.model.composer_text.borrow());
        let m = self.model.clone();
        let c2 = Rc::downgrade(&composer);
        composer.on_changed(move || {
            if let Some(c) = c2.upgrade() {
                *m.composer_text.borrow_mut() = c.text();
            }
        });
        card.append(&composer.widget);
        let actions = hbox(6);
        let cancel = button("Cancel");
        let m = self.model.clone();
        cancel.connect_clicked(move |_| {
            *m.composer.borrow_mut() = None;
            m.composer_text.borrow_mut().clear();
            m.emit(Change::Diff);
        });
        actions.append(&cancel);
        actions.append(&spacer());
        let add = button("Add to review");
        add.set_tooltip_text(Some("Save as pending and submit with the review (Ctrl+Enter)"));
        let single = suggested("Add single comment");
        actions.append(&add);
        actions.append(&single);
        card.append(&actions);
        let submit = {
            let m = self.model.clone();
            let weak_comp = Rc::downgrade(&composer);
            let a = a.clone();
            Rc::new(move |immediately: bool| {
                let Some(comp) = weak_comp.upgrade() else { return };
                if comp.is_blank() {
                    return;
                }
                let body = comp.text();
                let m = m.clone();
                let a = a.clone();
                crate::rt::spawn_local(async move {
                    m.add_inline(a, body, immediately).await;
                });
            })
        };
        let s = submit.clone();
        add.connect_clicked(move |_| s(false));
        let s = submit.clone();
        single.connect_clicked(move |_| s(true));
        let s = submit.clone();
        composer.on_submit(move || s(false));
        let comp = Rc::downgrade(&composer);
        glib::idle_add_local_once(move || {
            if let Some(c) = comp.upgrade() {
                c.grab_focus();
            }
        });
        keep_alive(&card, composer);
        card
    }
}
