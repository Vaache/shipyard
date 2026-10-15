use adw::prelude::*;

use super::claude::auth_view;
use super::dialogs;
use super::widgets::*;
use crate::claude::accounts::{self, Status};
use crate::{app, auth, settings, theme, updater, util};

fn combo(title: &str, options: &[&str], selected: usize, on_change: impl Fn(usize) + 'static) -> adw::ComboRow {
    let row = adw::ComboRow::new();
    row.set_title(title);
    row.set_model(Some(&gtk::StringList::new(options)));
    row.set_selected(selected as u32);
    row.connect_selected_notify(move |r| on_change(r.selected() as usize));
    row
}

fn switch(title: &str, subtitle: Option<&str>, active: bool, on_change: impl Fn(bool) + 'static) -> adw::SwitchRow {
    let row = adw::SwitchRow::new();
    row.set_title(title);
    if let Some(s) = subtitle {
        row.set_subtitle(s);
    }
    row.set_active(active);
    row.connect_active_notify(move |r| on_change(r.is_active()));
    row
}

fn group(title: &str, icon_name: &str, color: &str) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::new();
    g.set_title(title);
    g.set_header_suffix(Some(&icon_tile(icon_name, color, 18)));
    g
}

fn account_group(dialog: &adw::PreferencesDialog) -> adw::PreferencesGroup {
    let g = group("Account", "sy-person-symbolic", "teal");
    let a = app::get();
    let (user, scopes) = {
        let s = a.state.borrow();
        (s.user.clone(), s.scopes.clone())
    };
    match user {
        Some(u) => {
            let row = adw::ActionRow::new();
            let name = u.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| u.login.clone());
            row.set_title(&name);
            row.set_subtitle(&format!("@{}", u.login));
            row.add_prefix(&avatar(u.avatar_url.as_deref(), 36, Some(&name)));
            g.add(&row);
            let sc = adw::ActionRow::new();
            sc.set_title("Token scopes");
            sc.set_subtitle(&if scopes.is_empty() { "fine-grained / unknown".to_string() } else { scopes.join(", ") });
            sc.set_subtitle_selectable(true);
            g.add(&sc);
            let actions = adw::ActionRow::new();
            actions.set_title("Session");
            if auth::gh_available() {
                let refresh = button("Refresh token from gh");
                refresh.set_valign(gtk::Align::Center);
                refresh.connect_clicked(|b| {
                    b.set_sensitive(false);
                    let b = b.clone();
                    crate::rt::spawn_local(async move {
                        if let Ok(t) = auth::gh_token().await {
                            let _ = app::get().login(t, true).await;
                        }
                        b.set_sensitive(true);
                    });
                });
                actions.add_suffix(&refresh);
            }
            let out = destructive("Sign out");
            out.set_valign(gtk::Align::Center);
            let d = dialog.clone();
            out.connect_clicked(move |_| {
                d.close();
                app::get().logout();
            });
            actions.add_suffix(&out);
            g.add(&actions);
        }
        None => {
            let row = adw::ActionRow::new();
            row.set_title("Not signed in");
            g.add(&row);
        }
    }
    g
}

