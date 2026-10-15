use std::os::unix::fs::PermissionsExt;

use crate::host::{self, HostCmd};
use crate::util;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    App,
    Terminal,
    Copy,
}

impl Target {
    pub const ALL: [Target; 3] = [Target::App, Target::Terminal, Target::Copy];

    pub fn key(&self) -> &'static str {
        match self {
            Target::App => "app",
            Target::Terminal => "terminal",
            Target::Copy => "copy",
        }
    }

    pub fn from_key(k: &str) -> Self {
        Self::ALL.into_iter().find(|t| t.key() == k).unwrap_or(Target::App)
    }

    pub fn title(&self) -> &'static str {
        match self {
            Target::App => "In this app",
            Target::Terminal => "Terminal",
            Target::Copy => "Copy only",
        }
    }

    pub fn available(&self) -> bool {
        match self {
            Target::App => host::claude_binary().is_some(),
            Target::Terminal => terminal_emulator().is_some(),
            Target::Copy => true,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PermissionMode {
    Standard,
    AcceptEdits,
    Plan,
}

impl PermissionMode {
    pub const ALL: [PermissionMode; 3] = [PermissionMode::Standard, PermissionMode::AcceptEdits, PermissionMode::Plan];

    pub fn key(&self) -> &'static str {
        match self {
            PermissionMode::Standard => "default",
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Plan => "plan",
        }
    }

    pub fn from_key(k: &str) -> Self {
        Self::ALL.into_iter().find(|t| t.key() == k).unwrap_or(PermissionMode::Standard)
    }

    pub fn title(&self) -> &'static str {
        match self {
            PermissionMode::Standard => "Default (ask)",
            PermissionMode::AcceptEdits => "Accept edits",
            PermissionMode::Plan => "Plan first",
        }
    }
}

pub fn terminal_emulator() -> Option<(String, Vec<String>)> {
    let candidates: [(&str, &[&str]); 10] = [
        ("x-terminal-emulator", &["-e"]),
        ("kgx", &["--"]),
        ("gnome-terminal", &["--"]),
        ("ptyxis", &["--"]),
        ("konsole", &["-e"]),
        ("xfce4-terminal", &["-x"]),
        ("tilix", &["-e"]),
        ("alacritty", &["-e"]),
        ("kitty", &[]),
        ("xterm", &["-e"]),
    ];
    for (name, args) in candidates {
        if host::find(name, &[format!("/usr/bin/{name}")]).is_some() {
            return Some((name.to_string(), args.iter().map(|s| s.to_string()).collect()));
        }
    }
    None
}

fn write_private(path: &str, content: &str, mode: u32) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(path).map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes()).map_err(|e| e.to_string())
}

pub fn launch(prompt: &str, folder: &str, target: Target, mode: PermissionMode, session_name: &str, resume_id: Option<&str>, config_dir: Option<&str>) -> Result<(), String> {
    if target == Target::Copy {
        util::copy(prompt);
        return Ok(());
    }
    let (term, term_args) = terminal_emulator().ok_or_else(|| "No terminal emulator found".to_string())?;
    let dir = format!("{}/.cache/{}/claude", host::home(), crate::brand::BIN_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    let id: String = uuid::Uuid::new_v4().to_string().chars().take(8).collect();
    let prompt_path = format!("{dir}/prompt-{id}.md");
    let script_path = format!("{dir}/fix-{id}.sh");
    write_private(&prompt_path, prompt, 0o600)?;
    let config_export = config_dir.map(|d| format!("export CLAUDE_CONFIG_DIR={}", util::shell_quote(d))).unwrap_or_default();
    let mut args = String::new();
    if mode != PermissionMode::Standard {
        args.push_str(&format!(" --permission-mode {}", mode.key()));
    }
    match resume_id {
        Some(r) => args.push_str(&format!(" --resume {}", util::shell_quote(r))),
        None => args.push_str(&format!(" -n {}", util::shell_quote(session_name))),
    }
    let script = format!(
        "#!/bin/bash\nexport PATH=\"$HOME/.local/bin:$HOME/.claude/local:/home/linuxbrew/.linuxbrew/bin:/usr/local/bin:$PATH\"\nunset CLAUDECODE $(env | grep -o '^CLAUDE_CODE_[A-Z_]*')\n{config_export}\ncd {} || {{ printf '%s\\n' {}; read -r; exit 1; }}\nPROMPT=\"$(cat {})\"\nrm -f {} \"$0\"\nclear\nif ! command -v claude >/dev/null 2>&1; then echo \"claude CLI not found in PATH\"; read -r; exit 1; fi\nif [ -n \"$PROMPT\" ]; then claude{args} -- \"$PROMPT\"; else claude{args}; fi\nexec \"${{SHELL:-/bin/bash}}\"\n",
        util::shell_quote(folder),
        util::shell_quote(&format!("Folder not found: {folder}")),
        util::shell_quote(&prompt_path),
        util::shell_quote(&prompt_path),
    );
    write_private(&script_path, &script, 0o700)?;
    let mut cmd = HostCmd::new(&term).cwd(folder);
    for a in term_args {
        cmd = cmd.arg(a);
    }
    cmd = cmd.arg("/bin/bash").arg(script_path);
    cmd.command().spawn().map(|_| ()).map_err(|e| e.to_string())
}
