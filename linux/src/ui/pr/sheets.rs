use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use adw::prelude::*;

use super::model::{Change, PrModel};
use crate::api::models::{CheckContext, RepoRef, ReviewThread};
use crate::api::GitHubApi;
use crate::ui::composer::Composer;
use crate::ui::dialogs::Sheet;
use crate::ui::widgets::*;
use crate::util;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    pub const ALL: [MergeMethod; 3] = [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase];

    pub fn key(&self) -> &'static str {
        match self {
            MergeMethod::Merge => "MERGE",
            MergeMethod::Squash => "SQUASH",
            MergeMethod::Rebase => "REBASE",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == k)
    }

    pub fn title(&self) -> &'static str {
        match self {
            MergeMethod::Merge => "Merge commit",
            MergeMethod::Squash => "Squash and merge",
            MergeMethod::Rebase => "Rebase and merge",
        }
    }
}

fn close_on_success(sheet: &Sheet) -> impl Fn(bool) + Clone {
    let d = sheet.dialog.clone();
    move |ok: bool| {
        if ok {
            d.close();
        }
    }
}

pub fn review(model: Rc<PrModel>) {
    let Some(pr) = model.pr.borrow().clone() else { return };
    let sheet = Sheet::new(&format!("Review #{}", pr.number), 560, -1);
    let pending = model.pending.borrow().len();
    if pending > 0 {
        let l = hbox(6);
        l.append(&icon("sy-comment-symbolic", 13));
        l.append(&label_with(&format!("Inline comments to be submitted: {pending}"), &["sy-muted"]));
        sheet.body.append(&l);
    }
    let composer = Composer::new("Review summary (optional when approving)", 140);
    sheet.body.append(&composer.widget);
    let comment = gtk::CheckButton::with_label("Comment — without approval");
    comment.set_active(true);
    let approve = gtk::CheckButton::with_label("Approve");
    approve.set_group(Some(&comment));
    let changes = gtk::CheckButton::with_label("Request changes");
    changes.set_group(Some(&comment));
    approve.set_sensitive(!pr.viewer_did_author);
    changes.set_sensitive(!pr.viewer_did_author);
    sheet.body.append(&comment);
    sheet.body.append(&approve);
    sheet.body.append(&changes);
    if pending > 0 {
        let discard = destructive("Discard pending comments");
        let m = model.clone();
        let d = sheet.dialog.clone();
        discard.connect_clicked(move |_| {
            m.pending.borrow_mut().clear();
            m.emit(Change::Diff);
            m.emit(Change::Pr);
            d.close();
        });
        sheet.footer.append(&discard);
    }
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let submit = suggested("Submit review");
    sheet.footer.append(&submit);
    let done = close_on_success(&sheet);
    let m = model.clone();
    let comp = composer.clone();
    let go = Rc::new(move || {
        let event = if approve.is_active() {
            "APPROVE"
        } else if changes.is_active() {
            "REQUEST_CHANGES"
        } else {
            "COMMENT"
        };
        if m.busy.get() || (event != "APPROVE" && comp.is_blank() && m.pending.borrow().is_empty()) {
            return;
        }
        let body = comp.text();
        let m = m.clone();
        let done = done.clone();
        crate::rt::spawn_local(async move { done(m.submit_review(event.to_string(), body).await) });
    });
    let g = go.clone();
    submit.connect_clicked(move |_| g());
    let g = go.clone();
    composer.on_submit(move || g());
    sheet.present();
    composer.grab_focus();
}

