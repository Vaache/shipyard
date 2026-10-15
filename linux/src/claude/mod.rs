pub mod accounts;
pub mod chat;
pub mod git;
pub mod launcher;
pub mod memory;
pub mod sessions;
pub mod workspace;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelOption {
    Default,
    Opus,
    Sonnet,
    Haiku,
    Fable,
}

impl ModelOption {
    pub const ALL: [ModelOption; 5] = [ModelOption::Default, ModelOption::Opus, ModelOption::Sonnet, ModelOption::Haiku, ModelOption::Fable];

    pub fn key(&self) -> &'static str {
        match self {
            ModelOption::Default => "",
            ModelOption::Opus => "opus",
            ModelOption::Sonnet => "sonnet",
            ModelOption::Haiku => "haiku",
            ModelOption::Fable => "fable",
        }
    }

    pub fn from_key(k: &str) -> Self {
        Self::ALL.into_iter().find(|m| m.key() == k).unwrap_or(ModelOption::Default)
    }

    pub fn title(&self) -> &'static str {
        match self {
            ModelOption::Default => "Default",
            ModelOption::Opus => "Opus",
            ModelOption::Sonnet => "Sonnet",
            ModelOption::Haiku => "Haiku",
            ModelOption::Fable => "Fable",
        }
    }
}

pub const EFFORTS: [&str; 6] = ["", "low", "medium", "high", "xhigh", "max"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClaudeMode {
    Standard,
    AcceptEdits,
    Plan,
    Auto,
}

impl ClaudeMode {
    pub const ALL: [ClaudeMode; 4] = [ClaudeMode::Standard, ClaudeMode::AcceptEdits, ClaudeMode::Plan, ClaudeMode::Auto];

    pub fn key(&self) -> &'static str {
        match self {
            ClaudeMode::Standard => "default",
            ClaudeMode::AcceptEdits => "acceptEdits",
            ClaudeMode::Plan => "plan",
            ClaudeMode::Auto => "auto",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == k)
    }

    pub fn title(&self) -> &'static str {
        match self {
            ClaudeMode::Standard => "Ask",
            ClaudeMode::AcceptEdits => "Accept edits",
            ClaudeMode::Plan => "Plan",
            ClaudeMode::Auto => "Auto",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            ClaudeMode::Standard => "sy-attention-symbolic",
            ClaudeMode::AcceptEdits => "document-edit-symbolic",
            ClaudeMode::Plan => "sy-checklist-symbolic",
            ClaudeMode::Auto => "sy-bolt-symbolic",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabKind {
    Chat,
    Terminal,
}

impl TabKind {
    pub fn key(&self) -> &'static str {
        match self {
            TabKind::Chat => "chat",
            TabKind::Terminal => "terminal",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        match k {
            "chat" => Some(TabKind::Chat),
            "terminal" => Some(TabKind::Terminal),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabState {
    Working,
    Waiting,
    Active,
    Sleeping,
    Stopped,
}

impl TabState {
    pub fn color(&self) -> &'static str {
        match self {
            TabState::Working => "sky",
            TabState::Waiting => "amber",
            TabState::Active => "green",
            TabState::Sleeping => "indigo",
            TabState::Stopped => "muted",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            TabState::Working => "sy-bolt-symbolic",
            TabState::Waiting => "sy-attention-symbolic",
            TabState::Active => "sy-dot-symbolic",
            TabState::Sleeping => "sy-moon-symbolic",
            TabState::Stopped => "sy-stop-symbolic",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            TabState::Working => "Working",
            TabState::Waiting => "Needs you",
            TabState::Active => "Active",
            TabState::Sleeping => "Sleeping",
            TabState::Stopped => "Stopped",
        }
    }

    pub fn help(&self) -> &'static str {
        match self {
            TabState::Sleeping => "Process stopped, memory freed. A new message resumes the session.",
            TabState::Active => "Process is running and uses memory. Sleeps after idle time.",
            _ => self.title(),
        }
    }
}

pub fn clean_env_keys() -> Vec<String> {
    std::env::vars().map(|(k, _)| k).filter(|k| k == &crate::brand::env_key("TOKEN") || k == "CLAUDECODE" || k == "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS" || k.starts_with("CLAUDE_CODE_")).collect()
}
