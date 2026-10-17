use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use crate::brand;

pub const DEFAULT_KEY: &str = "github-token";

fn attributes(key: &str) -> [(&'static str, &str); 2] {
    [("service", brand::APP_ID), ("account", key)]
}

fn fallback_path(dir: PathBuf, key: &str) -> PathBuf {
    if key == DEFAULT_KEY {
        dir.join("token")
    } else {
        dir.join(format!("token-{key}"))
    }
}

async fn keyring() -> Result<oo7::Keyring, oo7::Error> {
    let k = oo7::Keyring::new().await?;
    let _ = k.unlock().await;
    Ok(k)
}

pub async fn load(config_dir: PathBuf, key: String) -> Option<String> {
    if let Ok(k) = keyring().await {
        if let Ok(items) = k.search_items(&attributes(&key)).await {
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
    std::fs::read_to_string(fallback_path(config_dir, &key)).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Saved {
    Keyring,
    File,
    Failed,
}

pub async fn save(config_dir: PathBuf, key: String, token: String) -> Saved {
    let label = if key == DEFAULT_KEY { format!("{} GitHub token", brand::APP_NAME) } else { format!("{} Git account token", brand::APP_NAME) };
    let stored = match keyring().await {
        Ok(k) => k.create_item(&label, &attributes(&key), token.as_str(), true).await.is_ok(),
        Err(_) => false,
    };
    let path = fallback_path(config_dir.clone(), &key);
    if stored {
        let _ = std::fs::remove_file(path);
        return Saved::Keyring;
    }
    let _ = std::fs::create_dir_all(&config_dir);
    match std::fs::OpenOptions::new().create(true).write(true).truncate(true).mode(0o600).open(&path) {
        Ok(mut f) => {
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            if f.write_all(token.as_bytes()).is_ok() {
                Saved::File
            } else {
                Saved::Failed
            }
        }
        Err(_) => Saved::Failed,
    }
}

pub async fn delete(config_dir: PathBuf, key: String) {
    if let Ok(k) = keyring().await {
        let _ = k.delete(&attributes(&key)).await;
    }
    let _ = std::fs::remove_file(fallback_path(config_dir, &key));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_key_keeps_its_fallback_file() {
        let dir = PathBuf::from("/cfg");
        assert_eq!(fallback_path(dir.clone(), DEFAULT_KEY), PathBuf::from("/cfg/token"));
        assert_eq!(fallback_path(dir, "account-1"), PathBuf::from("/cfg/token-account-1"));
    }
}