pub fn merge(model: Rc<PrModel>) {
    let Some(pr) = model.pr.borrow().clone() else { return };
    let repo = model.repo.borrow().clone();
    let allowed: Vec<MergeMethod> = match &repo {
        None => MergeMethod::ALL.to_vec(),
        Some(r) => MergeMethod::ALL
            .into_iter()
            .filter(|m| match m {
                MergeMethod::Merge => r.merge_commit_allowed,
                MergeMethod::Squash => r.squash_merge_allowed,
                MergeMethod::Rebase => r.rebase_merge_allowed,
            })
            .collect(),
    };
    let sheet = Sheet::new(&format!("Merge #{} into {}", pr.number, pr.base_ref_name), 560, -1);
    let names: Vec<&str> = allowed.iter().map(|m| m.title()).collect();
    let method = gtk::DropDown::from_strings(&names);
    let default = MergeMethod::from_key(repo.as_ref().map(|r| r.viewer_default_merge_method.as_str()).unwrap_or("MERGE")).unwrap_or(MergeMethod::Merge);
    let initial = allowed.iter().position(|m| *m == default).unwrap_or(0);
    method.set_selected(initial as u32);
    let row = hbox(8);
    row.append(&label_with("Method", &["sy-muted"]));
    row.append(&method);
    sheet.body.append(&row);
    let title = gtk::Entry::new();
    title.set_placeholder_text(Some("Commit title (GitHub default)"));
    let message = Composer::new("Commit description (optional)", 80);
    let commit_box = vbox(8);
    commit_box.append(&title);
    commit_box.append(&message.widget);
    sheet.body.append(&commit_box);
    let delete_on_merge = repo.as_ref().map(|r| r.delete_branch_on_merge).unwrap_or(false);
    let same_repo = pr.head_repository.as_ref().map(|h| h.name_with_owner == model.r.repo.full()).unwrap_or(false);
    let delete = gtk::CheckButton::with_label(&format!("Delete branch {} after merge", pr.head_ref_name));
    delete.set_active(true);
    delete.set_visible(!delete_on_merge && same_repo);
    sheet.body.append(&delete);
    let prefill = {
        let title = title.clone();
        let commit_box = commit_box.clone();
        let pr_title = pr.title.clone();
        let number = pr.number;
        let allowed = allowed.clone();
        Rc::new(move |idx: u32| {
            let m = allowed.get(idx as usize).copied().unwrap_or(MergeMethod::Merge);
            commit_box.set_visible(m != MergeMethod::Rebase);
            title.set_text(&if m == MergeMethod::Squash { format!("{pr_title} (#{number})") } else { String::new() });
        })
    };
    prefill(initial as u32);
    let p = prefill.clone();
    method.connect_selected_notify(move |d| p(d.selected()));
    let done = close_on_success(&sheet);
    if repo.as_ref().map(|r| r.auto_merge_allowed).unwrap_or(false) && pr.auto_merge_request.is_none() && pr.merge_state_status.as_deref() != Some("CLEAN") {
        let auto = button("Enable auto-merge");
        auto.set_tooltip_text(Some("Merge automatically when checks and reviews pass"));
        let m = model.clone();
        let method = method.clone();
        let allowed = allowed.clone();
        let done = done.clone();
        auto.connect_clicked(move |_| {
            let key = allowed.get(method.selected() as usize).copied().unwrap_or(MergeMethod::Merge).key().to_string();
            let m = m.clone();
            let done = done.clone();
            crate::rt::spawn_local(async move { done(m.enable_auto_merge(key).await) });
        });
        sheet.footer.append(&auto);
    }
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let go = tinted("Merge", "green");
    go.set_sensitive(!allowed.is_empty());
    sheet.footer.append(&go);
    let m = model.clone();
    go.connect_clicked(move |_| {
        if m.busy.get() {
            return;
        }
        let key = allowed.get(method.selected() as usize).copied().unwrap_or(MergeMethod::Merge).key().to_string();
        let t = title.text().trim().to_string();
        let b = message.text().trim().to_string();
        let del = delete.is_active() && !delete_on_merge && same_repo;
        let m = m.clone();
        let done = done.clone();
        crate::rt::spawn_local(async move { done(m.merge(key, if t.is_empty() { None } else { Some(t) }, if b.is_empty() { None } else { Some(b) }, del).await) });
    });
    sheet.present();
}

pub fn edit(model: Rc<PrModel>) {
    let Some(pr) = model.pr.borrow().clone() else { return };
    let sheet = Sheet::new(&format!("Edit #{}", pr.number), 680, 560);
    let title = gtk::Entry::new();
    title.set_text(&pr.title);
    title.set_placeholder_text(Some("Title"));
    sheet.body.append(&title);
    let body = Composer::new("Description (Markdown)", 260);
    body.set_text(&pr.body);
    sheet.body.append(&body.widget);
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let save = suggested("Save");
    sheet.footer.append(&save);
    let done = close_on_success(&sheet);
    let m = model.clone();
    let b2 = body.clone();
    let go = Rc::new(move || {
        let t = title.text().to_string();
        if t.trim().is_empty() || m.busy.get() {
            return;
        }
        let text = b2.text();
        let m = m.clone();
        let done = done.clone();
        crate::rt::spawn_local(async move { done(m.edit(t, text).await) });
    });
    let g = go.clone();
    save.connect_clicked(move |_| g());
    let g = go.clone();
    body.on_submit(move || g());
    sheet.present();
}

