use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::dialogs::{self, Sheet};
use super::widgets::*;
use crate::account_lock::{self, PinCheck};
use crate::app;
use crate::git_accounts::GitAccount;

pub fn reset_note(account: &GitAccount) -> String {
    let service = account.provider.title();
    format!(
        "Without the PIN there is no way to prove it's you, so resetting removes the lock, signs out the Claude Code accounts linked to {} and deletes its {service} sign-in. You'll need to sign in to {service} again.",
        account.subtitle()
    )
}

fn digits_only(entry: &gtk::PasswordEntry) {
    entry.connect_changed(|e| {
        let text = e.text().to_string();
        let clean: String = text.chars().filter(|c| c.is_ascii_digit()).take(account_lock::MAX_PIN).collect();
        if clean != text {
            e.set_text(&clean);
            e.set_position(-1);
        }
    });
}

fn pin_entry(placeholder: &str) -> gtk::PasswordEntry {
    let e = gtk::PasswordEntry::new();
    e.set_placeholder_text(Some(placeholder));
    digits_only(&e);
    e
}

struct Form {
    account: GitAccount,
    entry: gtk::PasswordEntry,
    unlock: gtk::Button,
    message: gtk::Label,
    countdown: gtk::Label,
    forgot: gtk::Button,
    blocked_text: gtk::Label,
    input_row: gtk::Box,
    blocked: Cell<bool>,
    wait_until: Cell<Option<i64>>,
    tick: RefCell<Option<glib::SourceId>>,
    on_success: Rc<dyn Fn()>,
}

impl Form {
    fn waiting(&self) -> bool {
        self.wait_until.get().is_some_and(|u| u > chrono::Utc::now().timestamp())
    }

    fn sync(&self) {
        let blocked = self.blocked.get();
        self.input_row.set_visible(!blocked);
        self.blocked_text.set_visible(blocked);
        self.forgot.set_label(if blocked { "Reset PIN…" } else { "Forgot PIN?" });
        let waiting = self.waiting();
        self.entry.set_sensitive(!waiting);
        self.unlock.set_sensitive(!waiting && account_lock::is_valid_pin(&self.entry.text()));
        match self.wait_until.get().filter(|_| waiting) {
            Some(until) => {
                self.countdown.set_text(&format!("Try again in {} s", until - chrono::Utc::now().timestamp()));
                self.countdown.set_visible(true);
            }
            None => self.countdown.set_visible(false),
        }
    }

    fn apply(self: &Rc<Self>, state: Option<PinCheck>) {
        match state {
            Some(PinCheck::Blocked) => {
                self.blocked.set(true);
                self.message.set_visible(false);
            }
            Some(PinCheck::Wait(until)) => self.start_wait(until),
            _ => {}
        }
        self.sync();
    }

    fn start_wait(self: &Rc<Self>, until: i64) {
        self.wait_until.set(Some(until));
        if let Some(id) = self.tick.borrow_mut().take() {
            id.remove();
        }
        let weak = Rc::downgrade(self);
        let id = glib::timeout_add_seconds_local(1, move || {
            let Some(me) = weak.upgrade() else { return glib::ControlFlow::Break };
            me.sync();
            if me.waiting() {
                glib::ControlFlow::Continue
            } else {
                *me.tick.borrow_mut() = None;
                me.entry.grab_focus();
                glib::ControlFlow::Break
            }
        });
        *self.tick.borrow_mut() = Some(id);
    }

    fn submit(self: &Rc<Self>) {
        let pin = self.entry.text().to_string();
        if !account_lock::is_valid_pin(&pin) || self.waiting() || self.blocked.get() {
            return;
        }
        self.entry.set_text("");
        self.unlock.set_sensitive(false);
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let result = account_lock::check(&me.account.id, pin).await;
            me.message.set_visible(true);
            match result {
                PinCheck::Ok => {
                    me.message.set_visible(false);
                    (me.on_success)();
                    return;
                }
                PinCheck::Wrong(1) => me.message.set_text("Wrong PIN. 1 attempt left before a pause."),
                PinCheck::Wrong(left) => me.message.set_text(&format!("Wrong PIN. {left} attempts left before a pause.")),
                PinCheck::Wait(until) => {
                    me.message.set_text("Wrong PIN.");
                    me.start_wait(until);
                }
                PinCheck::Blocked => {
                    me.blocked.set(true);
                    me.message.set_visible(false);
                }
            }
            me.sync();
        });
    }
}

