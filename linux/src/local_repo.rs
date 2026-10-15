use std::collections::HashSet;
use std::path::Path;

use crate::api::models::RepoRef;
use crate::settings;

pub fn saved(repo: &RepoRef) -> Option<String> {
    let path = settings::read(|s| s.local_repo_paths.get(&repo.full().to_lowercase()).cloned())?;
    if Path::new(&format!("{path}/.git")).exists() {
        Some(path)
    } else {
        None
    }
}

pub fn save(repo: &RepoRef, path: &str) {
    let key = repo.full().to_lowercase();
    let path = path.to_string();
    settings::update(|s| {
        s.local_repo_paths.insert(key, path);
    });
}

pub fn remote_matches(path: &str, repo: &RepoRef) -> bool {
    let Ok(config) = std::fs::read_to_string(format!("{path}/.git/config")) else { return false };
    let target = repo.full().to_lowercase();
    for line in config.to_lowercase().lines() {
        if !(line.contains("url") && line.contains("github.com")) {
            continue;
        }
        let mut s = line.splitn(2, '=').nth(1).unwrap_or("").trim().to_string();
        if s.ends_with(".git") {
            s.truncate(s.len() - 4);
        }
        if s.ends_with(&format!("/{target}")) || s.ends_with(&format!(":{target}")) {
            return true;
        }
    }
    false
}

pub fn scan_blocking(repo: RepoRef, home: String) -> Option<String> {
    let roots = ["Documents", "Developer", "Projects", "projects", "Code", "code", "src", "dev", "work", "GitHub", "git", "repos", "Desktop", "workspace", ""];
    let mut seen = HashSet::new();
    for root in roots {
        let dir = if root.is_empty() { home.clone() } else { format!("{home}/{root}") };
        let depth = if root.is_empty() { 1 } else { 3 };
        if let Some(hit) = scan(&dir, &repo, depth, &mut seen) {
            return Some(hit);
        }
    }
    None
}

fn scan(dir: &str, repo: &RepoRef, depth: i32, seen: &mut HashSet<String>) -> Option<String> {
    if depth < 0 || !seen.insert(dir.to_string()) {
        return None;
    }
    if Path::new(&format!("{dir}/.git")).exists() && remote_matches(dir, repo) {
        return Some(dir.to_string());
    }
    if depth == 0 {
        return None;
    }
    let skip = ["node_modules", "snap", "Pods", "build", "target", ".cache", "Music", "Videos", "Pictures", "Downloads"];
    let mut items: Vec<String> = std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| e.file_name().to_string_lossy().to_string()).collect();
    let name = repo.name.to_lowercase();
    items.sort_by_key(|i| if i.to_lowercase() == name { 0 } else { 1 });
    for item in items {
        if item.starts_with('.') || skip.contains(&item.as_str()) {
            continue;
        }
        if let Some(hit) = scan(&format!("{dir}/{item}"), repo, depth - 1, seen) {
            return Some(hit);
        }
    }
    None
}

pub async fn detect(repo: &RepoRef) -> Option<String> {
    if let Some(s) = saved(repo) {
        return Some(s);
    }
    let r = repo.clone();
    let home = crate::util::home();
    let found = crate::rt::blocking(move || scan_blocking(r, home)).await;
    if let Some(f) = &found {
        save(repo, f);
    }
    found
}