pub fn reviewers(model: Rc<PrModel>) {
    let sheet = Sheet::new("Request review", 560, -1);
    let viewer = model.viewer_login.borrow().clone();
    let candidates: Vec<super::model::ReviewerInfo> = model.reviewers().into_iter().filter(|r| r.login != viewer && !r.login.ends_with("[bot]") && !r.login.contains('/')).collect();
    let rerequest: HashSet<String> = model.reviewers_to_rerequest().into_iter().collect();
    let selected: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(rerequest.clone()));
    let go = suggested("Request");
    if !candidates.is_empty() {
        sheet.body.append(&label_with("Reviewers on this pull request", &["sy-muted", "sy-small"]));
        for r in &candidates {
            let check = gtk::CheckButton::new();
            check.set_active(selected.borrow().contains(&r.login));
            let content = hbox(6);
            content.append(&super::reviewer_badge(r));
            content.append(&label_with(&r.login, &["sy-text"]));
            if rerequest.contains(&r.login) {
                content.append(&label_with("reviewed before latest push", &["fg-amber", "sy-small"]));
            } else if r.state == "PENDING" {
                content.append(&label_with("already requested", &["sy-muted", "sy-small"]));
            }
            check.set_child(Some(&content));
            let s = selected.clone();
            let login = r.login.clone();
            check.connect_toggled(move |c| {
                if c.is_active() {
                    s.borrow_mut().insert(login.clone());
                } else {
                    s.borrow_mut().remove(&login);
                }
            });
            sheet.body.append(&check);
        }
    }
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Other logins, comma-separated, e.g. alice, bob"));
    sheet.body.append(&entry);
    sheet.add_footer_spacer();
    sheet.cancel_button();
    sheet.footer.append(&go);
    let requested = {
        let selected = selected.clone();
        let entry = entry.clone();
        let candidates = candidates.clone();
        Rc::new(move || -> Vec<String> {
            let typed: Vec<String> = entry.text().split([',', ' ']).map(|s| s.trim_matches(|c| c == '@' || c == ' ').to_string()).filter(|s| !s.is_empty()).collect();
            let mut seen = HashSet::new();
            candidates.iter().map(|c| c.login.clone()).filter(|l| selected.borrow().contains(l)).chain(typed).filter(|l| seen.insert(l.clone())).collect()
        })
    };
    let update = {
        let go = go.clone();
        let requested = requested.clone();
        let m = model.clone();
        Rc::new(move || {
            let n = requested().len();
            go.set_label(&if n == 0 { "Request".to_string() } else { format!("Request ({n})") });
            go.set_sensitive(n > 0 && !m.busy.get());
        })
    };
    update();
    let u = update.clone();
    entry.connect_changed(move |_| u());
    let mut c = sheet.body.first_child();
    while let Some(w) = c {
        if let Ok(check) = w.clone().downcast::<gtk::CheckButton>() {
            let u = update.clone();
            check.connect_toggled(move |_| u());
        }
        c = w.next_sibling();
    }
    let done = close_on_success(&sheet);
    let m = model.clone();
    let send = Rc::new(move || {
        let list = requested();
        if list.is_empty() {
            return;
        }
        let m = m.clone();
        let done = done.clone();
        crate::rt::spawn_local(async move { done(m.request_reviewers(list).await) });
    });
    let s = send.clone();
    go.connect_clicked(move |_| s());
    let s = send.clone();
    entry.connect_activate(move |_| s());
    sheet.present();
}

