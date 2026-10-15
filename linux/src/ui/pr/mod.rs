pub mod files;
pub mod model;
pub mod sheets;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;

use self::model::{Change, PrModel, PrTab};
use super::composer::Composer;
use super::dialogs;
use super::fix_claude;
use super::html::HtmlView;
use super::menu::{context_menu, Menu};
use super::tabstrip::{TabItem, TabStrip};
use super::widgets::*;
use crate::api::models::*;
use crate::api::queries as q;
use crate::{app, util};

pub struct PrPage {
    pub model: Rc<PrModel>,
    page: gtk::glib::WeakRef<adw::NavigationPage>,
    title: adw::WindowTitle,
    actions: gtk::Box,
    busy: gtk::Spinner,
    status: Rc<RefreshStatus>,
    header_box: gtk::Box,
    banners: gtk::Box,
    tabs: Rc<TabStrip>,
    tab_stack: gtk::Stack,
    body: gtk::Stack,
    conversation: RefCell<Option<Rc<Conversation>>>,
    commits: RefCell<Option<gtk::ListBox>>,
    checks: RefCell<Option<gtk::ListBox>>,
    files: RefCell<Option<Rc<files::FilesTab>>>,
    stack_expanded: Cell<bool>,
    rerequested: RefCell<Option<Vec<String>>>,
    sending_rerequest: Cell<bool>,
    replaced: Cell<bool>,
    actions_key: RefCell<String>,
}

pub struct Conversation {
    pub html: Rc<HtmlView>,
    composer: Rc<Composer>,
    pub widget: gtk::Box,
}

pub fn page(r: &ItemRef) -> adw::NavigationPage {
    let Some(model) = app::pr_model(r) else {
        return adw::NavigationPage::new(&loading(), &r.title());
    };
    PrPage::new(model)
}

impl PrPage {
    pub fn new(model: Rc<PrModel>) -> adw::NavigationPage {
        let title = adw::WindowTitle::new(&model.r.title(), "");
        let header_box = vbox(8);
        margins(&header_box, 12, 16, 0, 16);
        let banners = vbox(0);
        let tabs = TabStrip::new(vec![
            TabItem { id: "conversation", title: "Conversation".into(), icon: "sy-comment-symbolic" },
            TabItem { id: "commits", title: "Commits".into(), icon: "sy-commit-symbolic" },
            TabItem { id: "checks", title: "Checks".into(), icon: "sy-checklist-symbolic" },
            TabItem { id: "files", title: "Files changed".into(), icon: "sy-file-symbolic" },
        ]);
        let tab_stack = gtk::Stack::new();
        tab_stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        tab_stack.set_transition_duration(120);
        tab_stack.set_vexpand(true);
        let main = vbox(0);
        main.append(&header_box);
        main.append(&banners);
        tabs.widget.set_margin_top(6);
        main.append(&tabs.widget);
        main.append(&line());
        main.append(&tab_stack);
        let body = gtk::Stack::new();
        body.add_named(&loading(), Some("loading"));
        body.add_named(&main, Some("main"));
        let (toolbar, hb, _) = toolbar_page("", "", &body);
        hb.set_title_widget(Some(&title));
        toolbar.add_css_class("sy-panel-pane");
        let actions = hbox(4);
        let busy = spinner(14);
        busy.set_visible(false);
        let holder: Rc<RefCell<Option<std::rc::Weak<PrPage>>>> = Rc::new(RefCell::new(None));
        let h = holder.clone();
        let status = RefreshStatus::new(true, move || {
            if let Some(me) = h.borrow().as_ref().and_then(|w| w.upgrade()) {
                me.model.load(true);
            }
        });
        hb.pack_end(&actions);
        hb.pack_end(&status.widget);
        hb.pack_end(&busy);
        let page = adw::NavigationPage::new(&toolbar, &model.r.title());
        let me = Rc::new(PrPage {
            model: model.clone(),
            page: page.downgrade(),
            title,
            actions,
            busy,
            status,
            header_box,
            banners,
            tabs,
            tab_stack,
            body,
            conversation: RefCell::new(None),
            commits: RefCell::new(None),
            checks: RefCell::new(None),
            files: RefCell::new(None),
            stack_expanded: Cell::new(false),
            rerequested: RefCell::new(None),
            sending_rerequest: Cell::new(false),
            replaced: Cell::new(false),
            actions_key: RefCell::new(String::new()),
        });
        *holder.borrow_mut() = Some(Rc::downgrade(&me));

        let w = Rc::downgrade(&me);
        me.tabs.on_change(move |id| {
            if let Some(me) = w.upgrade() {
                me.model.set_tab(PrTab::from_id(id));
            }
        });
        let w = Rc::downgrade(&me);
        model.changed.connect(move |c| match w.upgrade() {
            Some(me) => {
                me.on_change(c);
                true
            }
            None => false,
        });
        let w = Rc::downgrade(&me);
        app::on_refresh(move |_| match w.upgrade() {
            Some(me) => {
                me.model.load(true);
                true
            }
            None => false,
        });
        let w = Rc::downgrade(&me);
        app::on_live(move |_| match w.upgrade() {
            Some(me) => {
                me.model.load(false);
                true
            }
            None => false,
        });
        let w = Rc::downgrade(&me);
        crate::theme::on_change(move |_| match w.upgrade() {
            Some(me) => {
                me.model.rerender_html();
                true
            }
            None => false,
        });
        me.show_tab();
        me.render();
        model.load(false);
        keep_alive(&toolbar, me);
        page
    }

