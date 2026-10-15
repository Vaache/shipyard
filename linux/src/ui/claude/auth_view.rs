use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;

use crate::claude::accounts::{self, LoginMethod, Status};
use crate::settings::ClaudeProfile;
use crate::ui::dialogs::{self, Sheet};
use crate::ui::menu::Menu;
use crate::ui::widgets::*;
use crate::{app, util};

pub const INSTALL_COMMAND: &str = "curl -fsSL https://claude.ai/install.sh | bash";

pub fn switch_profile(id: &str) {
    if id == accounts::active_id() {
        return;
    }
    let a = app::get();
    a.claude.close_all_for_profile_switch();
    accounts::set_active(id);
    crate::claude::chat::USAGE.with(|u| {
        let mut u = u.borrow_mut();
        u.windows.clear();
        u.subscription = None;
    });
    let store = accounts::store();
    *store.status.borrow_mut() = Status::Checking;
    store.changed.emit(&());
    let ws = a.claude.clone();
    crate::rt::spawn_local(async move {
        store.refresh().await;
        ws.reload_projects().await;
    });
}

pub fn remove_profile(p: &ClaudeProfile) {
    if p.is_default() {
        return;
    }
    if accounts::active_id() == p.id {
        switch_profile(accounts::DEFAULT_ID);
    }
    let dir = p.config_dir.clone();
    let id = p.id.clone();
    crate::rt::spawn_local(async move {
        accounts::store().logout(dir).await;
        accounts::remove(&id);
        accounts::store().changed.emit(&());
    });
}

pub fn profile_badge(p: &ClaudeProfile, size: i32) -> gtk::Box {
    let color = if p.is_default() { "coral" } else { crate::theme::palette_for(&p.id) };
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("state-dot");
    b.add_css_class(color);
    b.set_size_request(size, size);
    b.set_valign(gtk::Align::Center);
    let l = label_with(&p.initial(), &["sy-bold", &format!("fg-{color}")]);
    l.set_hexpand(true);
    l.set_xalign(0.5);
    b.append(&l);
    b
}

pub fn profile_menu(menu: &mut Menu) {
    menu.section(Some("Claude accounts"));
    let active = accounts::active_id();
    for p in accounts::profiles() {
        let text = format!("{}{}{}", if p.id == active { "✓ " } else { "" }, p.label, p.email.as_ref().map(|e| format!(" — {e}")).unwrap_or_default());
        let id = p.id.clone();
        menu.item(&text, move || switch_profile(&id));
    }
    menu.section(None);
    menu.item("Add account…", add_profile_sheet);
    let a = accounts::active();
    if !a.is_default() {
        let label = a.label.clone();
        menu.item(&format!("Remove “{label}”…"), move || {
            let p = accounts::active();
            let p2 = p.clone();
            dialogs::confirm(
                "Remove account?",
                Some(&format!("The app signs out of this account and removes it from the list. Its history folder stays on disk: {}", p.config_dir.clone().unwrap_or_default())),
                &format!("Sign out and remove “{}”", p.label),
                true,
                move || remove_profile(&p2),
            );
        });
    }
}

pub fn profile_switcher() -> gtk::MenuButton {
    let active = accounts::active();
    let b = gtk::MenuButton::new();
    let content = hbox(6);
    content.append(&profile_badge(&active, 18));
    content.append(&label_with(&active.label, &["sy-text"]));
    let n = accounts::profiles().len();
    if n > 1 {
        content.append(&label_with(&n.to_string(), &["sy-muted", "sy-tiny"]));
    }
    b.set_child(Some(&content));
    b.add_css_class("flat");
    b.set_tooltip_text(Some("Switch Claude account"));
    let mut menu = Menu::new();
    profile_menu(&mut menu);
    menu.apply_to(&b);
    b
}

