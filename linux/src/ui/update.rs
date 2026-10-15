use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::splash::app_icon;
use super::widgets::*;
use crate::updater::{self, InstallKind, State};
use crate::{brand, util};

thread_local! {
    static OPEN: RefCell<Option<gtk::glib::WeakRef<adw::Dialog>>> = const { RefCell::new(None) };
}

struct View {
    title: gtk::Label,
    date: gtk::Label,
    spinner: gtk::Spinner,
    notes: gtk::Box,
    progress: gtk::ProgressBar,
    progress_label: gtk::Label,
    progress_box: gtk::Box,
    error: gtk::Label,
    error_box: gtk::Box,
    buttons: gtk::Box,
    dialog: gtk::glib::WeakRef<adw::Dialog>,
}

fn shows_release(state: &State, release: Option<&updater::Release>) -> bool {
    match state {
        State::Idle | State::Checking | State::UpToDate => false,
        State::Failed(_) => release.map(|r| updater::compare(&r.version, updater::current_version()) == std::cmp::Ordering::Greater).unwrap_or(false),
        _ => true,
    }
}

impl View {
    fn render(&self) {
        let state = updater::state();
        let release = updater::release();
        let version = release.as_ref().map(|r| r.version.clone()).unwrap_or_default();
        self.title.set_text(&match &state {
            State::Idle | State::Checking => "Checking for updates…".to_string(),
            State::UpToDate => "You're up to date".into(),
            State::Failed(_) => release.as_ref().map(|r| format!("{} {} is available", brand::APP_NAME, r.version)).unwrap_or_else(|| "Couldn't check for updates".into()),
            State::Installing => "Preparing update…".into(),
            State::ReadyToRelaunch => format!("{} {version} is ready to install", brand::APP_NAME),
            State::Available(_) | State::Downloading(_) => format!("{} {version} is available", brand::APP_NAME),
        });
        let show = shows_release(&state, release.as_ref());
        match release.as_ref().and_then(|r| r.published_at).filter(|_| show) {
            Some(d) => {
                self.date.set_text(&util::full_date(d));
                self.date.set_visible(true);
            }
            None => self.date.set_visible(false),
        }
        self.spinner.set_visible(matches!(state, State::Checking | State::Installing));
        clear(&self.notes);
        if show {
            if let Some(r) = &release {
                self.notes.append(&label_with(&r.name, &["heading", "sy-text"]));
                if r.notes.trim().is_empty() {
                    self.notes.append(&label_with("No release notes.", &["sy-muted"]));
                } else {
                    self.notes.append(&super::markdown::view(&r.notes));
                }
            }
        } else if state == State::UpToDate {
            self.notes.append(&wrap_label(&format!("{} {} is the newest version available.", brand::APP_NAME, updater::current_version()), &["sy-muted"]));
        }
        if let State::Downloading(p) = state {
            self.progress_box.set_visible(true);
            self.progress.set_fraction(p);
            let size = release.as_ref().map(|r| r.asset_size).unwrap_or(0);
            self.progress_label.set_text(&if size > 0 { format!("Downloading… {} / {}", util::byte_count((size as f64 * p) as i64), util::byte_count(size)) } else { "Downloading…".into() });
        } else {
            self.progress_box.set_visible(false);
        }
        match &state {
            State::Failed(msg) => {
                self.error.set_text(msg);
                self.error_box.set_visible(true);
            }
            _ => self.error_box.set_visible(false),
        }
        clear(&self.buttons);
        let close = |label: &str| {
            let b = button(label);
            let d = self.dialog.clone();
            b.connect_clicked(move |_| {
                updater::later();
                if let Some(d) = d.upgrade() {
                    d.close();
                }
            });
            b
        };
        let page = || {
            let b = button("Open Release Page");
            b.connect_clicked(|_| util::open_url(&updater::page_url()));
            b
        };
        match &state {
            State::Available(r) => {
                let skip = button("Skip This Version");
                let d = self.dialog.clone();
                skip.connect_clicked(move |_| {
                    updater::skip();
                    if let Some(d) = d.upgrade() {
                        d.close();
                    }
                });
                self.buttons.append(&skip);
                self.buttons.append(&spacer());
                self.buttons.append(&close("Remind Me Later"));
                if updater::can_self_install() && r.asset_url.is_some() {
                    self.buttons.append(&page());
                    let go = suggested("Download and Install");
                    go.connect_clicked(|_| crate::rt::spawn_local(updater::download_and_install()));
                    self.buttons.append(&go);
                } else {
                    let hint = match updater::install_kind() {
                        InstallKind::Flatpak => "Download the new Flatpak bundle from the release page",
                        InstallKind::Package => "Download the new package from the release page",
                        InstallKind::Portable => "Download the new archive from the release page",
                    };
                    let p = page();
                    p.add_css_class("suggested-action");
                    p.set_tooltip_text(Some(hint));
                    self.buttons.append(&p);
                }
            }
            State::Downloading(_) | State::Installing | State::Checking | State::Idle => {
                self.buttons.append(&spacer());
                self.buttons.append(&close("Close"));
            }
            State::ReadyToRelaunch => {
                self.buttons.append(&spacer());
                self.buttons.append(&close("Later"));
                let go = suggested("Restart to Update");
                go.connect_clicked(|_| updater::relaunch());
                self.buttons.append(&go);
            }
            State::UpToDate => {
                self.buttons.append(&spacer());
                let ok = close("OK");
                ok.add_css_class("suggested-action");
                self.buttons.append(&ok);
            }
            State::Failed(_) => {
                self.buttons.append(&page());
                self.buttons.append(&spacer());
                self.buttons.append(&close("Close"));
                let retry = suggested("Try Again");
                retry.connect_clicked(|_| crate::rt::spawn_local(updater::check(true)));
                self.buttons.append(&retry);
            }
        }
    }
}