    fn on_change(self: &Rc<Self>, c: &Change) {
        match c {
            Change::Pr => self.render(),
            Change::Busy => {
                self.busy.set_visible(self.model.busy.get());
                self.status.update(self.model.last_updated.get(), self.model.loading.get(), self.model.refresh_error.borrow().clone());
            }
            Change::Stack => self.render_header(),
            Change::Tab => self.show_tab(),
            Change::ScrollBottom => {
                if let Some(c) = self.conversation.borrow().as_ref() {
                    c.html.scroll_to_bottom_after_load();
                }
            }
            Change::ActionError(e) => dialogs::error(e),
            Change::Diff => {
                let pr = self.model.pr.borrow().clone();
                if let Some(pr) = pr {
                    self.render_actions(&pr);
                }
            }
            Change::Files | Change::Reveal(_) => {}
        }
    }

    fn show_tab(self: &Rc<Self>) {
        let tab = self.model.tab.get();
        if self.tabs.active() != tab.id() {
            self.tabs.select(tab.id());
        }
        if self.tab_stack.child_by_name(tab.id()).is_none() {
            let w: gtk::Widget = match tab {
                PrTab::Conversation => {
                    let c = self.build_conversation();
                    let w = c.widget.clone().upcast();
                    *self.conversation.borrow_mut() = Some(c);
                    w
                }
                PrTab::Commits => {
                    let l = gtk::ListBox::new();
                    l.add_css_class("sy-list");
                    let w = Rc::downgrade(self);
                    l.connect_row_activated(move |_, row| {
                        if let Some(me) = w.upgrade() {
                            let name = row.widget_name().to_string();
                            if let Some(oid) = name.strip_prefix("commit:") {
                                app::open(Route::Commit(me.model.r.repo.clone(), oid.to_string()), true);
                            } else if name == "all-commits" {
                                if let Some(pr) = me.model.pr.borrow().as_ref() {
                                    util::open_url(&format!("{}/commits", pr.url));
                                }
                            }
                        }
                    });
                    *self.commits.borrow_mut() = Some(l.clone());
                    scrolled(&l).upcast()
                }
                PrTab::Checks => {
                    let l = gtk::ListBox::new();
                    l.add_css_class("sy-list");
                    l.set_selection_mode(gtk::SelectionMode::None);
                    *self.checks.borrow_mut() = Some(l.clone());
                    scrolled(&l).upcast()
                }
                PrTab::Files => {
                    let f = files::FilesTab::new(self.model.clone());
                    let w = f.widget.clone().upcast();
                    *self.files.borrow_mut() = Some(f);
                    w
                }
            };
            self.tab_stack.add_named(&w, Some(tab.id()));
            self.render_tabs();
        }
        self.tab_stack.set_visible_child_name(tab.id());
        if tab == PrTab::Files {
            let m = self.model.clone();
            crate::rt::spawn_local(async move { m.load_files(false).await });
        }
    }

    fn build_conversation(self: &Rc<Self>) -> Rc<Conversation> {
        let html = HtmlView::new();
        let composer = Composer::new("Comment (Markdown) — Ctrl+Enter to send", 56);
        composer.widget.set_hexpand(true);
        let send = suggested("Comment");
        send.set_valign(gtk::Align::End);
        let row = hbox(8);
        pad(&row, 10);
        row.append(&composer.widget);
        row.append(&send);
        let widget = vbox(0);
        widget.append(&html.view);
        widget.append(&line());
        widget.append(&row);
        let c = Rc::new(Conversation { html: html.clone(), composer: composer.clone(), widget });
        let w = Rc::downgrade(self);
        let weak_comp = Rc::downgrade(&composer);
        let submit = Rc::new(move || {
            let Some(me) = w.upgrade() else { return };
            let Some(comp) = weak_comp.upgrade() else { return };
            if comp.is_blank() || me.model.busy.get() {
                return;
            }
            let body = comp.text();
            let comp = comp.clone();
            let m = me.model.clone();
            crate::rt::spawn_local(async move {
                if m.comment(body).await {
                    comp.set_text("");
                }
            });
        });
        let s = submit.clone();
        send.connect_clicked(move |_| s());
        let s = submit.clone();
        composer.on_submit(move || s());
        let w = Rc::downgrade(self);
        html.on_action(move |kind, id| {
            let Some(me) = w.upgrade() else { return };
            let m = me.model.clone();
            match kind.as_str() {
                "reply" => {
                    let t = m.pr.borrow().as_ref().and_then(|p| p.review_threads.nodes.iter().find(|t| t.id == id).cloned());
                    if let Some(t) = t {
                        sheets::reply(m.clone(), t);
                    }
                }
                "resolve" => crate::rt::spawn_local(async move {
                    m.set_resolved(id, true).await;
                }),
                "unresolve" => crate::rt::spawn_local(async move {
                    m.set_resolved(id, false).await;
                }),
                "file" => m.reveal_file(&id),
                _ => {}
            }
        });
        html.on_link(|url| {
            if url == "https://github.com/" || url == "https://github.com" {
                return;
            }
            match util::parse_github_url(&url, false) {
                Some(r) => app::open(r, true),
                None => util::open_url(&url),
            }
        });
        let h = self.model.html.borrow().clone();
        if !h.is_empty() {
            html.set_html(&h);
        }
        c
    }

