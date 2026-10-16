use chrono::{DateTime, Local, Utc};
use gtk::prelude::*;
use gtk::{gdk, gio};

use crate::api::models::{ItemRef, RepoRef, Route};
use crate::settings;

const RESERVED: &[&str] = &[
    "orgs", "settings", "notifications", "users", "marketplace", "features", "pulls", "issues", "search", "login", "logout", "apps", "sponsors", "topics",
    "explore", "new", "codespaces", "organizations", "enterprise", "about", "pricing", "collections", "trending",
];

pub fn parse_github_url(raw: &str, allow_short: bool) -> Option<Route> {
    let s = raw.trim();
    if s.is_empty() || s.contains(' ') {
        return None;
    }
    if allow_short && !s.contains("://") {
        if let Some(hash) = s.find('#') {
            let repo_part = &s[..hash];
            let num = s[hash + 1..].parse::<i64>().ok()?;
            let repo = RepoRef::from_full(repo_part)?;
            if repo.name.contains('/') {
                return None;
            }
            return Some(Route::Pr(ItemRef { repo, number: num }));
        }
    }
    let full = if s.starts_with("http") { s.to_string() } else { format!("https://{s}") };
    let url = url::Url::parse(&full).ok()?;
    let host = url.host_str()?.to_lowercase();
    let parts: Vec<String> = url
        .path_segments()
        .map(|p| p.filter(|x| !x.is_empty()).map(|x| percent_encoding::percent_decode_str(x).decode_utf8_lossy().to_string()).collect())
        .unwrap_or_default();
    if host == "github.com" || host == "www.github.com" {
        if parts.len() < 2 || RESERVED.contains(&parts[0].to_lowercase().as_str()) {
            return None;
        }
        let repo = RepoRef::new(&parts[0], &parts[1]);
        if parts.len() >= 4 {
            if let Ok(n) = parts[3].parse::<i64>() {
                if parts[2] == "pull" {
                    return Some(Route::Pr(ItemRef { repo, number: n }));
                }
                if parts[2] == "issues" {
                    return Some(Route::Issue(ItemRef { repo, number: n }));
                }
            }
            if parts[2] == "commit" {
                return Some(Route::Commit(repo, parts[3].clone()));
            }
            return None;
        }
        if parts.len() == 2 {
            return Some(Route::Repo(repo));
        }
        if parts.len() == 3 && (parts[2] == "pulls" || parts[2] == "issues") {
            return Some(Route::Repo(repo));
        }
        return None;
    }
    if host == "api.github.com" && parts.len() >= 5 && parts[0] == "repos" {
        if let Ok(n) = parts[4].parse::<i64>() {
            let repo = RepoRef::new(&parts[1], &parts[2]);
            if parts[3] == "pulls" {
                return Some(Route::Pr(ItemRef { repo, number: n }));
            }
            if parts[3] == "issues" {
                return Some(Route::Issue(ItemRef { repo, number: n }));
            }
        }
    }
    None
}

pub fn rel(date: Option<DateTime<Utc>>) -> String {
    let Some(date) = date else { return String::new() };
    rel_to(date, Utc::now())
}

pub fn rel_to(date: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - date).num_seconds();
    let future = secs < 0;
    let s = secs.abs();
    if s < 45 {
        return "just now".into();
    }
    let (n, unit) = if s < 60 {
        (s, "sec.")
    } else if s < 3600 {
        (s / 60, "min.")
    } else if s < 86_400 {
        (s / 3600, "hr.")
    } else if s < 7 * 86_400 {
        let d = s / 86_400;
        (d, if d == 1 { "day" } else { "days" })
    } else if s < 30 * 86_400 {
        (s / (7 * 86_400), "wk.")
    } else if s < 365 * 86_400 {
        (s / (30 * 86_400), "mo.")
    } else {
        (s / (365 * 86_400), "yr.")
    };
    if future {
        format!("in {n} {unit}")
    } else {
        format!("{n} {unit} ago")
    }
}

pub fn full_date(date: DateTime<Utc>) -> String {
    date.with_timezone(&Local).format("%b %-d, %Y at %-I:%M %p").to_string()
}