pub fn add_profile_sheet() {
    let sheet = Sheet::new("New Claude account", 460, -1);
    sheet.body.append(&wrap_label("The account gets its own sign-in, session history and limits. Sign-in opens right after.", &["sy-muted"]));
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Name, e.g. “Personal” or “Work”"));
    sheet.body.append(&entry);
    let share = gtk::CheckButton::with_label("Use my settings, CLAUDE.md, skills and plugins from the main account");
    share.set_active(true);
    sheet.body.append(&share);
    let error = label_with("", &["fg-red", "sy-small"]);
    error.set_visible(false);
    sheet.body.append(&error);
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let create = tinted("Create and sign in", "coral");
    sheet.footer.append(&create);
    let d = sheet.dialog.clone();
    let e = entry.clone();
    let go = Rc::new(move || match accounts::add(&e.text(), share.is_active()) {
        Ok(p) => {
            switch_profile(&p.id);
            app::set_section(app::Section::Claude);
            d.close();
        }
        Err(msg) => {
            error.set_text(&msg);
            error.set_visible(true);
        }
    });
    let g = go.clone();
    create.connect_clicked(move |_| g());
    let g = go.clone();
    entry.connect_activate(move |_| g());
    sheet.present();
}

pub fn account_card(info: &accounts::AccountInfo, compact: bool) -> gtk::Box {
    let profile = accounts::active();
    let card = hbox(10);
    card.add_css_class("sy-card-flat");
    pad(&card, if compact { 8 } else { 14 });
    card.append(&profile_badge(&profile, if compact { 26 } else { 34 }));
    let col = vbox(1);
    col.set_hexpand(true);
    let top = hbox(5);
    let email = ellipsized(info.email.as_deref().unwrap_or("Signed in"), &["sy-text", if compact { "sy-bold" } else { "heading" }]);
    top.append(&email);
    if accounts::profiles().len() > 1 {
        top.append(&chip(&profile.label, "muted", None, false));
    }
    col.append(&top);
    let sub = hbox(5);
    if let Some(org) = &info.org_name {
        sub.append(&ellipsized(org, &["sy-muted", "sy-small"]));
    }
    if let Some(plan) = &info.subscription {
        sub.append(&chip(plan, "coral", None, false));
    } else if info.method.as_deref() == Some("console") || info.provider.as_deref() != Some("firstParty") {
        let label = if info.provider.as_deref() == Some("firstParty") { "API".to_string() } else { info.provider.clone().unwrap_or_else(|| "API".into()) };
        sub.append(&label_with(&label, &["sy-muted", "sy-tiny"]));
    }
    col.append(&sub);
    card.append(&col);
    let mut menu = Menu::new();
    profile_menu(&mut menu);
    menu.section(None);
    menu.item("Sign out of this account…", || {
        let body = if accounts::active().is_default() {
            "This also signs out Claude Code in the terminal. You can sign in again afterwards, including with another account.".to_string()
        } else {
            format!("Signs out only “{}”. Other accounts are not affected.", accounts::active().label)
        };
        dialogs::confirm("Sign out of Claude Code?", Some(&body), "Sign out", true, || {
            let a = app::get();
            a.claude.sleep_all();
            crate::rt::spawn_local(async { accounts::store().logout(None).await });
        });
    });
    menu.section(None);
    menu.item("Usage page", || util::open_url("https://claude.ai/settings/usage"));
    menu.item("Refresh status", || crate::rt::spawn_local(async { accounts::store().refresh().await }));
    card.append(&menu.button("view-more-symbolic", "Account"));
    card
}

pub fn build() -> gtk::Widget {
    let column = vbox(18);
    pad(&column, 36);
    column.set_halign(gtk::Align::Center);
    let tile = icon_tile("sy-sparkles-symbolic", "coral", 56);
    column.append(&tile);
    let t = label_with("Claude Code", &["login-title", "sy-text"]);
    t.set_xalign(0.5);
    column.append(&t);
    let switcher = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    switcher.set_halign(gtk::Align::Center);
    column.append(&switcher);
    let content = vbox(10);
    column.append(&content);
    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(520);
    clamp.set_child(Some(&column));
    let scroll = scrolled(&clamp);
    scroll.add_css_class("sy-panel-pane");
    let method = Rc::new(Cell::new(LoginMethod::ClaudeAi));
    let render = {
        let content_w = content.downgrade();
        let switcher_w = switcher.downgrade();
        let method = method.clone();
        Rc::new(move || -> bool {
            let (Some(content), Some(switcher)) = (content_w.upgrade(), switcher_w.upgrade()) else { return false };
            clear(&switcher);
            switcher.append(&profile_switcher());
            clear(&content);
            let store = accounts::store();
            let status = store.status.borrow().clone();
            match status {
                Status::NotInstalled => content.append(&install_card()),
                Status::Unknown | Status::Checking => content.append(&spinner(18)),
                Status::LoggedOut => content.append(&login_card(method.clone())),
                Status::LoggedIn(info) => content.append(&account_card(&info, false)),
            }
            true
        })
    };
    render();
    let r = render.clone();
    accounts::store().changed.connect(move |_| r());
    crate::rt::spawn_local(async { accounts::store().refresh().await });
    scroll.upcast()
}