    fn render(self: &Rc<Self>) {
        self.on_change(&Change::Busy);
        if self.model.not_pr.get() {
            if !self.replaced.get() {
                self.replaced.set(true);
                let child = super::issue::page_child(&self.model.r);
                if let Some(page) = self.page.upgrade() {
                    page.set_child(Some(&child));
                }
            }
            return;
        }
        let pr = self.model.pr.borrow().clone();
        let Some(pr) = pr else {
            if let Some(e) = self.model.error.borrow().clone() {
                let w = Rc::downgrade(self);
                let err = error_state(
                    &e,
                    Some(Box::new(move || {
                        if let Some(me) = w.upgrade() {
                            me.model.load(false);
                        }
                    })),
                );
                if let Some(old) = self.body.child_by_name("error") {
                    self.body.remove(&old);
                }
                self.body.add_named(&err, Some("error"));
                self.body.set_visible_child_name("error");
            } else {
                self.body.set_visible_child_name("loading");
            }
            return;
        };
        self.body.set_visible_child_name("main");
        self.title.set_title(&pr.title);
        self.title.set_subtitle(&format!("{} #{}", self.model.r.repo.full(), pr.number));
        if let Some(page) = self.page.upgrade() {
            page.set_title(&pr.title);
        }
        self.render_header();
        self.render_banners(&pr);
        self.render_actions(&pr);
        self.render_tabs();
    }

    fn render_tabs(self: &Rc<Self>) {
        let pr = self.model.pr.borrow().clone();
        let Some(pr) = pr else { return };
        let unresolved = pr.review_threads.nodes.iter().filter(|t| !t.is_resolved && !t.is_outdated).count();
        self.tabs.set_badge("conversation", if unresolved > 0 { Some(unresolved.to_string()) } else { None }.as_deref(), None);
        self.tabs.set_badge("commits", Some(&pr.all_commits.total_count.unwrap_or(pr.all_commits.nodes.len() as i64).to_string()), None);
        let checks = pr.checks();
        let failed = checks.iter().filter(|c| c.outcome() == CheckOutcome::Failure).count();
        let pending = checks.iter().filter(|c| c.outcome() == CheckOutcome::Pending).count();
        let (text, color) = if checks.is_empty() {
            ("0".to_string(), "muted")
        } else if failed > 0 {
            (format!("✕{failed}"), "red")
        } else if pending > 0 {
            (format!("…{pending}"), "amber")
        } else {
            (format!("✓{}", checks.len()), "green")
        };
        self.tabs.set_badge("checks", Some(&text), Some(color));
        self.tabs.set_badge("files", Some(&pr.changed_files.to_string()), None);
        if let Some(c) = self.conversation.borrow().as_ref() {
            let h = self.model.html.borrow().clone();
            c.html.set_html(&h);
        }
        if let Some(l) = self.commits.borrow().as_ref() {
            self.render_commits(l, &pr);
        }
        if let Some(l) = self.checks.borrow().as_ref() {
            self.render_checks(l, &pr);
        }
    }

    fn render_commits(&self, l: &gtk::ListBox, pr: &PullRequestDetail) {
        l.remove_all();
        for node in pr.all_commits.nodes.iter().rev() {
            let c = &node.commit;
            let row = gtk::ListBoxRow::new();
            row.set_widget_name(&format!("commit:{}", c.oid));
            let b = hbox(10);
            margins(&b, 2, 0, 2, 0);
            let author = c.author.as_ref();
            b.append(&avatar(author.and_then(|a| a.avatar_url.as_deref()), 22, None));
            let col = vbox(2);
            col.set_hexpand(true);
            let t = wrap_label(&c.message_headline, &["sy-text"]);
            t.set_lines(2);
            t.set_ellipsize(gtk::pango::EllipsizeMode::End);
            col.append(&t);
            let who = author.and_then(|a| a.user.as_ref().map(|u| u.login.clone()).or_else(|| a.name.clone())).unwrap_or_default();
            col.append(&label_with(&format!("{who} · {}", util::rel(Some(c.committed_date))), &["sy-muted", "sy-small"]));
            b.append(&col);
            if let Some(ci) = ci_icon(c.status_check_rollup.as_ref().map(|s| s.state.as_str())) {
                b.append(&ci);
            }
            b.append(&label_with(&c.abbreviated_oid, &["sy-mono", "sy-muted", "sy-small"]));
            b.append(&icon("go-next-symbolic", 12));
            row.set_child(Some(&b));
            let oid = c.oid.clone();
            let url = c.url.clone();
            context_menu(&row, move || {
                let m = Menu::ctx();
                let o = oid.clone();
                m.item("Copy SHA", move || util::copy(&o));
                let u = url.clone();
                m.item("Open in browser", move || util::open_url(&u));
                m
            });
            l.append(&row);
        }
        let total = pr.all_commits.total_count.unwrap_or(0);
        if total > pr.all_commits.nodes.len() as i64 {
            let row = gtk::ListBoxRow::new();
            row.set_widget_name("all-commits");
            row.set_child(Some(&label_with(&format!("Show all {total} commits in browser"), &["fg-accent"])));
            l.append(&row);
        }
    }