pub fn refresh_ago(updated: DateTime<Utc>) -> String {
    let s = (Utc::now() - updated).num_seconds().max(0);
    if s < 10 {
        "just now".into()
    } else if s < 60 {
        format!("{}s ago", s / 5 * 5)
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else {
        format!("{}h ago", s / 3600)
    }
}

pub fn esc(s: &str) -> String {
    let mut r = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => r.push_str("&amp;"),
            '<' => r.push_str("&lt;"),
            '>' => r.push_str("&gt;"),
            '"' => r.push_str("&quot;"),
            '\'' => r.push_str("&#39;"),
            c => r.push(c),
        }
    }
    r
}

pub fn markup_esc(s: &str) -> String {
    gtk::glib::markup_escape_text(s).to_string()
}

pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn home() -> String {
    gtk::glib::home_dir().to_string_lossy().to_string()
}

pub fn tilde(path: &str) -> String {
    let h = home();
    if !h.is_empty() && path.starts_with(&h) {
        format!("~{}", &path[h.len()..])
    } else {
        path.to_string()
    }
}

pub fn last_component(path: &str) -> String {
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(path).to_string()
}

pub fn parent_path(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

pub fn extension(path: &str) -> String {
    let name = last_component(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

pub fn plural(n: i64, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

pub fn byte_count(bytes: i64) -> String {
    let b = bytes as f64;
    if b < 1000.0 {
        format!("{bytes} bytes")
    } else if b < 1_000_000.0 {
        format!("{:.0} KB", b / 1000.0)
    } else if b < 1_000_000_000.0 {
        format!("{:.1} MB", b / 1_000_000.0)
    } else {
        format!("{:.2} GB", b / 1_000_000_000.0)
    }
}

pub fn rgba(hex: &str) -> gdk::RGBA {
    let mut s = hex.trim_start_matches('#').to_string();
    if s.len() == 3 {
        s = s.chars().flat_map(|c| [c, c]).collect();
    }
    let v = u32::from_str_radix(&s, 16).unwrap_or(0x888888);
    gdk::RGBA::new(((v >> 16) & 0xff) as f32 / 255.0, ((v >> 8) & 0xff) as f32 / 255.0, (v & 0xff) as f32 / 255.0, 1.0)
}

pub fn clean_hex(hex: &str) -> String {
    let s: String = hex.trim_start_matches('#').chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if s.len() == 3 || s.len() == 6 {
        s
    } else {
        "888888".into()
    }
}

pub fn copy(text: &str) {
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(text);
    }
}

pub fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

pub fn open_url(uri: &str) {
    if !(uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("mailto:")) {
        return;
    }
    let browser = settings::read(|s| s.link_browser.clone());
    if browser != "system" && !is_flatpak() && uri.starts_with("http") {
        let found = gio::AppInfo::all_for_type("x-scheme-handler/https").into_iter().find(|i| i.id().map(|s| s.as_str() == browser).unwrap_or(false));
        if let Some(info) = found {
            if info.launch_uris(&[uri], None::<&gio::AppLaunchContext>).is_ok() {
                return;
            }
        }
    }
    open_in_default_browser(uri);
}

pub fn open_in_default_browser(uri: &str) {
    let launcher = gtk::UriLauncher::new(uri);
    let window = crate::app::main_window();
    launcher.launch(window.as_ref(), None::<&gio::Cancellable>, |_| {});
}

pub fn open_path(path: &str) {
    let file = gio::File::for_path(path);
    let launcher = gtk::FileLauncher::new(Some(&file));
    let window = crate::app::main_window();
    launcher.launch(window.as_ref(), None::<&gio::Cancellable>, |_| {});
}

pub fn reveal_path(path: &str) {
    let file = gio::File::for_path(path);
    let launcher = gtk::FileLauncher::new(Some(&file));
    let window = crate::app::main_window();
    launcher.open_containing_folder(window.as_ref(), None::<&gio::Cancellable>, |_| {});
}

pub struct BrowserApp {
    pub id: String,
    pub name: String,
}

pub fn installed_browsers() -> Vec<BrowserApp> {
    let mut seen = std::collections::HashSet::new();
    let mut list: Vec<BrowserApp> = gio::AppInfo::all_for_type("x-scheme-handler/https")
        .into_iter()
        .filter_map(|info| {
            let id = info.id()?.to_string();
            if !seen.insert(id.clone()) {
                return None;
            }
            Some(BrowserApp { id, name: info.display_name().to_string() })
        })
        .collect();
    list.sort_by_key(|b| b.name.to_lowercase());
    list
}

pub fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    bi.next();
                }
                let va = na.trim_start_matches('0');
                let vb = nb.trim_start_matches('0');
                let ord = va.len().cmp(&vb.len()).then_with(|| va.cmp(vb));
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

pub const FUZZY_WORKER_MIN: usize = 4000;

pub fn fuzzy_query(query: &str) -> String {
    query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect()
}

pub struct FuzzyIndex {
    paths: Vec<String>,
    lower: Vec<Box<str>>,
    names: Vec<(u32, u32)>,
    chars: Vec<u32>,
}

impl FuzzyIndex {
    pub fn new(paths: Vec<String>) -> Self {
        let mut lower = Vec::with_capacity(paths.len());
        let mut names = Vec::with_capacity(paths.len());
        let mut chars = Vec::with_capacity(paths.len());
        for p in &paths {
            let l = p.to_lowercase();
            let trimmed = l.trim_end_matches('/');
            let start = trimmed.rfind('/').map(|i| i + 1).unwrap_or(0);
            names.push((start as u32, trimmed.len() as u32));
            chars.push(l.chars().count() as u32);
            lower.push(l.into_boxed_str());
        }
        FuzzyIndex { paths, lower, names, chars }
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn path(&self, i: u32) -> &str {
        &self.paths[i as usize]
    }

    fn score(&self, i: usize, q: &[char], qs: &str) -> Option<i64> {
        let lower = &self.lower[i];
        let mut score = 0i64;
        let mut qi = 0usize;
        let mut last: Option<usize> = None;
        for (ci, c) in lower.chars().enumerate() {
            if qi >= q.len() {
                break;
            }
            if c == q[qi] {
                score += match last {
                    Some(l) if l + 1 == ci => 3,
                    _ => 1,
                };
                last = Some(ci);
                qi += 1;
            }
        }
        if qi < q.len() {
            return None;
        }
        let (start, end) = self.names[i];
        let name = &lower[start as usize..end as usize];
        if name.contains(qs) {
            score += 20;
        }
        if name.starts_with(qs) {
            score += 10;
        }
        score -= self.chars[i] as i64 / 20;
        Some(score)
    }

    pub fn search(&self, query: &str, candidates: Option<&[u32]>, limit: usize) -> (Vec<u32>, Vec<u32>) {
        let q: Vec<char> = query.chars().collect();
        let all: Box<dyn Iterator<Item = u32> + '_> = match candidates {
            Some(c) => Box::new(c.iter().copied()),
            None => Box::new(0..self.paths.len() as u32),
        };
        let mut matched = Vec::new();
        let mut scored: Vec<(u32, i64)> = Vec::new();
        for i in all {
            if let Some(score) = self.score(i as usize, &q, query) {
                matched.push(i);
                scored.push((i, score));
            }
        }
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        (matched, scored.into_iter().take(limit).map(|(i, _)| i).collect())
    }
}

pub struct FuzzySearch {
    index: std::sync::Arc<FuzzyIndex>,
    last: std::cell::RefCell<Option<(String, std::sync::Arc<Vec<u32>>)>>,
    generation: std::cell::Cell<u64>,
}

impl FuzzySearch {
    pub async fn build(paths: Vec<String>) -> std::rc::Rc<Self> {
        let index = if paths.len() >= FUZZY_WORKER_MIN { crate::rt::blocking(move || FuzzyIndex::new(paths)).await } else { FuzzyIndex::new(paths) };
        std::rc::Rc::new(FuzzySearch { index: std::sync::Arc::new(index), last: std::cell::RefCell::new(None), generation: std::cell::Cell::new(0) })
    }

    fn candidates(&self, q: &str) -> Option<std::sync::Arc<Vec<u32>>> {
        self.last.borrow().as_ref().filter(|(prev, _)| q.starts_with(prev.as_str())).map(|(_, m)| m.clone())
    }

    fn finish(&self, q: String, matched: Vec<u32>, top: Vec<u32>) -> Vec<String> {
        *self.last.borrow_mut() = Some((q, std::sync::Arc::new(matched)));
        top.into_iter().map(|i| self.index.path(i).to_string()).collect()
    }

    fn first(&self, limit: usize) -> Vec<String> {
        *self.last.borrow_mut() = None;
        self.index.paths.iter().take(limit).cloned().collect()
    }

    pub fn run_now(&self, query: &str, limit: usize, span: &'static str) -> Vec<String> {
        self.generation.set(self.generation.get() + 1);
        let q = fuzzy_query(query);
        if q.is_empty() {
            return self.first(limit);
        }
        let candidates = self.candidates(&q);
        let _span = crate::diagnostics::ui_span(span);
        let (matched, top) = self.index.search(&q, candidates.as_deref().map(|v| v.as_slice()), limit);
        self.finish(q, matched, top)
    }

    pub async fn run(&self, query: &str, limit: usize, span: &'static str) -> Option<Vec<String>> {
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        let q = fuzzy_query(query);
        if q.is_empty() {
            return Some(self.first(limit));
        }
        let candidates = self.candidates(&q);
        let size = candidates.as_ref().map(|c| c.len()).unwrap_or(self.index.len());
        let (matched, top) = if size >= FUZZY_WORKER_MIN {
            let index = self.index.clone();
            let qq = q.clone();
            crate::rt::blocking(move || index.search(&qq, candidates.as_deref().map(|v| v.as_slice()), limit)).await
        } else {
            let _span = crate::diagnostics::ui_span(span);
            self.index.search(&q, candidates.as_deref().map(|v| v.as_slice()), limit)
        };
        if self.generation.get() != generation {
            return None;
        }
        Some(self.finish(q, matched, top))
    }
}

pub fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 {
        format!("-{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links() {
        assert_eq!(parse_github_url("https://github.com/a/b/pull/12", true), Some(Route::Pr(ItemRef { repo: RepoRef::new("a", "b"), number: 12 })));
        assert_eq!(parse_github_url("a/b#7", true), Some(Route::Pr(ItemRef { repo: RepoRef::new("a", "b"), number: 7 })));
        assert_eq!(parse_github_url("github.com/a/b/issues/3", true), Some(Route::Issue(ItemRef { repo: RepoRef::new("a", "b"), number: 3 })));
        assert_eq!(parse_github_url("https://github.com/a/b", true), Some(Route::Repo(RepoRef::new("a", "b"))));
        assert_eq!(parse_github_url("https://github.com/orgs/x", true), None);
        assert_eq!(parse_github_url("https://api.github.com/repos/a/b/pulls/5", false), Some(Route::Pr(ItemRef { repo: RepoRef::new("a", "b"), number: 5 })));
        assert_eq!(parse_github_url("a/b#7", false), None);
    }

    #[test]
    fn fuzzy() {
        let index = FuzzyIndex::new(vec!["src/main.rs".to_string(), "README.md".to_string(), "src/ui/pr.rs".to_string(), "Src/Domain/Mainline.rs".to_string()]);
        let (_, top) = index.search(&fuzzy_query("main"), None, 10);
        assert_eq!(top.first().map(|&i| index.path(i)), Some("src/main.rs"));
        let (matched, _) = index.search(&fuzzy_query("ma"), None, 10);
        let narrowed = index.search(&fuzzy_query("main"), Some(&matched), 10);
        assert_eq!(narrowed, index.search(&fuzzy_query("main"), None, 10));
        assert!(index.search("zzz", None, 10).0.is_empty());
    }

    #[test]
    fn natural() {
        assert_eq!(natural_cmp("file2", "file10"), std::cmp::Ordering::Less);
    }
}
