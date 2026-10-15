use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;

use super::dialogs::{self, Sheet};
use super::pr::model::PrModel;
use super::widgets::*;
use crate::api::models::*;
use crate::claude::accounts;
use crate::claude::launcher::{self, PermissionMode, Target};
use crate::fix_prompt::{self, FixOptions, FoundFixPrompt};
use crate::{app, local_repo, settings, util};

pub fn banner(pr: &PullRequestDetail, action: impl Fn() + 'static) -> Option<gtk::Box> {
    let open: Vec<&ReviewThread> = pr.review_threads.nodes.iter().filter(|t| !t.is_resolved).collect();
    let unresolved = open.iter().filter(|t| !t.is_outdated).count();
    let outdated = open.len() - unresolved;
    let last_commit = pr.last_commit_date();
    let found = fix_prompt::latest(pr).filter(|f| match (f.date, last_commit) {
        (Some(d), Some(lc)) if lc > d => unresolved > 0,
        _ => true,
    });
    if !pr.is_open() || (unresolved == 0 && found.is_none()) {
        return None;
    }
    let b = hbox(8);
    b.add_css_class("banner-box");
    b.add_css_class("coral");
    margins(&b, 6, 16, 0, 16);
    b.append(&icon_tile("sy-sparkles-symbolic", "coral", 18));
    if unresolved > 0 {
        b.append(&label_with(&util::plural(unresolved as i64, "unresolved comment", "unresolved comments"), &["sy-text"]));
    }
    if outdated > 0 {
        let l = label_with(&format!("· {outdated} outdated"), &["sy-dim"]);
        l.set_tooltip_text(Some("Code these comments point to has changed since they were written"));
        b.append(&l);
    }
    if let Some(f) = &found {
        if unresolved > 0 {
            b.append(&label_with("·", &["sy-dim"]));
        }
        b.append(&ellipsized(&format!("Fix prompt by @{}{}", f.author, f.date.map(|d| format!(" · {}", util::rel(Some(d)))).unwrap_or_default()), &["sy-muted"]));
    }
    b.append(&spacer());
    let btn = tinted("Fix with Claude Code", "coral");
    btn.connect_clicked(move |_| action());
    b.append(&btn);
    Some(b)
}

struct State {
    model: Rc<PrModel>,
    pr: PullRequestDetail,
    found: Option<FoundFixPrompt>,
    use_reviewer: Cell<bool>,
    selected: RefCell<HashSet<String>>,
    options: RefCell<FixOptions>,
    custom: RefCell<Option<String>>,
    folder: RefCell<Option<String>>,
    detecting: Cell<bool>,
    prompt_view: gtk::TextView,
    edited: gtk::Box,
    count: gtk::Label,
    folder_row: gtk::Box,
    launch: gtk::Button,
    target: Cell<Target>,
    suppress: Cell<bool>,
}

impl State {
    fn threads(&self) -> Vec<ReviewThread> {
        self.pr.review_threads.nodes.iter().filter(|t| !t.is_resolved).cloned().collect()
    }

    fn generated(&self) -> String {
        let options = self.options.borrow().clone();
        if self.use_reviewer.get() {
            if let Some(f) = &self.found {
                return fix_prompt::wrap(f, &self.pr, &options);
            }
        }
        let threads = self.threads();
        let selected = self.selected.borrow();
        let chosen: Vec<&ReviewThread> = threads.iter().filter(|t| selected.contains(&t.id)).collect();
        fix_prompt::build(&self.pr, &self.model.r.repo, &chosen, &fix_prompt::general_feedback(&self.pr), &options)
    }

    fn prompt(&self) -> String {
        self.custom.borrow().clone().unwrap_or_else(|| self.generated())
    }

    fn refresh_prompt(&self) {
        *self.custom.borrow_mut() = None;
        self.suppress.set(true);
        self.prompt_view.buffer().set_text(&self.generated());
        self.suppress.set(false);
        self.edited.set_visible(false);
        self.update();
    }

    fn update(&self) {
        let p = self.prompt();
        self.count.set_text(&p.chars().count().to_string());
        let target = self.target.get();
        self.launch.set_label(if target == Target::Copy { "Copy" } else { "Launch" });
        self.launch.set_sensitive(!p.trim().is_empty() && (target == Target::Copy || self.folder.borrow().is_some()));
    }