fn form(account: &GitAccount, title: &str, on_success: Rc<dyn Fn()>, on_reset: Rc<dyn Fn()>) -> gtk::Box {
    let column = vbox(12);
    column.set_halign(gtk::Align::Center);
    column.set_valign(gtk::Align::Center);
    let display = account.display_name();
    let a = avatar(account.avatar_url.as_deref(), 56, Some(&display));
    a.set_halign(gtk::Align::Center);
    column.append(&a);
    let name = label_with(&display, &["title-3", "sy-text"]);
    name.set_xalign(0.5);
    column.append(&name);
    let sub = label_with(&format!("{} · {}", account.provider.title(), account.subtitle()), &["sy-muted"]);
    sub.set_xalign(0.5);
    column.append(&sub);
    let heading = label_with(title, &["heading", "sy-text"]);
    heading.set_xalign(0.5);
    heading.set_margin_top(4);
    column.append(&heading);

    let input_row = hbox(8);
    input_row.set_halign(gtk::Align::Center);
    let entry = pin_entry("PIN");
    entry.set_width_chars(10);
    let unlock = suggested("Unlock");
    unlock.set_sensitive(false);
    input_row.append(&entry);
    input_row.append(&unlock);
    column.append(&input_row);

    let blocked_text = wrap_label(&format!("Too many wrong PINs. Reset the PIN to continue. {}", reset_note(account)), &["fg-red"]);
    blocked_text.set_justify(gtk::Justification::Center);
    blocked_text.set_max_width_chars(48);
    blocked_text.set_visible(false);
    column.append(&blocked_text);
    let message = wrap_label("", &["fg-amber"]);
    message.set_justify(gtk::Justification::Center);
    message.set_visible(false);
    column.append(&message);
    let countdown = label_with("", &["sy-muted"]);
    countdown.set_xalign(0.5);
    countdown.set_visible(false);
    column.append(&countdown);

    let forgot = link_button("Forgot PIN?");
    forgot.set_halign(gtk::Align::Center);
    column.append(&forgot);

    let me = Rc::new(Form {
        account: account.clone(),
        entry,
        unlock,
        message,
        countdown,
        forgot,
        blocked_text,
        input_row,
        blocked: Cell::new(false),
        wait_until: Cell::new(None),
        tick: RefCell::new(None),
        on_success,
    });
    let weak = Rc::downgrade(&me);
    me.entry.connect_changed(move |_| {
        if let Some(me) = weak.upgrade() {
            me.sync();
        }
    });
    let weak = Rc::downgrade(&me);
    me.entry.connect_activate(move |_| {
        if let Some(me) = weak.upgrade() {
            me.submit();
        }
    });
    let weak = Rc::downgrade(&me);
    me.unlock.connect_clicked(move |_| {
        if let Some(me) = weak.upgrade() {
            me.submit();
        }
    });
    let target = account.clone();
    me.forgot.connect_clicked(move |_| {
        let target = target.clone();
        let on_reset = on_reset.clone();
        dialogs::confirm("Reset the PIN?", Some(&reset_note(&target)), "Reset PIN", true, move || {
            let target = target.clone();
            let on_reset = on_reset.clone();
            crate::rt::spawn_local(async move {
                app::get().reset_lock(target).await;
                on_reset();
            });
        });
    });
    let weak = Rc::downgrade(&me);
    let id = account.id.clone();
    crate::rt::spawn_local(async move {
        let state = account_lock::current_state(&id).await;
        if let Some(me) = weak.upgrade() {
            me.apply(state);
            me.entry.grab_focus();
        }
    });
    let weak = Rc::downgrade(&me);
    column.connect_destroy(move |_| {
        if let Some(id) = weak.upgrade().and_then(|me| me.tick.borrow_mut().take()) {
            id.remove();
        }
    });
    keep_alive(&column, me);
    column
}

