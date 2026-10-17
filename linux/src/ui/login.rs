use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;

use super::splash::app_icon;
use super::widgets::*;
use crate::git_accounts::{self, GitCredential, GitProvider};
use crate::{app, auth, brand, rt, settings, util};

thread_local! {
    static CURRENT: RefCell<Weak<Login>> = const { RefCell::new(Weak::new()) };
}

struct Login {
    provider: Cell<GitProvider>,
    tabs: Vec<(GitProvider, gtk::ToggleButton)>,
    on_done: Option<Rc<dyn Fn()>>,
    busy: gtk::Spinner,
    error: gtk::Label,
    error_box: gtk::Box,
    host_card: gtk::Box,
    host_title: gtk::Label,
    host: gtk::Entry,
    cli_card: gtk::Box,
    cli_title: gtk::Label,
    cli_hint: gtk::Label,
    cli_button: gtk::Button,
    token_title: gtk::Label,
    token_hint: gtk::Label,
    email: gtk::Entry,
    token: gtk::PasswordEntry,
    sign: gtk::Button,
    create: gtk::Button,
    device_card: gtk::Box,
    device_help: gtk::Label,
    client: gtk::Entry,
    device_box: gtk::Box,
    code_label: gtk::Label,
    device_hint: gtk::Label,
    get_code: gtk::Button,
    device_uri: RefCell<Option<(String, String)>>,
    cancel: RefCell<Option<tokio::task::AbortHandle>>,
}

fn client_id_for(provider: GitProvider) -> String {
    settings::read(|s| if provider == GitProvider::Github { s.oauth_client_id.clone() } else { s.oauth_client_ids.get(provider.key()).cloned().unwrap_or_default() })
}

