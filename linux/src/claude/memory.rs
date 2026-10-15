use std::collections::HashMap;

use crate::host::HostCmd;
use crate::util;

#[derive(Clone, Copy)]
pub struct Entry {
    pub ppid: i32,
    pub rss: i64,
}

pub fn snapshot() -> HashMap<i32, Entry> {
    let mut map = HashMap::new();
    if util::is_flatpak() {
        let (out, _, _) = HostCmd::new("ps").args(["-axo", "pid=,ppid=,rss="]).output();
        for line in out.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() >= 3 {
                if let (Ok(pid), Ok(ppid), Ok(rss)) = (f[0].parse::<i32>(), f[1].parse::<i32>(), f[2].parse::<i64>()) {
                    map.insert(pid, Entry { ppid, rss: rss * 1024 });
                }
            }
        }
        return map;
    }
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(4096) as i64;
    let Ok(dir) = std::fs::read_dir("/proc") else { return map };
    for e in dir.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Ok(pid) = name.parse::<i32>() else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else { continue };
        let Some(close) = stat.rfind(')') else { continue };
        let fields: Vec<&str> = stat[close + 1..].split_whitespace().collect();
        let ppid = fields.get(1).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
        let rss_pages = fields.get(21).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
        map.insert(pid, Entry { ppid, rss: rss_pages * page });
    }
    map
}

pub fn tree_bytes(root: i32, snap: &HashMap<i32, Entry>) -> i64 {
    let Some(root_entry) = snap.get(&root) else { return 0 };
    if root <= 0 {
        return 0;
    }
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    for (pid, e) in snap {
        children.entry(e.ppid).or_default().push(*pid);
    }
    let mut total = root_entry.rss;
    let mut stack = children.get(&root).cloned().unwrap_or_default();
    let mut seen = std::collections::HashSet::from([root]);
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(e) = snap.get(&pid) {
            total += e.rss;
            if let Some(c) = children.get(&pid) {
                stack.extend(c.iter().copied());
            }
        }
    }
    total
}

pub fn format(bytes: i64) -> String {
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{} MB", mb.round() as i64)
    }
}