fn claude_group(dialog: &adw::PreferencesDialog) -> adw::PreferencesGroup {
    let g = group("Claude Code", "sy-sparkles-symbolic", "coral");
    let profiles = accounts::profiles();
    let active = accounts::active_id();
    let names: Vec<String> = profiles.iter().map(|p| format!("{}{}", p.label, p.email.as_ref().map(|e| format!(" — {e}")).unwrap_or_default())).collect();
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let ids: Vec<String> = profiles.iter().map(|p| p.id.clone()).collect();
    let row = combo("Account", &refs, profiles.iter().position(|p| p.id == active).unwrap_or(0), move |i| {
        if let Some(id) = ids.get(i) {
            auth_view::switch_profile(id);
        }
    });
    let add = button("Add…");
    add.set_valign(gtk::Align::Center);
    let d = dialog.clone();
    add.connect_clicked(move |_| {
        d.close();
        auth_view::add_profile_sheet();
    });
    row.add_suffix(&add);
    g.add(&row);
    let status = accounts::store().status.borrow().clone();
    match status {
        Status::LoggedIn(info) => {
            let r = adw::ActionRow::new();
            r.set_title(info.email.as_deref().unwrap_or("Signed in"));
            r.set_subtitle(&[info.org_name.clone(), info.subscription.clone()].into_iter().flatten().collect::<Vec<_>>().join(" · "));
            let out = destructive("Sign out of Claude Code");
            out.set_valign(gtk::Align::Center);
            out.connect_clicked(|_| {
                dialogs::confirm(
                    "Sign out of Claude Code?",
                    Some("This also signs out Claude Code in the terminal. Open sessions will go to sleep. You can sign in again (including with another account) here or in the Claude Code section."),
                    "Sign out",
                    true,
                    || {
                        app::get().claude.sleep_all();
                        crate::rt::spawn_local(async { accounts::store().logout(None).await });
                    },
                );
            });
            r.add_suffix(&out);
            g.add(&r);
        }
        Status::NotInstalled => {
            let r = adw::ActionRow::new();
            r.set_title("claude CLI isn't installed");
            r.set_subtitle(auth_view::INSTALL_COMMAND);
            r.set_subtitle_selectable(true);
            g.add(&r);
        }
        _ => {
            let r = adw::ActionRow::new();
            r.set_title("Not signed in to Claude");
            let b = button("Sign in…");
            b.set_valign(gtk::Align::Center);
            let d = dialog.clone();
            b.connect_clicked(move |_| {
                d.close();
                app::set_section(app::Section::Claude);
                app::get().present();
            });
            r.add_suffix(&b);
            g.add(&r);
        }
    }
    let minutes = [0i64, 5, 15, 30, 60];
    let labels = ["Never", "5 minutes", "15 minutes", "30 minutes", "1 hour"];
    let current = app::get().claude.idle_minutes();
    let idle = combo("Sleep idle sessions after", &labels, minutes.iter().position(|m| *m == current).unwrap_or(2), move |i| {
        app::get().claude.set_idle_minutes(minutes[i.min(minutes.len() - 1)]);
    });
    idle.set_subtitle("A sleeping session uses no memory: the claude process stops, the conversation stays, and the next message resumes it.");
    idle.set_subtitle_lines(3);
    g.add(&idle);
    g
}