fn store_client_id(provider: GitProvider, value: String) {
    if client_id_for(provider) == value {
        return;
    }
    settings::update(|s| {
        if provider == GitProvider::Github {
            s.oauth_client_id = value;
        } else {
            s.oauth_client_ids.insert(provider.key().to_string(), value);
        }
    });
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

    fn effective_host(&self) -> String {
        self.provider.get().effective_host(&self.host.text())
    }

    fn can_submit_token(&self) -> bool {
        let has_email = self.provider.get() != GitProvider::Bitbucket || !self.email.text().trim().is_empty();
        !self.token.text().trim().is_empty() && has_email && !self.effective_host().is_empty()
    }

    fn sync_state(&self) {
        let p = self.provider.get();
        let host = self.effective_host();
        self.sign.set_sensitive(self.can_submit_token());
        self.cli_button.set_sensitive(!host.is_empty());
        self.get_code.set_sensitive(!self.client.text().trim().is_empty() && !host.is_empty());
        match p.token_create_url(&host, brand::APP_NAME) {
            Some(_) => {
                self.create.set_label(&p.token_create_label(&host));
                self.create.set_visible(true);
            }
            None => self.create.set_visible(false),
        }
    }

    fn select(&self, p: GitProvider) {
        if let Some(h) = self.cancel.borrow_mut().take() {
            h.abort();
        }
        self.provider.set(p);
        for (tp, t) in &self.tabs {
            if *tp == p && !t.is_active() {
                t.set_active(true);
            }
        }
        if settings::read(|s| s.login_provider != p.key()) {
            settings::update(|s| s.login_provider = p.key().to_string());
        }
        self.host_card.set_visible(p.host_editable());
        self.host_title.set_text(&format!("{} server", p.title()));
        self.host.set_placeholder_text(Some(p.host_placeholder()));
        self.host.set_text(p.default_host());
        let cli = p.cli_name().filter(|_| auth::cli_available(p));
        self.cli_card.set_visible(cli.is_some());
        if let Some(name) = cli {
            let (title, cli_label) = if name == "glab" { ("GitLab CLI", "glab") } else { ("GitHub CLI", "gh") };
            self.cli_title.set_text(title);
            self.cli_hint.set_text(&format!("Use the token you're signed in with in {cli_label}."));
            self.cli_button.set_child(Some(&icon_label_content("sy-terminal-symbolic", &format!("Sign in with {cli_label}"))));
        }
        self.token_title.set_text(p.token_title());
        self.token_hint.set_text(p.token_hint());
        self.email.set_visible(p == GitProvider::Bitbucket);
        self.token.set_text("");
        self.token.set_placeholder_text(Some(p.token_placeholder()));
        self.device_card.set_visible(p.supports_device_flow());
        self.device_help.set_text(p.device_help());
        self.client.set_placeholder_text(Some(p.client_id_placeholder()));
        self.client.set_text(&client_id_for(p));
        self.device_box.set_visible(false);
        self.device_hint.set_visible(false);
        self.get_code.set_visible(true);
        self.show_error(None);
        self.sync_state();
    }

    fn finish(self: &Rc<Self>, credential: GitCredential) {
        let p = self.provider.get();
        let host = self.effective_host();
        let email = if p == GitProvider::Bitbucket { Some(self.email.text().trim().to_string()) } else { None };
        self.set_busy(true);
        self.show_error(None);
        let me = self.clone();
        rt::spawn_local(async move {
            let result = app::get().add_account(p, host, credential, email).await;
            me.set_busy(false);
            match result {
                Ok(()) => {
                    if let Some(done) = &me.on_done {
                        done();
                    }
                }
                Err(e) => me.show_error(Some(&e.message)),
            }
        });
    }

    fn sign_in_with_cli(self: &Rc<Self>) {
        let p = self.provider.get();
        let host = self.effective_host();
        if host.is_empty() {
            return;
        }
        self.set_busy(true);
        self.show_error(None);
        let me = self.clone();
        rt::spawn_local(async move {
            match auth::cli_token(p, host).await {
                Ok(t) => me.finish(GitCredential::token(t)),
                Err(e) => {
                    me.set_busy(false);
                    me.show_error(Some(&e.message));
                }
            }
        });
    }

    fn sign_in_with_token(self: &Rc<Self>) {
        if !self.can_submit_token() {
            return;
        }
        let t = self.token.text().trim().to_string();
        self.finish(GitCredential::token(t));
    }

    fn start_device(self: &Rc<Self>) {
        let p = self.provider.get();
        let host = self.effective_host();
        let id = self.client.text().trim().to_string();
        if id.is_empty() || host.is_empty() {
            return;
        }
        self.show_error(None);
        if let Some(h) = self.cancel.borrow_mut().take() {
            h.abort();
        }
        let endpoint = auth::Endpoint::for_provider(p, &host);
        let keep_refresh = p == GitProvider::Gitlab;
        let (tx, rx) = async_channel::bounded::<Result<auth::DeviceCode, String>>(1);
        let (ttx, trx) = async_channel::bounded::<Result<auth::DeviceToken, String>>(1);
        let client_id = id.clone();
        let task = rt::runtime().spawn(async move {
            match auth::device_start(client_id.clone(), endpoint.clone()).await {
                Ok(code) => {
                    let _ = tx.send(Ok(code.clone())).await;
                    let r = auth::device_poll(client_id, code, endpoint).await.map_err(|e| e.message);
                    let _ = ttx.send(r).await;
                }
                Err(e) => {
                    let _ = tx.send(Err(e.message)).await;
                }
            }
        });
        *self.cancel.borrow_mut() = Some(task.abort_handle());
        let me = self.clone();
        rt::spawn_local(async move {
            match rx.recv().await {
                Ok(Ok(code)) => {
                    me.code_label.set_text(&code.user_code);
                    me.device_box.set_visible(true);
                    me.get_code.set_visible(false);
                    me.device_hint.set_text(&format!("Enter the code at {} — you'll be signed in automatically.", code.verification_uri));
                    me.device_hint.set_visible(true);
                    util::copy(&code.user_code);
                    util::open_url(&code.verification_uri);
                    *me.device_uri.borrow_mut() = Some((code.user_code.clone(), code.verification_uri.clone()));
                    match trx.recv().await {
                        Ok(Ok(token)) if me.provider.get() == p => {
                            let credential = if keep_refresh { token.credential(Some(id)) } else { GitCredential::token(token.access_token) };
                            me.finish(credential);
                        }
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => me.show_error(Some(&e)),
                        Err(_) => {}
                    }
                    me.device_box.set_visible(false);
                    me.get_code.set_visible(true);
                    me.device_hint.set_visible(false);
                }
                Ok(Err(e)) => me.show_error(Some(&e)),
                Err(_) => {}
            }
        });
    }
}

