use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use vte4::prelude::*;

use super::accounts;
use super::chat::ChatSession;
use super::git::WorkingCopy;
use super::sessions::{self, Project, SessionInfo};
use super::{ClaudeMode, ModelOption, TabKind, TabState};
use crate::host::{self, HostCmd};
use crate::signal::Signal;
use crate::{demo, settings, util};

pub struct Tab {
    pub id: u64,
    pub folder: String,
    pub kind: TabKind,
    pub title: RefCell<String>,
    pub session_id: RefCell<String>,
    pub git: Rc<WorkingCopy>,
    pub chat: Option<Rc<ChatSession>>,
    pub config_dir: Option<String>,
    terminal: RefCell<Option<vte4::Terminal>>,
    pub terminal_running: Cell<bool>,
    pub terminal_sleeping: Cell<bool>,
    pub terminal_memory: Cell<i64>,
    pub exit_code: Cell<Option<i32>>,
    pub last_output: Cell<Instant>,
    terminal_pid: Cell<i32>,
    wired: Cell<bool>,
}

impl Tab {
    pub fn terminal(&self) -> vte4::Terminal {
        if let Some(t) = self.terminal.borrow().as_ref() {
            return t.clone();
        }
        let t = vte4::Terminal::new();
        t.set_vexpand(true);
        t.set_hexpand(true);
        t.set_scrollback_lines(10000);
        t.set_mouse_autohide(true);
        t.set_allow_hyperlink(true);
        t.set_bold_is_bright(false);
        t.set_font(Some(&gtk::pango::FontDescription::from_string("Monospace 11")));
        apply_terminal_colors(&t);
        *self.terminal.borrow_mut() = Some(t.clone());
        t
    }

    pub fn has_terminal(&self) -> bool {
        self.terminal.borrow().is_some()
    }

    pub fn running(&self) -> bool {
        match &self.chat {
            Some(c) => c.running.get(),
            None => self.terminal_running.get(),
        }
    }

    pub fn state(&self) -> TabState {
        if let Some(chat) = &self.chat {
            if chat.pending_count() > 0 {
                return TabState::Waiting;
            }
            if chat.busy.get() {
                return TabState::Working;
            }
            if chat.running.get() {
                return TabState::Active;
            }
            return if chat.sleeping.get() { TabState::Sleeping } else { TabState::Stopped };
        }
        if self.terminal_running.get() {
            return if self.last_output.get().elapsed() < Duration::from_secs(3) { TabState::Working } else { TabState::Active };
        }
        if self.terminal_sleeping.get() {
            TabState::Sleeping
        } else {
            TabState::Stopped
        }
    }

    pub fn memory_bytes(&self) -> i64 {
        match &self.chat {
            Some(c) => c.memory_bytes.get(),
            None => self.terminal_memory.get(),
        }
    }

    pub fn pid(&self) -> i32 {
        match &self.chat {
            Some(c) => c.pid(),
            None => {
                if self.terminal_running.get() {
                    self.terminal_pid.get()
                } else {
                    0
                }
            }
        }
    }

    pub fn sleep_now(&self) {
        if let Some(c) = &self.chat {
            c.sleep();
        } else if self.terminal_running.get() {
            self.terminal_sleeping.set(true);
            self.kill_terminal();
        }
    }

    pub fn kill_terminal(&self) {
        let pid = self.terminal_pid.get();
        if pid > 0 && self.terminal_running.get() {
            unsafe {
                libc::kill(pid, libc::SIGHUP);
            }
        }
    }

    pub fn busy(&self) -> bool {
        self.chat.as_ref().map(|c| c.busy.get()).unwrap_or(false)
    }

    pub fn needs_attention(&self) -> bool {
        self.chat.as_ref().map(|c| c.pending_count() > 0).unwrap_or(false)
    }

    pub fn project_name(&self) -> String {
        util::last_component(&self.folder)
    }
}