    fn render_folder(self: &Rc<Self>) {
        clear(&self.folder_row);
        self.folder_row.append(&colored_icon("sy-folder-symbolic", 14, "teal"));
        let repo = self.model.r.repo.clone();
        if self.detecting.get() {
            self.folder_row.append(&spinner(12));
            self.folder_row.append(&label_with(&format!("Looking for a local clone of {}…", repo.full()), &["sy-muted"]));
        } else if let Some(f) = self.folder.borrow().clone() {
            let l = label_with(&util::tilde(&f), &["sy-mono", "sy-text"]);
            l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            self.folder_row.append(&l);
            if !local_repo::remote_matches(&f, &repo) {
                let warn = hbox(4);
                warn.append(&colored_icon("sy-attention-symbolic", 12, "amber"));
                warn.append(&label_with(&format!("remote doesn't match {}", repo.full()), &["fg-amber", "sy-small"]));
                self.folder_row.append(&warn);
            }
        } else {
            self.folder_row.append(&label_with("No local clone found", &["fg-amber"]));
            let copy = link_button("Copy “gh repo clone”");
            let full = repo.full();
            copy.connect_clicked(move |_| util::copy(&format!("gh repo clone {full}")));
            self.folder_row.append(&copy);
        }
        self.folder_row.append(&spacer());
        let choose = button("Choose folder…");
        let w = Rc::downgrade(self);
        choose.connect_clicked(move |_| {
            let w = w.clone();
            dialogs::choose_folder("Choose the local folder of the repository", move |p| {
                if let Some(me) = w.upgrade() {
                    local_repo::save(&me.model.r.repo, &p);
                    *me.folder.borrow_mut() = Some(p);
                    me.render_folder();
                    me.update();
                }
            });
        });
        self.folder_row.append(&choose);
        self.update();
    }
}

