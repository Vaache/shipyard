use std::cell::RefCell;
use std::collections::HashMap;

use adw::prelude::*;

use super::widgets::*;
use crate::api::models::SearchItem;
use crate::{app, util};

thread_local! {
    static ITEMS: RefCell<HashMap<String, SearchItem>> = RefCell::new(HashMap::new());
}

pub fn remember(items: &[SearchItem]) {
    ITEMS.with(|m| {
        let mut m = m.borrow_mut();
        if m.len() > 2000 {
            m.clear();
        }
        for item in items {
            m.insert(item.url.clone(), item.clone());
        }
    });
}

fn lookup(url: &str) -> Option<SearchItem> {
    ITEMS.with(|m| m.borrow().get(url).cloned())
}

fn fact(grid: &gtk::Grid, row: i32, title: &str, value: &gtk::Widget) {
    let t = label_with(title, &["sy-muted", "sy-small"]);
    t.set_xalign(0.0);
    t.set_valign(gtk::Align::Start);
    grid.attach(&t, 0, row, 1, 1);
    value.set_halign(gtk::Align::Start);
    grid.attach(value, 1, row, 1, 1);
}

fn text(value: &str) -> gtk::Widget {
    let l = label_with(value, &["sy-text"]);
    l.set_selectable(true);
    l.set_xalign(0.0);
    l.upcast()
}

pub fn page(url: &str) -> adw::NavigationPage {
    let provider = app::provider();
    let prefix = provider.number_prefix();
    let body = vbox(16);
    pad(&body, 24);
    let item = lookup(url);
    let title = match &item {
        Some(i) => format!("{}{prefix}{}", i.repository.name_with_owner, i.number),
        None => provider.request_short().to_string(),
    };
    if let Some(i) = &item {
        let head = hbox(12);
        head.append(&state_badge(i.visual_state(), false));
        let t = wrap_label(&i.title, &["title-2", "sy-text"]);
        t.set_selectable(true);
        t.set_hexpand(true);
        head.append(&t);
        body.append(&head);

        let grid = gtk::Grid::new();
        grid.set_row_spacing(8);
        grid.set_column_spacing(16);
        let mut row = 0;
        fact(&grid, row, "Repository", &text(&format!("{}{prefix}{}", i.repository.name_with_owner, i.number)));
        row += 1;
        if let Some(a) = &i.author {
            let who = hbox(6);
            who.append(&avatar(a.avatar_url.as_deref(), 18, Some(&a.login)));
            who.append(&label_with(&a.login, &["sy-text"]));
            fact(&grid, row, "Author", &who.upcast());
            row += 1;
        }
        if let Some(base) = &i.base_ref_name {
            fact(&grid, row, "Target branch", &text(base));
            row += 1;
        }
        fact(&grid, row, "Opened", &text(&util::rel(Some(i.created_at))));
        row += 1;
        fact(&grid, row, "Updated", &text(&util::rel(Some(i.updated_at))));
        row += 1;
        match i.review_decision.as_deref() {
            Some("APPROVED") => {
                fact(&grid, row, "Review", &text("Approved"));
                row += 1;
            }
            Some("CHANGES_REQUESTED") => {
                fact(&grid, row, "Review", &text("Changes requested"));
                row += 1;
            }
            _ => {}
        }
        if i.comments.total_count > 0 {
            fact(&grid, row, "Comments", &text(&i.comments.total_count.to_string()));
            row += 1;
        }
        let labels = i.labels();
        if !labels.is_empty() {
            let lb = hbox(4);
            for l in &labels {
                lb.append(&label_chip(l));
            }
            fact(&grid, row, "Labels", &lb.upcast());
        }
        body.append(&grid);
    }

    let actions = hbox(8);
    let open = suggested("Open in Browser");
    let u = url.to_string();
    open.connect_clicked(move |_| util::open_url(&u));
    actions.append(&open);
    let copy = button("Copy Link");
    let u = url.to_string();
    copy.connect_clicked(move |_| {
        util::copy(&u);
        app::toast("Link copied");
    });
    actions.append(&copy);
    body.append(&actions);

    let note = wrap_label(&format!("Diffs, reviews and comments for {} {}s open in the browser for now.", provider.title(), provider.request_title()), &["sy-muted", "sy-small"]);
    note.set_xalign(0.0);
    body.append(&note);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(760);
    clamp.set_child(Some(&body));
    let (tv, _, _) = toolbar_page(&title, provider.title(), &scrolled(&clamp));
    tv.add_css_class("sy-panel-pane");
    adw::NavigationPage::new(&tv, &title)
}
