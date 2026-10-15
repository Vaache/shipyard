use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, Stdio};
use std::rc::Rc;
use std::time::Instant;

use base64::Engine;
use serde_json::{json, Map, Value};

use super::accounts;
use crate::host;
use crate::signal::Signal;

pub struct ChatText {
    pub id: String,
    pub text: RefCell<String>,
    pub finalized: Cell<bool>,
}

#[derive(Clone)]
pub struct PermissionSuggestion {
    pub raw: Value,
    pub label: String,
}

pub struct PermissionRequest {
    pub id: String,
    pub tool_name: String,
    pub input: Map<String, Value>,
    pub description: Option<String>,
    pub blocked_path: Option<String>,
    pub suggestions: Vec<PermissionSuggestion>,
    pub tool_use_id: Option<String>,
    pub resolved: Cell<bool>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolStatus {
    Running,
    Done,
    Error,
    Denied,
}

pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: RefCell<Map<String, Value>>,
    pub partial_input: RefCell<String>,
    pub status: Cell<ToolStatus>,
    pub result_text: RefCell<Option<String>>,
    pub structured: RefCell<Option<Map<String, Value>>>,
    pub permission: RefCell<Option<Rc<PermissionRequest>>>,
    pub parent: RefCell<Option<String>>,
}

impl ToolCall {
    fn new(id: &str, name: &str) -> Rc<Self> {
        Rc::new(ToolCall {
            id: id.into(),
            name: name.into(),
            input: RefCell::new(Map::new()),
            partial_input: RefCell::new(String::new()),
            status: Cell::new(ToolStatus::Running),
            result_text: RefCell::new(None),
            structured: RefCell::new(None),
            permission: RefCell::new(None),
            parent: RefCell::new(None),
        })
    }

    pub fn string(&self, key: &str) -> Option<String> {
        self.input.borrow().get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }
}

#[derive(Clone)]
pub struct ChatImage {
    pub data: Vec<u8>,
    pub media_type: String,
}

#[derive(Clone)]
pub enum ChatItem {
    User { id: String, text: String, images: Vec<ChatImage> },
    Text(Rc<ChatText>),
    Thinking(Rc<ChatText>),
    Tool(Rc<ToolCall>),
    Notice { id: String, text: String, is_error: bool },
}

impl ChatItem {
    pub fn id(&self) -> String {
        match self {
            ChatItem::User { id, .. } => format!("u{id}"),
            ChatItem::Text(t) => format!("t{}", t.id),
            ChatItem::Thinking(t) => format!("k{}", t.id),
            ChatItem::Tool(c) => format!("c{}", c.id),
            ChatItem::Notice { id, .. } => format!("n{id}"),
        }
    }
}

#[derive(Clone)]
pub struct ModelOption {
    pub value: String,
    pub display_name: String,
    pub description: String,
    pub efforts: Vec<String>,
}

#[derive(Clone)]
pub struct ContextCategory {
    pub name: String,
    pub tokens: i64,
    pub color: String,
    pub kind: String,
}

#[derive(Clone)]
pub struct ContextUsage {
    pub total: i64,
    pub max: i64,
    pub percentage: i64,
    pub auto_compact_threshold: Option<i64>,
    pub auto_compact_enabled: bool,
    pub categories: Vec<ContextCategory>,
}

#[derive(Clone)]
pub struct RateWindow {
    pub key: String,
    pub utilization: f64,
    pub resets_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Default)]
pub struct UsageStore {
    pub windows: Vec<RateWindow>,
    pub subscription: Option<String>,
    pub overage_note: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

thread_local! {
    pub static USAGE: RefCell<UsageStore> = RefCell::new(UsageStore::default());
}

#[derive(Clone)]
pub struct Command {
    pub name: String,
    pub description: String,
}

#[derive(Clone, Debug)]
pub enum Event {
    Appended,
    Updated(String),
    Reset,
    State,
}

enum Incoming {
    Line(String),
    Stderr(String),
    Exit(i32),
}

pub struct ChatSession {
    pub folder: String,
    pub config_dir: Option<String>,
    pub session_id: RefCell<Option<String>>,
    pub items: RefCell<Vec<ChatItem>>,
    pub running: Cell<bool>,
    pub busy: Cell<bool>,
    pub status: RefCell<Option<String>>,
    pub model: RefCell<String>,
    pub model_value: RefCell<String>,
    pub permission_mode: RefCell<String>,
    pub effort: RefCell<String>,
    pub models: RefCell<Vec<ModelOption>>,
    pub commands: RefCell<Vec<Command>>,
    pub pending: RefCell<Vec<Rc<PermissionRequest>>>,
    pub total_cost: Cell<f64>,
    pub context_percent: Cell<Option<i64>>,
    pub context: RefCell<Option<ContextUsage>>,
    pub last_error: RefCell<Option<String>>,
    pub turn_started: Cell<Option<Instant>>,
    pub sleeping: Cell<bool>,
    pub last_activity: Cell<Instant>,
    pub memory_bytes: Cell<i64>,
    pid: Cell<u32>,
    stdin: RefCell<Option<ChildStdin>>,
    stderr_tail: RefCell<String>,
    tools: RefCell<HashMap<String, Rc<ToolCall>>>,
    stream_texts: RefCell<HashMap<String, Rc<ChatText>>>,
    tool_streams: RefCell<HashMap<String, String>>,
    message_texts: RefCell<HashMap<String, Vec<Rc<ChatText>>>>,
    queued_prompt: RefCell<Option<String>>,
    request_counter: Cell<u64>,
    intentional_stop: Cell<bool>,
    generation: Cell<u64>,
    pub on_session_id: RefCell<Option<Rc<dyn Fn(String)>>>,
    pub on_turn_finished: RefCell<Option<Rc<dyn Fn()>>>,
    pub on_attention: RefCell<Option<Rc<dyn Fn()>>>,
    pub changed: Signal<Event>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.to_string())
}

