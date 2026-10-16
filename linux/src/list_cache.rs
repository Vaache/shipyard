use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::api::models::SearchItem;

const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);
const MAX_ENTRIES: usize = 60;
const REWRITE_AFTER: Duration = Duration::from_secs(3600);

static EPOCHS: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn epochs() -> std::sync::MutexGuard<'static, HashMap<String, u64>> {
    EPOCHS.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub query: String,
    pub saved_at: DateTime<Utc>,
    pub total: i64,
    pub cursor: Option<String>,
    pub has_more: bool,
    pub items: Vec<SearchItem>,
}

fn enabled() -> bool {
    !crate::demo::is_on()
}

fn root() -> PathBuf {
    crate::settings::cache_dir().join("lists")
}

fn account_key(login: &str) -> Option<String> {
    let key: String = login.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    if key.is_empty() || key.chars().all(|c| c == '_') {
        None
    } else {
        Some(key)
    }
}

fn account_dir(login: &str) -> Option<PathBuf> {
    account_key(login).map(|k| root().join(k))
}

pub fn fingerprint(token: &str) -> String {
    format!("{:016x}", fnv1a(&format!("pullyard-account:{token}")))
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3))
}

fn file_name(query: &str) -> String {
    format!("{:016x}.json", fnv1a(query))
}

pub async fn load(login: &str, query: &str) -> Option<Entry> {
    if !enabled() {
        return None;
    }
    let dir = account_dir(login)?;
    let query = query.to_string();
    crate::rt::blocking(move || read_entry(&dir, &query, Utc::now())).await
}

pub fn store(login: &str, entry: Entry) {
    if !enabled() {
        return;
    }
    let Some(key) = account_key(login) else { return };
    let dir = root().join(&key);
    let epoch = epochs().get(&key).copied().unwrap_or(0);
    crate::rt::runtime().spawn_blocking(move || store_in(&dir, &key, epoch, &entry));
}

pub fn clear(login: &str) {
    let Some(key) = account_key(login) else { return };
    let dir = root().join(&key);
    *epochs().entry(key).or_default() += 1;
    crate::rt::runtime().spawn_blocking(move || {
        let _guard = epochs();
        let _ = std::fs::remove_dir_all(dir);
    });
}

fn store_in(dir: &Path, key: &str, epoch: u64, entry: &Entry) {
    let guard = epochs();
    if guard.get(key).copied().unwrap_or(0) != epoch || !should_write(dir, entry, Utc::now()) {
        return;
    }
    if write_entry(dir, entry).is_ok() {
        prune(dir, SystemTime::now());
    }
    drop(guard);
}

fn should_write(dir: &Path, entry: &Entry, now: DateTime<Utc>) -> bool {
    let Some(old) = read_entry(dir, &entry.query, now) else { return true };
    let age = now.signed_duration_since(old.saved_at).to_std().unwrap_or_default();
    let same = old.total == entry.total && old.has_more == entry.has_more && serde_json::to_value(&old.items).ok() == serde_json::to_value(&entry.items).ok();
    !same || age >= REWRITE_AFTER
}

fn read_entry(dir: &Path, query: &str, now: DateTime<Utc>) -> Option<Entry> {
    let data = std::fs::read(dir.join(file_name(query))).ok()?;
    let entry: Entry = serde_json::from_slice(&data).ok()?;
    let age = now.signed_duration_since(entry.saved_at).to_std().unwrap_or_default();
    (entry.query == query && age <= MAX_AGE).then_some(entry)
}

