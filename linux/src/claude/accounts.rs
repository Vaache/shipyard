use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::rc::Rc;

use crate::host::{self, HostCmd};
use crate::settings::{self, ClaudeProfile};
use crate::signal::Signal;
use crate::{demo, util};

pub const DEFAULT_ID: &str = "default";
const SHARED_ITEMS: &[&str] = &["settings.json", "CLAUDE.md", "RTK.md", "skills", "commands", "plugins", "agents", "output-styles", "keybindings.json"];

pub fn profiles() -> Vec<ClaudeProfile> {
    let mut list = settings::read(|s| s.claude_profiles.clone());
    if !list.iter().any(|p| p.id == DEFAULT_ID) {
        list.insert(0, ClaudeProfile { id: DEFAULT_ID.into(), label: "Main".into(), config_dir: None, email: None, plan: None });
    }
    list
}

pub fn active_id() -> String {
    let id = settings::read(|s| s.claude_active_profile.clone());
    if profiles().iter().any(|p| p.id == id) {
        id
    } else {
        DEFAULT_ID.into()
    }
}

pub fn active() -> ClaudeProfile {
    let id = active_id();
    profiles().into_iter().find(|p| p.id == id).unwrap_or_else(|| profiles()[0].clone())
}

pub fn active_override() -> Option<String> {
    active().config_dir
}

pub fn default_config_dir() -> String {
    std::env::var("CLAUDE_CONFIG_DIR").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| format!("{}/.claude", host::home()))
}

pub fn config_dir(profile: Option<&ClaudeProfile>) -> String {
    let p = profile.cloned().unwrap_or_else(active);
    p.config_dir.unwrap_or_else(default_config_dir)
}

fn save(list: Vec<ClaudeProfile>, active: String) {
    settings::update(|s| {
        s.claude_profiles = list;
        s.claude_active_profile = active;
    });
}

pub fn base_dir() -> std::path::PathBuf {
    if util::is_flatpak() {
        std::path::PathBuf::from(format!("{}/.local/share/{}/ClaudeAccounts", host::home(), crate::brand::BIN_NAME))
    } else {
        settings::data_dir().join("ClaudeAccounts")
    }
}

pub fn update_cached_info(email: Option<String>, plan: Option<String>) {
    let mut list = profiles();
    let id = active_id();
    if let Some(p) = list.iter_mut().find(|p| p.id == id) {
        if p.email != email || p.plan != plan {
            p.email = email;
            p.plan = plan;
            save(list, id);
        }
    }
}

pub fn add(label: &str, share: bool) -> Result<ClaudeProfile, String> {
    let id = uuid::Uuid::new_v4().to_string().to_lowercase();
    let dir = base_dir().join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if share {
        let source = std::path::PathBuf::from(default_config_dir());
        for item in SHARED_ITEMS {
            let src = source.join(item);
            if src.exists() {
                let _ = std::os::unix::fs::symlink(&src, dir.join(item));
            }
        }
    }
    let mut list = profiles();
    let name = label.trim();
    let p = ClaudeProfile {
        id,
        label: if name.is_empty() { format!("Account {}", list.len() + 1) } else { name.to_string() },
        config_dir: Some(dir.to_string_lossy().to_string()),
        email: None,
        plan: None,
    };
    list.push(p.clone());
    save(list, active_id());
    Ok(p)
}

pub fn rename(id: &str, label: &str) {
    let name = label.trim();
    if name.is_empty() {
        return;
    }
    let mut list = profiles();
    if let Some(p) = list.iter_mut().find(|p| p.id == id) {
        p.label = name.to_string();
    }
    save(list, active_id());
}

pub fn remove(id: &str) {
    if id == DEFAULT_ID {
        return;
    }
    let list: Vec<ClaudeProfile> = profiles().into_iter().filter(|p| p.id != id).collect();
    let active = if active_id() == id { DEFAULT_ID.to_string() } else { active_id() };
    save(list, active);
}

pub fn set_active(id: &str) {
    if profiles().iter().any(|p| p.id == id) {
        save(profiles(), id.to_string());
    }
}

pub fn browser_command() -> String {
    let chosen = settings::read(|s| s.link_browser.clone());
    if chosen != "system" && !util::is_flatpak() {
        use gtk::prelude::*;
        if let Some(info) = gtk::gio::AppInfo::all_for_type("x-scheme-handler/https").into_iter().find(|i| i.id().map(|s| s.as_str() == chosen).unwrap_or(false)) {
            if let Some(exe) = info.executable().to_str() {
                if !exe.is_empty() {
                    return exe.to_string();
                }
            }
        }
    }
    "xdg-open".into()
}