fn card() -> gtk::Box {
    let c = vbox(10);
    c.add_css_class("sy-card");
    c
}

fn install_card() -> gtk::Box {
    let c = card();
    c.append(&label_with("Claude Code isn't installed", &["heading"]));
    c.append(&wrap_label("Install the CLI from a terminal, then press “Check again”.", &["sy-muted"]));
    let cmd = selectable(label_with(INSTALL_COMMAND, &["sy-mono"]));
    pad(&cmd, 8);
    c.append(&frame(&cmd, "sy-sunken"));
    let row = hbox(6);
    let copy = button("Copy command");
    copy.connect_clicked(|_| util::copy(INSTALL_COMMAND));
    let docs = button("Docs");
    docs.connect_clicked(|_| util::open_url("https://code.claude.com/docs/en/setup"));
    row.append(&copy);
    row.append(&docs);
    row.append(&spacer());
    let again = tinted("Check again", "coral");
    again.connect_clicked(|_| {
        crate::host::forget("claude");
        crate::rt::spawn_local(async { accounts::store().refresh().await });
    });
    row.append(&again);
    c.append(&row);
    c
}

fn login_card(method: Rc<Cell<LoginMethod>>) -> gtk::Box {
    let store = accounts::store();
    let c = card();
    c.append(&label_with("Sign in to Claude", &["heading"]));
    c.append(&wrap_label("Sign-in opens in your browser. The account is shared with Claude Code in the terminal.", &["sy-muted"]));
    if store.logging_in.get() {
        let row = hbox(8);
        row.append(&spinner(14));
        row.append(&label_with("Confirm the sign-in in your browser…", &["sy-text"]));
        c.append(&row);
        if let Some(url) = store.fallback_url.borrow().clone() {
            c.append(&label_with("Browser didn't open, or the page shows a code?", &["sy-muted", "sy-small"]));
            let links = hbox(6);
            let open = button("Open sign-in link");
            let u = url.clone();
            open.connect_clicked(move |_| util::open_url(&u));
            let copy = button("Copy link");
            copy.connect_clicked(move |_| util::copy(&url));
            links.append(&open);
            links.append(&copy);
            c.append(&links);
            let code_row = hbox(6);
            let code = gtk::PasswordEntry::new();
            code.set_placeholder_text(Some("Paste the code from the page"));
            code.set_hexpand(true);
            let submit = tinted("Submit", "coral");
            let cc = code.clone();
            submit.connect_clicked(move |_| {
                accounts::store().submit_code(&cc.text());
                cc.set_text("");
            });
            code.connect_activate(|e| {
                accounts::store().submit_code(&e.text());
                e.set_text("");
            });
            code_row.append(&code);
            code_row.append(&submit);
            c.append(&code_row);
        }
        let cancel = button("Cancel");
        cancel.set_halign(gtk::Align::Start);
        cancel.connect_clicked(|_| accounts::store().cancel_login());
        c.append(&cancel);
    } else {
        let mut first: Option<gtk::CheckButton> = None;
        for m in LoginMethod::ALL {
            let r = gtk::CheckButton::with_label(m.title());
            if let Some(f) = &first {
                r.set_group(Some(f));
            } else {
                first = Some(r.clone());
            }
            r.set_active(method.get() == m);
            let mm = method.clone();
            r.connect_toggled(move |b| {
                if b.is_active() {
                    mm.set(m);
                }
            });
            c.append(&r);
        }
        let email = gtk::Entry::new();
        email.set_placeholder_text(Some("Email (optional)"));
        c.append(&email);
        let row = hbox(0);
        row.append(&spacer());
        let go = tinted("Sign in with browser", "coral");
        go.connect_clicked(move |_| accounts::store().login(method.get(), &email.text()));
        row.append(&go);
        c.append(&row);
    }
    if let Some(e) = store.login_error.borrow().clone() {
        c.append(&selectable(wrap_label(&e, &["fg-red", "sy-small"])));
    }
    c
}
