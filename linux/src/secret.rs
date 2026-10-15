use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

use crate::brand;

const ACCOUNT: &str = "github-token";

fn attributes() -> [(&'static str, &'static str); 2] {
    [("service", brand::APP_ID), ("account", ACCOUNT)]
}

fn fallback_path(dir: PathBuf) -> PathBuf {
    dir.join("token")
}

async fn keyring() -> Result<oo7::Keyring, oo7::Error> {
    let k = oo7::Keyring::new().await?;
    let _ = k.unlock().await;
    Ok(k)
}

pub async fn load(config_dir: PathBuf) -> Option<String> {
    if let Ok(k) = keyring().await {
        if let Ok(items) = k.search_items(&attributes()).await {
            for item in items {
                if let Ok(secret) = item.secret().await {
                    let s = String::from_utf8_lossy(secret.as_bytes()).trim().to_string();
                    if !s.is_empty() {
                        return Some(s);
                    }
                }
            }
        }
    }
    std::fs::read_to_string(fallback_path(config_dir)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub async fn save(config_dir: PathBuf, token: String) -> bool {
    let label = format!("{} GitHub token", brand::APP_NAME);
    let stored = match keyring().await {
        Ok(k) => k.create_item(&label, &attributes(), token.as_str(), true).await.is_ok(),
        Err(_) => false,
    };
    let path = fallback_path(config_dir.clone());
    if stored {
        let _ = std::fs::remove_file(path);
        return true;
    }
    let _ = std::fs::create_dir_all(&config_dir);
    match std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&path) {
        Ok(mut f) => f.write_all(token.as_bytes()).is_ok(),
        Err(_) => false,
    }
}

pub async fn delete(config_dir: PathBuf) {
    if let Ok(k) = keyring().await {
        let _ = k.delete(&attributes()).await;
    }
    let _ = std::fs::remove_file(fallback_path(config_dir));
}
