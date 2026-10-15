use adw::prelude::*;

use super::widgets::*;
use crate::api::models::SearchItem;
use crate::app::{self, PrFilter, Section};
use crate::{updater, util};

fn section(container: &gtk::Box, popover: &gtk::Popover, title: &str, count: i64, items: &[SearchItem], filter: PrFilter) {
    let head = gtk::Button::new();
    head.add_css_class("flat");
    let hb = hbox(6);
    hb.append(&icon_tile(filter.icon(), filter.tint(), 16));
    let t = label_with(&title.to_uppercase(), &["sy-muted", "sy-tiny", "sy-bold"]);
    t.set_hexpand(true);
    hb.append(&t);
    let count_class = if count > 0 { format!("fg-{}", filter.tint()) } else { "sy-dim".to_string() };
    hb.append(&label_with(&count.to_string(), &[count_class.as_str(), "sy-small", "sy-bold"]));
    head.set_child(Some(&hb));
    let p = popover.clone();
    head.connect_clicked(move |_| {
        p.popdown();
        app::set_section(Section::Prs(filter));
        app::get().present();
    });
    container.append(&head);
    if items.is_empty() {
        let l = label_with("Nothing here", &["sy-dim"]);
        margins(&l, 0, 8, 6, 12);
        container.append(&l);
    }
    for item in items {
        let b = gtk::Button::new();
        b.add_css_class("flat");
        let row = hbox(8);
        row.append(&state_badge(item.visual_state(), true));
        let col = vbox(1);
        col.set_hexpand(true);
        col.append(&ellipsized(&item.title, &["sy-text", "sy-bold"]));
        col.append(&ellipsized(
            &format!("{}#{} · {} · {}", item.repository.name_with_owner, item.number, item.author.as_ref().map(|a| a.login.as_str()).unwrap_or(""), util::rel(Some(item.updated_at))),
            &["sy-muted", "sy-small"],
        ));
        row.append(&col);
        if let Some(ci) = ci_icon(item.ci_state().as_deref()) {
            row.append(&ci);
        }
        b.set_child(Some(&row));
        let route = item.route();
        let p = popover.clone();
        b.connect_clicked(move |_| {
            p.popdown();
            app::select(Some(route.clone()));
            app::get().present();
        });
        container.append(&b);
    }
}

fn fill(container: &gtk::Box, popover: &gtk::Popover) {
    clear(container);
    let a = app::get();
    if let Some(v) = updater::available_version() {
        let b = gtk::Button::new();
        b.add_css_class("flat");
        b.add_css_class("update-row");
        let row = hbox(8);
        row.append(&colored_icon("sy-download-symbolic", 14, "sky"));
        row.append(&label_with(&format!("Update to {v}"), &["sy-text"]));
        b.set_child(Some(&row));
        let p = popover.clone();
        b.connect_clicked(move |_| {
            p.popdown();
            updater::present();
        });
        container.append(&b);
    }
    let (rr, rr_count, mine, mine_count, unread) = {
        let s = a.state.borrow();
        (s.review_requests.clone(), s.review_request_count, s.my_prs.clone(), s.my_pr_count, s.unread_count)
    };
    section(container, popover, "Review requests", rr_count, &rr, PrFilter::ReviewRequested);
    section(container, popover, "My open PRs", mine_count, &mine, PrFilter::Mine);
    let n = gtk::Button::new();
    n.add_css_class("flat");
    let row = hbox(8);
    row.append(&icon_tile("sy-bell-symbolic", "amber", 16));
    let l = label_with("Notifications", &["sy-text"]);
    l.set_hexpand(true);
    row.append(&l);
    if unread > 0 {
        row.append(&chip(&if unread >= 50 { "50+".to_string() } else { unread.to_string() }, "amber", None, false));
    }
    n.set_child(Some(&row));
    let p = popover.clone();
    n.connect_clicked(move |_| {
        p.popdown();
        app::set_section(Section::Notifications);
    });
    container.append(&n);
}

pub fn button() -> gtk::MenuButton {
    let b = gtk::MenuButton::new();
    b.set_tooltip_text(Some("Review requests and your open pull requests"));
    let content = hbox(3);
    content.append(&icon("sy-eye-symbolic", 14));
    let count = label_with("", &["sy-small", "sy-bold"]);
    content.append(&count);
    b.set_child(Some(&content));
    let popover = gtk::Popover::new();
    let col = vbox(2);
    pad(&col, 6);
    let header = hbox(8);
    margins(&header, 4, 6, 6, 6);
    header.append(&label_with(crate::brand::APP_NAME, &["heading"]));
    let user = label_with("", &["sy-muted"]);
    header.append(&user);
    header.append(&spacer());
    let refresh = icon_button("view-refresh-symbolic", "Refresh");
    refresh.connect_clicked(|_| {
        let a = app::get();
        crate::rt::spawn_local(async move { a.refresh_badges().await });
    });
    header.append(&refresh);
    col.append(&header);
    col.append(&line());
    let items = vbox(2);
    let scroll = gtk::ScrolledWindow::new();
    scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
    scroll.set_propagate_natural_height(true);
    scroll.set_max_content_height(460);
    scroll.set_min_content_width(360);
    scroll.set_child(Some(&items));
    col.append(&scroll);
    popover.set_child(Some(&col));
    b.set_popover(Some(&popover));
    let p = popover.clone();
    let it = items.clone();
    let u = user.clone();
    popover.connect_show(move |_| {
        if let Some(login) = app::get().state.borrow().user.as_ref().map(|u| u.login.clone()) {
            u.set_text(&format!("@{login}"));
        }
        fill(&it, &p);
    });
    let c = count.downgrade();
    app::on_badges(move |_| match c.upgrade() {
        Some(c) => {
            let n = app::get().state.borrow().review_request_count;
            c.set_text(&if n > 0 { n.to_string() } else { String::new() });
            true
        }
        None => false,
    });
    b
}
