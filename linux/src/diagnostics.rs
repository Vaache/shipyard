use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, SecondsFormat, Utc};
use reqwest::header::HeaderMap;
use serde::{Serialize, Serializer};

pub const REQUEST_CAPACITY: usize = 1000;
pub const TICK_CAPACITY: usize = 200;
pub const SLOW_UI_CAPACITY: usize = 200;
pub const SLOW_UI_MS: f64 = 16.0;
const TICK_SETTLE: Duration = Duration::from_millis(300);
const TICK_MAX: Duration = Duration::from_secs(30);

pub const ORIGIN_LIVE: &str = "live-tick";
pub const ORIGIN_USER: &str = "user";

static ENABLED: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(Store::default()));

fn ts_millis<S: Serializer>(ts: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&ts.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn round_ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 10_000.0).round() / 10.0
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RequestEntry {
    #[serde(serialize_with = "ts_millis")]
    pub ts: DateTime<Utc>,
    pub kind: &'static str,
    pub name: String,
    pub origin: &'static str,
    pub duration_ms: f64,
    pub bytes: u64,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub rate_limit_remaining: Option<i64>,
    pub rate_limit_reset: Option<i64>,
    pub rate_limit_resource: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TickEntry {
    #[serde(serialize_with = "ts_millis")]
    pub ts: DateTime<Utc>,
    pub duration_ms: f64,
    pub requests: usize,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SlowUiEntry {
    #[serde(serialize_with = "ts_millis")]
    pub ts: DateTime<Utc>,
    pub name: &'static str,
    pub duration_ms: f64,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum Line<'a> {
    #[serde(rename = "request")]
    Request(&'a RequestEntry),
    #[serde(rename = "tick")]
    Tick(&'a TickEntry),
    #[serde(rename = "slow-ui")]
    SlowUi(&'a SlowUiEntry),
}

struct OpenTick {
    id: u64,
    ts: DateTime<Utc>,
    started: Instant,
    in_flight: usize,
    requests: usize,
    last_end: Option<Instant>,
}

#[derive(Default)]
struct Store {
    requests: VecDeque<RequestEntry>,
    ticks: VecDeque<TickEntry>,
    slow_ui: VecDeque<SlowUiEntry>,
    tick: Option<OpenTick>,
    next_tick: u64,
}

fn push_capped<T>(q: &mut VecDeque<T>, item: T, cap: usize) {
    while q.len() >= cap {
        q.pop_front();
    }
    q.push_back(item);
}

impl Store {
    fn attach(&mut self) -> (&'static str, u64) {
        match self.tick.as_mut() {
            Some(t) => {
                t.in_flight += 1;
                t.requests += 1;
                (ORIGIN_LIVE, t.id)
            }
            None => (ORIGIN_USER, 0),
        }
    }

    fn detach(&mut self, tick: u64, now: Instant) {
        if let Some(t) = self.tick.as_mut().filter(|t| t.id == tick) {
            t.in_flight = t.in_flight.saturating_sub(1);
            t.last_end = Some(now);
        }
    }

    fn close_tick(&mut self) {
        if let Some(t) = self.tick.take() {
            let duration = t.last_end.map(|e| e.saturating_duration_since(t.started)).unwrap_or_default();
            push_capped(&mut self.ticks, TickEntry { ts: t.ts, duration_ms: round_ms(duration), requests: t.requests }, TICK_CAPACITY);
            bump();
        }
    }

    fn settle_tick(&mut self, id: u64, now: Instant) -> bool {
        let Some(t) = self.tick.as_ref().filter(|t| t.id == id) else { return true };
        let quiet = t.in_flight == 0 && now.saturating_duration_since(t.last_end.unwrap_or(t.started)) >= TICK_SETTLE;
        if quiet || now.saturating_duration_since(t.started) >= TICK_MAX {
            self.close_tick();
            return true;
        }
        false
    }
}

fn store() -> MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

fn bump() {
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if !on {
        store().close_tick();
    }
    bump();
}

pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

pub fn clear() {
    let mut s = store();
    s.requests.clear();
    s.ticks.clear();
    s.slow_ui.clear();
    drop(s);
    bump();
}

pub struct Snapshot {
    pub requests: Vec<RequestEntry>,
    pub ticks: Vec<TickEntry>,
    pub slow_ui: Vec<SlowUiEntry>,
}

pub fn snapshot() -> Snapshot {
    let s = store();
    Snapshot { requests: s.requests.iter().cloned().collect(), ticks: s.ticks.iter().cloned().collect(), slow_ui: s.slow_ui.iter().cloned().collect() }
}

pub fn export_lines(snap: &Snapshot) -> String {
    let mut lines: Vec<(DateTime<Utc>, String)> = Vec::with_capacity(snap.requests.len() + snap.ticks.len() + snap.slow_ui.len());
    let mut add = |ts: DateTime<Utc>, line: Line| {
        if let Ok(s) = serde_json::to_string(&line) {
            lines.push((ts, s));
        }
    };
    for r in &snap.requests {
        add(r.ts, Line::Request(r));
    }
    for t in &snap.ticks {
        add(t.ts, Line::Tick(t));
    }
    for u in &snap.slow_ui {
        add(u.ts, Line::SlowUi(u));
    }
    lines.sort_by_key(|(ts, _)| *ts);
    let mut out = String::new();
    for (_, l) in lines {
        out.push_str(&l);
        out.push('\n');
    }
    out
}

struct Pending {
    ts: DateTime<Utc>,
    started: Instant,
    kind: &'static str,
    name: String,
    origin: &'static str,
    tick: u64,
    status: Option<u16>,
    bytes: u64,
    error: Option<String>,
    rate_limit_remaining: Option<i64>,
    rate_limit_reset: Option<i64>,
    rate_limit_resource: Option<String>,
}

pub struct Probe(Option<Pending>);

pub fn begin(kind: &'static str, name: impl FnOnce() -> String) -> Probe {
    if !enabled() {
        return Probe(None);
    }
    let (origin, tick) = store().attach();
    Probe(Some(Pending {
        ts: Utc::now(),
        started: Instant::now(),
        kind,
        name: name(),
        origin,
        tick,
        status: None,
        bytes: 0,
        error: None,
        rate_limit_remaining: None,
        rate_limit_reset: None,
        rate_limit_resource: None,
    }))
}

fn header<'a>(headers: &'a HeaderMap, key: &str) -> Option<&'a str> {
    headers.get(key).and_then(|v| v.to_str().ok()).map(str::trim)
}

impl Probe {
    pub fn response(&mut self, status: u16, headers: &HeaderMap) {
        if let Some(p) = self.0.as_mut() {
            p.status = Some(status);
            p.rate_limit_remaining = header(headers, "x-ratelimit-remaining").and_then(|v| v.parse().ok());
            p.rate_limit_reset = header(headers, "x-ratelimit-reset").and_then(|v| v.parse().ok());
            p.rate_limit_resource = header(headers, "x-ratelimit-resource").map(str::to_string);
        }
    }

    pub fn body(&mut self, bytes: usize) {
        if let Some(p) = self.0.as_mut() {
            p.bytes = bytes as u64;
        }
    }

    pub fn fail(&mut self, error: impl std::fmt::Display) {
        if let Some(p) = self.0.as_mut() {
            if p.error.is_none() {
                p.error = Some(error.to_string());
            }
        }
    }

    pub fn track<T, E: std::fmt::Display>(&mut self, result: Result<T, E>) -> Result<T, E> {
        if let Err(e) = &result {
            self.fail(e);
        }
        result
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        let Some(p) = self.0.take() else { return };
        let now = Instant::now();
        let entry = RequestEntry {
            ts: p.ts,
            kind: p.kind,
            name: p.name,
            origin: p.origin,
            duration_ms: round_ms(now.saturating_duration_since(p.started)),
            bytes: p.bytes,
            status: p.status,
            error: p.error,
            rate_limit_remaining: p.rate_limit_remaining,
            rate_limit_reset: p.rate_limit_reset,
            rate_limit_resource: p.rate_limit_resource,
        };
        let mut s = store();
        s.detach(p.tick, now);
        push_capped(&mut s.requests, entry, REQUEST_CAPACITY);
        drop(s);
        bump();
    }
}

pub fn tick_begin() {
    if !enabled() {
        return;
    }
    let id = {
        let mut s = store();
        s.close_tick();
        s.next_tick += 1;
        let id = s.next_tick;
        s.tick = Some(OpenTick { id, ts: Utc::now(), started: Instant::now(), in_flight: 0, requests: 0, last_end: None });
        id
    };
    gtk::glib::timeout_add_local(Duration::from_millis(100), move || {
        if store().settle_tick(id, Instant::now()) {
            gtk::glib::ControlFlow::Break
        } else {
            gtk::glib::ControlFlow::Continue
        }
    });
}

pub struct UiSpan(Option<(&'static str, Instant)>);

pub fn ui_span(name: &'static str) -> UiSpan {
    if !enabled() {
        return UiSpan(None);
    }
    UiSpan(Some((name, Instant::now())))
}

impl Drop for UiSpan {
    fn drop(&mut self) {
        let Some((name, started)) = self.0.take() else { return };
        let ms = round_ms(started.elapsed());
        if ms <= SLOW_UI_MS {
            return;
        }
        push_capped(&mut store().slow_ui, SlowUiEntry { ts: Utc::now(), name, duration_ms: ms }, SLOW_UI_CAPACITY);
        bump();
    }
}

fn identifier(s: &str) -> (&str, &str) {
    let end = s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(s.len());
    (&s[..end], &s[end..])
}

pub fn graphql_name(query: &str) -> String {
    let q = query.trim_start();
    for keyword in ["query", "mutation", "subscription"] {
        if let Some(rest) = q.strip_prefix(keyword) {
            let (name, _) = identifier(rest.trim_start());
            if rest.starts_with(|c: char| c.is_whitespace()) && !name.is_empty() {
                return name.to_string();
            }
        }
    }
    let Some(open) = q.find('{') else { return "graphql".into() };
    let (first, rest) = identifier(q[open + 1..].trim_start());
    let field = match rest.trim_start().strip_prefix(':') {
        Some(after) => identifier(after.trim_start()).0,
        None => first,
    };
    if field.is_empty() {
        "graphql".into()
    } else {
        field.to_string()
    }
}

pub fn rest_name(method: &str, path: &str) -> String {
    let without_host = match path.find("://") {
        Some(i) => {
            let after = &path[i + 3..];
            after.find('/').map(|j| &after[j..]).unwrap_or("/")
        }
        None => path,
    };
    let clean = without_host.split(['?', '#']).next().unwrap_or("");
    format!("{method} {clean}")
}

pub fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub count: usize,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub bytes: u64,
    pub errors: usize,
    pub live: usize,
}

pub fn stats<'a>(entries: impl IntoIterator<Item = &'a RequestEntry>) -> Stats {
    let mut durations = vec![];
    let mut bytes = 0;
    let mut errors = 0;
    let mut live = 0;
    for e in entries {
        durations.push(e.duration_ms);
        bytes += e.bytes;
        if e.error.is_some() {
            errors += 1;
        }
        if e.origin == ORIGIN_LIVE {
            live += 1;
        }
    }
    durations.sort_by(|a, b| a.total_cmp(b));
    let count = durations.len();
    let avg_ms = if count == 0 { 0.0 } else { durations.iter().sum::<f64>() / count as f64 };
    Stats { count, avg_ms, p95_ms: percentile(&durations, 95.0), bytes, errors, live }
}

pub fn grouped<'a>(entries: impl IntoIterator<Item = &'a RequestEntry>) -> Vec<(String, Stats)> {
    let mut by_name: HashMap<&str, Vec<&RequestEntry>> = HashMap::new();
    for e in entries {
        by_name.entry(e.name.as_str()).or_default().push(e);
    }
    let mut out: Vec<(String, Stats)> = by_name.into_iter().map(|(name, list)| (name.to_string(), stats(list))).collect();
    out.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(b.1.bytes.cmp(&a.1.bytes)).then(a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, ms: f64, bytes: u64, origin: &'static str) -> RequestEntry {
        RequestEntry {
            ts: Utc::now(),
            kind: "rest",
            name: name.into(),
            origin,
            duration_ms: ms,
            bytes,
            status: Some(200),
            error: None,
            rate_limit_remaining: Some(4999),
            rate_limit_reset: Some(1_760_000_000),
            rate_limit_resource: Some("core".into()),
        }
    }

    #[test]
    fn graphql_names() {
        assert_eq!(graphql_name("query Badges { viewer { login } }"), "Badges");
        assert_eq!(graphql_name("query($o: String!) { repository(owner: $o) { id } }"), "repository");
        assert_eq!(graphql_name("mutation($id: ID!, $b: String!) { addComment(input: {subjectId: $id, body: $b}) { clientMutationId } }"), "addComment");
        assert_eq!(graphql_name("query {\n  rr: search(query: \"x\") { issueCount } }"), "search");
        assert_eq!(graphql_name("{ viewer { login } }"), "viewer");
        assert_eq!(graphql_name("garbage"), "graphql");
    }

    #[test]
    fn rest_names_strip_host_and_query() {
        assert_eq!(rest_name("GET", "/repos/o/r/pulls?per_page=100&page=2"), "GET /repos/o/r/pulls");
        assert_eq!(rest_name("PUT", "https://api.github.com/notifications/threads/1?x=1"), "PUT /notifications/threads/1");
        assert_eq!(rest_name("GET", "https://example.com"), "GET /");
        assert_eq!(rest_name("GET", "/user#frag"), "GET /user");
    }

    #[test]
    fn percentiles() {
        let v: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&v, 95.0), 95.0);
        assert_eq!(percentile(&v, 50.0), 50.0);
        assert_eq!(percentile(&[7.0], 95.0), 7.0);
        assert_eq!(percentile(&[], 95.0), 0.0);
    }

    #[test]
    fn grouping_and_stats() {
        let list = [entry("a", 10.0, 100, ORIGIN_USER), entry("b", 30.0, 50, ORIGIN_LIVE), entry("a", 20.0, 200, ORIGIN_LIVE)];
        let s = stats(&list);
        assert_eq!(s.count, 3);
        assert_eq!(s.bytes, 350);
        assert_eq!(s.live, 2);
        assert!((s.avg_ms - 20.0).abs() < 1e-9);
        let g = grouped(&list);
        assert_eq!(g[0].0, "a");
        assert_eq!(g[0].1.count, 2);
        assert_eq!(g[0].1.p95_ms, 20.0);
        assert_eq!(g[1].0, "b");
    }

    #[test]
    fn ring_buffer_is_capped() {
        let mut q = VecDeque::new();
        for i in 0..(REQUEST_CAPACITY + 25) {
            push_capped(&mut q, i, REQUEST_CAPACITY);
        }
        assert_eq!(q.len(), REQUEST_CAPACITY);
        assert_eq!(q.front(), Some(&25));
    }

    #[test]
    fn tick_attribution_and_settle() {
        let mut s = Store::default();
        assert_eq!(s.attach(), (ORIGIN_USER, 0));
        let started = Instant::now();
        s.tick = Some(OpenTick { id: 1, ts: Utc::now(), started, in_flight: 0, requests: 0, last_end: None });
        let (origin, id) = s.attach();
        assert_eq!((origin, id), (ORIGIN_LIVE, 1));
        assert!(!s.settle_tick(1, started + Duration::from_secs(1)));
        s.detach(1, started + Duration::from_millis(400));
        assert!(!s.settle_tick(1, started + Duration::from_millis(500)));
        assert!(s.settle_tick(1, started + Duration::from_millis(800)));
        assert!(s.tick.is_none());
        assert_eq!(s.ticks.len(), 1);
        assert_eq!(s.ticks[0].requests, 1);
        assert_eq!(s.ticks[0].duration_ms, 400.0);
    }

    #[test]
    fn export_is_tagged_camel_case_without_secrets() {
        let snap = Snapshot { requests: vec![entry("GET /user", 12.5, 10, ORIGIN_USER)], ticks: vec![], slow_ui: vec![SlowUiEntry { ts: Utc::now(), name: "list.render", duration_ms: 20.0 }] };
        let out = export_lines(&snap);
        let lines: Vec<serde_json::Value> = out.lines().map(|l| serde_json::from_str(l).expect("json")).collect();
        assert_eq!(lines.len(), 2);
        let req = lines.iter().find(|l| l["type"] == "request").expect("request");
        assert_eq!(req["durationMs"], 12.5);
        assert_eq!(req["rateLimitRemaining"], 4999);
        assert_eq!(req["origin"], "user");
        assert!(req.get("authorization").is_none());
        assert!(lines.iter().any(|l| l["type"] == "slow-ui"));
    }

    #[test]
    fn disabled_probe_records_nothing() {
        ENABLED.store(false, Ordering::Relaxed);
        let before = store().requests.len();
        let mut p = begin("rest", || unreachable!());
        p.response(200, &HeaderMap::new());
        drop(p);
        assert_eq!(store().requests.len(), before);
    }
}
