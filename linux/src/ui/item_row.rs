use adw::prelude::*;

use super::menu::Menu;
use super::widgets::*;
use crate::api::models::SearchItem;
use crate::util;

pub fn item_row(item: &SearchItem, show_repo: bool) -> gtk::Box {
    let row = hbox(10);
    margins(&row, 5, 0, 5, 0);
    row.append(&state_badge(item.visual_state(), true));
    let col = vbox(3);
    col.set_hexpand(true);
    let title = wrap_label(&item.title, &["sy-bold", "sy-text"]);
    title.set_lines(2);
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    col.append(&title);
    let meta = hbox(5);
    let where_ = if show_repo { format!("{}#{}", item.repository.name_with_owner, item.number) } else { format!("#{}", item.number) };
    meta.append(&ellipsized(&where_, &["sy-muted", "sy-small"]));
    meta.append(&label_with("·", &["sy-muted", "sy-small"]));
    meta.append(&label_with(&util::rel(Some(item.updated_at)), &["sy-muted", "sy-small"]));
    col.append(&meta);
    let info = hbox(6);
    info.append(&avatar(item.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 14, None));
    info.append(&label_with(item.author.as_ref().map(|a| a.login.as_str()).unwrap_or(""), &["sy-muted", "sy-small"]));
    if item.is_pr() {
        if let Some(ci) = ci_icon(item.ci_state().as_deref()) {
            info.append(&ci);
        }
        match item.review_decision.as_deref() {
            Some("APPROVED") => info.append(&tooltip(colored_icon("sy-check-symbolic", 12, "green"), "Approved")),
            Some("CHANGES_REQUESTED") => info.append(&tooltip(colored_icon("sy-plusminus-symbolic", 12, "red"), "Changes requested")),
            Some("REVIEW_REQUIRED") => info.append(&tooltip(colored_icon("sy-eye-symbolic", 12, "muted"), "Review required")),
            _ => {}
        }
        if let (Some(a), Some(d)) = (item.additions, item.deletions) {
            info.append(&diff_stat(a, d));
        }
    }
    if item.comments.total_count > 0 {
        let c = hbox(3);
        c.append(&colored_icon("sy-comment-symbolic", 11, "muted"));
        c.append(&label_with(&item.comments.total_count.to_string(), &["sy-muted", "sy-small"]));
        info.append(&c);
    }
    col.append(&info);
    if let Some(base) = item.stacked_on() {
        let s = hbox(4);
        s.append(&colored_icon("sy-stack-symbolic", 12, "indigo"));
        let l = label_with(&format!("stacked on {base}"), &["fg-indigo", "sy-small"]);
        l.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        s.append(&l);
        s.set_tooltip_text(Some(&base));
        col.append(&s);
    }
    let labels = item.labels();
    if !labels.is_empty() {
        let lb = hbox(4);
        for l in labels.iter().take(4) {
            lb.append(&label_chip(l));
        }
        col.append(&lb);
    }
    row.append(&col);
    row
}

pub fn item_menu(item: &SearchItem) -> Menu {
    let menu = Menu::ctx();
    let url = item.url.clone();
    menu.item("Open in Browser", move || util::open_url(&url));
    let url = item.url.clone();
    menu.item("Copy Link", move || util::copy(&url));
    if item.is_pr() {
        let cmd = format!("gh pr checkout {} -R {}", item.number, item.repository.name_with_owner);
        menu.item("Copy “gh pr checkout”", move || util::copy(&cmd));
    }
    menu
}
