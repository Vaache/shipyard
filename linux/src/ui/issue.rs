use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use chrono::{DateTime, Utc};
use serde_json::json;

use super::composer::Composer;
use super::dialogs;
use super::html::HtmlView;
use super::widgets::*;
use crate::api::models::*;
use crate::api::queries as q;
use crate::conversation_html::{self, Look};
use crate::{app, util};

struct IssuePage {
    r: ItemRef,
    issue: RefCell<Option<IssueDetail>>,
    is_pr: Cell<bool>,
    loading: Cell<bool>,
    busy: Cell<bool>,
    updated: Cell<Option<DateTime<Utc>>>,
    error: RefCell<Option<String>>,
    refresh_error: RefCell<Option<String>>,
    html: Rc<HtmlView>,
    composer: Rc<Composer>,
    header: gtk::Box,
    body: gtk::Stack,
    title: adw::WindowTitle,
    actions: gtk::Box,
    status: Rc<RefreshStatus>,
    busy_spinner: gtk::Spinner,
    page: gtk::glib::WeakRef<adw::Bin>,
}

impl IssuePage {
    fn load(self: &Rc<Self>) {
        let Some(api) = app::api() else { return };
        let me = self.clone();
        me.loading.set(true);
        me.render_status();
        crate::rt::spawn_local(async move {
            let r = me.r.clone();
            let res = crate::rt::run(async move { api.issue(&r).await }).await;
            me.loading.set(false);
            match res {
                Ok(resp) => match resp.repository.and_then(|r| r.issue_or_pull_request) {
                    None => *me.error.borrow_mut() = Some(format!("Issue {} not found", me.r.title())),
                    Some(node) => {
                        if node.typename == "PullRequest" {
                            me.is_pr.set(true);
                        } else if let Some(issue) = node.issue {
                            me.html.set_html(&conversation_html::render_issue(&issue, Look::current()));
                            *me.issue.borrow_mut() = Some(issue);
                            me.updated.set(Some(Utc::now()));
                            *me.error.borrow_mut() = None;
                            *me.refresh_error.borrow_mut() = None;
                        }
                    }
                },
                Err(e) => {
                    if me.issue.borrow().is_none() {
                        *me.error.borrow_mut() = Some(e.message);
                    } else {
                        *me.refresh_error.borrow_mut() = Some(e.message);
                    }
                }
            }
            me.render();
        });
    }

    fn render_status(&self) {
        self.status.update(self.updated.get(), self.loading.get(), self.refresh_error.borrow().clone());
        self.busy_spinner.set_visible(self.busy.get());
    }

