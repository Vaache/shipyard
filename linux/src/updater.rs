use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use chrono::Utc;
use serde::Deserialize;

use crate::api::{http_client, ApiError};
use crate::brand;
use crate::settings;
use crate::signal::Signal;

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub version: String,
    pub name: String,
    pub notes: String,
    pub page_url: String,
    pub published_at: Option<chrono::DateTime<Utc>>,
    pub asset_url: Option<String>,
    pub asset_size: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(f64),
    Installing,
    ReadyToRelaunch,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallKind {
    Flatpak,
    Package,
    Portable,
}

pub fn install_kind() -> InstallKind {
    if crate::util::is_flatpak() {
        return InstallKind::Flatpak;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let s = exe.to_string_lossy();
    if s.starts_with("/usr/") || s.starts_with("/opt/") || s.starts_with("/snap/") {
        return InstallKind::Package;
    }
    if Path::new(&format!("/var/lib/dpkg/info/{}.list", brand::BIN_NAME)).exists() && s.starts_with("/usr") {
        return InstallKind::Package;
    }
    InstallKind::Portable
}

pub const CHECK_INTERVAL: i64 = 6 * 3600;

pub struct Updater {
    pub state: State,
    pub release: Option<Release>,
    pub staged: Option<PathBuf>,
    launch_done: bool,
}

thread_local! {
    static UPDATER: Rc<RefCell<Updater>> = Rc::new(RefCell::new(Updater { state: State::Idle, release: None, staged: None, launch_done: false }));
    static CHANGED: Signal<()> = Signal::new();
    static PRESENT: Signal<()> = Signal::new();
}

pub fn shared() -> Rc<RefCell<Updater>> {
    UPDATER.with(|u| u.clone())
}

pub fn state() -> State {
    UPDATER.with(|u| u.borrow().state.clone())
}

pub fn release() -> Option<Release> {
    UPDATER.with(|u| u.borrow().release.clone())
}

pub fn on_change(f: impl Fn(&()) -> bool + 'static) {
    CHANGED.with(|s| s.connect(f));
}

pub fn on_present(f: impl Fn(&()) -> bool + 'static) {
    PRESENT.with(|s| s.connect(f));
}

pub fn present() {
    PRESENT.with(|s| s.emit(&()));
}

fn set_state(state: State) {
    UPDATER.with(|u| u.borrow_mut().state = state);
    CHANGED.with(|s| s.emit(&()));
}

pub fn current_version() -> &'static str {
    brand::VERSION
}

pub fn available_version() -> Option<String> {
    match state() {
        State::Available(r) => Some(r.version),
        _ => None,
    }
}

pub fn is_busy() -> bool {
    matches!(state(), State::Checking | State::Downloading(_) | State::Installing)
}

pub fn page_url() -> String {
    release().map(|r| r.page_url).unwrap_or_else(brand::releases_page)
}

pub fn version_components(v: &str) -> Vec<i64> {
    let s = v.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next().unwrap_or(s);
    core.split('.').map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<i64>().unwrap_or(0)).collect()
}

