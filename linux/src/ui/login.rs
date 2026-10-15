use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::splash::app_icon;
use super::widgets::*;
use crate::{app, auth, brand, rt, settings, util};

struct Login {
    busy: gtk::Spinner,
    error: gtk::Label,
    error_box: gtk::Box,
    device_box: gtk::Box,
    code_label: gtk::Label,
    get_code: gtk::Button,
    cancel: RefCell<Option<tokio::task::AbortHandle>>,
}

impl Login {
    fn set_busy(&self, on: bool) {
        self.busy.set_visible(on);
    }

    fn show_error(&self, msg: Option<&str>) {
        match msg {
            Some(m) => {
                self.error.set_text(m);
                self.error_box.set_visible(true);
            }
            None => self.error_box.set_visible(false),
        }
    }

    fn sign_in(self: &Rc<Self>, token: String) {
        self.set_busy(true);
        self.show_error(None);
        let me = self.clone();
        rt::spawn_local(async move {
            if let Err(e) = app::get().login(token, true).await {
                me.show_error(Some(&e.message));
            }
            me.set_busy(false);
        });
    }
}

fn card_title(title: &str, icon_name: &str, color: &str) -> gtk::Box {
    let b = hbox(8);
    b.append(&icon_tile(icon_name, color, 22));
    b.append(&label_with(title, &["heading"]));
    b
}

fn card() -> gtk::Box {
    let c = vbox(8);
    c.add_css_class("sy-card");
    c
}