pub fn reply(model: Rc<PrModel>, thread: ReviewThread) {
    let sheet = Sheet::new("Reply to thread", 560, -1);
    sheet.body.append(&label_with(&format!("{}{}", thread.path, thread.line.map(|l| format!(":{l}")).unwrap_or_default()), &["sy-mono", "sy-muted", "sy-small"]));
    if let Some(last) = thread.comments.nodes.last() {
        let row = hbox(8);
        row.add_css_class("sy-sunken");
        pad(&row, 8);
        let av = avatar(last.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 18, None);
        av.set_valign(gtk::Align::Start);
        row.append(&av);
        let body = crate::ui::markdown::inline_label(&last.body);
        body.set_lines(6);
        body.set_ellipsize(gtk::pango::EllipsizeMode::End);
        row.append(&body);
        sheet.body.append(&row);
    }
    let composer = Composer::new("Reply (Markdown)", 120);
    sheet.body.append(&composer.widget);
    let done = close_on_success(&sheet);
    let send = {
        let m = model.clone();
        let comp = composer.clone();
        let id = thread.id.clone();
        Rc::new(move |resolve: bool| {
            if comp.is_blank() || m.busy.get() {
                return;
            }
            let body = comp.text();
            let m = m.clone();
            let id = id.clone();
            let done = done.clone();
            crate::rt::spawn_local(async move {
                if m.clone().reply(id.clone(), body).await {
                    if resolve {
                        m.clone().set_resolved(id, true).await;
                    }
                    done(true);
                }
            });
        })
    };
    if thread.viewer_can_resolve && !thread.is_resolved {
        let rr = button("Reply and resolve");
        let s = send.clone();
        rr.connect_clicked(move |_| s(true));
        sheet.footer.append(&rr);
    }
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let go = suggested("Reply");
    let s = send.clone();
    go.connect_clicked(move |_| s(false));
    let s = send.clone();
    composer.on_submit(move || s(false));
    sheet.footer.append(&go);
    sheet.present();
    composer.grab_focus();
}

