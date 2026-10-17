use adw::prelude::*;

use super::dialogs;
use super::menu::Menu;
use crate::app;
use crate::git_accounts::{self, GitAccount};

pub fn confirm_switch(account: GitAccount) {
    if app::account().is_some_and(|a| a.id == account.id) {
        return;
    }
    let id = account.id.clone();
    let run = move || app::get().request_switch(id.clone());
    if app::claude_running() {
        dialogs::confirm(&format!("Switch to {}?", account.subtitle()), Some(app::CLAUDE_SWITCH_NOTE), "Switch", false, run);
    } else {
        run();
    }
}

pub fn confirm_sign_out() {
    let who = app::account().map(|a| a.subtitle()).unwrap_or_else(|| "this account".into());
    dialogs::confirm(&format!("Sign out of {who}?"), Some(&format!("The account is removed from {}. {}", crate::brand::APP_NAME, app::CLAUDE_SIGN_OUT_NOTE)), "Sign Out", true, || {
        crate::rt::spawn_local(async { app::get().sign_out().await });
    });
}

pub fn menu() -> Menu {
    let mut menu = Menu::new();
    menu.section(Some("Accounts"));
    let current = app::account().map(|a| a.id);
    for a in git_accounts::accounts() {
        let active = current.as_deref() == Some(a.id.as_str());
        let label = format!("{}{} — {}", if active { "✓ " } else { "" }, a.provider.title(), a.subtitle());
        menu.item(&label, move || confirm_switch(a.clone()));
    }
    menu.section(None);
    menu.item("Add Account…", super::login::add_account_sheet);
    menu.section(None);
    menu.item("Sign Out…", confirm_sign_out);
    menu
}

pub fn menu_button() -> gtk::MenuButton {
    let menu = menu();
    let b = menu.button("sy-person-add-symbolic", "Switch account");
    b.add_css_class("flat");
    b
}