fn card_title(title: &str, icon_name: &str, color: &str) -> (gtk::Box, gtk::Label) {
    let b = hbox(8);
    b.append(&icon_tile(icon_name, color, 22));
    let l = label_with(title, &["heading"]);
    b.append(&l);
    (b, l)
}

fn card() -> gtk::Box {
    let c = vbox(8);
    c.add_css_class("sy-card");
    c
}

type Refill = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

const SIGN_IN_AGAIN: &str = "Sign in again";

fn prefill(a: &git_accounts::GitAccount) {
    let Some(l) = CURRENT.with(|c| c.borrow().upgrade()) else { return };
    l.select(a.provider);
    if a.provider.host_editable() {
        l.host.set_text(&a.host);
    }
    if let Some(email) = &a.email {
        l.email.set_text(email);
    }
    l.show_error(Some(&format!("Sign in again to {} as @{}.", a.host, a.login)));
    l.token.grab_focus();
}

fn continue_with(a: &git_accounts::GitAccount, refill: std::rc::Weak<RefCell<Option<Rc<dyn Fn()>>>>, on_done: Option<Rc<dyn Fn()>>) {
    let id = a.id.clone();
    let run = move || {
        let id = id.clone();
        let refill = refill.clone();
        let done = on_done.clone();
        rt::spawn_local(async move {
            app::get().switch_account(id).await;
            if app::get().state.borrow().phase == app::Phase::LoggedIn {
                if let Some(d) = done {
                    d();
                }
            } else if let Some(f) = refill.upgrade().and_then(|r| r.borrow().clone()) {
                f();
            }
        });
    };
    let other = app::account().is_some_and(|current| current.id != a.id);
    if other && app::claude_running() {
        super::dialogs::confirm(&format!("Switch to {}?", a.subtitle()), Some(app::CLAUDE_SWITCH_NOTE), "Switch", false, run);
    } else {
        run();
    }
}

fn saved_accounts(on_done: Option<Rc<dyn Fn()>>) -> gtk::Box {
    let c = card();
    let fill: Refill = Rc::new(RefCell::new(None));
    let weak_card = c.downgrade();
    let filler = Rc::downgrade(&fill);
    let refill: Rc<dyn Fn()> = Rc::new(move || {
        let Some(target) = weak_card.upgrade() else { return };
        clear(&target);
        let accounts = git_accounts::accounts();
        target.set_visible(!accounts.is_empty());
        if accounts.is_empty() {
            return;
        }
        target.append(&card_title("Your accounts", "sy-person-symbolic", "green").0);
        for a in accounts {
            let row = hbox(10);
            let display = a.display_name();
            row.append(&avatar(a.avatar_url.as_deref(), 24, Some(&display)));
            let texts = vbox(0);
            texts.set_hexpand(true);
            texts.append(&ellipsized(&display, &["sy-text", "sy-bold"]));
            texts.append(&ellipsized(&format!("{} · {}", a.provider.title(), a.subtitle()), &["sy-muted", "sy-small"]));
            row.append(&texts);
            let go = button("Continue");
            go.set_valign(gtk::Align::Center);
            let weak_go = go.downgrade();
            let probe = a.clone();
            rt::spawn_local(async move {
                if !git_accounts::has_credential(&probe).await {
                    if let Some(g) = weak_go.upgrade() {
                        g.set_label(SIGN_IN_AGAIN);
                    }
                }
            });
            let filler = filler.clone();
            let done = on_done.clone();
            go.connect_clicked(move |b| {
                if b.label().as_deref() == Some(SIGN_IN_AGAIN) {
                    prefill(&a);
                } else {
                    continue_with(&a, filler.clone(), done.clone());
                }
            });
            row.append(&go);
            target.append(&row);
        }
    });
    *fill.borrow_mut() = Some(refill.clone());
    refill();
    keep_alive(&c, fill);
    c
}

