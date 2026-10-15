use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct SessionInfo {
    pub id: String,
    pub file: PathBuf,
    pub cwd: String,
    pub title: String,
    pub branch: Option<String>,
    pub modified: DateTime<Utc>,
    pub pr_url: Option<String>,
    pub pr_number: Option<i64>,
    pub model: Option<String>,
    pub lines_added: Option<i64>,
    pub lines_removed: Option<i64>,
}

impl SessionInfo {
    pub fn worktree_name(&self) -> Option<String> {
        let idx = self.cwd.find("/.claude/worktrees/")?;
        self.cwd[idx + "/.claude/worktrees/".len()..].split('/').next().map(|s| s.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Project {
    pub id: String,
    pub path: String,
    pub sessions: Vec<SessionInfo>,
}

impl Project {
    pub fn name(&self) -> String {
        crate::util::last_component(&self.path)
    }

    pub fn exists(&self) -> bool {
        Path::new(&self.path).exists()
    }

    pub fn last_activity(&self) -> DateTime<Utc> {
        self.sessions.first().map(|s| s.modified).unwrap_or(DateTime::<Utc>::MIN_UTC)
    }
}

static CACHE: LazyLock<Mutex<HashMap<String, (SystemTime, Option<SessionInfo>)>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn project_root(cwd: &str) -> String {
    match cwd.find("/.claude/worktrees/") {
        Some(i) => cwd[..i].to_string(),
        None => cwd.to_string(),
    }
}

pub fn load_all(root: PathBuf) -> Vec<Project> {
    let Ok(dirs) = std::fs::read_dir(&root) else { return vec![] };
    let mut by_project: HashMap<String, Vec<SessionInfo>> = HashMap::new();
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if p.extension().map(|e| e == "jsonl").unwrap_or(false) {
                if let Some(info) = parse(&p) {
                    by_project.entry(project_root(&info.cwd)).or_default().push(info);
                }
            }
        }
    }
    let mut projects: Vec<Project> = by_project
        .into_iter()
        .map(|(path, mut sessions)| {
            sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
            Project { id: path.clone(), path, sessions }
        })
        .collect();
    projects.sort_by_key(|p| std::cmp::Reverse(p.last_activity()));
    projects
}

pub fn parse(file: &Path) -> Option<SessionInfo> {
    let mtime = std::fs::metadata(file).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
    let key = file.to_string_lossy().to_string();
    if let Ok(cache) = CACHE.lock() {
        if let Some((t, info)) = cache.get(&key) {
            if *t == mtime {
                return info.clone();
            }
        }
    }
    let info = parse_uncached(file, mtime);
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(key, (mtime, info.clone()));
    }
    info
}

fn lines(data: &[u8], drop_first: bool, drop_last: bool) -> Vec<Value> {
    let mut parts: Vec<&[u8]> = data.split(|b| *b == b'\n').filter(|p| !p.is_empty()).collect();
    if drop_first && !parts.is_empty() {
        parts.remove(0);
    }
    if drop_last && !parts.is_empty() {
        parts.pop();
    }
    parts.into_iter().filter_map(|p| serde_json::from_slice::<Value>(p).ok()).filter(|v| v.is_object()).collect()
}

fn text_of(message: Option<&Value>) -> Option<String> {
    let m = message?;
    if let Some(s) = m.get("content").and_then(|c| c.as_str()) {
        return Some(s.to_string());
    }
    m.get("content")?.as_array()?.iter().find_map(|b| if b.get("type").and_then(|t| t.as_str()) == Some("text") { b.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()) } else { None })
}

pub fn clean_prompt(raw: &str) -> Option<String> {
    let mut s = raw.trim().to_string();
    if s.starts_with("<command-name>") || s.starts_with("<local-command") || s.starts_with("Caveat:") || s.starts_with("[Request interrupted") {
        return None;
    }
    if s.starts_with('<') {
        let re = regex::Regex::new("<[^>]+>").ok()?;
        s = re.replace_all(&s, " ").trim().to_string();
    }
    let ws = regex::Regex::new(r"\s+").ok()?;
    s = ws.replace_all(&s, " ").to_string();
    if s.is_empty() {
        None
    } else {
        Some(s.chars().take(160).collect())
    }
}