    fn render(self: &Rc<Self>) {
        let _span = crate::diagnostics::ui_span("issue.render");
        self.render_status();
        if self.is_pr.get() {
            if let Some(bin) = self.page.upgrade() {
                let pr = super::pr::page(&self.r);
                if let Some(child) = pr.child() {
                    pr.set_child(None::<&gtk::Widget>);
                    bin.set_child(Some(&child));
                }
            }
            return;
        }
        let issue = self.issue.borrow().clone();
        let Some(issue) = issue else {
            if let Some(e) = self.error.borrow().clone() {
                let w = Rc::downgrade(self);
                if let Some(old) = self.body.child_by_name("error") {
                    self.body.remove(&old);
                }
                self.body.add_named(
                    &error_state(
                        &e,
                        Some(Box::new(move || {
                            if let Some(me) = w.upgrade() {
                                me.load();
                            }
                        })),
                    ),
                    Some("error"),
                );
                self.body.set_visible_child_name("error");
            }
            return;
        };
        self.body.set_visible_child_name("main");
        self.title.set_title(&issue.title);
        self.title.set_subtitle(&format!("{} #{}", self.r.repo.full(), issue.number));
        clear(&self.header);
        let top = hbox(8);
        let t = wrap_label(&issue.title, &["sy-title", "sy-text"]);
        t.set_selectable(true);
        top.append(&t);
        top.append(&label_with(&format!("#{}", issue.number), &["sy-title-num"]));
        self.header.append(&top);
        let meta = hbox(8);
        meta.append(&state_badge(issue.visual_state(), false));
        meta.append(&avatar(issue.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 18, None));
        meta.append(&label_with(issue.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost"), &["sy-bold", "sy-text"]));
        meta.append(&chip(&self.r.repo.full(), "teal", Some("sy-repo-symbolic"), false));
        meta.append(&chip(&util::plural(issue.comments.total_count, "comment", "comments"), "sky", Some("sy-comment-symbolic"), false));
        meta.append(&spacer());
        let created = label_with(&util::rel(Some(issue.created_at)), &["sy-muted"]);
        created.set_tooltip_text(Some(&util::full_date(issue.created_at)));
        meta.append(&created);
        self.header.append(&meta);
        if !issue.labels.nodes.is_empty() || !issue.assignees.nodes.is_empty() {
            let row = hbox(6);
            for l in &issue.labels.nodes {
                row.append(&label_chip(l));
            }
            if !issue.assignees.nodes.is_empty() {
                row.append(&label_with("Assignees:", &["sy-muted", "sy-small"]));
                for a in &issue.assignees.nodes {
                    row.append(&tooltip(avatar(a.avatar_url.as_deref(), 18, None), &a.login));
                }
            }
            self.header.append(&row);
        }
        clear(&self.actions);
        if issue.viewer_can_update {
            let open = issue.issue_state == "OPEN";
            let b = icon_button(if open { "sy-issue-done-symbolic" } else { "sy-issue-symbolic" }, if open { "Close issue" } else { "Reopen issue" });
            let w = Rc::downgrade(self);
            b.connect_clicked(move |_| {
                if let Some(me) = w.upgrade() {
                    me.set_open(!open);
                }
            });
            self.actions.append(&b);
        }
        let url = issue.url.clone();
        let copy = icon_button("edit-copy-symbolic", "Copy Link");
        copy.connect_clicked(move |_| {
            util::copy(&url);
            app::toast("Copied link");
        });
        self.actions.append(&copy);
        let url = issue.url.clone();
        let browser = icon_button("sy-external-symbolic", "Open in Browser");
        browser.connect_clicked(move |_| util::open_url(&url));
        self.actions.append(&browser);
    }

    fn run(self: &Rc<Self>, mutation: &'static str, body: Option<String>) {
        let Some(api) = app::api() else { return };
        let Some(id) = self.issue.borrow().as_ref().map(|i| i.id.clone()) else { return };
        let me = self.clone();
        me.busy.set(true);
        me.render_status();
        crate::rt::spawn_local(async move {
            let res = crate::rt::run(async move {
                match body {
                    Some(b) => api.add_comment(&id, &b).await,
                    None => api.mutate(mutation, json!({ "id": id })).await,
                }
            })
            .await;
            me.busy.set(false);
            match res {
                Ok(()) => {
                    if mutation.is_empty() {
                        me.composer.set_text("");
                        me.html.scroll_to_bottom_after_load();
                    }
                    me.load();
                }
                Err(e) => dialogs::error(&e.message),
            }
            me.render_status();
        });
    }

    fn set_open(self: &Rc<Self>, open: bool) {
        self.run(if open { q::REOPEN_ISSUE } else { q::CLOSE_ISSUE }, None);
    }
}

pub fn page_child(r: &ItemRef) -> gtk::Widget {
    let header = vbox(8);
    pad(&header, 16);
    let html = HtmlView::new();
    let composer = Composer::new("Comment (Markdown) — Ctrl+Enter to send", 56);
    composer.widget.set_hexpand(true);
    let send = suggested("Comment");
    send.set_valign(gtk::Align::End);
    let row = hbox(8);
    row.add_css_class("sy-bar-top");
    pad(&row, 10);
    row.append(&composer.widget);
    row.append(&send);
    let main = vbox(0);
    main.append(&header);
    main.append(&line());
    main.append(&html.view);
    main.append(&row);
    let body = gtk::Stack::new();
    body.add_named(&loading(), Some("loading"));
    body.add_named(&main, Some("main"));
    let title = adw::WindowTitle::new(&r.title(), "");
    let (tv, hb, _) = toolbar_page("", "", &body);
    hb.set_title_widget(Some(&title));
    tv.add_css_class("sy-panel-pane");
    let actions = hbox(4);
    let busy_spinner = spinner(14);
    busy_spinner.set_visible(false);
    let holder: Rc<RefCell<Option<std::rc::Weak<IssuePage>>>> = Rc::new(RefCell::new(None));
    let h = holder.clone();
    let status = RefreshStatus::new(true, move || {
        if let Some(me) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
            me.load();
        }
    });
    hb.pack_end(&actions);
    hb.pack_end(&status.widget);
    hb.pack_end(&busy_spinner);
    let bin = adw::Bin::new();
    bin.set_child(Some(&tv));
    let me = Rc::new(IssuePage {
        r: r.clone(),
        issue: RefCell::new(None),
        is_pr: Cell::new(false),
        loading: Cell::new(false),
        busy: Cell::new(false),
        updated: Cell::new(None),
        error: RefCell::new(None),
        refresh_error: RefCell::new(None),
        html: html.clone(),
        composer: composer.clone(),
        header,
        body,
        title,
        actions,
        status,
        busy_spinner,
        page: bin.downgrade(),
    });
    *holder.borrow_mut() = Some(Rc::downgrade(&me));
    let w = Rc::downgrade(&me);
    let submit = Rc::new(move || {
        let Some(me) = w.upgrade() else { return };
        if me.composer.is_blank() || me.busy.get() {
            return;
        }
        let text = me.composer.text();
        me.run("", Some(text));
    });
    let s = submit.clone();
    send.connect_clicked(move |_| s());
    let s = submit.clone();
    composer.on_submit(move || s());
    html.on_link(|url| match util::parse_github_url(&url, false) {
        Some(r) => app::open(r, true),
        None => {
            if url != "https://github.com/" {
                util::open_url(&url)
            }
        }
    });
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
            app::on_live_visible(&tv, f);
        }
    }
    me.load();
    keep_alive(&tv, me);
    bin.upcast()
}

pub fn page(r: &ItemRef) -> adw::NavigationPage {
    adw::NavigationPage::new(&page_child(r), &r.title())
}
