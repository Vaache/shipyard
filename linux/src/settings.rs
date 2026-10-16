use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::brand;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ClaudeProfile {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub config_dir: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
}

impl ClaudeProfile {
    pub fn is_default(&self) -> bool {
        self.config_dir.is_none()
    }

    pub fn initial(&self) -> String {
        let base = if self.label.is_empty() { self.email.clone().unwrap_or_else(|| "C".into()) } else { self.label.clone() };
        base.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "C".into())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub poll_interval: f64,
    pub oauth_client_id: String,
    pub theme_style: String,
    pub appearance: String,
    pub link_browser: String,
    pub update_auto_check: bool,
    pub update_last_check: Option<i64>,
    pub update_skipped: Option<String>,
    pub update_announced: Option<String>,
    pub claude_model: String,
    pub claude_effort: String,
    pub claude_mode: String,
    pub claude_idle_minutes: i64,
    pub claude_ui: String,
    pub claude_target: String,
    pub claude_launch_mode: String,
    pub claude_profiles: Vec<ClaudeProfile>,
    pub claude_active_profile: String,
    pub local_repo_paths: HashMap<String, String>,
    pub run_in_background: bool,
    pub notify_review_requests: bool,
    pub diagnostics_enabled: bool,
    pub window_width: i32,
    pub window_height: i32,
    pub window_maximized: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            poll_interval: 30.0,
            oauth_client_id: String::new(),
            theme_style: "pullyard".into(),
            appearance: "auto".into(),
            link_browser: "system".into(),
            update_auto_check: true,
            update_last_check: None,
            update_skipped: None,
            update_announced: None,
            claude_model: String::new(),
            claude_effort: String::new(),
            claude_mode: "default".into(),
            claude_idle_minutes: 15,
            claude_ui: "chat".into(),
            claude_target: "app".into(),
            claude_launch_mode: "default".into(),
            claude_profiles: Vec::new(),
            claude_active_profile: "default".into(),
            local_repo_paths: HashMap::new(),
            run_in_background: false,
            notify_review_requests: true,
            diagnostics_enabled: false,
            window_width: 1380,
            window_height: 860,
            window_maximized: false,
        }
    }
}

thread_local! {
    static SETTINGS: RefCell<Settings> = RefCell::new(load());
}

pub fn config_dir() -> PathBuf {
    gtk::glib::user_config_dir().join(brand::BIN_NAME)
}

pub fn data_dir() -> PathBuf {
    gtk::glib::user_data_dir().join(brand::BIN_NAME)
}

pub fn cache_dir() -> PathBuf {
    gtk::glib::user_cache_dir().join(brand::BIN_NAME)
}

fn path() -> PathBuf {
    config_dir().join("settings.json")
}

fn load() -> Settings {
    let Ok(data) = std::fs::read(path()) else {
        return Settings::default();
    };
    match serde_json::from_slice::<Settings>(&data) {
        Ok(settings) => settings,
        Err(_) => {
            let _ = std::fs::rename(path(), config_dir().join("settings.json.corrupt"));
            Settings::default()
        }
    }
}

fn save(s: &Settings) {
    let dir = config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(data) = serde_json::to_vec_pretty(s) {
        let tmp = dir.join("settings.json.tmp");
        let written = std::fs::File::create(&tmp).and_then(|mut f| {
            std::io::Write::write_all(&mut f, &data)?;
            f.sync_all()
        });
        if written.is_ok() {
            let _ = std::fs::rename(&tmp, path());
        }
    }
}

pub fn read<T>(f: impl FnOnce(&Settings) -> T) -> T {
    SETTINGS.with(|s| f(&s.borrow()))
}

pub fn update(f: impl FnOnce(&mut Settings)) {
    SETTINGS.with(|s| {
        let mut s = s.borrow_mut();
        f(&mut s);
        save(&s);
    });
}

pub fn refresh_interval() -> f64 {
    read(|s| s.poll_interval).max(15.0)
}