    fn render_checks(self: &Rc<Self>, l: &gtk::ListBox, pr: &PullRequestDetail) {
        l.remove_all();
        let mut checks = pr.checks();
        checks.sort_by(|a, b| (a.outcome(), a.title()).cmp(&(b.outcome(), b.title())));
        let failed_runs: std::collections::HashSet<i64> = checks.iter().filter(|c| c.outcome() == CheckOutcome::Failure).filter_map(|c| c.run_id()).collect();
        if checks.is_empty() {
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&label_with("No checks for the latest commit", &["sy-muted"])));
            l.append(&row);
        }
        if !failed_runs.is_empty() && self.model.can_write() {
            let b = icon_label_button("view-refresh-symbolic", &format!("Re-run failed jobs ({} workflow)", failed_runs.len()));
            b.add_css_class("flat");
            b.set_halign(gtk::Align::Start);
            let m = self.model.clone();
            b.connect_clicked(move |_| {
                let m = m.clone();
                crate::rt::spawn_local(async move {
                    m.rerun_failed().await;
                });
            });
            let row = gtk::ListBoxRow::new();
            row.set_child(Some(&b));
            l.append(&row);
        }
        for c in checks {
            let row = gtk::ListBoxRow::new();
            let b = hbox(10);
            margins(&b, 2, 0, 2, 0);
            b.append(&outcome_icon(c.outcome()));
            let col = vbox(1);
            col.set_hexpand(true);
            col.append(&ellipsized(&c.workflow_name().map(|w| format!("{w} / {}", c.title())).unwrap_or_else(|| c.title()), &["sy-text"]));
            let mut parts = vec![match c.outcome() {
                CheckOutcome::Success => "Successful".to_string(),
                CheckOutcome::Failure => "Failed".into(),
                CheckOutcome::Pending => {
                    if c.status.as_deref() == Some("QUEUED") {
                        "Queued".into()
                    } else {
                        "In progress".into()
                    }
                }
                CheckOutcome::Neutral => {
                    if c.conclusion.as_deref() == Some("CANCELLED") {
                        "Cancelled".into()
                    } else {
                        "Neutral".into()
                    }
                }
                CheckOutcome::Skipped => "Skipped".into(),
            }];
            if let Some(d) = c.duration() {
                parts.push(d);
            }
            if let Some(d) = c.description.clone().filter(|d| !d.is_empty()) {
                parts.push(d);
            }
            col.append(&ellipsized(&parts.join(" · "), &["sy-muted", "sy-small"]));
            b.append(&col);
            if c.is_actions_job() && c.outcome() != CheckOutcome::Pending && c.outcome() != CheckOutcome::Skipped {
                let log = button("Log");
                log.add_css_class("flat");
                let m = self.model.clone();
                let cc = c.clone();
                log.connect_clicked(move |_| sheets::job_log(m.api.clone(), m.r.repo.clone(), cc.clone()));
                b.append(&log);
            }
            if let Some(link) = c.link() {
                let open = icon_button("sy-external-symbolic", "Open in browser");
                open.connect_clicked(move |_| util::open_url(&link));
                b.append(&open);
            }
            row.set_child(Some(&b));
            l.append(&row);
        }
    }

    fn render_header(self: &Rc<Self>) {
        let pr = self.model.pr.borrow().clone();
        let Some(pr) = pr else { return };
        clear(&self.header_box);
        let top = hbox(8);
        let t = wrap_label(&pr.title, &["sy-title", "sy-text"]);
        t.set_selectable(true);
        top.append(&t);
        top.append(&label_with(&format!("#{}", pr.number), &["sy-title-num"]));
        self.header_box.append(&top);
        let meta = hbox(8);
        meta.append(&state_badge(pr.visual_state(), false));
        meta.append(&avatar(pr.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 18, None));
        meta.append(&label_with(pr.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost"), &["sy-bold", "sy-text"]));
        meta.append(&branch_chip(&pr.base_ref_name, "sky"));
        meta.append(&icon("go-previous-symbolic", 12));
        meta.append(&branch_chip(&pr.head_ref_name, "violet"));
        meta.append(&diff_stat(pr.additions, pr.deletions));
        meta.append(&spacer());
        let created = label_with(&util::rel(Some(pr.created_at)), &["sy-muted"]);
        created.set_tooltip_text(Some(&util::full_date(pr.created_at)));
        meta.append(&created);
        self.header_box.append(&meta);
        let reviewers = self.model.reviewers();
        if !pr.labels.nodes.is_empty() || !reviewers.is_empty() || !pr.assignees.nodes.is_empty() {
            let row = gtk::FlowBox::new();
            row.set_selection_mode(gtk::SelectionMode::None);
            row.set_max_children_per_line(40);
            row.set_column_spacing(6);
            row.set_row_spacing(4);
            for l in &pr.labels.nodes {
                row.insert(&label_chip(l), -1);
            }
            if !reviewers.is_empty() {
                row.insert(&label_with("Reviewers:", &["sy-muted", "sy-small"]), -1);
                for r in &reviewers {
                    row.insert(&reviewer_badge(r), -1);
                }
            }
            if !pr.assignees.nodes.is_empty() {
                row.insert(&label_with("Assignees:", &["sy-muted", "sy-small"]), -1);
                for a in &pr.assignees.nodes {
                    row.insert(&tooltip(avatar(a.avatar_url.as_deref(), 18, None), &a.login), -1);
                }
            }
            self.header_box.append(&row);
        }
        let stack = self.model.stack.borrow().clone();
        if stack.len() > 1 {
            self.header_box.append(&self.stack_strip(&stack, pr.number));
        }
        if let Some(bar) = merge_status_bar(&self.model, &pr, Rc::new({
            let m = self.model.clone();
            move || sheets::merge(m.clone())
        })) {
            self.header_box.append(&bar);
        }
    }

    fn stack_strip(self: &Rc<Self>, stack: &[StackPr], current: i64) -> gtk::Box {
        let index = stack.iter().position(|s| s.number == current).unwrap_or(0);
        let b = vbox(0);
        b.add_css_class("stack-strip");
        let head = hbox(8);
        margins(&head, 6, 10, 6, 10);
        head.append(&colored_icon("sy-stack-symbolic", 14, "indigo"));
        head.append(&label_with("Stack", &["sy-bold", "sy-text"]));
        head.append(&label_with(&format!("{} of {}", index + 1, stack.len()), &["sy-muted"]));
        let pills = hbox(3);
        for s in stack {
            let p = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            p.set_size_request(if s.number == current { 18 } else { 10 }, 5);
            p.set_valign(gtk::Align::Center);
            p.add_css_class(&format!("edge-{}", if s.number == current { "indigo" } else { s.visual_state().css() }));
            if s.number != current {
                p.set_opacity(0.55);
            }
            pills.append(&p);
        }
        head.append(&pills);
        head.append(&spacer());
        let repo = self.model.r.repo.clone();
        let nav = |s: &StackPr, icon_name: &str, help: &str| {
            let btn = gtk::Button::new();
            btn.add_css_class("flat");
            let c = adw::ButtonContent::new();
            c.set_icon_name(icon_name);
            c.set_label(&format!("#{}", s.number));
            btn.set_child(Some(&c));
            btn.set_tooltip_text(Some(&format!("{help}: #{} {}", s.number, s.title)));
            let r = ItemRef { repo: repo.clone(), number: s.number };
            btn.connect_clicked(move |_| app::open(Route::Pr(r.clone()), false));
            btn
        };
        if index > 0 {
            head.append(&nav(&stack[index - 1], "sy-down-symbolic", "Down the stack"));
        }
        if index + 1 < stack.len() {
            head.append(&nav(&stack[index + 1], "sy-up-symbolic", "Up the stack"));
        }
        let expand = gtk::ToggleButton::new();
        expand.set_icon_name("pan-down-symbolic");
        expand.add_css_class("flat");
        expand.set_active(self.stack_expanded.get());
        head.append(&expand);
        b.append(&head);
        let list = vbox(0);
        list.set_visible(self.stack_expanded.get());
        for (i, s) in stack.iter().enumerate().rev() {
            let row = hbox(8);
            margins(&row, 4, 10, 4, 10);
            if s.number == current {
                row.add_css_class("bg-indigo");
            }
            row.append(&label_with(&(i + 1).to_string(), &["sy-dim", "sy-small"]));
            row.append(&state_badge(s.visual_state(), true));
            row.append(&label_with(&format!("#{}", s.number), &["sy-muted"]));
            let t = ellipsized(&s.title, &["sy-text"]);
            if s.number == current {
                t.add_css_class("sy-bold");
            }
            t.set_hexpand(true);
            row.append(&t);
            row.append(&chip(&s.head_ref_name, if s.number == current { "violet" } else { "slate" }, Some("sy-branch-symbolic"), true));
            if s.number != current {
                let r = ItemRef { repo: repo.clone(), number: s.number };
                on_click_widget(&row, move || app::open(Route::Pr(r.clone()), false));
            }
            list.append(&row);
        }
        b.append(&list);
        let w = Rc::downgrade(self);
        let l = list.clone();
        expand.connect_toggled(move |t| {
            l.set_visible(t.is_active());
            if let Some(me) = w.upgrade() {
                me.stack_expanded.set(t.is_active());
            }
        });
        b
    }

    fn render_banners(self: &Rc<Self>, pr: &PullRequestDetail) {
        clear(&self.banners);
        if let Some(b) = self.rereview_banner(pr, true) {
            self.banners.append(&b);
        }
        let m = self.model.clone();
        if let Some(b) = fix_claude::banner(pr, move || fix_claude::show(m.clone())) {
            self.banners.append(&b);
        }
    }

    pub fn rereview_banner(self: &Rc<Self>, pr: &PullRequestDetail, inset: bool) -> Option<gtk::Box> {
        rereview_banner(&self.model, pr, inset, self.rerequested.borrow().clone(), self.sending_rerequest.get(), {
            let w = Rc::downgrade(self);
            Rc::new(move |logins: Vec<String>| {
                let Some(me) = w.upgrade() else { return };
                me.sending_rerequest.set(true);
                me.render();
                let m = me.model.clone();
                crate::rt::spawn_local(async move {
                    let ok = m.request_reviewers(logins.clone()).await;
                    me.sending_rerequest.set(false);
                    if ok {
                        *me.rerequested.borrow_mut() = Some(logins);
                        me.render();
                        crate::rt::sleep(std::time::Duration::from_millis(2500)).await;
                        *me.rerequested.borrow_mut() = None;
                    }
                    me.render();
                });
            })
        })
    }

    fn render_actions(self: &Rc<Self>, pr: &PullRequestDetail) {
        let m = self.model.clone();
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{:?}|{}|{}|{}|{}",
            pr.number,
            pr.state,
            pr.is_draft,
            pr.merged,
            pr.viewer_can_update,
            pr.viewer_did_author,
            m.can_write(),
            pr.merge_state_status,
            pr.auto_merge_request.is_some(),
            pr.head_ref.is_some(),
            m.pending.borrow().len(),
            pr.head_ref_name
        );
        if *self.actions_key.borrow() == key {
            return;
        }
        *self.actions_key.borrow_mut() = key;
        clear(&self.actions);
        if pr.is_open() {
            let pending = m.pending.borrow().len();
            let split = adw::SplitButton::new();
            split.set_label(&if pending == 0 { "Review".to_string() } else { format!("Review ({pending})") });
            split.set_tooltip_text(Some("Submit review"));
            let menu = Menu::new();
            let mm = m.clone();
            menu.item("Write review…", move || sheets::review(mm.clone()));
            if !pr.viewer_did_author {
                let mm = m.clone();
                menu.item("Approve", move || {
                    let mm = mm.clone();
                    dialogs::confirm("Approve this pull request?", None, "Approve", false, move || {
                        let mm = mm.clone();
                        crate::rt::spawn_local(async move {
                            mm.submit_review("APPROVE".into(), String::new()).await;
                        });
                    });
                });
            }
            split.set_menu_model(Some(&menu.model));
            menu.attach(&split);
            let mm = m.clone();
            split.connect_clicked(move |_| sheets::review(mm.clone()));
            self.actions.append(&split);
            let rr = icon_button("sy-person-add-symbolic", "Request or re-request review");
            let mm = m.clone();
            rr.connect_clicked(move |_| sheets::reviewers(mm.clone()));
            self.actions.append(&rr);
        }
        let fix = icon_button("sy-sparkles-symbolic", "Send review comments to a new Claude Code session");
        let mm = m.clone();
        fix.connect_clicked(move |_| fix_claude::show(mm.clone()));
        self.actions.append(&fix);
        if pr.is_open() && !pr.is_draft && m.can_write() {
            let merge = icon_button("sy-merge-symbolic", "Merge pull request");
            let mm = m.clone();
            merge.connect_clicked(move |_| sheets::merge(mm.clone()));
            self.actions.append(&merge);
        }
        let more = self.more_menu(pr);
        self.actions.append(&more.button("view-more-symbolic", "More"));
        let open = icon_button("sy-external-symbolic", "Open on github.com");
        let url = pr.url.clone();
        open.connect_clicked(move |_| util::open_url(&url));
        self.actions.append(&open);
    }

    fn more_menu(self: &Rc<Self>, pr: &PullRequestDetail) -> Menu {
        let mut menu = Menu::new();
        let m = self.model.clone();
        let url = pr.url.clone();
        menu.item("Copy link", move || util::copy(&url));
        let cmd = format!("gh pr checkout {} -R {}", pr.number, m.r.repo.full());
        menu.item(&format!("Copy “gh pr checkout {}”", pr.number), move || util::copy(&cmd));
        let head = pr.head_ref_name.clone();
        menu.item("Copy branch name", move || util::copy(&head));
        menu.section(None);
        if pr.viewer_can_update {
            let mm = m.clone();
            menu.item("Edit title and description…", move || sheets::edit(mm.clone()));
        }
        let simple = |mm: Rc<PrModel>, q: &'static str| {
            move || {
                let mm = mm.clone();
                crate::rt::spawn_local(async move {
                    mm.simple(q).await;
                });
            }
        };
        if pr.is_open() {
            let mm = m.clone();
            menu.item("Request review…", move || sheets::reviewers(mm.clone()));
            if pr.viewer_can_update {
                if pr.is_draft {
                    menu.item("Ready for review", simple(m.clone(), q::READY_FOR_REVIEW));
                } else {
                    menu.item("Convert to draft", simple(m.clone(), q::CONVERT_TO_DRAFT));
                }
            }
            if matches!(pr.merge_state_status.as_deref(), Some("BEHIND") | Some("BLOCKED") | Some("UNSTABLE")) {
                menu.item(&format!("Update branch from {}", pr.base_ref_name), simple(m.clone(), q::UPDATE_BRANCH));
            }
            if pr.auto_merge_request.is_some() {
                let mm = m.clone();
                menu.item("Disable auto-merge", move || {
                    let f = simple(mm.clone(), q::DISABLE_AUTO_MERGE);
                    dialogs::confirm("Disable auto-merge?", None, "Disable", true, f);
                });
            }
            menu.section(None);
            if pr.viewer_can_update {
                let mm = m.clone();
                menu.item("Close pull request…", move || {
                    let f = simple(mm.clone(), q::CLOSE_PR);
                    dialogs::confirm("Close pull request without merging?", None, "Close pull request", true, f);
                });
            }
        } else if !pr.merged && pr.viewer_can_update {
            menu.item("Reopen pull request", simple(m.clone(), q::REOPEN_PR));
        }
        if !pr.is_open() && pr.head_ref.is_some() && m.can_write() {
            let mm = m.clone();
            let head = pr.head_ref_name.clone();
            menu.item(&format!("Delete branch {head}…"), move || {
                let mm = mm.clone();
                dialogs::confirm(&format!("Delete branch {head}?"), None, "Delete branch", true, move || {
                    let mm = mm.clone();
                    crate::rt::spawn_local(async move {
                        mm.delete_branch().await;
                    });
                });
            });
        }
        menu
    }
}

