use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};

use crate::api::models::{PrFile, RepoRef};
use crate::diff::{self, DiffFile};
use crate::host::{self, HostCmd};
use crate::signal::Signal;

#[derive(Clone, Debug, PartialEq)]
pub struct CommitLine {
    pub id: String,
    pub short: String,
    pub subject: String,
}

pub fn run(args: &[&str], dir: &str) -> (String, bool) {
    let git = host::git_binary();
    let mut full: Vec<String> = vec!["-C".into(), dir.into(), "-c".into(), "core.quotepath=off".into(), "-c".into(), "core.fsmonitor=false".into(), "--no-pager".into()];
    full.extend(args.iter().map(|s| s.to_string()));
    let (out, _, ok) = HostCmd::new(&git).args(full).env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0").env("GIT_PAGER", "cat").env("LC_ALL", "C").unset("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS").output();
    (out, ok)
}

pub fn remote_repo(dir: &str) -> Option<RepoRef> {
    let (url, _) = run(&["remote", "get-url", "origin"], dir);
    let url = url.trim();
    if !url.contains("github.com") {
        return None;
    }
    let mut tail = url.rsplit("github.com").next().unwrap_or("").trim_matches(|c| c == ':' || c == '/').to_string();
    if tail.ends_with(".git") {
        tail.truncate(tail.len() - 4);
    }
    RepoRef::from_full(&tail)
}

pub struct Snapshot {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub repo: Option<RepoRef>,
    pub changes: Vec<DiffFile>,
    pub unpushed: Vec<CommitLine>,
}

pub fn snapshot(dir: String) -> Snapshot {
    if !run(&["rev-parse", "--is-inside-work-tree"], &dir).1 {
        return Snapshot { is_repo: false, branch: None, repo: None, changes: vec![], unpushed: vec![] };
    }
    let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"], &dir).0.trim().to_string();
    let repo = remote_repo(&dir);
    let mut files: Vec<PrFile> = diff::parse_unified(&run(&["diff", "HEAD", "--no-ext-diff", "--no-textconv", "--no-color", "-M"], &dir).0);
    if files.is_empty() {
        files = diff::parse_unified(&run(&["diff", "--no-ext-diff", "--no-textconv", "--no-color", "-M"], &dir).0);
    }
    let untracked = run(&["ls-files", "--others", "--exclude-standard"], &dir).0;
    for name in untracked.lines().take(200) {
        let full = format!("{dir}/{name}");
        let meta = std::fs::symlink_metadata(&full).ok().filter(|m| m.is_file());
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let mut patch = None;
        let mut adds = 0;
        if meta.is_some() && size < 300_000 {
            if let Ok(text) = std::fs::read_to_string(&full) {
                let lines: Vec<&str> = text.split('\n').collect();
                adds = lines.len() as i64;
                patch = Some(format!("@@ -0,0 +1,{} @@\n{}", lines.len(), lines.iter().map(|l| format!("+{l}")).collect::<Vec<_>>().join("\n")));
            }
        }
        files.push(PrFile { filename: name.to_string(), status: "added".into(), additions: adds, deletions: 0, patch, previous_filename: None });
    }
    let (log, ok) = run(&["log", "--format=%H%x09%h%x09%s", "-n", "30", "@{upstream}..HEAD"], &dir);
    let unpushed = if ok {
        log.lines()
            .filter_map(|l| {
                let p: Vec<&str> = l.splitn(3, '\t').collect();
                if p.len() == 3 {
                    Some(CommitLine { id: p[0].into(), short: p[1].into(), subject: p[2].into() })
                } else {
                    None
                }
            })
            .collect()
    } else {
        vec![]
    };
    Snapshot { is_repo: true, branch: if branch.is_empty() { None } else { Some(branch) }, repo, changes: files.iter().map(DiffFile::new).collect(), unpushed }
}

pub struct WorkingCopy {
    pub path: String,
    pub is_repo: Cell<bool>,
    pub branch: RefCell<Option<String>>,
    pub repo: RefCell<Option<RepoRef>>,
    pub changes: RefCell<Vec<DiffFile>>,
    pub unpushed: RefCell<Vec<CommitLine>>,
    pub loading: Cell<bool>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    pending: RefCell<Option<glib::SourceId>>,
    queued: Cell<bool>,
    pub changed: Signal<()>,
}

impl WorkingCopy {
    pub fn new(path: &str) -> Rc<Self> {
        Rc::new(WorkingCopy {
            path: path.to_string(),
            is_repo: Cell::new(false),
            branch: RefCell::new(None),
            repo: RefCell::new(None),
            changes: RefCell::new(vec![]),
            unpushed: RefCell::new(vec![]),
            loading: Cell::new(false),
            monitors: RefCell::new(vec![]),
            pending: RefCell::new(None),
            queued: Cell::new(false),
            changed: Signal::new(),
        })
    }

    pub fn ahead(&self) -> usize {
        self.unpushed.borrow().len()
    }

    pub fn refresh(self: &Rc<Self>) {
        if self.loading.get() {
            self.queued.set(true);
            return;
        }
        self.loading.set(true);
        self.changed.emit(&());
        let me = self.clone();
        crate::rt::spawn_local(async move {
            loop {
                me.queued.set(false);
                let dir = me.path.clone();
                let s = crate::rt::blocking(move || snapshot(dir)).await;
                me.is_repo.set(s.is_repo);
                *me.branch.borrow_mut() = s.branch;
                *me.repo.borrow_mut() = s.repo;
                *me.changes.borrow_mut() = s.changes;
                *me.unpushed.borrow_mut() = s.unpushed;
                if !me.queued.get() {
                    break;
                }
            }
            me.loading.set(false);
            me.changed.emit(&());
        });
    }

    pub fn start_watching(self: &Rc<Self>) {
        if !self.monitors.borrow().is_empty() {
            return;
        }
        for dir in [self.path.clone(), format!("{}/.git", self.path)] {
            let file = gio::File::for_path(&dir);
            if let Ok(m) = file.monitor_directory(gio::FileMonitorFlags::WATCH_MOVES, None::<&gio::Cancellable>) {
                let w = Rc::downgrade(self);
                m.connect_changed(move |_, f, _, _| {
                    let p = f.path().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                    if p.contains("/.git/objects") || p.contains("/.git/logs") || p.ends_with(".lock") || p.contains("/node_modules") || p.contains("/target/") {
                        return;
                    }
                    if let Some(me) = w.upgrade() {
                        me.schedule();
                    }
                });
                self.monitors.borrow_mut().push(m);
            }
        }
    }

    fn schedule(self: &Rc<Self>) {
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let w = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(Duration::from_millis(800), move || {
            if let Some(me) = w.upgrade() {
                *me.pending.borrow_mut() = None;
                me.refresh();
            }
        });
        *self.pending.borrow_mut() = Some(id);
    }
}