fn write_entry(dir: &Path, entry: &Entry) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let data = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
    let name = file_name(&entry.query);
    let tmp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        std::io::Write::write_all(&mut f, &data)?;
        f.sync_all()
    });
    match written.and_then(|_| std::fs::rename(&tmp, dir.join(name))) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn prune(dir: &Path, now: SystemTime) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut kept: Vec<(SystemTime, PathBuf)> = vec![];
    for entry in read.flatten() {
        let path = entry.path();
        let Some(modified) = entry.metadata().ok().and_then(|m| m.modified().ok()) else { continue };
        let old = now.duration_since(modified).unwrap_or_default() > MAX_AGE;
        let json = path.extension().is_some_and(|e| e == "json");
        if old {
            let _ = std::fs::remove_file(&path);
        } else if json {
            kept.push((modified, path));
        }
    }
    if kept.len() > MAX_ENTRIES {
        kept.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, path) in kept.drain(MAX_ENTRIES..) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pullyard-list-cache-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item(number: i64) -> SearchItem {
        serde_json::from_value(serde_json::json!({
            "__typename": "PullRequest",
            "number": number,
            "title": format!("PR {number}"),
            "url": format!("https://github.com/o/r/pull/{number}"),
            "prState": "OPEN",
            "isDraft": false,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z",
            "author": { "login": "octo", "avatarUrl": null },
            "repository": { "nameWithOwner": "o/r", "defaultBranchRef": { "name": "main" } },
            "comments": { "totalCount": 3 },
            "labels": { "nodes": [{ "name": "bug", "color": "ff0000" }] },
            "commits": { "nodes": [{ "commit": { "statusCheckRollup": { "state": "SUCCESS" } } }] }
        }))
        .unwrap()
    }

    fn entry(query: &str, saved_at: DateTime<Utc>) -> Entry {
        Entry { query: query.into(), saved_at, total: 2, cursor: Some("c1".into()), has_more: true, items: vec![item(1), item(2)] }
    }

    #[test]
    fn round_trip() {
        let dir = temp_dir();
        let now = Utc::now();
        write_entry(&dir, &entry("is:pr is:open", now)).unwrap();
        let back = read_entry(&dir, "is:pr is:open", now).unwrap();
        assert_eq!(back.items.len(), 2);
        assert_eq!(back.items[1].number, 2);
        assert_eq!(back.items[0].pr_state.as_deref(), Some("OPEN"));
        assert_eq!(back.items[0].comments.total_count, 3);
        assert_eq!(back.items[0].labels().len(), 1);
        assert_eq!(back.cursor.as_deref(), Some("c1"));
        assert!(read_entry(&dir, "is:pr is:closed", now).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn expired_entries_are_ignored() {
        let dir = temp_dir();
        let now = Utc::now();
        write_entry(&dir, &entry("q", now - chrono::Duration::days(8))).unwrap();
        assert!(read_entry(&dir, "q", now).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn prune_caps_entry_count() {
        let dir = temp_dir();
        let now = Utc::now();
        for i in 0..(MAX_ENTRIES + 5) {
            write_entry(&dir, &entry(&format!("q{i}"), now)).unwrap();
        }
        prune(&dir, SystemTime::now());
        let count = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(count, MAX_ENTRIES);
        prune(&dir, SystemTime::now() + MAX_AGE + Duration::from_secs(60));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn unchanged_entries_are_not_rewritten() {
        let dir = temp_dir();
        let now = Utc::now();
        let first = entry("q", now);
        assert!(should_write(&dir, &first, now));
        write_entry(&dir, &first).unwrap();
        assert!(!should_write(&dir, &entry("q", now), now));
        let mut changed = entry("q", now);
        changed.items.pop();
        assert!(should_write(&dir, &changed, now));
        assert!(should_write(&dir, &entry("q", now), now + chrono::Duration::hours(2)));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stores_queued_before_clear_are_dropped() {
        let dir = temp_dir();
        let key = format!("epoch-test-{}", uuid::Uuid::new_v4().simple());
        let before = epochs().get(&key).copied().unwrap_or(0);
        *epochs().entry(key.clone()).or_default() += 1;
        store_in(&dir, &key, before, &entry("q", Utc::now()));
        assert!(read_entry(&dir, "q", Utc::now()).is_none());
        store_in(&dir, &key, before + 1, &entry("q", Utc::now()));
        assert!(read_entry(&dir, "q", Utc::now()).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn account_keys_are_safe() {
        assert_eq!(account_key("Octo-Cat").as_deref(), Some("octo-cat"));
        assert_eq!(account_key("../x").as_deref(), Some("___x"));
        assert!(account_key("").is_none());
        assert!(account_key("..").is_none());
        assert_ne!(file_name("a"), file_name("b"));
        assert_eq!(file_name("is:pr"), file_name("is:pr"));
        assert_eq!(fingerprint("t1"), fingerprint("t1"));
        assert_ne!(fingerprint("t1"), fingerprint("t2"));
        assert!(!fingerprint("ghp_secret").contains("secret"));
    }
}