fn parse_uncached(file: &Path, mtime: SystemTime) -> Option<SessionInfo> {
    let mut f = std::fs::File::open(file).ok()?;
    let size = f.metadata().ok()?.len();
    let head_size = size.min(96 * 1024);
    let mut head = vec![0u8; head_size as usize];
    f.read_exact(&mut head).ok()?;
    let tail_size = size.min(160 * 1024);
    let mut tail = vec![];
    if size > head_size {
        f.seek(SeekFrom::Start(size - tail_size)).ok()?;
        tail = vec![0u8; tail_size as usize];
        f.read_exact(&mut tail).ok()?;
    }
    let mut cwd: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    for obj in lines(&head, false, size > head_size) {
        if cwd.is_none() {
            cwd = obj.get("cwd").and_then(|c| c.as_str()).map(|s| s.to_string());
        }
        if branch.is_none() {
            if let Some(b) = obj.get("gitBranch").and_then(|b| b.as_str()) {
                if !b.is_empty() && b != "HEAD" {
                    branch = Some(b.to_string());
                }
            }
        }
        if first_prompt.is_none()
            && obj.get("type").and_then(|t| t.as_str()) == Some("user")
            && obj.get("isMeta").and_then(|v| v.as_bool()) != Some(true)
            && obj.get("isSidechain").and_then(|v| v.as_bool()) != Some(true)
        {
            if let Some(t) = text_of(obj.get("message")) {
                first_prompt = clean_prompt(&t);
            }
        }
        if cwd.is_some() && branch.is_some() && first_prompt.is_some() {
            break;
        }
    }
    let cwd = cwd?;
    if first_prompt.is_none() && size <= 2048 {
        return None;
    }
    let mut title: Option<String> = None;
    let mut last_prompt: Option<String> = None;
    let mut pr_url = None;
    let mut pr_number = None;
    let mut model = None;
    let mut added = None;
    let mut removed = None;
    let tail_objects = if tail.is_empty() { lines(&head, false, false) } else { lines(&tail, true, false) };
    for obj in tail_objects {
        match obj.get("type").and_then(|t| t.as_str()) {
            Some("custom-title") => {
                if let Some(t) = obj.get("customTitle").and_then(|t| t.as_str()) {
                    title = Some(t.to_string());
                }
            }
            Some("last-prompt") => {
                if let Some(t) = obj.get("lastPrompt").and_then(|t| t.as_str()) {
                    last_prompt = Some(t.to_string());
                }
            }
            Some("pr-link") => {
                if let Some(u) = obj.get("prUrl").and_then(|t| t.as_str()) {
                    pr_url = Some(u.to_string());
                }
                if let Some(n) = obj.get("prNumber").and_then(|t| t.as_i64()) {
                    pr_number = Some(n);
                }
            }
            Some("assistant") => {
                if let Some(m) = obj.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
                    if !m.starts_with('<') {
                        model = Some(m.to_string());
                    }
                }
                if let Some(b) = obj.get("gitBranch").and_then(|b| b.as_str()) {
                    if !b.is_empty() && b != "HEAD" {
                        branch = Some(b.to_string());
                    }
                }
            }
            Some("cost-state") => {
                if let Some(a) = obj.get("totalLinesAdded").and_then(|v| v.as_i64()) {
                    added = Some(a);
                }
                if let Some(r) = obj.get("totalLinesRemoved").and_then(|v| v.as_i64()) {
                    removed = Some(r);
                }
            }
            _ => {}
        }
    }
    let display = match title.filter(|t| !t.is_empty()) {
        Some(t) => t,
        None => first_prompt.or_else(|| last_prompt.as_deref().and_then(clean_prompt)).unwrap_or_else(|| "Untitled".into()),
    };
    let modified: DateTime<Utc> = mtime.into();
    Some(SessionInfo {
        id: file.file_stem()?.to_string_lossy().to_string(),
        file: file.to_path_buf(),
        cwd,
        title: display,
        branch,
        modified,
        pr_url,
        pr_number,
        model,
        lines_added: added,
        lines_removed: removed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_session_file() {
        let dir = std::env::temp_dir().join(format!("sy-test-{}", uuid::Uuid::new_v4()));
        let proj = dir.join("p");
        std::fs::create_dir_all(&proj).unwrap();
        let file = proj.join("abc.jsonl");
        std::fs::write(
            &file,
            "{\"type\":\"user\",\"cwd\":\"/home/x/repo\",\"gitBranch\":\"feat-a\",\"message\":{\"content\":\"Fix the bug please\"}}\n{\"type\":\"assistant\",\"message\":{\"model\":\"claude-x\"}}\n{\"type\":\"pr-link\",\"prUrl\":\"https://github.com/a/b/pull/3\",\"prNumber\":3}\n",
        )
        .unwrap();
        let all = load_all(dir.clone());
        assert_eq!(all.len(), 1);
        let s = &all[0].sessions[0];
        assert_eq!(s.title, "Fix the bug please");
        assert_eq!(s.branch.as_deref(), Some("feat-a"));
        assert_eq!(s.pr_number, Some(3));
        let _ = std::fs::remove_dir_all(dir);
    }
}