pub fn apply_terminal_colors(t: &vte4::Terminal) {
    let dark = crate::theme::is_dark();
    let brand = crate::theme::is_brand();
    let (bg, fg, cursor) = match (brand, dark) {
        (true, true) => ("#101828", "#E6EBF2", "#5BB8EC"),
        (true, false) => ("#FDFCFA", "#1A2333", "#2A8BC8"),
        (false, true) => ("#1e1e1f", "#e6e6e6", "#3584e4"),
        (false, false) => ("#fcfcfc", "#1f1f1f", "#1c71d8"),
    };
    let palette: Vec<gtk::gdk::RGBA> = if dark {
        ["#2e3436", "#ef2929", "#8ae234", "#fce94f", "#729fcf", "#ad7fa8", "#34e2e2", "#d3d7cf", "#555753", "#ff6b6b", "#9ee66b", "#ffef7a", "#8fb8e8", "#c9a0dc", "#6af0f0", "#eeeeec"]
    } else {
        ["#2e3436", "#cc0000", "#4e9a06", "#c4a000", "#3465a4", "#75507b", "#06989a", "#d3d7cf", "#555753", "#ef2929", "#4e9a06", "#a68b00", "#3465a4", "#ad7fa8", "#06989a", "#eeeeec"]
    }
    .iter()
    .map(|c| util::rgba(c))
    .collect();
    let refs: Vec<&gtk::gdk::RGBA> = palette.iter().collect();
    t.set_colors(Some(&util::rgba(fg)), Some(&util::rgba(bg)), &refs);
    t.set_color_cursor(Some(&util::rgba(cursor)));
}

#[derive(Clone, Debug)]
pub enum WsEvent {
    Tabs,
    Selection,
    Projects,
    TabState,
}

pub struct Workspace {
    pub tabs: RefCell<Vec<Rc<Tab>>>,
    pub selected: Cell<Option<u64>>,
    pub projects: RefCell<Vec<Project>>,
    pub loading_projects: Cell<bool>,
    pub projects_updated: Cell<Option<chrono::DateTime<chrono::Utc>>>,
    pub show_inspector: Cell<bool>,
    gits: RefCell<HashMap<String, Rc<WorkingCopy>>>,
    monitor: Cell<bool>,
    next_id: Cell<u64>,
    pub changed: Signal<WsEvent>,
}

impl Workspace {
    pub fn new() -> Rc<Self> {
        Rc::new(Workspace {
            tabs: RefCell::new(vec![]),
            selected: Cell::new(None),
            projects: RefCell::new(vec![]),
            loading_projects: Cell::new(false),
            projects_updated: Cell::new(None),
            show_inspector: Cell::new(true),
            gits: RefCell::new(HashMap::new()),
            monitor: Cell::new(false),
            next_id: Cell::new(1),
            changed: Signal::new(),
        })
    }

    pub fn emit(&self, e: WsEvent) {
        self.changed.emit(&e);
    }

    pub fn model(&self) -> ModelOption {
        ModelOption::from_key(&settings::read(|s| s.claude_model.clone()))
    }

    pub fn set_model(&self, m: ModelOption) {
        settings::update(|s| s.claude_model = m.key().to_string());
        self.emit(WsEvent::TabState);
    }

    pub fn effort(&self) -> String {
        settings::read(|s| s.claude_effort.clone())
    }

    pub fn set_effort(&self, e: &str) {
        let e = e.to_string();
        settings::update(|s| s.claude_effort = e);
        self.emit(WsEvent::TabState);
    }

    pub fn mode(&self) -> ClaudeMode {
        ClaudeMode::from_key(&settings::read(|s| s.claude_mode.clone())).unwrap_or(ClaudeMode::Standard)
    }

    pub fn set_mode(&self, m: ClaudeMode) {
        settings::update(|s| s.claude_mode = m.key().to_string());
        self.emit(WsEvent::TabState);
    }

    pub fn idle_minutes(&self) -> i64 {
        settings::read(|s| s.claude_idle_minutes)
    }

    pub fn set_idle_minutes(&self, m: i64) {
        settings::update(|s| s.claude_idle_minutes = m);
        self.emit(WsEvent::TabState);
    }

    pub fn preferred_kind(&self) -> TabKind {
        TabKind::from_key(&settings::read(|s| s.claude_ui.clone())).unwrap_or(TabKind::Chat)
    }

    pub fn set_preferred_kind(&self, k: TabKind) {
        settings::update(|s| s.claude_ui = k.key().to_string());
        self.emit(WsEvent::TabState);
    }

    pub fn selected_tab(&self) -> Option<Rc<Tab>> {
        let id = self.selected.get()?;
        self.tabs.borrow().iter().find(|t| t.id == id).cloned()
    }