pub fn reviewer_badge(r: &model::ReviewerInfo) -> gtk::Box {
    let b = hbox(3);
    b.append(&avatar(r.avatar.as_deref(), 18, None));
    let (icon_name, color, title) = match r.state.as_str() {
        "APPROVED" => ("sy-check-symbolic", "green", "approved"),
        "CHANGES_REQUESTED" => ("sy-plusminus-symbolic", "red", "changes requested"),
        "COMMENTED" => ("sy-comment-symbolic", "muted", "commented"),
        "DISMISSED" => ("sy-x-symbolic", "muted", "dismissed"),
        _ => ("sy-dot-symbolic", "amber", "review pending"),
    };
    b.append(&colored_icon(icon_name, 11, color));
    b.set_tooltip_text(Some(&format!("{}: {title}", r.login)));
    b
}

pub struct MergeStatus {
    pub icon: &'static str,
    pub color: &'static str,
    pub text: String,
    pub ready: bool,
}

pub fn merge_status(pr: &PullRequestDetail) -> MergeStatus {
    let checks = pr.checks();
    let failed = checks.iter().filter(|c| c.outcome() == CheckOutcome::Failure).count();
    let pending = checks.iter().filter(|c| c.outcome() == CheckOutcome::Pending).count();
    let failed_text = if failed == 1 { "1 check failed".to_string() } else { format!("{failed} checks failed") };
    let pending_text = if pending == 1 { "1 check is still running".to_string() } else { format!("{pending} checks are still running") };
    let s = |icon: &'static str, color: &'static str, text: String, ready: bool| MergeStatus { icon, color, text, ready };
    if pr.is_draft {
        return s("sy-pr-draft-symbolic", "slate", "Draft — merging is unavailable".into(), false);
    }
    if pr.mergeable == "CONFLICTING" || pr.merge_state_status.as_deref() == Some("DIRTY") {
        return s("sy-attention-symbolic", "red", format!("Has conflicts with {}", pr.base_ref_name), false);
    }
    match pr.merge_state_status.as_deref() {
        Some("BEHIND") => s("sy-down-symbolic", "amber", format!("Branch is behind {}", pr.base_ref_name), false),
        Some("BLOCKED") => match pr.review_decision.as_deref() {
            Some("CHANGES_REQUESTED") => s("sy-plusminus-symbolic", "red", "Changes requested".into(), false),
            Some("REVIEW_REQUIRED") => s("sy-eye-symbolic", "amber", "Review required".into(), false),
            _ => {
                if failed > 0 {
                    s("sy-x-symbolic", "red", failed_text, false)
                } else if pending > 0 {
                    s("sy-clock-symbolic", "amber", pending_text, false)
                } else {
                    s("sy-lock-symbolic", "amber", "Blocked by branch protection".into(), false)
                }
            }
        },
        Some("UNSTABLE") => {
            if failed > 0 {
                s("sy-attention-symbolic", "amber", format!("Can be merged, but {}", failed_text.to_lowercase()), true)
            } else if pending > 0 {
                s("sy-clock-symbolic", "amber", pending_text, true)
            } else {
                s("sy-check-symbolic", "green", "Ready to merge".into(), true)
            }
        }
        Some("CLEAN") | Some("HAS_HOOKS") => s("sy-check-symbolic", "green", "Ready to merge".into(), true),
        _ => s("sy-clock-symbolic", "slate", "Checking mergeability…".into(), false),
    }
}