pub fn show(model: Rc<PrModel>) {
    let Some(pr) = model.pr.borrow().clone() else { return };
    let sheet = Sheet::new("Fix with Claude Code", 780, 680);
    let head = hbox(10);
    head.append(&icon_tile("sy-sparkles-symbolic", "coral", 30));
    let hc = vbox(1);
    hc.append(&label_with("Fix with Claude Code", &["heading", "sy-text"]));
    let sub = hbox(6);
    sub.append(&label_with(&format!("#{}", pr.number), &["fg-violet", "sy-bold"]));
    sub.append(&ellipsized(&pr.title, &["sy-muted"]));
    hc.append(&sub);
    head.append(&hc);
    sheet.body.append(&head);

    let found = fix_prompt::latest(&pr);
    let prompt_view = gtk::TextView::new();
    prompt_view.set_monospace(true);
    prompt_view.set_wrap_mode(gtk::WrapMode::WordChar);
    prompt_view.set_left_margin(8);
    prompt_view.set_top_margin(8);
    prompt_view.add_css_class("sy-small");
    let edited = hbox(6);
    edited.append(&chip("edited", "amber", None, false));
    let reset = link_button("Reset");
    edited.append(&reset);
    edited.set_visible(false);
    let count = label_with("", &["sy-dim", "sy-tiny"]);
    let folder_row = hbox(8);
    folder_row.add_css_class("sy-card-flat");
    pad(&folder_row, 8);
    let launch = tinted("Launch", "coral");
    let saved_target = Target::from_key(&settings::read(|s| s.claude_target.clone()));
    let target = if saved_target.available() { saved_target } else { Target::Copy };
    let threads: Vec<ReviewThread> = pr.review_threads.nodes.iter().filter(|t| !t.is_resolved).cloned().collect();
    let mut initial: HashSet<String> = threads.iter().filter(|t| !t.is_outdated).map(|t| t.id.clone()).collect();
    if initial.is_empty() {
        initial = threads.iter().map(|t| t.id.clone()).collect();
    }
    let state = Rc::new(State {
        model: model.clone(),
        pr: pr.clone(),
        found: found.clone(),
        use_reviewer: Cell::new(found.is_some()),
        selected: RefCell::new(initial),
        options: RefCell::new(FixOptions::default()),
        custom: RefCell::new(None),
        folder: RefCell::new(None),
        detecting: Cell::new(true),
        prompt_view: prompt_view.clone(),
        edited: edited.clone(),
        count: count.clone(),
        folder_row: folder_row.clone(),
        launch: launch.clone(),
        target: Cell::new(target),
        suppress: Cell::new(false),
    });

    let composed_box = vbox(4);
    match &found {
        Some(f) => {
            let toggles = hbox(0);
            toggles.add_css_class("linked");
            let a = gtk::ToggleButton::with_label(&format!("Fix prompt by @{}{}", f.author, f.date.map(|d| format!(" · {}", util::rel(Some(d)))).unwrap_or_default()));
            let b = gtk::ToggleButton::with_label(&format!("Build from comments ({})", threads.len()));
            b.set_group(Some(&a));
            a.set_active(true);
            a.set_hexpand(true);
            b.set_hexpand(true);
            toggles.append(&a);
            toggles.append(&b);
            let st = state.clone();
            let cb = composed_box.clone();
            b.connect_toggled(move |t| {
                st.use_reviewer.set(!t.is_active());
                cb.set_visible(t.is_active());
                st.refresh_prompt();
            });
            composed_box.set_visible(false);
            sheet.body.append(&toggles);
        }
        None => {
            let l = hbox(6);
            l.append(&icon("dialog-information-symbolic", 14));
            l.append(&label_with("No Fix prompt in this PR — built from unresolved comments", &["sy-muted"]));
            sheet.body.append(&l);
        }
    }
    let ch = hbox(6);
    let t = label_with("Unresolved comments", &["sy-muted", "sy-small", "sy-bold"]);
    t.set_hexpand(true);
    ch.append(&t);
    let all = link_button("All");
    let none = link_button("None");
    ch.append(&all);
    ch.append(&none);
    composed_box.append(&ch);
    let checks = vbox(4);
    pad(&checks, 8);
    if threads.is_empty() {
        checks.append(&label_with("No unresolved comments", &["sy-dim"]));
    }
    let mut boxes: Vec<(String, gtk::CheckButton)> = vec![];
    for th in &threads {
        let cbtn = gtk::CheckButton::new();
        cbtn.set_active(state.selected.borrow().contains(&th.id));
        let row = hbox(6);
        let p = label_with(&format!("{}{}", th.path, th.line.or(th.original_line).map(|l| format!(":{l}")).unwrap_or_default()), &["sy-mono", "fg-sky", "sy-small"]);
        p.set_ellipsize(gtk::pango::EllipsizeMode::Start);
        p.set_max_width_chars(40);
        row.append(&p);
        if th.is_outdated {
            row.append(&chip("outdated", "amber", None, false));
        }
        let first = th.comments.nodes.first();
        row.append(&ellipsized(
            &format!("@{}: {}", first.and_then(|c| c.author.as_ref()).map(|a| a.login.as_str()).unwrap_or("?"), first.map(|c| c.body.replace('\n', " ")).unwrap_or_default()),
            &["sy-muted", "sy-small"],
        ));
        cbtn.set_child(Some(&row));
        let st = state.clone();
        let id = th.id.clone();
        cbtn.connect_toggled(move |c| {
            if c.is_active() {
                st.selected.borrow_mut().insert(id.clone());
            } else {
                st.selected.borrow_mut().remove(&id);
            }
            st.refresh_prompt();
        });
        checks.append(&cbtn);
        boxes.push((th.id.clone(), cbtn));
    }
    let general = fix_prompt::general_feedback(&pr);
    if !general.is_empty() {
        let g = gtk::CheckButton::with_label(&format!("General review feedback ({})", general.len()));
        g.set_active(true);
        let st = state.clone();
        g.connect_toggled(move |c| {
            st.options.borrow_mut().include_general = c.is_active();
            st.refresh_prompt();
        });
        checks.append(&g);
    }
    let cs = gtk::ScrolledWindow::new();
    cs.set_child(Some(&checks));
    cs.set_max_content_height(130);
    cs.set_propagate_natural_height(true);
    composed_box.append(&frame(&cs, "sy-card-flat"));
    let boxes = Rc::new(boxes);
    let bx = boxes.clone();
    all.connect_clicked(move |_| {
        for (_, b) in bx.iter() {
            b.set_active(true);
        }
    });
    let bx = boxes.clone();
    none.connect_clicked(move |_| {
        for (_, b) in bx.iter() {
            b.set_active(false);
        }
    });
    sheet.body.append(&composed_box);

    let opts = hbox(16);
    let commit = gtk::CheckButton::with_label("Commit");
    commit.set_active(true);
    let push = gtk::CheckButton::with_label("Push");
    let reply = gtk::CheckButton::with_label("Reply & resolve threads");
    reply.set_sensitive(false);
    let st = state.clone();
    commit.connect_toggled(move |c| {
        st.options.borrow_mut().commit = c.is_active();
        st.refresh_prompt();
    });
    let st = state.clone();
    let r2 = reply.clone();
    push.connect_toggled(move |c| {
        st.options.borrow_mut().push = c.is_active();
        r2.set_sensitive(c.is_active());
        if !c.is_active() {
            r2.set_active(false);
        }
        st.refresh_prompt();
    });
    let st = state.clone();
    reply.connect_toggled(move |c| {
        st.options.borrow_mut().reply_and_resolve = c.is_active();
        st.refresh_prompt();
    });
    opts.append(&commit);
    opts.append(&push);
    opts.append(&reply);
    sheet.body.append(&opts);

    let ph = hbox(6);
    ph.append(&label_with("Prompt", &["sy-muted", "sy-small", "sy-bold"]));
    ph.append(&edited);
    ph.append(&spacer());
    ph.append(&count);
    sheet.body.append(&ph);
    let ps = gtk::ScrolledWindow::new();
    ps.set_child(Some(&prompt_view));
    ps.set_min_content_height(220);
    ps.set_vexpand(true);
    sheet.body.append(&frame(&ps, "sy-sunken"));
    let st = state.clone();
    prompt_view.buffer().connect_changed(move |b| {
        if st.suppress.get() {
            return;
        }
        *st.custom.borrow_mut() = Some(b.text(&b.start_iter(), &b.end_iter(), false).to_string());
        st.edited.set_visible(true);
        st.update();
    });
    let st = state.clone();
    reset.connect_clicked(move |_| st.refresh_prompt());
    sheet.body.append(&folder_row);

    let targets: Vec<Target> = Target::ALL.into_iter().filter(|t| t.available()).collect();
    let names: Vec<&str> = targets.iter().map(|t| t.title()).collect();
    let target_dd = gtk::DropDown::from_strings(&names);
    target_dd.set_selected(targets.iter().position(|t| *t == target).unwrap_or(0) as u32);
    let modes = PermissionMode::ALL;
    let mode_names: Vec<&str> = modes.iter().map(|m| m.title()).collect();
    let mode_dd = gtk::DropDown::from_strings(&mode_names);
    let saved_mode = PermissionMode::from_key(&settings::read(|s| s.claude_launch_mode.clone()));
    mode_dd.set_selected(modes.iter().position(|m| *m == saved_mode).unwrap_or(0) as u32);
    mode_dd.set_sensitive(target == Target::Terminal);
    let st = state.clone();
    let md = mode_dd.clone();
    let tg = targets.clone();
    target_dd.connect_selected_notify(move |d| {
        let t = tg.get(d.selected() as usize).copied().unwrap_or(Target::Copy);
        st.target.set(t);
        settings::update(|s| s.claude_target = t.key().to_string());
        md.set_sensitive(t == Target::Terminal);
        st.update();
    });
    mode_dd.connect_selected_notify(move |d| {
        let m = modes.get(d.selected() as usize).copied().unwrap_or(PermissionMode::Standard);
        settings::update(|s| s.claude_launch_mode = m.key().to_string());
    });
    sheet.footer.append(&label_with("Open in", &["sy-muted"]));
    sheet.footer.append(&target_dd);
    sheet.footer.append(&label_with("Mode", &["sy-muted"]));
    sheet.footer.append(&mode_dd);
    sheet.add_footer_spacer();
    sheet.cancel_button();
    sheet.footer.append(&launch);
    let d = sheet.dialog.clone();
    let st = state.clone();
    launch.connect_clicked(move |_| {
        let prompt = st.prompt();
        let name = format!("Fix PR #{}: {}", st.pr.number, st.pr.title);
        let folder = st.folder.borrow().clone();
        let target = st.target.get();
        if target == Target::App {
            if let Some(f) = folder {
                app::get().claude.new_session(&f, Some(prompt), Some(name), None, None);
                app::set_section(app::Section::Claude);
                d.close();
            }
            return;
        }
        let mode = modes.get(mode_dd.selected() as usize).copied().unwrap_or(PermissionMode::Standard);
        match launcher::launch(&prompt, folder.as_deref().unwrap_or(""), target, mode, &name, None, accounts::active_override().as_deref()) {
            Ok(()) => {
                if target == Target::Copy {
                    app::toast("Prompt copied");
                }
                d.close();
            }
            Err(e) => dialogs::error(&e),
        }
    });
    state.refresh_prompt();
    state.render_folder();
    let st = state.clone();
    let repo = model.r.repo.clone();
    crate::rt::spawn_local(async move {
        let f = local_repo::detect(&repo).await;
        *st.folder.borrow_mut() = f;
        st.detecting.set(false);
        st.render_folder();
    });
    sheet.present();
}