pub fn show(user_initiated: bool) {
    let existing = OPEN.with(|o| o.borrow().as_ref().and_then(|w| w.upgrade()));
    if existing.is_some() {
        if user_initiated && !updater::is_busy() {
            crate::rt::spawn_local(updater::check(true));
        }
        return;
    }
    let dialog = adw::Dialog::new();
    dialog.set_title("Software Update");
    dialog.set_content_width(520);
    dialog.set_content_height(420);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    let body = vbox(14);
    margins(&body, 6, 20, 20, 20);
    let header = hbox(14);
    header.append(&app_icon(64));
    let texts = vbox(4);
    texts.set_hexpand(true);
    let title = label_with("", &["title-4", "sy-text"]);
    texts.append(&title);
    texts.append(&label_with(&format!("You have {}", updater::current_version()), &["sy-muted"]));
    let date = label_with("", &["sy-dim", "sy-small"]);
    texts.append(&date);
    let changelog = gtk::Button::with_label("What's new in all versions");
    changelog.add_css_class("link");
    changelog.add_css_class("sy-small");
    changelog.set_halign(gtk::Align::Start);
    changelog.connect_clicked(|_| {
        let current = updater::current_version().to_string();
        let version = updater::release()
            .map(|r| r.version)
            .filter(|v| updater::version_components(v) > updater::version_components(&current))
            .unwrap_or(current);
        util::open_url(&brand::changelog_url(&version));
    });
    texts.append(&changelog);
    header.append(&texts);
    let sp = spinner(16);
    header.append(&sp);
    body.append(&header);
    let notes = vbox(8);
    pad(&notes, 12);
    let ns = scrolled(&notes);
    body.append(&frame(&ns, "sy-sunken"));
    let progress_box = vbox(4);
    let progress = gtk::ProgressBar::new();
    let progress_label = label_with("", &["sy-muted", "sy-small"]);
    progress_box.append(&progress);
    progress_box.append(&progress_label);
    body.append(&progress_box);
    let error_box = hbox(8);
    error_box.add_css_class("banner-box");
    error_box.add_css_class("red");
    error_box.append(&colored_icon("sy-attention-symbolic", 14, "red"));
    let error = selectable(wrap_label("", &["fg-red"]));
    error_box.append(&error);
    body.append(&error_box);
    let buttons = hbox(8);
    body.append(&buttons);
    tv.set_content(Some(&body));
    dialog.set_child(Some(&tv));
    let view = Rc::new(View { title, date, spinner: sp, notes, progress, progress_label, progress_box, error, error_box, buttons, dialog: dialog.downgrade() });
    view.render();
    let w = Rc::downgrade(&view);
    updater::on_change(move |_| match w.upgrade() {
        Some(v) => {
            v.render();
            true
        }
        None => false,
    });
    keep_alive(&tv, view);
    OPEN.with(|o| *o.borrow_mut() = Some(dialog.downgrade()));
    let w = crate::app::main_window();
    dialog.present(w.as_ref());
    if user_initiated || updater::state() == State::Idle {
        crate::rt::spawn_local(updater::check(true));
    }
}