pub fn merge_status_bar(model: &Rc<PrModel>, pr: &PullRequestDetail, on_merge: Rc<dyn Fn()>) -> Option<gtk::Box> {
    if !pr.is_open() {
        return None;
    }
    let st = merge_status(pr);
    let b = hbox(8);
    b.add_css_class("banner-box");
    b.add_css_class(st.color);
    b.append(&colored_icon(st.icon, 14, st.color));
    b.append(&wrap_label(&st.text, &["sy-text"]));
    if let Some(auto) = &pr.auto_merge_request {
        let method = auto.merge_method.as_deref().map(|m| format!(" ({})", sheets::MergeMethod::from_key(m).map(|x| x.title()).unwrap_or(m))).unwrap_or_default();
        b.append(&label_with(&format!("· auto-merge enabled{method}"), &["fg-green"]));
    }
    let checks = pr.checks();
    if !checks.is_empty() {
        let failed = checks.iter().filter(|c| c.outcome() == CheckOutcome::Failure).count();
        let pending = checks.iter().filter(|c| c.outcome() == CheckOutcome::Pending).count();
        let ok = checks.iter().filter(|c| c.outcome() == CheckOutcome::Success).count();
        let btn = gtk::Button::new();
        btn.add_css_class("flat");
        btn.set_tooltip_text(Some("Checks"));
        let c = hbox(6);
        let add = |icon_name: &str, n: usize, color: &str| {
            let x = hbox(2);
            x.append(&colored_icon(icon_name, 12, color));
            x.append(&label_with(&n.to_string(), &[&format!("fg-{color}"), "sy-small"]));
            x
        };
        if failed > 0 {
            c.append(&add("sy-x-symbolic", failed, "red"));
        }
        if pending > 0 {
            c.append(&add("sy-clock-symbolic", pending, "amber"));
        }
        c.append(&add("sy-check-symbolic", ok, "green"));
        btn.set_child(Some(&c));
        let m = model.clone();
        btn.connect_clicked(move |_| m.set_tab(PrTab::Checks));
        b.append(&btn);
    }
    b.append(&spacer());
    if pr.merge_state_status.as_deref() == Some("BEHIND") && (model.can_write() || pr.viewer_can_update) {
        let u = button("Update branch");
        let m = model.clone();
        u.connect_clicked(move |_| {
            let m = m.clone();
            crate::rt::spawn_local(async move {
                m.simple(q::UPDATE_BRANCH).await;
            });
        });
        b.append(&u);
    }
    if model.can_write() && !pr.is_draft {
        let mbtn = if st.ready { tinted("Merge…", "green") } else { button("Merge…") };
        mbtn.connect_clicked(move |_| on_merge());
        b.append(&mbtn);
    }
    Some(b)
}