    pub fn select(&self, id: u64) {
        self.selected.set(Some(id));
        self.emit(WsEvent::Selection);
    }

    pub fn total_memory(&self) -> i64 {
        self.tabs.borrow().iter().map(|t| t.memory_bytes()).sum()
    }

    pub fn running_count(&self) -> usize {
        self.tabs.borrow().iter().filter(|t| t.running()).count()
    }

    pub fn attention_count(&self) -> usize {
        self.tabs.borrow().iter().filter(|t| t.needs_attention()).count()
    }

    pub fn git(&self, folder: &str) -> Rc<WorkingCopy> {
        if let Some(g) = self.gits.borrow().get(folder) {
            return g.clone();
        }
        let g = WorkingCopy::new(folder);
        self.gits.borrow_mut().insert(folder.to_string(), g.clone());
        g.start_watching();
        g.refresh();
        g
    }

    fn start_monitoring(self: &Rc<Self>) {
        if self.monitor.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(15, move || {
            let Some(me) = weak.upgrade() else { return glib::ControlFlow::Break };
            me.tick();
            glib::ControlFlow::Continue
        });
        let weak = Rc::downgrade(self);
        glib::timeout_add_seconds_local(2, move || {
            let Some(me) = weak.upgrade() else { return glib::ControlFlow::Break };
            if me.tabs.borrow().iter().any(|t| t.chat.is_none() && t.terminal_running.get()) {
                me.emit(WsEvent::TabState);
            }
            glib::ControlFlow::Continue
        });
    }

    fn tick(self: &Rc<Self>) {
        let live: Vec<Rc<Tab>> = self.tabs.borrow().iter().filter(|t| t.running()).cloned().collect();
        if !live.is_empty() {
            let me = self.clone();
            crate::rt::spawn_local(async move {
                let snap = crate::rt::blocking(super::memory::snapshot).await;
                for tab in &live {
                    let bytes = super::memory::tree_bytes(tab.pid(), &snap);
                    match &tab.chat {
                        Some(c) => c.memory_bytes.set(bytes),
                        None => tab.terminal_memory.set(bytes),
                    }
                }
                me.emit(WsEvent::TabState);
            });
        }
        let limit = self.idle_minutes();
        if limit <= 0 {
            return;
        }
        let threshold = Duration::from_secs(limit as u64 * 60);
        for tab in self.tabs.borrow().iter() {
            if let Some(c) = &tab.chat {
                if c.can_sleep() && c.last_activity.get().elapsed() > threshold {
                    c.sleep();
                }
            } else if tab.terminal_running.get() && tab.last_output.get().elapsed() > threshold {
                tab.sleep_now();
            }
        }
    }

    pub fn close_all_for_profile_switch(&self) {
        for t in self.tabs.borrow().iter() {
            if let Some(c) = &t.chat {
                c.stop();
            }
            t.kill_terminal();
        }
        self.tabs.borrow_mut().clear();
        self.selected.set(None);
        self.projects.borrow_mut().clear();
        self.emit(WsEvent::Tabs);
        self.emit(WsEvent::Projects);
    }

    pub fn sleep_all(&self) {
        for t in self.tabs.borrow().iter() {
            if t.running() && t.state() != TabState::Working && t.state() != TabState::Waiting {
                t.sleep_now();
            }
        }
    }

    pub fn stop_all(&self) {
        for t in self.tabs.borrow().iter() {
            if let Some(c) = &t.chat {
                c.stop();
            }
            t.kill_terminal();
        }
    }

    pub async fn reload_projects(self: &Rc<Self>) {
        self.loading_projects.set(true);
        self.emit(WsEvent::Projects);
        let root = std::path::PathBuf::from(accounts::config_dir(None)).join("projects");
        let profile = accounts::active_id();
        let loaded = crate::rt::blocking(move || sessions::load_all(root)).await;
        self.loading_projects.set(false);
        if profile != accounts::active_id() {
            return;
        }
        let next = match demo::folder() {
            Some(f) => loaded.into_iter().filter(|p| p.path.starts_with(&f)).collect(),
            None => loaded,
        };
        *self.projects.borrow_mut() = next;
        self.projects_updated.set(Some(chrono::Utc::now()));
        self.emit(WsEvent::Projects);
    }