pub fn result_text(content: Option<&Value>) -> Option<String> {
    let c = content?;
    if let Some(t) = c.as_str() {
        return Some(t.to_string());
    }
    let arr = c.as_array()?;
    Some(arr.iter().filter_map(|b| if b.get("type").and_then(|t| t.as_str()) == Some("text") { b.get("text").and_then(|t| t.as_str()) } else { None }).collect::<Vec<_>>().join("\n"))
}

impl ChatSession {
    pub fn new(folder: &str, config_dir: Option<String>) -> Rc<Self> {
        Rc::new(ChatSession {
            folder: folder.to_string(),
            config_dir,
            session_id: RefCell::new(None),
            items: RefCell::new(vec![]),
            running: Cell::new(false),
            busy: Cell::new(false),
            status: RefCell::new(None),
            model: RefCell::new(String::new()),
            model_value: RefCell::new("default".into()),
            permission_mode: RefCell::new("default".into()),
            effort: RefCell::new(String::new()),
            models: RefCell::new(vec![]),
            commands: RefCell::new(vec![]),
            pending: RefCell::new(vec![]),
            total_cost: Cell::new(0.0),
            context_percent: Cell::new(None),
            context: RefCell::new(None),
            last_error: RefCell::new(None),
            turn_started: Cell::new(None),
            sleeping: Cell::new(false),
            last_activity: Cell::new(Instant::now()),
            memory_bytes: Cell::new(0),
            pid: Cell::new(0),
            stdin: RefCell::new(None),
            stderr_tail: RefCell::new(String::new()),
            tools: RefCell::new(HashMap::new()),
            stream_texts: RefCell::new(HashMap::new()),
            tool_streams: RefCell::new(HashMap::new()),
            message_texts: RefCell::new(HashMap::new()),
            queued_prompt: RefCell::new(None),
            request_counter: Cell::new(0),
            intentional_stop: Cell::new(false),
            generation: Cell::new(0),
            on_session_id: RefCell::new(None),
            on_turn_finished: RefCell::new(None),
            on_attention: RefCell::new(None),
            changed: Signal::new(),
        })
    }

    fn emit(&self, e: Event) {
        self.changed.emit(&e);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.borrow().iter().filter(|p| !p.resolved.get()).count()
    }

    pub fn pid(&self) -> i32 {
        if self.running.get() {
            self.pid.get() as i32
        } else {
            0
        }
    }

    pub fn can_sleep(&self) -> bool {
        self.running.get() && !self.busy.get() && self.pending_count() == 0
    }

    pub fn sleep(&self) {
        if !self.can_sleep() {
            return;
        }
        self.intentional_stop.set(true);
        self.kill();
    }

    pub fn stop(&self) {
        self.intentional_stop.set(true);
        self.kill();
    }