pub fn form(embedded: bool, on_done: Option<Rc<dyn Fn()>>) -> gtk::Box {
    let column = vbox(18);

    if !embedded {
        let head = vbox(6);
        head.append(&app_icon(104));
        let t = label_with(brand::APP_NAME, &["login-title", "sy-text"]);
        t.set_xalign(0.5);
        head.append(&t);
        let s = label_with("Fast PR browsing without a browser", &["sy-muted"]);
        s.set_xalign(0.5);
        head.append(&s);
        column.append(&head);
        column.append(&saved_accounts(on_done.clone()));
    }

    let tabs_box = hbox(0);
    tabs_box.add_css_class("linked");
    tabs_box.set_halign(gtk::Align::Fill);
    let mut tabs = vec![];
    let mut first: Option<gtk::ToggleButton> = None;
    for p in GitProvider::ALL {
        let t = gtk::ToggleButton::with_label(p.short_title());
        t.set_hexpand(true);
        t.set_tooltip_text(Some(p.title()));
        match &first {
            Some(g) => t.set_group(Some(g)),
            None => first = Some(t.clone()),
        }
        tabs_box.append(&t);
        tabs.push((p, t));
    }
    column.append(&tabs_box);

    let host_card = card();
    let (title, host_title) = card_title("Server", "sy-globe-symbolic", "teal");
    host_card.append(&title);
    let host = gtk::Entry::new();
    host_card.append(&host);
    column.append(&host_card);

    let cli_card = card();
    let (title, cli_title) = card_title("GitHub CLI", "sy-terminal-symbolic", "sky");
    cli_card.append(&title);
    let cli_hint = wrap_label("", &["sy-muted", "sy-small"]);
    cli_card.append(&cli_hint);
    let cli_button = icon_label_button("sy-terminal-symbolic", "Sign in with gh");
    cli_button.add_css_class("suggested-action");
    cli_button.add_css_class("pill");
    cli_card.append(&cli_button);
    column.append(&cli_card);

    let token_card = card();
    let (title, token_title) = card_title("Personal Access Token", "sy-key-symbolic", "amber");
    token_card.append(&title);
    let token_hint = wrap_label("", &["sy-muted", "sy-small"]);
    token_card.append(&token_hint);
    let email = gtk::Entry::new();
    email.set_placeholder_text(Some("Atlassian account email"));
    email.set_input_purpose(gtk::InputPurpose::Email);
    token_card.append(&email);
    let row = hbox(8);
    let token = gtk::PasswordEntry::new();
    token.set_show_peek_icon(true);
    token.set_hexpand(true);
    let sign = suggested("Sign in");
    row.append(&token);
    row.append(&sign);
    token_card.append(&row);
    let create = link_button("Create a token");
    create.set_halign(gtk::Align::Start);
    token_card.append(&create);
    column.append(&token_card);

    let device_card = card();
    let expander = gtk::Expander::new(None);
    expander.set_label_widget(Some(&card_title("Sign in via browser (OAuth Device Flow)", "sy-globe-symbolic", "indigo").0));
    let inner = vbox(8);
    inner.set_margin_top(8);
    let device_help = wrap_label("", &["sy-muted", "sy-small"]);
    inner.append(&device_help);
    let client = gtk::Entry::new();
    inner.append(&client);
    let device_box = hbox(12);
    let code_label = label_with("", &["user-code", "sy-text"]);
    code_label.set_selectable(true);
    device_box.append(&code_label);
    let copy_open = suggested("Copy and open");
    device_box.append(&copy_open);
    device_box.append(&spinner(14));
    device_box.set_visible(false);
    inner.append(&device_box);
    let device_hint = wrap_label("", &["sy-muted", "sy-tiny"]);
    device_hint.set_visible(false);
    inner.append(&device_hint);
    let get_code = button("Get code");
    get_code.set_halign(gtk::Align::Start);
    inner.append(&get_code);
    expander.set_child(Some(&inner));
    device_card.append(&expander);
    column.append(&device_card);

    let busy = spinner(16);
    busy.set_visible(false);
    busy.set_halign(gtk::Align::Center);
    column.append(&busy);
    let error = wrap_label("", &["fg-red"]);
    error.set_selectable(true);
    let error_box = hbox(8);
    error_box.add_css_class("banner-box");
    error_box.add_css_class("red");
    error_box.append(&colored_icon("sy-attention-symbolic", 16, "red"));
    error_box.append(&error);
    error_box.set_visible(false);
    column.append(&error_box);

    let login = Rc::new(Login {
        provider: Cell::new(GitProvider::Github),
        tabs,
        on_done,
        busy,
        error,
        error_box,
        host_card,
        host_title,
        host,
        cli_card,
        cli_title,
        cli_hint,
        cli_button,
        token_title,
        token_hint,
        email,
        token,
        sign,
        create,
        device_card,
        device_help,
        client,
        device_box,
        code_label,
        device_hint,
        get_code,
        device_uri: RefCell::new(None),
        cancel: RefCell::new(None),
    });

    for (p, t) in &login.tabs {
        let weak = Rc::downgrade(&login);
        let p = *p;
        t.connect_toggled(move |t| {
            if let Some(l) = weak.upgrade().filter(|l| t.is_active() && l.provider.get() != p) {
                l.select(p);
            }
        });
    }
    let weak = Rc::downgrade(&login);
    let sync = move || {
        if let Some(l) = weak.upgrade() {
            l.sync_state();
        }
    };
    let s = sync.clone();
    login.host.connect_changed(move |_| s());
    let s = sync.clone();
    login.email.connect_changed(move |_| s());
    let s = sync.clone();
    login.token.connect_changed(move |_| s());
    let weak = Rc::downgrade(&login);
    login.client.connect_changed(move |e| {
        if let Some(l) = weak.upgrade() {
            store_client_id(l.provider.get(), e.text().trim().to_string());
            l.sync_state();
        }
    });
    let weak = Rc::downgrade(&login);
    login.cli_button.connect_clicked(move |_| {
        if let Some(l) = weak.upgrade() {
            l.sign_in_with_cli();
        }
    });
    let weak = Rc::downgrade(&login);
    login.sign.connect_clicked(move |_| {
        if let Some(l) = weak.upgrade() {
            l.sign_in_with_token();
        }
    });
    let weak = Rc::downgrade(&login);
    login.token.connect_activate(move |_| {
        if let Some(l) = weak.upgrade() {
            l.sign_in_with_token();
        }
    });
    let weak = Rc::downgrade(&login);
    login.create.connect_clicked(move |_| {
        if let Some(l) = weak.upgrade() {
            if let Some(url) = l.provider.get().token_create_url(&l.effective_host(), brand::APP_NAME) {
                util::open_url(&url);
            }
        }
    });
    let weak = Rc::downgrade(&login);
    login.get_code.connect_clicked(move |_| {
        if let Some(l) = weak.upgrade() {
            l.start_device();
        }
    });
    let weak = Rc::downgrade(&login);
    copy_open.connect_clicked(move |_| {
        if let Some((code, uri)) = weak.upgrade().and_then(|l| l.device_uri.borrow().clone()) {
            util::copy(&code);
            util::open_url(&uri);
        }
    });

    let initial = settings::read(|s| GitProvider::from_key(&s.login_provider)).unwrap_or(GitProvider::Github);
    login.select(initial);
    if let Some((_, t)) = login.tabs.iter().find(|(p, _)| *p == initial) {
        t.set_active(true);
    }
    CURRENT.with(|c| *c.borrow_mut() = Rc::downgrade(&login));
    let weak = Rc::downgrade(&login);
    column.connect_destroy(move |_| {
        if let Some(l) = weak.upgrade() {
            if let Some(h) = l.cancel.borrow_mut().take() {
                h.abort();
            }
        }
    });
    keep_alive(&column, login);
    column
}

pub fn select_provider(p: GitProvider) {
    if let Some(l) = CURRENT.with(|c| c.borrow().upgrade()) {
        l.select(p);
    }
}

pub fn build() -> gtk::Widget {
    let column = form(false, None);
    pad(&column, 40);
    column.set_halign(gtk::Align::Center);
    column.set_size_request(480, -1);

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

pub fn add_account_sheet() {
    let sheet = super::dialogs::Sheet::new("Add account", 560, 680);
    let d = sheet.dialog.clone();
    let done: Rc<dyn Fn()> = Rc::new(move || {
        d.close();
    });
    sheet.body.append(&form(true, Some(done)));
    sheet.add_footer_spacer();
    sheet.cancel_button();
    sheet.present();
}