pub fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    let pa = version_components(a);
    let pb = version_components(b);
    for i in 0..pa.len().max(pb.len()) {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

pub fn parse_release(data: &[u8]) -> Result<Release, ApiError> {
    #[derive(Deserialize)]
    struct Asset {
        name: String,
        browser_download_url: String,
        size: i64,
    }
    #[derive(Deserialize)]
    struct Payload {
        tag_name: String,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        body: Option<String>,
        html_url: String,
        #[serde(default)]
        published_at: Option<chrono::DateTime<Utc>>,
        #[serde(default)]
        assets: Vec<Asset>,
    }
    let p: Payload = serde_json::from_slice(data).map_err(|e| ApiError::new(format!("Couldn't read the release: {e}")))?;
    let wanted = brand::tarball_asset_name();
    let asset = p.assets.iter().find(|a| a.name == wanted);
    let version = p.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let name = p.name.unwrap_or_default().trim().to_string();
    Ok(Release {
        name: if name.is_empty() { format!("{} {version}", brand::APP_NAME) } else { name },
        version,
        notes: p.body.unwrap_or_default(),
        page_url: p.html_url,
        published_at: p.published_at,
        asset_url: asset.map(|a| a.browser_download_url.clone()),
        asset_size: asset.map(|a| a.size).unwrap_or(0),
    })
}

async fn fetch_latest() -> Result<Release, ApiError> {
    let resp = http_client().get(brand::latest_release_api()).header("Accept", "application/vnd.github+json").send().await?;
    let code = resp.status().as_u16();
    if code == 404 {
        return Err(ApiError::new("No releases found."));
    }
    if !(200..300).contains(&code) {
        return Err(ApiError::new(if code == 403 || code == 429 { "GitHub rate limit reached. Try again later.".to_string() } else { format!("GitHub responded with error {code}.") }));
    }
    let bytes = resp.bytes().await?;
    parse_release(&bytes)
}

pub async fn check_on_launch() {
    let done = UPDATER.with(|u| {
        let mut u = u.borrow_mut();
        let d = u.launch_done;
        u.launch_done = true;
        d
    });
    if done {
        return;
    }
    start_periodic();
    if !settings::read(|s| s.update_auto_check) {
        return;
    }
    if let Some(last) = settings::read(|s| s.update_last_check) {
        if Utc::now().timestamp() - last < CHECK_INTERVAL {
            return;
        }
    }
    check(false).await;
}

fn start_periodic() {
    gtk::glib::timeout_add_seconds_local(3600, || {
        if settings::read(|s| s.update_auto_check) {
            let due = settings::read(|s| s.update_last_check).map(|l| Utc::now().timestamp() - l >= CHECK_INTERVAL).unwrap_or(true);
            if due && !matches!(state(), State::Available(_)) {
                crate::rt::spawn_local(check(false));
            }
        }
        gtk::glib::ControlFlow::Continue
    });
}

pub async fn check(user_initiated: bool) {
    match state() {
        State::Checking | State::Downloading(_) | State::Installing | State::ReadyToRelaunch => {
            if user_initiated {
                present();
            }
            return;
        }
        _ => {}
    }
    set_state(State::Checking);
    match crate::rt::run(fetch_latest()).await {
        Ok(latest) => {
            settings::update(|s| s.update_last_check = Some(Utc::now().timestamp()));
            UPDATER.with(|u| u.borrow_mut().release = Some(latest.clone()));
            if compare(&latest.version, current_version()) == std::cmp::Ordering::Greater {
                set_state(State::Available(latest.clone()));
                let (skipped, announced) = settings::read(|s| (s.update_skipped.clone(), s.update_announced.clone()));
                if user_initiated {
                    present();
                } else if skipped.as_deref() != Some(&latest.version) && announced.as_deref() != Some(&latest.version) {
                    let v = latest.version.clone();
                    settings::update(|s| s.update_announced = Some(v));
                    if crate::app::is_active() {
                        present();
                    } else {
                        crate::app::notify_update(&latest.version);
                    }
                }
            } else {
                set_state(State::UpToDate);
                if user_initiated {
                    present();
                }
            }
        }
        Err(e) => {
            set_state(State::Failed(e.message));
            if user_initiated {
                present();
            }
        }
    }
}

pub fn skip() {
    if let Some(r) = release() {
        settings::update(|s| s.update_skipped = Some(r.version.clone()));
        set_state(State::Idle);
    }
}

pub fn later() {
    if matches!(state(), State::UpToDate | State::Failed(_)) {
        set_state(State::Idle);
    }
}

pub fn can_self_install() -> bool {
    if install_kind() != InstallKind::Portable {
        return false;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let Some(dir) = exe.parent() else { return false };
    let probe = dir.join(format!(".{}-write-test", brand::BIN_NAME));
    let ok = std::fs::write(&probe, b"x").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

fn staging_dir() -> PathBuf {
    settings::cache_dir().join("updates")
}

async fn download(url: String, dest: PathBuf, expected: i64, progress: async_channel::Sender<f64>) -> Result<PathBuf, String> {
    use tokio::io::AsyncWriteExt;
    let resp = http_client().get(&url).header("Accept", "application/octet-stream").timeout(std::time::Duration::from_secs(600)).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let total = resp.content_length().map(|v| v as i64).unwrap_or(expected).max(1);
    let mut file = tokio::fs::File::create(&dest).await.map_err(|e| e.to_string())?;
    let mut written = 0i64;
    let mut last = -1.0f64;
    let mut resp = resp;
    while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        written += chunk.len() as i64;
        let f = (written as f64 / total as f64).min(1.0);
        if f - last >= 0.005 || f >= 1.0 {
            last = f;
            let _ = progress.send(f).await;
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    Ok(dest)
}

fn stage(archive: PathBuf, version: String) -> Result<PathBuf, String> {
    let root = staging_dir().join("extract");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("tar").arg("xzf").arg(&archive).arg("-C").arg(&root).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let mut found: Option<PathBuf> = None;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.file_name().map(|n| n == brand::BIN_NAME).unwrap_or(false) {
                    found = Some(p);
                }
            }
        }
    }
    let bin = found.ok_or_else(|| format!("{} wasn't found in the archive.", brand::BIN_NAME))?;
    let check = std::process::Command::new(&bin).arg("--version").output().map_err(|e| e.to_string())?;
    let reported = String::from_utf8_lossy(&check.stdout).to_string();
    if !reported.contains(&version) {
        return Err(format!("Expected version {version}, found {}.", reported.trim()));
    }
    Ok(bin)
}

pub async fn download_and_install() {
    let Some(release) = release() else { return };
    if is_busy() {
        return;
    }
    let Some(url) = release.asset_url.clone() else {
        set_state(State::Failed(format!("The release has no {} file.", brand::tarball_asset_name())));
        present();
        return;
    };
    set_state(State::Downloading(0.0));
    let dir = staging_dir();
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        set_state(State::Failed(e.to_string()));
        present();
        return;
    }
    let dest = dir.join(brand::tarball_asset_name());
    let (tx, rx) = async_channel::unbounded::<f64>();
    crate::rt::spawn_local(async move {
        while let Ok(p) = rx.recv().await {
            if matches!(state(), State::Downloading(_)) {
                set_state(State::Downloading(p));
            }
        }
    });
    let size = release.asset_size;
    let result = crate::rt::run(download(url, dest, size, tx)).await;
    let archive = match result {
        Ok(p) => p,
        Err(e) => {
            set_state(State::Failed(format!("Couldn't download the update: {e}")));
            present();
            return;
        }
    };
    set_state(State::Installing);
    let version = release.version.clone();
    match crate::rt::blocking(move || stage(archive, version)).await {
        Ok(bin) => {
            UPDATER.with(|u| u.borrow_mut().staged = Some(bin));
            set_state(State::ReadyToRelaunch);
        }
        Err(e) => set_state(State::Failed(format!("Couldn't install the update: {e}"))),
    }
    present();
}