pub fn build() -> gtk::Widget {
    let column = vbox(18);
    pad(&column, 40);
    column.set_halign(gtk::Align::Center);
    column.set_size_request(480, -1);

    let head = vbox(6);
    head.append(&app_icon(104));
    let t = label_with(brand::APP_NAME, &["login-title", "sy-text"]);
    t.set_xalign(0.5);
    head.append(&t);
    let s = label_with("Fast PR browsing without a browser", &["sy-muted"]);
    s.set_xalign(0.5);
    head.append(&s);
    column.append(&head);

    let busy = spinner(16);
    busy.set_visible(false);
    busy.set_halign(gtk::Align::Center);
    let error = wrap_label("", &["fg-red"]);
    error.set_selectable(true);
    let error_box = hbox(8);
    error_box.add_css_class("banner-box");
    error_box.add_css_class("red");
    error_box.append(&colored_icon("sy-attention-symbolic", 16, "red"));
    error_box.append(&error);
    error_box.set_visible(false);

    let device_box = hbox(12);
    let code_label = label_with("", &["user-code", "sy-text"]);
    code_label.set_selectable(true);
    let get_code = button("Get code");
    let login = Rc::new(Login { busy, error, error_box, device_box, code_label, get_code, cancel: RefCell::new(None) });

    if auth::gh_available() {
        let c = card();
        c.append(&card_title("GitHub CLI", "sy-terminal-symbolic", "sky"));
        c.append(&wrap_label("Use the token you're signed in with in gh.", &["sy-muted", "sy-small"]));
        let b = icon_label_button("sy-terminal-symbolic", "Sign in with gh");
        b.add_css_class("suggested-action");
        b.add_css_class("pill");
        let l = login.clone();
        b.connect_clicked(move |_| {
            let l = l.clone();
            l.set_busy(true);
            l.show_error(None);
            rt::spawn_local(async move {
                match auth::gh_token().await {
                    Ok(t) => l.sign_in(t),
                    Err(e) => {
                        l.set_busy(false);
                        l.show_error(Some(&e.message));
                    }
                }
            });
        });
        c.append(&b);
        column.append(&c);
    }

    let c = card();
    c.append(&card_title("Personal Access Token", "sy-key-symbolic", "amber"));
    c.append(&wrap_label("Required scopes: repo, read:org, read:project, project, notifications, workflow.", &["sy-muted", "sy-small"]));
    let row = hbox(8);
    let entry = gtk::PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.set_hexpand(true);
    entry.set_placeholder_text(Some("ghp_… or github_pat_…"));
    let sign = suggested("Sign in");
    sign.set_sensitive(false);
    let s2 = sign.clone();
    entry.connect_changed(move |e| s2.set_sensitive(!e.text().trim().is_empty()));
    let l = login.clone();
    let e2 = entry.clone();
    let submit = Rc::new(move || {
        let t = e2.text().trim().to_string();
        if !t.is_empty() {
            l.sign_in(t);
        }
    });
    let sub = submit.clone();
    sign.connect_clicked(move |_| sub());
    let sub = submit.clone();
    entry.connect_activate(move |_| sub());
    row.append(&entry);
    row.append(&sign);
    c.append(&row);
    let create = link_button("Create a token on github.com");
    create.set_halign(gtk::Align::Start);
    create.connect_clicked(|_| {
        util::open_url(&format!(
            "https://github.com/settings/tokens/new?scopes=repo,read:org,read:project,project,notifications,workflow&description={}",
            brand::APP_NAME
        ))
    });
    c.append(&create);
    column.append(&c);

    let c = card();
    let expander = gtk::Expander::new(None);
    expander.set_label_widget(Some(&card_title("Sign in via browser (OAuth Device Flow)", "sy-globe-symbolic", "indigo")));
    let inner = vbox(8);
    inner.set_margin_top(8);
    inner.append(&wrap_label("Requires the Client ID of your own OAuth App (Settings → Developer settings → OAuth Apps, enable “Enable Device Flow”).", &["sy-muted", "sy-small"]));
    let client = gtk::Entry::new();
    client.set_placeholder_text(Some("Client ID"));
    client.set_text(&settings::read(|s| s.oauth_client_id.clone()));
    client.connect_changed(|e| {
        let v = e.text().to_string();
        settings::update(|s| s.oauth_client_id = v);
    });
    inner.append(&client);
    login.device_box.append(&login.code_label);
    let copy_open = suggested("Copy and open");
    login.device_box.append(&copy_open);
    login.device_box.append(&spinner(14));
    login.device_box.set_visible(false);
    inner.append(&login.device_box);
    let hint = wrap_label("Enter the code at github.com/login/device — you'll be signed in automatically.", &["sy-muted", "sy-tiny"]);
    hint.set_visible(false);
    inner.append(&hint);
    login.get_code.set_halign(gtk::Align::Start);
    login.get_code.set_sensitive(!client.text().trim().is_empty());
    let gc = login.get_code.clone();
    client.connect_changed(move |e| gc.set_sensitive(!e.text().trim().is_empty()));
    inner.append(&login.get_code);
    expander.set_child(Some(&inner));
    c.append(&expander);
    column.append(&c);

    let uri_holder: Rc<RefCell<Option<(String, String)>>> = Rc::new(RefCell::new(None));
    let u = uri_holder.clone();
    copy_open.connect_clicked(move |_| {
        if let Some((code, uri)) = u.borrow().clone() {
            util::copy(&code);
            util::open_url(&uri);
        }
    });
    let l = login.clone();
    let client2 = client.clone();
    let hint2 = hint.clone();
    login.get_code.connect_clicked(move |_| {
        let id = client2.text().trim().to_string();
        l.show_error(None);
        if let Some(h) = l.cancel.borrow_mut().take() {
            h.abort();
        }
        let l2 = l.clone();
        let holder = uri_holder.clone();
        let hint = hint2.clone();
        let (tx, rx) = async_channel::bounded::<Result<auth::DeviceCode, String>>(1);
        let (ttx, trx) = async_channel::bounded::<Result<String, String>>(1);
        let task = rt::runtime().spawn(async move {
            match auth::device_start(id.clone()).await {
                Ok(code) => {
                    let _ = tx.send(Ok(code.clone())).await;
                    let r = auth::device_poll(id, code).await.map_err(|e| e.message);
                    let _ = ttx.send(r).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(e.message)).await;
                }
            }
        });
        *l.cancel.borrow_mut() = Some(task.abort_handle());
        rt::spawn_local(async move {
            match rx.recv().await {
                Ok(Ok(code)) => {
                    l2.code_label.set_text(&code.user_code);
                    l2.device_box.set_visible(true);
                    l2.get_code.set_visible(false);
                    hint.set_visible(true);
                    util::copy(&code.user_code);
                    util::open_url(&code.verification_uri);
                    *holder.borrow_mut() = Some((code.user_code.clone(), code.verification_uri.clone()));
                    match trx.recv().await {
                        Ok(Ok(token)) => l2.sign_in(token),
                        Ok(Err(e)) => l2.show_error(Some(&e)),
                        Err(_) => {}
                    }
                    l2.device_box.set_visible(false);
                    l2.get_code.set_visible(true);
                    hint.set_visible(false);
                }
                Ok(Err(e)) => l2.show_error(Some(&e)),
                Err(_) => {}
            }
        });
    });

    column.append(&login.busy);
    column.append(&login.error_box);

    let clamp = adw::Clamp::new();
    clamp.set_maximum_size(520);
    clamp.set_child(Some(&column));
    let scroll = scrolled(&clamp);
    let page = vbox(0);
    page.add_css_class("sy-panel-pane");
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::new();
    hb.set_show_title(false);
    tv.add_top_bar(&hb);
    tv.set_content(Some(&scroll));
    page.append(&tv);
    tv.set_vexpand(true);
    page.upcast()
}