pub fn lock_page(account: &GitAccount) -> gtk::Widget {
    let target = account.clone();
    let on_success: Rc<dyn Fn()> = Rc::new(move || app::get().unlocked(target.clone()));
    let column = form(account, "Enter PIN", on_success, Rc::new(|| {}));
    let other = link_button("Use another account");
    other.set_halign(gtk::Align::Center);
    other.connect_clicked(|_| app::get().leave_lock());
    column.append(&other);
    pad(&column, 32);
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_title(false);
    tv.add_top_bar(&hb);
    tv.set_content(Some(&column));
    tv.add_css_class("sy-panel-pane");
    tv.upcast()
}

pub fn unlock_sheet(account: &GitAccount, title: &str, on_success: Rc<dyn Fn()>) {
    let sheet = Sheet::new(title, 460, -1);
    let d = sheet.dialog.clone();
    let success: Rc<dyn Fn()> = Rc::new(move || {
        d.close();
        on_success();
    });
    let d = sheet.dialog.clone();
    let reset: Rc<dyn Fn()> = Rc::new(move || {
        d.close();
    });
    sheet.body.append(&form(account, title, success, reset));
    sheet.add_footer_spacer();
    sheet.cancel_button();
    sheet.present();
}

pub fn set_pin_sheet(account: &GitAccount, on_done: Rc<dyn Fn()>) {
    let sheet = Sheet::new(&format!("Protect {} with a PIN", account.subtitle()), 440, -1);
    sheet.body.append(&wrap_label(
        &format!("{} asks for the PIN when it starts with this account and when someone switches to it. Use 4 to 6 digits.", crate::brand::APP_NAME),
        &["sy-muted"],
    ));
    let pin = pin_entry("New PIN");
    let confirm = pin_entry("Repeat PIN");
    sheet.body.append(&pin);
    sheet.body.append(&confirm);
    let error = label_with("", &["fg-red", "sy-small"]);
    error.set_visible(false);
    sheet.body.append(&error);
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let set = suggested("Set PIN");
    set.set_sensitive(false);
    sheet.footer.append(&set);
    let sync = {
        let (pin, confirm, set) = (pin.clone(), confirm.clone(), set.clone());
        move || set.set_sensitive(account_lock::is_valid_pin(&pin.text()) && !confirm.text().is_empty())
    };
    let s = sync.clone();
    pin.connect_changed(move |_| s());
    let s = sync.clone();
    confirm.connect_changed(move |_| s());
    let d = sheet.dialog.clone();
    let id = account.id.clone();
    let (p, c, b) = (pin.clone(), confirm.clone(), set.clone());
    let save = Rc::new(move || {
        let first = p.text().to_string();
        if !account_lock::is_valid_pin(&first) {
            error.set_text("Use 4 to 6 digits.");
            error.set_visible(true);
            return;
        }
        if first != c.text().as_str() {
            error.set_text("The PINs don't match.");
            error.set_visible(true);
            return;
        }
        b.set_sensitive(false);
        let (d, id, error, b, on_done) = (d.clone(), id.clone(), error.clone(), b.clone(), on_done.clone());
        crate::rt::spawn_local(async move {
            if account_lock::set_pin(&id, first).await {
                d.close();
                on_done();
            } else {
                error.set_text("Could not save the PIN. Check that the keyring is available.");
                error.set_visible(true);
                b.set_sensitive(true);
            }
        });
    });
    let s = save.clone();
    set.connect_clicked(move |_| s());
    let s = save.clone();
    confirm.connect_activate(move |_| s());
    sheet.present();
    pin.grab_focus();
}