pub fn show() {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_title("Preferences");
    let page = adw::PreferencesPage::new();
    page.set_title("General");
    page.set_icon_name(Some("emblem-system-symbolic"));

    let appearance = group("Appearance", "preferences-desktop-appearance-symbolic", "violet");
    let style = settings::read(|s| s.theme_style.clone());
    appearance.add(&combo(
        "Theme",
        &[crate::brand::APP_NAME, "System"],
        if style == "system" { 1 } else { 0 },
        |i| {
            settings::update(|s| s.theme_style = if i == 1 { "system".into() } else { "pullyard".into() });
            theme::apply();
        },
    ));
    let modes = ["auto", "light", "dark"];
    let mode = settings::read(|s| s.appearance.clone());
    appearance.add(&combo("Mode", &["Match system", "Light", "Dark"], modes.iter().position(|m| *m == mode).unwrap_or(0), move |i| {
        settings::update(|s| s.appearance = modes[i.min(2)].to_string());
        theme::apply();
    }));
    page.add(&appearance);
    page.add(&account_group(&dialog));

    let links = group("Links", "sy-external-symbolic", "indigo");
    let browsers = util::installed_browsers();
    let mut names: Vec<String> = vec!["System default".into()];
    names.extend(browsers.iter().map(|b| b.name.clone()));
    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
    let ids: Vec<String> = std::iter::once("system".to_string()).chain(browsers.iter().map(|b| b.id.clone())).collect();
    let current = settings::read(|s| s.link_browser.clone());
    let ids2 = ids.clone();
    let browser_row = combo("Open links in", &refs, ids.iter().position(|i| *i == current).unwrap_or(0), move |i| {
        let id = ids2.get(i).cloned().unwrap_or_else(|| "system".into());
        settings::update(|s| s.link_browser = id);
    });
    if util::is_flatpak() {
        browser_row.set_subtitle("Inside Flatpak, links open through the desktop portal.");
        browser_row.set_sensitive(false);
    }
    links.add(&browser_row);
    page.add(&links);

    let refresh = group("Refresh", "view-refresh-symbolic", "green");
    let intervals = [15.0, 30.0, 60.0, 120.0, 300.0, 900.0];
    let ilabels = ["Every 15 seconds", "Every 30 seconds", "Every minute", "Every 2 minutes", "Every 5 minutes", "Every 15 minutes"];
    let cur = settings::read(|s| s.poll_interval);
    refresh.add(&combo("Auto-refresh pull requests", &ilabels, intervals.iter().position(|v| (*v - cur).abs() < 0.5).unwrap_or(1), move |i| {
        settings::update(|s| s.poll_interval = intervals[i.min(intervals.len() - 1)]);
        app::get().start_polling();
    }));
    page.add(&refresh);

    let bg = group("Notifications", "sy-bell-symbolic", "amber");
    bg.add(&switch("Notify about new review requests", None, settings::read(|s| s.notify_review_requests), |v| settings::update(|s| s.notify_review_requests = v)));
    bg.add(&switch(
        "Keep running after the window is closed",
        Some("Reopen the window from the app launcher. Review requests and updates still notify you."),
        settings::read(|s| s.run_in_background),
        |v| settings::update(|s| s.run_in_background = v),
    ));
    page.add(&bg);

    page.add(&claude_group(&dialog));

    let updates = group("Updates", "sy-download-symbolic", "green");
    updates.add(&switch("Check for updates automatically", None, settings::read(|s| s.update_auto_check), |v| settings::update(|s| s.update_auto_check = v)));
    let ver = adw::ActionRow::new();
    ver.set_title(&format!("Current version {}", updater::current_version()));
    ver.set_subtitle(match updater::install_kind() {
        updater::InstallKind::Flatpak => "Installed as a Flatpak",
        updater::InstallKind::Package => "Installed from a package",
        updater::InstallKind::Portable => "Portable install",
    });
    let check = button("Check Now");
    check.set_valign(gtk::Align::Center);
    check.set_sensitive(!updater::is_busy());
    let d = dialog.clone();
    check.connect_clicked(move |_| {
        d.close();
        super::update::show(true);
    });
    ver.add_suffix(&check);
    updates.add(&ver);
    page.add(&updates);

    let oauth = group("OAuth Device Flow", "sy-key-symbolic", "amber");
    let entry = adw::EntryRow::new();
    entry.set_title("Client ID");
    entry.set_text(&settings::read(|s| s.oauth_client_id.clone()));
    entry.connect_changed(|e| {
        let v = e.text().to_string();
        settings::update(|s| s.oauth_client_id = v);
    });
    oauth.add(&entry);
    page.add(&oauth);

    let support = group(&format!("Support {}", crate::brand::APP_NAME), "sy-star-symbolic", "coral");
    let pizza = adw::ActionRow::new();
    pizza.set_title("Buy me a pizza 🍕");
    pizza.set_subtitle(&format!("Pay what you want, from $3. {} stays free and open source.", crate::brand::APP_NAME));
    pizza.set_activatable(true);
    pizza.add_suffix(&icon("sy-external-symbolic", 16));
    pizza.connect_activated(|_| util::open_in_default_browser(crate::brand::DONATE_URL));
    support.add(&pizza);
    page.add(&support);

    dialog.add(&page);
    crate::rt::spawn_local(async { accounts::store().refresh().await });
    let w = app::main_window();
    dialog.present(w.as_ref());
}