pub struct Env {
    pub set: Vec<(String, String)>,
    pub unset: Vec<String>,
}

pub fn env(config_dir: Option<&str>, bin: &str) -> Env {
    let mut set = vec![("PATH".to_string(), host::path_with(bin)), ("BROWSER".to_string(), browser_command())];
    if let Some(d) = config_dir {
        set.push(("CLAUDE_CONFIG_DIR".into(), d.to_string()));
    }
    let mut unset = super::clean_env_keys();
    if config_dir.is_none() && util::is_flatpak() {
        unset.push("CLAUDE_CONFIG_DIR".into());
    }
    Env { set, unset }
}

pub fn command(bin: &str, config_dir: Option<&str>) -> HostCmd {
    let e = env(config_dir, bin);
    let mut c = HostCmd::new(bin);
    for (k, v) in e.set {
        c = c.env(&k, &v);
    }
    for k in e.unset {
        c = c.unset(&k);
    }
    c
}

#[derive(Clone, Debug)]
pub struct AccountInfo {
    pub email: Option<String>,
    pub org_name: Option<String>,
    pub subscription: Option<String>,
    pub method: Option<String>,
    pub provider: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Status {
    Unknown,
    Checking,
    NotInstalled,
    LoggedOut,
    LoggedIn(AccountInfo),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    ClaudeAi,
    Console,
    Sso,
}

impl LoginMethod {
    pub const ALL: [LoginMethod; 3] = [LoginMethod::ClaudeAi, LoginMethod::Console, LoginMethod::Sso];

    pub fn title(&self) -> &'static str {
        match self {
            LoginMethod::ClaudeAi => "Claude subscription (Pro, Max, Team)",
            LoginMethod::Console => "Anthropic Console (API billing)",
            LoginMethod::Sso => "SSO",
        }
    }

    pub fn flag(&self) -> &'static str {
        match self {
            LoginMethod::ClaudeAi => "--claudeai",
            LoginMethod::Console => "--console",
            LoginMethod::Sso => "--sso",
        }
    }
}

pub struct AccountStore {
    pub status: RefCell<Status>,
    pub logging_in: std::cell::Cell<bool>,
    pub fallback_url: RefCell<Option<String>>,
    pub login_error: RefCell<Option<String>>,
    login_input: RefCell<Option<std::process::ChildStdin>>,
    login_pid: RefCell<Option<u32>>,
    output: RefCell<String>,
    pub changed: Signal<()>,
}

thread_local! {
    static STORE: Rc<AccountStore> = Rc::new(AccountStore {
        status: RefCell::new(Status::Unknown),
        logging_in: std::cell::Cell::new(false),
        fallback_url: RefCell::new(None),
        login_error: RefCell::new(None),
        login_input: RefCell::new(None),
        login_pid: RefCell::new(None),
        output: RefCell::new(String::new()),
        changed: Signal::new(),
    });
}

pub fn store() -> Rc<AccountStore> {
    STORE.with(|s| s.clone())
}

impl AccountStore {
    pub fn is_logged_in(&self) -> bool {
        matches!(*self.status.borrow(), Status::LoggedIn(_))
    }

    pub fn info(&self) -> Option<AccountInfo> {
        match &*self.status.borrow() {
            Status::LoggedIn(i) => Some(i.clone()),
            _ => None,
        }
    }

    fn set_status(&self, s: Status) {
        *self.status.borrow_mut() = s;
        self.changed.emit(&());
    }