pub fn relaunch() {
    let staged = UPDATER.with(|u| u.borrow().staged.clone());
    let Some(staged) = staged else { return };
    let Ok(exe) = std::env::current_exe() else { return };
    let tmp = exe.with_extension("update");
    let result = std::fs::copy(&staged, &tmp).and_then(|_| {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&tmp, &exe)
    });
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        set_state(State::Failed(format!("Couldn't install the update: {e}")));
        present();
        return;
    }
    let _ = std::fs::remove_dir_all(staging_dir());
    let _ = std::process::Command::new(&exe).spawn();
    crate::app::quit();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(compare("2.10.0", "2.9.9"), std::cmp::Ordering::Greater);
        assert_eq!(compare("v2.5", "2.5.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare("2.5.0-beta", "2.5.1"), std::cmp::Ordering::Less);
    }

    #[test]
    fn parses_release() {
        let name = brand::tarball_asset_name();
        let json = format!("{{\"tag_name\":\"v9.0.0\",\"name\":\"\",\"body\":\"notes\",\"html_url\":\"https://x\",\"published_at\":\"2026-01-01T00:00:00Z\",\"assets\":[{{\"name\":\"{name}\",\"browser_download_url\":\"https://d\",\"size\":10}}]}}");
        let r = parse_release(json.as_bytes()).unwrap();
        assert_eq!(r.version, "9.0.0");
        assert_eq!(r.asset_url.as_deref(), Some("https://d"));
    }
}