    fn kill(&self) {
        let pid = self.pid.get();
        if pid > 0 && self.running.get() {
            let group = -(pid as i32);
            unsafe {
                libc::kill(group, libc::SIGTERM);
            }
            gtk::glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || unsafe {
                if libc::kill(group, 0) == 0 {
                    libc::kill(group, libc::SIGKILL);
                }
            });
        }
    }

    pub fn wake(self: &Rc<Self>) {
        let sid = self.session_id.borrow().clone();
        let Some(sid) = sid.filter(|s| !s.is_empty()) else { return };
        if self.running.get() {
            return;
        }
        let model = self.model_value.borrow().clone();
        let effort = self.effort.borrow().clone();
        let mode = self.permission_mode.borrow().clone();
        self.start(
            Some(sid),
            false,
            if model == "default" || model.is_empty() { None } else { Some(model) },
            if effort.is_empty() { None } else { Some(effort) },
            if mode == "default" { None } else { Some(mode) },
            None,
        );
    }

    pub fn start(self: &Rc<Self>, resume: Option<String>, fork: bool, model: Option<String>, effort: Option<String>, mode: Option<String>, initial_prompt: Option<String>) {
        if self.running.get() {
            return;
        }
        let Some(bin) = host::claude_binary() else {
            *self.last_error.borrow_mut() = Some("claude CLI not found".into());
            self.emit(Event::State);
            return;
        };
        let mut args: Vec<String> = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--permission-prompts", "host", "--permission-prompt-tool", "stdio"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if let Some(m) = model.filter(|m| !m.is_empty()) {
            args.extend(["--model".into(), m.clone()]);
            *self.model_value.borrow_mut() = m;
        }
        if let Some(e) = effort.filter(|e| !e.is_empty()) {
            args.extend(["--effort".into(), e.clone()]);
            *self.effort.borrow_mut() = e;
        }
        if let Some(m) = mode.filter(|m| !m.is_empty() && m != "default") {
            args.extend(["--permission-mode".into(), m.clone()]);
            *self.permission_mode.borrow_mut() = m;
        }
        if let Some(r) = resume {
            args.extend(["--resume".into(), r.clone()]);
            if fork {
                args.push("--fork-session".into());
            } else {
                *self.session_id.borrow_mut() = Some(r);
            }
        }
        let cmd = accounts::command(&bin, self.config_dir.as_deref()).args(args).cwd(&self.folder);
        let mut c = cmd.command();
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = match c.spawn() {
            Ok(ch) => ch,
            Err(e) => {
                *self.last_error.borrow_mut() = Some(e.to_string());
                self.emit(Event::State);
                return;
            }
        };
        let gen = self.generation.get() + 1;
        self.generation.set(gen);
        self.pid.set(child.id());
        *self.stdin.borrow_mut() = child.stdin.take();
        let (tx, rx) = async_channel::unbounded::<Incoming>();
        if let Some(out) = child.stdout.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::with_capacity(1 << 16, out);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                            if !line.is_empty() && tx.send_blocking(Incoming::Line(line)).is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
        if let Some(mut err) = child.stderr.take() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut chunk = [0u8; 4096];
                while let Ok(n) = err.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    let _ = tx.send_blocking(Incoming::Stderr(String::from_utf8_lossy(&chunk[..n]).to_string()));
                }
            });
        }
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.code().unwrap_or(15)).unwrap_or(-1);
            let _ = tx.send_blocking(Incoming::Exit(code));
        });
        self.running.set(true);
        self.sleeping.set(false);
        self.intentional_stop.set(false);
        self.last_activity.set(Instant::now());
        *self.last_error.borrow_mut() = None;
        let weak = Rc::downgrade(self);
        crate::rt::spawn_local(async move {
            while let Ok(msg) = rx.recv().await {
                let Some(me) = weak.upgrade() else { return };
                if me.generation.get() != gen {
                    return;
                }
                match msg {
                    Incoming::Line(l) => {
                        if let Ok(v) = serde_json::from_str::<Value>(&l) {
                            me.handle(&v);
                        }
                    }
                    Incoming::Stderr(s) => {
                        let mut t = me.stderr_tail.borrow_mut();
                        t.push_str(&s);
                        let len = t.chars().count();
                        if len > 2000 {
                            *t = t.chars().skip(len - 2000).collect();
                        }
                    }
                    Incoming::Exit(code) => {
                        me.terminated(code);
                        return;
                    }
                }
            }
        });
        self.control("initialize", Map::new());
        self.emit(Event::State);
        if let Some(p) = initial_prompt.filter(|p| !p.is_empty()) {
            self.send(&p, vec![]);
        }
    }

    fn terminated(&self, code: i32) {
        let intentional = self.intentional_stop.replace(false);
        self.running.set(false);
        self.busy.set(false);
        *self.stdin.borrow_mut() = None;
        self.memory_bytes.set(0);
        if intentional {
            self.sleeping.set(self.session_id.borrow().is_some());
            self.pending.borrow_mut().clear();
            self.emit(Event::State);
            return;
        }
        for p in self.pending.borrow().iter() {
            p.resolved.set(true);
        }
        self.pending.borrow_mut().clear();
        if code != 0 && code != 15 && code != 143 {
            let tail = self.stderr_tail.borrow().trim().to_string();
            let short: String = tail.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
            *self.last_error.borrow_mut() = Some(format!("Claude exited with code {code}{}", if short.is_empty() { String::new() } else { format!("\n{short}") }));
        }
        self.emit(Event::State);
    }

    fn write(&self, obj: Value) {
        let Ok(mut line) = serde_json::to_vec(&obj) else { return };
        line.push(b'\n');
        if let Some(stdin) = self.stdin.borrow_mut().as_mut() {
            let _ = stdin.write_all(&line);
            let _ = stdin.flush();
        }
    }

    fn control(&self, subtype: &str, mut extra: Map<String, Value>) {
        let n = self.request_counter.get() + 1;
        self.request_counter.set(n);
        extra.insert("subtype".into(), Value::String(subtype.into()));
        self.write(json!({ "type": "control_request", "request_id": format!("gl-{n}-{subtype}"), "request": Value::Object(extra) }));
    }

    pub fn send(&self, text: &str, images: Vec<ChatImage>) {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() && images.is_empty() {
            return;
        }
        if !self.running.get() {
            *self.queued_prompt.borrow_mut() = Some(text.to_string());
            return;
        }
        self.last_activity.set(Instant::now());
        self.items.borrow_mut().push(ChatItem::User { id: uuid::Uuid::new_v4().to_string(), text: trimmed.clone(), images: images.clone() });
        let mut content: Vec<Value> = images
            .iter()
            .map(|i| json!({ "type": "image", "source": { "type": "base64", "media_type": i.media_type, "data": base64::engine::general_purpose::STANDARD.encode(&i.data) } }))
            .collect();
        if !trimmed.is_empty() {
            content.push(json!({ "type": "text", "text": trimmed }));
        }
        self.write(json!({ "type": "user", "message": { "role": "user", "content": content } }));
        self.busy.set(true);
        *self.status.borrow_mut() = None;
        self.turn_started.set(Some(Instant::now()));
        self.emit(Event::Appended);
        self.emit(Event::State);
    }

    pub fn interrupt(&self) {
        self.control("interrupt", Map::new());
    }

    pub fn set_model(&self, value: &str) {
        *self.model_value.borrow_mut() = value.to_string();
        let mut m = Map::new();
        m.insert("model".into(), Value::String(value.into()));
        self.control("set_model", m);
        self.emit(Event::State);
    }

    pub fn set_permission_mode(&self, mode: &str) {
        *self.permission_mode.borrow_mut() = mode.to_string();
        let mut m = Map::new();
        m.insert("mode".into(), Value::String(mode.into()));
        self.control("set_permission_mode", m);
        self.emit(Event::State);
    }

    pub fn set_effort(&self, level: &str) {
        *self.effort.borrow_mut() = level.to_string();
        let mut m = Map::new();
        m.insert("settings".into(), json!({ "effortLevel": if level.is_empty() { Value::Null } else { Value::String(level.into()) } }));
        self.control("apply_flag_settings", m);
        self.emit(Event::State);
    }

    pub fn respond(&self, req: &Rc<PermissionRequest>, allow: bool, always: Option<PermissionSuggestion>, message: Option<String>, updated_input: Option<Map<String, Value>>) {
        if req.resolved.get() {
            return;
        }
        req.resolved.set(true);
        self.last_activity.set(Instant::now());
        let response = if allow {
            let mut r = json!({ "behavior": "allow", "updatedInput": Value::Object(updated_input.unwrap_or_else(|| req.input.clone())) });
            if let Some(a) = always {
                r["updatedPermissions"] = json!([a.raw]);
            }
            r
        } else {
            json!({ "behavior": "deny", "message": message.filter(|m| !m.is_empty()).unwrap_or_else(|| "The user declined this action".into()) })
        };
        self.write(json!({ "type": "control_response", "response": { "subtype": "success", "request_id": req.id, "response": response } }));
        self.pending.borrow_mut().retain(|p| !p.resolved.get());
        if !allow {
            if let Some(tid) = &req.tool_use_id {
                if let Some(t) = self.tools.borrow().get(tid) {
                    t.status.set(ToolStatus::Denied);
                }
            }
        }
        if let Some(tid) = &req.tool_use_id {
            self.emit(Event::Updated(format!("c{tid}")));
        }
        self.emit(Event::State);
    }

    pub fn feed(&self, line: &str) {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            self.handle(&v);
        }
    }

    fn handle(&self, o: &Value) {
        self.last_activity.set(Instant::now());
        match o.get("type").and_then(|t| t.as_str()) {
            Some("system") => self.handle_system(o),
            Some("stream_event") => self.handle_stream(o),
            Some("assistant") => self.handle_assistant(o),
            Some("user") => self.handle_user(o),
            Some("result") => self.handle_result(o),
            Some("control_request") => self.handle_control_request(o),
            Some("rate_limit_event") => self.handle_rate_limit(o),
            Some("control_response") => self.handle_control_response(o),
            _ => {}
        }
    }

    fn handle_system(&self, o: &Value) {
        match o.get("subtype").and_then(|t| t.as_str()) {
            Some("init") => {
                if let Some(sid) = s(o, "session_id") {
                    if self.session_id.borrow().as_deref() != Some(sid.as_str()) {
                        *self.session_id.borrow_mut() = Some(sid.clone());
                        let cb = self.on_session_id.borrow().clone();
                        if let Some(cb) = cb {
                            cb(sid);
                        }
                    }
                }
                if let Some(m) = s(o, "model") {
                    *self.model.borrow_mut() = m;
                }
                if let Some(pm) = s(o, "permissionMode") {
                    *self.permission_mode.borrow_mut() = pm;
                }
                let q = self.queued_prompt.borrow_mut().take();
                if let Some(q) = q {
                    self.send(&q, vec![]);
                }
                self.emit(Event::State);
            }
            Some("status") => {
                let st = s(o, "status");
                *self.status.borrow_mut() = match st.as_deref() {
                    Some("compacting") => Some("Compacting context…".into()),
                    Some("requesting") | None => None,
                    Some(x) => Some(x.to_string()),
                };
                self.emit(Event::State);
            }
            Some("compact_boundary") => {
                self.items.borrow_mut().push(ChatItem::Notice { id: uuid::Uuid::new_v4().to_string(), text: "Context compacted".into(), is_error: false });
                self.emit(Event::Appended);
            }
            Some("permission_denied") => {
                if let Some(id) = s(o, "tool_use_id") {
                    if let Some(t) = self.tools.borrow().get(&id) {
                        t.status.set(ToolStatus::Denied);
                        *t.result_text.borrow_mut() = s(o, "message");
                    }
                    self.emit(Event::Updated(format!("c{id}")));
                }
            }
            _ => {}
        }
    }

    fn stream_key(o: &Value, index: i64) -> String {
        format!("{}:{index}", s(o, "api_message_id").unwrap_or_else(|| "m".into()))
    }

    fn handle_stream(&self, o: &Value) {
        let Some(ev) = o.get("event") else { return };
        let Some(kind) = ev.get("type").and_then(|t| t.as_str()) else { return };
        let parent = s(o, "parent_tool_use_id");
        match kind {
            "content_block_start" => {
                let (Some(index), Some(block)) = (ev.get("index").and_then(|i| i.as_i64()), ev.get("content_block")) else { return };
                let key = Self::stream_key(o, index);
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if parent.is_some() {
                            return;
                        }
                        let t = Rc::new(ChatText { id: key.clone(), text: RefCell::new(String::new()), finalized: Cell::new(false) });
                        self.stream_texts.borrow_mut().insert(key, t.clone());
                        self.message_texts.borrow_mut().entry(s(o, "api_message_id").unwrap_or_else(|| "m".into())).or_default().push(t.clone());
                        self.items.borrow_mut().push(ChatItem::Text(t));
                        self.emit(Event::Appended);
                    }
                    Some("thinking") => {
                        if parent.is_some() {
                            return;
                        }
                        let t = Rc::new(ChatText { id: key.clone(), text: RefCell::new(String::new()), finalized: Cell::new(false) });
                        self.stream_texts.borrow_mut().insert(key, t.clone());
                        self.items.borrow_mut().push(ChatItem::Thinking(t));
                        self.emit(Event::Appended);
                    }
                    Some("tool_use") => {
                        let (Some(id), Some(name)) = (s(block, "id"), s(block, "name")) else { return };
                        if self.tools.borrow().contains_key(&id) {
                            return;
                        }
                        let call = ToolCall::new(&id, &name);
                        *call.parent.borrow_mut() = parent.clone();
                        self.tools.borrow_mut().insert(id.clone(), call.clone());
                        self.tool_streams.borrow_mut().insert(key, id);
                        if parent.is_none() {
                            self.items.borrow_mut().push(ChatItem::Tool(call));
                            self.emit(Event::Appended);
                        }
                    }
                    _ => {}
                }
            }
            "content_block_delta" => {
                let (Some(index), Some(delta)) = (ev.get("index").and_then(|i| i.as_i64()), ev.get("delta")) else { return };
                let key = Self::stream_key(o, index);
                match delta.get("type").and_then(|t| t.as_str()) {
                    Some("text_delta") | Some("thinking_delta") => {
                        let piece = s(delta, "text").or_else(|| s(delta, "thinking")).unwrap_or_default();
                        let t = self.stream_texts.borrow().get(&key).cloned();
                        if let Some(t) = t {
                            t.text.borrow_mut().push_str(&piece);
                            self.emit(Event::Updated(key));
                        }
                    }
                    Some("input_json_delta") => {
                        let id = self.tool_streams.borrow().get(&key).cloned();
                        if let Some(id) = id {
                            if let Some(t) = self.tools.borrow().get(&id) {
                                t.partial_input.borrow_mut().push_str(&s(delta, "partial_json").unwrap_or_default());
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_assistant(&self, o: &Value) {
        let Some(msg) = o.get("message") else { return };
        let Some(content) = msg.get("content").and_then(|c| c.as_array()) else { return };
        let parent = s(o, "parent_tool_use_id");
        let mid = s(msg, "id").unwrap_or_else(|| "m".into());
        if parent.is_none() {
            if let Some(m) = s(msg, "model") {
                *self.model.borrow_mut() = m;
            }
        }
        for block in content {
            match block.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if parent.is_some() {
                        continue;
                    }
                    let full = s(block, "text").unwrap_or_default();
                    let existing = self.message_texts.borrow().get(&mid).and_then(|v| v.iter().find(|t| !t.finalized.get()).cloned());
                    match existing {
                        Some(t) => {
                            *t.text.borrow_mut() = full;
                            t.finalized.set(true);
                            self.emit(Event::Updated(t.id.clone()));
                        }
                        None => {
                            if !full.is_empty() {
                                let t = Rc::new(ChatText { id: format!("{mid}:{}", uuid::Uuid::new_v4()), text: RefCell::new(full), finalized: Cell::new(true) });
                                self.message_texts.borrow_mut().entry(mid.clone()).or_default().push(t.clone());
                                self.items.borrow_mut().push(ChatItem::Text(t));
                                self.emit(Event::Appended);
                            }
                        }
                    }
                }
                Some("tool_use") => {
                    let (Some(id), Some(name)) = (s(block, "id"), s(block, "name")) else { continue };
                    let existing = self.tools.borrow().get(&id).cloned();
                    let call = match existing {
                        Some(c) => c,
                        None => {
                            let c = ToolCall::new(&id, &name);
                            *c.parent.borrow_mut() = parent.clone();
                            self.tools.borrow_mut().insert(id.clone(), c.clone());
                            if parent.is_none() {
                                self.items.borrow_mut().push(ChatItem::Tool(c.clone()));
                                self.emit(Event::Appended);
                            }
                            c
                        }
                    };
                    *call.input.borrow_mut() = block.get("input").and_then(|i| i.as_object()).cloned().unwrap_or_default();
                    call.partial_input.borrow_mut().clear();
                    self.emit(Event::Updated(format!("c{id}")));
                }
                _ => {}
            }
        }
    }

    fn handle_user(&self, o: &Value) {
        let Some(content) = o.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) else { return };
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                continue;
            }
            let Some(id) = s(block, "tool_use_id") else { continue };
            let tool = self.tools.borrow().get(&id).cloned();
            let Some(tool) = tool else { continue };
            if tool.status.get() != ToolStatus::Denied {
                tool.status.set(if block.get("is_error").and_then(|v| v.as_bool()) == Some(true) { ToolStatus::Error } else { ToolStatus::Done });
            }
            *tool.result_text.borrow_mut() = result_text(block.get("content"));
            *tool.structured.borrow_mut() = o.get("tool_use_result").and_then(|v| v.as_object()).cloned();
            self.emit(Event::Updated(format!("c{id}")));
        }
    }

    fn handle_result(&self, o: &Value) {
        self.busy.set(false);
        *self.status.borrow_mut() = None;
        if let Some(c) = o.get("total_cost_usd").and_then(|v| v.as_f64()) {
            self.total_cost.set(self.total_cost.get() + c);
        }
        let running: Vec<Rc<ToolCall>> = self.tools.borrow().values().filter(|t| t.status.get() == ToolStatus::Running && t.parent.borrow().is_none()).cloned().collect();
        for t in running {
            t.status.set(ToolStatus::Done);
            self.emit(Event::Updated(format!("c{}", t.id)));
        }
        if o.get("is_error").and_then(|v| v.as_bool()) == Some(true) {
            let sub = s(o, "subtype").unwrap_or_default();
            let text = if sub == "error_during_execution" { "Stopped".to_string() } else { s(o, "result").unwrap_or(sub.clone()) };
            self.items.borrow_mut().push(ChatItem::Notice { id: uuid::Uuid::new_v4().to_string(), text, is_error: sub != "error_during_execution" });
            self.emit(Event::Appended);
        }
        self.control("get_context_usage", Map::new());
        self.emit(Event::State);
        let cb = self.on_turn_finished.borrow().clone();
        if let Some(cb) = cb {
            cb();
        }
    }

    fn handle_control_request(&self, o: &Value) {
        let (Some(id), Some(req)) = (s(o, "request_id"), o.get("request")) else { return };
        if req.get("subtype").and_then(|t| t.as_str()) != Some("can_use_tool") {
            self.write(json!({ "type": "control_response", "response": { "subtype": "error", "request_id": id, "error": "Unsupported" } }));
            return;
        }
        let suggestions: Vec<PermissionSuggestion> = req
            .get("permission_suggestions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|sug| match sug.get("type").and_then(|t| t.as_str()) {
                Some("addRules") => {
                    let rules: Vec<String> = sug
                        .get("rules")
                        .and_then(|r| r.as_array())
                        .cloned()
                        .unwrap_or_default()
                        .iter()
                        .map(|r| {
                            let tool = s(r, "toolName").unwrap_or_default();
                            match s(r, "ruleContent") {
                                Some(c) => format!("{tool}({c})"),
                                None => tool,
                            }
                        })
                        .collect();
                    Some(PermissionSuggestion { raw: sug.clone(), label: rules.join(", ") })
                }
                Some("addDirectories") => {
                    let dirs: Vec<String> = sug.get("directories").and_then(|d| d.as_array()).cloned().unwrap_or_default().iter().filter_map(|d| d.as_str().map(crate::util::last_component)).collect();
                    Some(PermissionSuggestion { raw: sug.clone(), label: format!("access to {}", dirs.join(", ")) })
                }
                Some("setMode") => Some(PermissionSuggestion { raw: sug.clone(), label: format!("mode {}", s(&sug, "mode").unwrap_or_default()) }),
                _ => None,
            })
            .collect();
        let p = Rc::new(PermissionRequest {
            id: id.clone(),
            tool_name: s(req, "tool_name").unwrap_or_else(|| "?".into()),
            input: req.get("input").and_then(|i| i.as_object()).cloned().unwrap_or_default(),
            description: s(req, "description"),
            blocked_path: s(req, "blocked_path"),
            suggestions,
            tool_use_id: s(req, "tool_use_id"),
            resolved: Cell::new(false),
        });
        let existing = p.tool_use_id.as_ref().and_then(|t| self.tools.borrow().get(t).cloned());
        match existing {
            Some(tool) => {
                *tool.permission.borrow_mut() = Some(p.clone());
                if tool.input.borrow().is_empty() {
                    *tool.input.borrow_mut() = p.input.clone();
                }
                self.emit(Event::Updated(format!("c{}", tool.id)));
            }
            None => {
                let cid = p.tool_use_id.clone().unwrap_or_else(|| id.clone());
                let call = ToolCall::new(&cid, &p.tool_name);
                *call.input.borrow_mut() = p.input.clone();
                *call.permission.borrow_mut() = Some(p.clone());
                self.tools.borrow_mut().insert(cid, call.clone());
                self.items.borrow_mut().push(ChatItem::Tool(call));
                self.emit(Event::Appended);
            }
        }
        self.pending.borrow_mut().push(p);
        self.emit(Event::State);
        let cb = self.on_attention.borrow().clone();
        if let Some(cb) = cb {
            cb();
        }
    }

    fn handle_rate_limit(&self, o: &Value) {
        let Some(info) = o.get("rate_limit_info") else { return };
        USAGE.with(|u| {
            let mut u = u.borrow_mut();
            if let Some(windows) = info.get("unifiedWindows").and_then(|w| w.as_object()) {
                let order = |k: &str| match k {
                    "five_hour" => 0,
                    "seven_day" => 1,
                    _ => 9,
                };
                let mut list: Vec<RateWindow> = windows
                    .iter()
                    .map(|(k, w)| RateWindow {
                        key: k.clone(),
                        utilization: w.get("utilization").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        resets_at: w.get("resetsAt").and_then(|v| v.as_f64()).and_then(|t| chrono::DateTime::from_timestamp(t as i64, 0)),
                    })
                    .collect();
                list.sort_by(|a, b| (order(&a.key), a.key.clone()).cmp(&(order(&b.key), b.key.clone())));
                u.windows = list;
            }
            u.overage_note = match info.get("status").and_then(|v| v.as_str()) {
                Some("rejected") => Some("Limit reached".into()),
                Some("allowed_warning") => Some("Close to the limit".into()),
                _ => None,
            };
            u.updated_at = Some(chrono::Utc::now());
        });
        self.emit(Event::State);
    }

    pub fn refresh_context(&self) {
        if self.running.get() {
            self.control("get_context_usage", Map::new());
        }
    }

    pub fn compact(&self) {
        self.send("/compact", vec![]);
    }

    fn handle_control_response(&self, o: &Value) {
        let Some(r) = o.get("response") else { return };
        let (Some(rid), Some(body)) = (s(r, "request_id"), r.get("response")) else { return };
        if rid.ends_with("-initialize") {
            *self.models.borrow_mut() = body
                .get("models")
                .and_then(|m| m.as_array())
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|m| {
                    let v = s(m, "value")?;
                    Some(ModelOption {
                        display_name: s(m, "displayName").unwrap_or_else(|| v.clone()),
                        description: s(m, "description").unwrap_or_default(),
                        efforts: m.get("supportedEffortLevels").and_then(|e| e.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default(),
                        value: v,
                    })
                })
                .collect();
            if let Some(sub) = body.get("account").and_then(|a| a.get("subscriptionType")).and_then(|v| v.as_str()) {
                USAGE.with(|u| u.borrow_mut().subscription = Some(sub.replace("Claude ", "")));
            }
            *self.commands.borrow_mut() = body
                .get("commands")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|c| Some(Command { name: s(c, "name")?, description: s(c, "description").unwrap_or_default() }))
                .collect();
            self.emit(Event::State);
        } else if rid.ends_with("-get_context_usage") {
            let num = |k: &str| body.get(k).and_then(|v| v.as_f64()).map(|f| f as i64);
            let total = num("totalTokens").unwrap_or(0);
            let max = num("maxTokens").unwrap_or(0);
            let pct = num("percentage").unwrap_or(if max > 0 { total * 100 / max } else { 0 });
            self.context_percent.set(Some(pct));
            let cats = body
                .get("categories")
                .and_then(|c| c.as_array())
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|c| {
                    Some(ContextCategory {
                        name: s(c, "name")?,
                        tokens: c.get("tokens").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64,
                        color: s(c, "color").unwrap_or_default(),
                        kind: s(c, "kind").unwrap_or_else(|| "used".into()),
                    })
                })
                .collect();
            *self.context.borrow_mut() = Some(ContextUsage {
                total,
                max,
                percentage: pct,
                auto_compact_threshold: num("autoCompactThreshold"),
                auto_compact_enabled: body.get("isAutoCompactEnabled").and_then(|v| v.as_bool()).unwrap_or(true),
                categories: cats,
            });
            self.emit(Event::State);
        }
    }

    pub fn load_history(&self, file: &std::path::Path, limit: usize) {
        let Ok(data) = std::fs::read(file) else { return };
        let mut loaded: Vec<ChatItem> = vec![];
        let mut map: HashMap<String, Rc<ToolCall>> = HashMap::new();
        for line in data.split(|b| *b == b'\n') {
            let Ok(o) = serde_json::from_slice::<Value>(line) else { continue };
            if o.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) || o.get("isMeta").and_then(|v| v.as_bool()) == Some(true) {
                continue;
            }
            let Some(msg) = o.get("message") else { continue };
            let uuid = s(&o, "uuid").unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            match o.get("type").and_then(|t| t.as_str()) {
                Some("user") => {
                    if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                        if o.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true) {
                            continue;
                        }
                        let clean = text.trim();
                        if clean.starts_with("<command-name>") || clean.starts_with("<local-command") || clean.starts_with("Caveat:") || clean.is_empty() {
                            continue;
                        }
                        loaded.push(ChatItem::User { id: uuid, text: clean.to_string(), images: vec![] });
                    } else if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                        let mut texts = vec![];
                        for b in blocks {
                            match b.get("type").and_then(|t| t.as_str()) {
                                Some("tool_result") => {
                                    if let Some(tool) = s(b, "tool_use_id").and_then(|id| map.get(&id).cloned()) {
                                        tool.status.set(if b.get("is_error").and_then(|v| v.as_bool()) == Some(true) { ToolStatus::Error } else { ToolStatus::Done });
                                        *tool.result_text.borrow_mut() = result_text(b.get("content"));
                                        *tool.structured.borrow_mut() = o.get("toolUseResult").and_then(|v| v.as_object()).cloned();
                                    }
                                }
                                Some("text") => {
                                    if let Some(t) = s(b, "text").filter(|t| !t.starts_with('<')) {
                                        texts.push(t);
                                    }
                                }
                                _ => {}
                            }
                        }
                        if !texts.is_empty() {
                            loaded.push(ChatItem::User { id: uuid, text: texts.join("\n"), images: vec![] });
                        }
                    }
                }
                Some("assistant") => {
                    if let Some(blocks) = msg.get("content").and_then(|c| c.as_array()) {
                        for (i, b) in blocks.iter().enumerate() {
                            match b.get("type").and_then(|t| t.as_str()) {
                                Some("text") => {
                                    if let Some(t) = s(b, "text").filter(|t| !t.is_empty()) {
                                        loaded.push(ChatItem::Text(Rc::new(ChatText { id: format!("h{uuid}:{i}"), text: RefCell::new(t), finalized: Cell::new(true) })));
                                    }
                                }
                                Some("tool_use") => {
                                    let (Some(id), Some(name)) = (s(b, "id"), s(b, "name")) else { continue };
                                    let c = ToolCall::new(&id, &name);
                                    *c.input.borrow_mut() = b.get("input").and_then(|i| i.as_object()).cloned().unwrap_or_default();
                                    c.status.set(ToolStatus::Done);
                                    map.insert(id, c.clone());
                                    loaded.push(ChatItem::Tool(c));
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let start = loaded.len().saturating_sub(limit);
        let tail: Vec<ChatItem> = loaded[start..].to_vec();
        for item in &tail {
            if let ChatItem::Tool(c) = item {
                self.tools.borrow_mut().insert(c.id.clone(), c.clone());
            }
        }
        let mut items = vec![];
        if loaded.len() > limit {
            items.push(ChatItem::Notice { id: "older".into(), text: format!("Showing the last {limit} messages"), is_error: false });
        }
        items.extend(tail);
        if !items.is_empty() {
            items.push(ChatItem::Notice { id: "resumed".into(), text: "Session resumed".into(), is_error: false });
        }
        *self.items.borrow_mut() = items;
        self.emit(Event::Reset);
    }
}