    pub async fn refresh(self: Rc<Self>) {
        let bin = crate::rt::blocking(host::claude_binary).await;
        let Some(bin) = bin else {
            self.set_status(Status::NotInstalled);
            return;
        };
        if matches!(*self.status.borrow(), Status::Unknown) {
            self.set_status(Status::Checking);
        }
        let cmd = command(&bin, active_override().as_deref()).args(["auth", "status"]);
        let (out, _, _) = crate::rt::blocking(move || cmd.output()).await;
        let json: Option<serde_json::Value> = serde_json::from_str(out.trim()).ok();
        let Some(json) = json else {
            self.set_status(Status::LoggedOut);
            return;
        };
        if json.get("loggedIn").and_then(|v| v.as_bool()) == Some(true) {
            let s = |k: &str| json.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
            let capitalize = |v: String| {
                let mut c = v.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()).unwrap_or_default()
            };
            let plan = s("subscriptionType").map(capitalize);
            if !demo::is_on() {
                update_cached_info(s("email"), plan.clone());
            }
            self.set_status(Status::LoggedIn(AccountInfo {
                email: if demo::is_on() { Some(demo::EMAIL.into()) } else { s("email") },
                org_name: if demo::is_on() { None } else { s("orgName") },
                subscription: plan,
                method: s("authMethod"),
                provider: s("apiProvider"),
            }));
        } else {
            self.set_status(Status::LoggedOut);
        }
    }

    pub fn login(self: &Rc<Self>, method: LoginMethod, email: &str) {
        if self.logging_in.get() {
            return;
        }
        let Some(bin) = host::claude_binary() else { return };
        *self.login_error.borrow_mut() = None;
        *self.fallback_url.borrow_mut() = None;
        self.output.borrow_mut().clear();
        let mut cmd = command(&bin, active_override().as_deref()).args(["auth", "login", method.flag()]).cwd(&host::home());
        if !email.trim().is_empty() {
            cmd = cmd.args(["--email", email.trim()]);
        }
        let mut c = cmd.command();
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => {
                *self.login_error.borrow_mut() = Some(e.to_string());
                self.changed.emit(&());
                return;
            }
        };
        *self.login_input.borrow_mut() = child.stdin.take();
        *self.login_pid.borrow_mut() = Some(child.id());
        self.logging_in.set(true);
        self.changed.emit(&());
        let (tx, rx) = async_channel::unbounded::<Option<String>>();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let tx2 = tx.clone();
        if let Some(out) = stdout {
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    let _ = tx2.send_blocking(Some(line));
                }
            });
        }
        let tx3 = tx.clone();
        if let Some(err) = stderr {
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    let _ = tx3.send_blocking(Some(line));
                }
            });
        }
        let (done_tx, done_rx) = async_channel::bounded::<i32>(1);
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            let _ = done_tx.send_blocking(code);
            drop(tx);
        });
        let me = self.clone();
        crate::rt::spawn_local(async move {
            while let Ok(Some(line)) = rx.recv().await {
                me.output.borrow_mut().push_str(&line);
                me.output.borrow_mut().push('\n');
                if me.fallback_url.borrow().is_none() {
                    if let Some(m) = regex::Regex::new(r"https://[^\s]+").ok().and_then(|re| re.find(&line).map(|m| m.as_str().to_string())) {
                        *me.fallback_url.borrow_mut() = Some(m);
                        me.changed.emit(&());
                    }
                }
            }
        });
        let me = self.clone();
        crate::rt::spawn_local(async move {
            let code = done_rx.recv().await.unwrap_or(-1);
            me.logging_in.set(false);
            *me.login_input.borrow_mut() = None;
            *me.login_pid.borrow_mut() = None;
            if code != 0 && code != 15 && code != -1 {
                let out = me.output.borrow().clone();
                let tail: Vec<&str> = out.lines().filter(|l| !l.contains("http") && !l.trim().is_empty()).collect();
                let start = tail.len().saturating_sub(3);
                let t = tail[start..].join("\n");
                *me.login_error.borrow_mut() = Some(if t.is_empty() { format!("Sign-in failed (code {code})") } else { t });
            }
            me.changed.emit(&());
            me.refresh().await;
        });
    }

    pub fn submit_code(&self, code: &str) {
        let t = code.trim();
        if t.is_empty() {
            return;
        }
        if let Some(input) = self.login_input.borrow_mut().as_mut() {
            let _ = input.write_all(format!("{t}\n").as_bytes());
            let _ = input.flush();
        }
    }

    pub fn cancel_login(&self) {
        if let Some(pid) = *self.login_pid.borrow() {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
        }
    }

    pub async fn logout(self: Rc<Self>, config_dir: Option<String>) {
        let Some(bin) = crate::rt::blocking(host::claude_binary).await else { return };
        let dir = config_dir.or_else(active_override);
        let cmd = command(&bin, dir.as_deref()).args(["auth", "logout"]);
        crate::rt::blocking(move || cmd.output()).await;
        self.refresh().await;
    }
}