    pub fn find_session(&self, id: &str) -> Option<SessionInfo> {
        self.projects.borrow().iter().flat_map(|p| p.sessions.iter()).find(|s| s.id == id).cloned()
    }

    pub fn tab_for_session(&self, id: &str) -> Option<Rc<Tab>> {
        self.tabs.borrow().iter().find(|t| *t.session_id.borrow() == id).cloned()
    }

    fn make_tab(self: &Rc<Self>, folder: &str, kind: TabKind, title: &str, session_id: &str) -> Rc<Tab> {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let config_dir = accounts::active_override();
        let tab = Rc::new(Tab {
            id,
            folder: folder.to_string(),
            kind,
            title: RefCell::new(title.to_string()),
            session_id: RefCell::new(session_id.to_string()),
            git: self.git(folder),
            chat: if kind == TabKind::Chat { Some(ChatSession::new(folder, config_dir.clone())) } else { None },
            config_dir,
            terminal: RefCell::new(None),
            terminal_running: Cell::new(false),
            terminal_sleeping: Cell::new(false),
            terminal_memory: Cell::new(0),
            exit_code: Cell::new(None),
            last_output: Cell::new(Instant::now()),
            terminal_pid: Cell::new(0),
            wired: Cell::new(false),
        });
        if let Some(chat) = &tab.chat {
            let w = Rc::downgrade(self);
            chat.changed.connect(move |e| match w.upgrade() {
                Some(me) => {
                    if matches!(e, super::chat::Event::State) {
                        me.emit(WsEvent::TabState);
                    }
                    true
                }
                None => false,
            });
        }
        self.tabs.borrow_mut().push(tab.clone());
        self.selected.set(Some(id));
        self.start_monitoring();
        self.emit(WsEvent::Tabs);
        self.emit(WsEvent::Selection);
        let w = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_secs(4), move || {
            if let Some(me) = w.upgrade() {
                crate::rt::spawn_local(async move { me.reload_projects().await });
            }
        });
        tab
    }

    pub fn new_session(self: &Rc<Self>, folder: &str, prompt: Option<String>, name: Option<String>, mode: Option<ClaudeMode>, kind: Option<TabKind>) -> Rc<Tab> {
        let kind = kind.unwrap_or_else(|| self.preferred_kind());
        let title = name.clone().or_else(|| prompt.as_ref().map(|p| p.chars().take(48).collect())).unwrap_or_else(|| "New session".into());
        if kind == TabKind::Chat {
            let tab = self.make_tab(folder, TabKind::Chat, &title, "");
            self.start_chat(&tab, None, false, prompt, mode);
            return tab;
        }
        let session_id = uuid::Uuid::new_v4().to_string().to_lowercase();
        let mut args = vec!["--session-id".to_string(), session_id.clone()];
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            args.extend(["-n".into(), n]);
        }
        if let Some(p) = prompt.filter(|p| !p.is_empty()) {
            args.push(p);
        }
        let tab = self.make_tab(folder, TabKind::Terminal, &title, &session_id);
        self.launch_terminal(&tab, args, mode);
        tab
    }

    pub fn resume(self: &Rc<Self>, s: &SessionInfo, fork: bool, kind: Option<TabKind>) -> Rc<Tab> {
        if !fork {
            if let Some(open) = self.tab_for_session(&s.id) {
                self.select(open.id);
                return open;
            }
        }
        let kind = kind.unwrap_or_else(|| self.preferred_kind());
        let tab = self.make_tab(&s.cwd, kind, &s.title, if fork { "" } else { &s.id });
        if kind == TabKind::Chat {
            if let Some(c) = &tab.chat {
                c.load_history(&s.file, 600);
            }
            self.start_chat(&tab, Some(s.id.clone()), fork, None, None);
        } else {
            let mut args = vec!["--resume".to_string(), s.id.clone()];
            if fork {
                args.push("--fork-session".into());
            }
            self.launch_terminal(&tab, args, None);
        }
        tab
    }

    fn session_file(&self, id: &str) -> Option<std::path::PathBuf> {
        self.find_session(id).map(|s| s.file)
    }

    pub fn open_in_terminal(self: &Rc<Self>, tab: &Rc<Tab>) {
        let sid = tab.session_id.borrow().clone();
        if sid.is_empty() {
            return;
        }
        if let Some(c) = &tab.chat {
            c.stop();
        }
        let title = tab.title.borrow().clone();
        let t = self.make_tab(&tab.folder, TabKind::Terminal, &title, &sid);
        self.launch_terminal(&t, vec!["--resume".into(), sid], None);
        self.tabs.borrow_mut().retain(|x| x.id != tab.id);
        self.selected.set(Some(t.id));
        self.emit(WsEvent::Tabs);
        self.emit(WsEvent::Selection);
    }

    pub fn open_in_chat(self: &Rc<Self>, tab: &Rc<Tab>) {
        let sid = tab.session_id.borrow().clone();
        if sid.is_empty() {
            return;
        }
        tab.kill_terminal();
        let title = tab.title.borrow().clone();
        let c = self.make_tab(&tab.folder, TabKind::Chat, &title, &sid);
        if let (Some(file), Some(chat)) = (self.session_file(&sid), &c.chat) {
            chat.load_history(&file, 600);
        }
        self.start_chat(&c, Some(sid), false, None, None);
        self.tabs.borrow_mut().retain(|x| x.id != tab.id);
        self.selected.set(Some(c.id));
        self.emit(WsEvent::Tabs);
        self.emit(WsEvent::Selection);
    }

    pub fn restart(self: &Rc<Self>, tab: &Rc<Tab>) {
        if tab.running() {
            return;
        }
        if tab.kind == TabKind::Chat {
            let has_sid = tab.chat.as_ref().and_then(|c| c.session_id.borrow().clone()).map(|s| !s.is_empty()).unwrap_or(false);
            if has_sid {
                if let Some(c) = &tab.chat {
                    c.wake();
                }
            } else {
                let sid = tab.session_id.borrow().clone();
                self.start_chat(tab, if sid.is_empty() { None } else { Some(sid) }, false, None, None);
            }
        } else {
            tab.terminal_sleeping.set(false);
            let sid = tab.session_id.borrow().clone();
            self.launch_terminal(tab, vec!["--resume".into(), sid], None);
        }
        self.emit(WsEvent::TabState);
    }

    pub fn close(&self, tab: &Rc<Tab>) {
        if let Some(c) = &tab.chat {
            c.stop();
        }
        tab.kill_terminal();
        self.tabs.borrow_mut().retain(|t| t.id != tab.id);
        if self.selected.get() == Some(tab.id) {
            self.selected.set(self.tabs.borrow().last().map(|t| t.id));
        }
        self.emit(WsEvent::Tabs);
        self.emit(WsEvent::Selection);
    }

    pub fn apply_model_to_current(&self) {
        let Some(tab) = self.selected_tab() else { return };
        if !tab.running() || self.model() == ModelOption::Default {
            return;
        }
        match &tab.chat {
            Some(c) => c.set_model(self.model().key()),
            None => tab.terminal().feed_child(format!("/model {}\r", self.model().key()).as_bytes()),
        }
    }

    pub fn send(self: &Rc<Self>, text: &str, tab: Option<Rc<Tab>>, submit: bool) {
        let Some(tab) = tab.or_else(|| self.selected_tab()) else { return };
        if let Some(c) = &tab.chat {
            if !c.running.get() {
                self.restart(&tab);
            }
            c.send(text, vec![]);
            return;
        }
        if !tab.running() {
            return;
        }
        let t = tab.terminal();
        t.feed_child(format!("\u{1b}[200~{text}\u{1b}[201~").as_bytes());
        if submit {
            t.feed_child(b"\r");
        }
        t.grab_focus();
    }

    fn start_chat(self: &Rc<Self>, tab: &Rc<Tab>, resume: Option<String>, fork: bool, prompt: Option<String>, mode: Option<ClaudeMode>) {
        let Some(chat) = tab.chat.clone() else { return };
        let wt = Rc::downgrade(tab);
        let ws = Rc::downgrade(self);
        *chat.on_session_id.borrow_mut() = Some(Rc::new(move |sid| {
            if let Some(t) = wt.upgrade() {
                *t.session_id.borrow_mut() = sid;
            }
            if let Some(w) = ws.upgrade() {
                w.emit(WsEvent::TabState);
            }
        }));
        let wt = Rc::downgrade(tab);
        *chat.on_turn_finished.borrow_mut() = Some(Rc::new(move || {
            if let Some(t) = wt.upgrade() {
                t.git.refresh();
            }
        }));
        let wt = Rc::downgrade(tab);
        *chat.on_attention.borrow_mut() = Some(Rc::new(move || {
            if let Some(t) = wt.upgrade() {
                crate::app::claude_attention(&t);
            }
        }));
        let m = mode.unwrap_or_else(|| self.mode());
        let model = self.model();
        let effort = self.effort();
        chat.start(
            resume,
            fork,
            if model == ModelOption::Default { None } else { Some(model.key().to_string()) },
            if effort.is_empty() { None } else { Some(effort) },
            if m == ClaudeMode::Standard { None } else { Some(m.key().to_string()) },
            prompt,
        );
    }

    pub fn launch_terminal(self: &Rc<Self>, tab: &Rc<Tab>, extra: Vec<String>, mode: Option<ClaudeMode>) {
        let Some(bin) = host::claude_binary() else {
            tab.exit_code.set(Some(127));
            self.emit(WsEvent::TabState);
            return;
        };
        let mode = mode.unwrap_or_else(|| self.mode());
        let mut args: Vec<String> = vec![];
        if self.model() != ModelOption::Default {
            args.extend(["--model".into(), self.model().key().into()]);
        }
        let effort = self.effort();
        if !effort.is_empty() {
            args.extend(["--effort".into(), effort]);
        }
        if mode != ClaudeMode::Standard {
            args.extend(["--permission-mode".into(), mode.key().into()]);
        }
        args.extend(extra);
        let env = accounts::env(tab.config_dir.as_deref(), &bin);
        let unset: String = env.unset.iter().map(|k| format!("-u {} ", util::shell_quote(k))).collect();
        let command = format!("exec env {unset}{} {}", util::shell_quote(&bin), args.iter().map(|a| util::shell_quote(a)).collect::<Vec<_>>().join(" "));
        let shell = host::user_shell();
        let mut cmd = HostCmd::new(&shell).args(["-l", "-c", command.as_str()]).cwd(&tab.folder);
        let mut envs: Vec<(String, String)> = vec![("TERM".into(), "xterm-256color".into()), ("COLORTERM".into(), "truecolor".into())];
        envs.extend(env.set);
        if std::env::var("LANG").is_err() {
            envs.push(("LANG".into(), "C.UTF-8".into()));
        }
        for (k, v) in &envs {
            cmd = cmd.env(k, v);
        }
        let argv = cmd.argv();
        let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        let env_strings: Vec<String> = if util::is_flatpak() { vec![] } else { envs.iter().map(|(k, v)| format!("{k}={v}")).collect() };
        let env_refs: Vec<&str> = env_strings.iter().map(|s| s.as_str()).collect();
        let term = tab.terminal();
        tab.terminal_running.set(true);
        tab.exit_code.set(None);
        tab.last_output.set(Instant::now());
        if !tab.wired.replace(true) {
            let wt = Rc::downgrade(tab);
            term.connect_contents_changed(move |_| {
                if let Some(t) = wt.upgrade() {
                    t.last_output.set(Instant::now());
                }
            });
            let wt = Rc::downgrade(tab);
            let ws = Rc::downgrade(self);
            term.connect_child_exited(move |_, status| {
                if let Some(t) = wt.upgrade() {
                    t.terminal_running.set(false);
                    t.exit_code.set(Some(status));
                }
                if let Some(w) = ws.upgrade() {
                    w.emit(WsEvent::TabState);
                }
            });
        }
        let wt = Rc::downgrade(tab);
        let ws = Rc::downgrade(self);
        term.spawn_async(
            vte4::PtyFlags::DEFAULT,
            Some(&tab.folder),
            &argv_refs,
            &env_refs,
            glib::SpawnFlags::SEARCH_PATH,
            || {},
            -1,
            None::<&gtk::gio::Cancellable>,
            move |res| {
                let Some(t) = wt.upgrade() else { return };
                match res {
                    Ok(pid) => t.terminal_pid.set(pid.0),
                    Err(e) => {
                        t.terminal_running.set(false);
                        t.exit_code.set(Some(-1));
                        t.terminal().feed(format!("\r\nCouldn't start claude: {e}\r\n").as_bytes());
                    }
                }
                if let Some(w) = ws.upgrade() {
                    w.emit(WsEvent::TabState);
                }
            },
        );
        self.emit(WsEvent::TabState);
    }
}