pub fn clean_log(raw: &str) -> String {
    raw.split('\n')
        .map(|line| {
            let l = line.strip_prefix('\u{feff}').unwrap_or(line);
            let chars: Vec<char> = l.chars().collect();
            if chars.len() > 29 && chars.get(4) == Some(&'-') {
                if let Some(space) = l.find(' ') {
                    if (20..=34).contains(&space) {
                        return l[space + 1..].to_string();
                    }
                }
            }
            l.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn filter_errors(s: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let mut keep: Vec<String> = vec![];
    for (i, l) in lines.iter().enumerate() {
        if l.contains("##[error]") || l.to_lowercase().contains("error") || l.contains("FAILED") || l.contains('✗') {
            let from = i.saturating_sub(3);
            if keep.last().map(|x| x != "…").unwrap_or(false) {
                keep.push("…".into());
            }
            keep.extend(lines[from..=i].iter().map(|x| x.to_string()));
        }
    }
    if keep.is_empty() {
        "No error lines found".into()
    } else {
        keep.join("\n")
    }
}

struct LogText {
    text: String,
    tags: Vec<(i32, usize)>,
}

const LOG_TAGS: [(&str, &str); 3] = [("err", "#e01b24"), ("warn", "#e66100"), ("group", "#3584e4")];

fn log_text(text: String) -> LogText {
    let tags = text
        .split('\n')
        .enumerate()
        .filter_map(|(i, line)| {
            let kind = if line.contains("##[error]") {
                0
            } else if line.contains("##[warning]") {
                1
            } else if line.starts_with("##[group]") {
                2
            } else {
                return None;
            };
            Some((i as i32, kind))
        })
        .collect();
    LogText { text, tags }
}

fn log_tag_table() -> gtk::TextTagTable {
    let table = gtk::TextTagTable::new();
    for (name, color) in LOG_TAGS {
        let t = gtk::TextTag::new(Some(name));
        t.set_foreground(Some(color));
        table.add(&t);
    }
    table
}

fn log_buffer(table: &gtk::TextTagTable, log: &LogText) -> gtk::TextBuffer {
    let _span = crate::diagnostics::ui_span("cilog.render");
    let buffer = gtk::TextBuffer::new(Some(table));
    buffer.set_text(&log.text);
    let tags: Vec<Option<gtk::TextTag>> = LOG_TAGS.iter().map(|(name, _)| table.lookup(name)).collect();
    for &(line, kind) in &log.tags {
        let (Some(t), Some(start)) = (tags[kind].as_ref(), buffer.iter_at_line(line)) else { continue };
        let mut end = start;
        end.forward_to_line_end();
        buffer.apply_tag(t, &start, &end);
    }
    buffer
}

fn show_log(view: &gtk::TextView, buffer: &gtk::TextBuffer) {
    view.set_buffer(Some(buffer));
    let end = buffer.end_iter();
    let mark = buffer.create_mark(None, &end, false);
    view.scroll_mark_onscreen(&mark);
}

pub fn job_log(api: GitHubApi, repo: RepoRef, check: CheckContext, fix: Option<Rc<dyn Fn(Option<String>)>>) {
    let sheet = Sheet::new(&check.workflow_name().map(|w| format!("{w} / {}", check.title())).unwrap_or_else(|| check.title()), 980, 640);
    let stack = gtk::Stack::new();
    stack.add_named(&loading(), Some("loading"));
    let view = gtk::TextView::new();
    view.set_editable(false);
    view.set_monospace(true);
    view.add_css_class("log-view");
    view.set_left_margin(8);
    view.set_top_margin(8);
    stack.add_named(&scrolled_both(&view), Some("log"));
    stack.set_size_request(-1, 480);
    sheet.body.append(&stack);
    let errors_only = gtk::CheckButton::with_label("Errors only");
    sheet.footer.append(&errors_only);
    let copy = button("Copy");
    copy.set_sensitive(false);
    sheet.footer.append(&copy);
    if let Some(link) = check.link() {
        let open = button("Open in browser");
        open.connect_clicked(move |_| util::open_url(&link));
        sheet.footer.append(&open);
    }
    sheet.add_footer_spacer();
    let log: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    if let Some(fix) = fix {
        let b = tinted("Fix with Claude Code", "coral");
        let d = sheet.dialog.clone();
        let l = log.clone();
        b.connect_clicked(move |_| {
            let text = l.borrow().clone();
            d.close();
            fix(text);
        });
        sheet.footer.append(&b);
    }
    let close = button("Close");
    let d = sheet.dialog.clone();
    close.connect_clicked(move |_| {
        d.close();
    });
    sheet.footer.append(&close);
    let l = log.clone();
    copy.connect_clicked(move |_| {
        if let Some(s) = l.borrow().as_ref() {
            util::copy(s);
        }
    });
    let table = log_tag_table();
    let full: Rc<RefCell<Option<gtk::TextBuffer>>> = Rc::new(RefCell::new(None));
    let errors: Rc<RefCell<Option<gtk::TextBuffer>>> = Rc::new(RefCell::new(None));
    let errors_text: Rc<RefCell<Option<LogText>>> = Rc::new(RefCell::new(None));
    let errors_buffer = {
        let table = table.clone();
        let errors = errors.clone();
        let errors_text = errors_text.clone();
        Rc::new(move || -> Option<gtk::TextBuffer> {
            if let Some(b) = errors.borrow().as_ref() {
                return Some(b.clone());
            }
            let b = log_buffer(&table, errors_text.borrow().as_ref()?);
            *errors.borrow_mut() = Some(b.clone());
            Some(b)
        })
    };
    let f = full.clone();
    let v = view.clone();
    let eb = errors_buffer.clone();
    errors_only.connect_toggled(move |c| {
        let buffer = if c.is_active() { eb() } else { f.borrow().clone() };
        if let Some(b) = buffer {
            show_log(&v, &b);
        }
    });
    sheet.present();
    let Some(job) = check.database_id else { return };
    crate::rt::spawn_local(async move {
        let res = crate::rt::run(async move { api.job_log(&repo, job).await }).await;
        let res = match res {
            Ok(raw) => Ok(crate::rt::blocking(move || {
                let text = clean_log(&raw);
                let errors = log_text(filter_errors(&text));
                (log_text(text), errors)
            })
            .await),
            Err(e) => Err(e),
        };
        match res {
            Ok((text, errs)) => {
                let buffer = log_buffer(&table, &text);
                *errors_text.borrow_mut() = Some(errs);
                let target = if errors_only.is_active() { errors_buffer() } else { Some(buffer.clone()) };
                *full.borrow_mut() = Some(buffer);
                if let Some(b) = target {
                    show_log(&view, &b);
                }
                *log.borrow_mut() = Some(text.text);
                copy.set_sensitive(true);
                stack.set_visible_child_name("log");
                gtk::glib::idle_add_local_once(move || {
                    errors_buffer();
                });
            }
            Err(e) => {
                stack.add_named(&error_state(&e.message, None), Some("error"));
                stack.set_visible_child_name("error");
            }
        }
    });
}
