use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};

use crate::util;

#[derive(Clone, Default)]
pub struct HostCmd {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub unset: Vec<String>,
    pub cwd: Option<String>,
}

impl HostCmd {
    pub fn new(program: &str) -> Self {
        HostCmd { program: program.to_string(), ..Default::default() }
    }

    pub fn arg(mut self, a: impl Into<String>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, a: I) -> Self {
        self.args.extend(a.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, k: &str, v: &str) -> Self {
        self.env.push((k.to_string(), v.to_string()));
        self
    }

    pub fn unset(mut self, k: &str) -> Self {
        self.unset.push(k.to_string());
        self
    }

    pub fn cwd(mut self, dir: &str) -> Self {
        self.cwd = Some(dir.to_string());
        self
    }

    pub fn argv(&self) -> Vec<String> {
        if util::is_flatpak() {
            let mut v = vec!["flatpak-spawn".to_string(), "--host".to_string(), "--watch-bus".to_string()];
            if let Some(dir) = &self.cwd {
                v.push(format!("--directory={dir}"));
            }
            for (k, val) in &self.env {
                v.push(format!("--env={k}={val}"));
            }
            for k in &self.unset {
                v.push(format!("--unset-env={k}"));
            }
            v.push(self.program.clone());
            v.extend(self.args.iter().cloned());
            v
        } else {
            let mut v = vec![self.program.clone()];
            v.extend(self.args.iter().cloned());
            v
        }
    }

    pub fn command(&self) -> Command {
        let argv = self.argv();
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..]);
        if !util::is_flatpak() {
            for k in &self.unset {
                c.env_remove(k);
            }
            for (k, v) in &self.env {
                c.env(k, v);
            }
            if let Some(dir) = &self.cwd {
                c.current_dir(dir);
            }
        }
        c
    }

    pub fn output(&self) -> (String, String, bool) {
        let mut c = self.command();
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        match c.output() {
            Ok(out) => (String::from_utf8_lossy(&out.stdout).to_string(), String::from_utf8_lossy(&out.stderr).to_string(), out.status.success()),
            Err(e) => (String::new(), e.to_string(), false),
        }
    }
}

static FOUND: LazyLock<Mutex<HashMap<String, Option<String>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

pub fn find(name: &str, candidates: &[String]) -> Option<String> {
    if let Some(hit) = FOUND.lock().ok().and_then(|m| m.get(name).cloned()) {
        if hit.as_deref().map(|p| util::is_flatpak() || executable(p)).unwrap_or(false) {
            return hit;
        }
    }
    let found = if util::is_flatpak() {
        let mut script = String::new();
        for c in candidates {
            script.push_str(&format!("[ -x {} ] && {{ echo {}; exit 0; }}; ", util::shell_quote(c), util::shell_quote(c)));
        }
        script.push_str(&format!("command -v {} 2>/dev/null", util::shell_quote(name)));
        let (out, _, _) = HostCmd::new("sh").arg("-lc").arg(script).output();
        out.lines().map(|l| l.trim().to_string()).find(|l| l.starts_with('/'))
    } else {
        candidates
            .iter()
            .find(|c| executable(c))
            .cloned()
            .or_else(|| gtk::glib::find_program_in_path(name).map(|p| p.to_string_lossy().to_string()))
    };
    if let Ok(mut m) = FOUND.lock() {
        m.insert(name.to_string(), found.clone());
    }
    found
}

pub fn forget(name: &str) {
    if let Ok(mut m) = FOUND.lock() {
        m.remove(name);
    }
}

pub fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| util::home())
}

pub fn gh_binary() -> Option<String> {
    let h = home();
    find(
        "gh",
        &[
            "/usr/bin/gh".into(),
            "/usr/local/bin/gh".into(),
            "/home/linuxbrew/.linuxbrew/bin/gh".into(),
            format!("{h}/.local/bin/gh"),
            "/snap/bin/gh".into(),
        ],
    )
}

pub fn claude_binary() -> Option<String> {
    let h = home();
    find(
        "claude",
        &[
            format!("{h}/.local/bin/claude"),
            format!("{h}/.claude/local/claude"),
            "/usr/local/bin/claude".into(),
            "/usr/bin/claude".into(),
            format!("{h}/.npm-global/bin/claude"),
            "/home/linuxbrew/.linuxbrew/bin/claude".into(),
        ],
    )
}

pub fn git_binary() -> String {
    find("git", &["/usr/bin/git".into(), "/usr/local/bin/git".into()]).unwrap_or_else(|| "git".into())
}

pub fn path_with(bin: &str) -> String {
    let h = home();
    let dir = Path::new(bin).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let base = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    let base = if util::is_flatpak() { "/usr/local/bin:/usr/bin:/bin".to_string() } else { base };
    [dir, format!("{h}/.local/bin"), "/home/linuxbrew/.linuxbrew/bin".into(), "/usr/local/bin".into(), base].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(":")
}

pub fn user_shell() -> String {
    if util::is_flatpak() {
        let (out, _, ok) = HostCmd::new("sh").arg("-c").arg("getent passwd \"$(id -un)\" | cut -d: -f7").output();
        let s = out.trim().to_string();
        if ok && s.starts_with('/') {
            return s;
        }
        return "/bin/bash".into();
    }
    std::env::var("SHELL").ok().filter(|s| executable(s)).unwrap_or_else(|| if executable("/bin/bash") { "/bin/bash".into() } else { "/bin/sh".into() })
}