pub fn rereview_banner(model: &Rc<PrModel>, pr: &PullRequestDetail, inset: bool, requested: Option<Vec<String>>, sending: bool, send: Rc<dyn Fn(Vec<String>)>) -> Option<gtk::Box> {
    let stale = model.stale_reviews();
    let logins: Vec<String> = stale.iter().map(|s| s.0.clone()).collect();
    let can_act = pr.is_open() && (pr.viewer_did_author || pr.viewer_can_update);
    let b = hbox(8);
    b.add_css_class("banner-box");
    b.add_css_class("sky");
    if inset {
        margins(&b, 6, 16, 0, 16);
    }
    if let Some(req) = requested {
        b.append(&colored_icon("sy-check-symbolic", 14, "green"));
        b.append(&label_with("Requested", &["sy-bold", "sy-text"]));
        b.append(&ellipsized(&format!("· Review re-requested from {}", req.iter().map(|l| format!("@{l}")).collect::<Vec<_>>().join(", ")), &["sy-muted"]));
        return Some(b);
    }
    if !can_act || logins.is_empty() {
        return None;
    }
    b.append(&icon_tile("view-refresh-symbolic", "sky", 18));
    let who = if stale.len() == 1 {
        format!("@{}", stale[0].0)
    } else {
        let names: Vec<String> = stale.iter().map(|s| format!("@{}", s.0)).collect();
        format!("{} and {}", names[..names.len() - 1].join(", "), names[names.len() - 1])
    };
    let counts: Vec<usize> = stale.iter().filter_map(|s| s.1).collect();
    let what = if counts.len() == stale.len() {
        counts.iter().max().map(|n| util::plural(*n as i64, "commit", "commits")).unwrap_or_else(|| "new commits".into())
    } else {
        "new commits".to_string()
    };
    let message = if pr.viewer_did_author {
        format!("You pushed {what} after {who} reviewed")
    } else {
        let mut c = what.chars();
        let cap = c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default();
        format!("{cap} pushed after {who} reviewed")
    };
    let msg = ellipsized(&message, &["sy-text"]);
    msg.set_hexpand(false);
    b.append(&msg);
    if let Some(d) = pr.last_commit_date() {
        b.append(&label_with(&format!("· {}", util::rel(Some(d))), &["sy-muted"]));
    }
    b.append(&spacer());
    let other = button("Other…");
    let m = model.clone();
    other.connect_clicked(move |_| sheets::reviewers(m.clone()));
    b.append(&other);
    let btn = gtk::Button::new();
    btn.add_css_class("tint-sky");
    let c = hbox(5);
    if sending {
        c.append(&spinner(12));
    } else {
        c.append(&icon("view-refresh-symbolic", 12));
    }
    c.append(&gtk::Label::new(Some(&if logins.len() == 1 { "Re-request review".to_string() } else { format!("Re-request review ({})", logins.len()) })));
    btn.set_child(Some(&c));
    btn.set_sensitive(!sending && !model.busy.get());
    btn.connect_clicked(move |_| send(logins.clone()));
    b.append(&btn);
    Some(b)
}
