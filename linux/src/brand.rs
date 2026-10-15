pub const APP_NAME: &str = env!("BRAND_APP_NAME");
pub const APP_ID: &str = env!("BRAND_APP_ID");
pub const BIN_NAME: &str = env!("BRAND_BIN_NAME");
pub const GITHUB_OWNER: &str = env!("BRAND_GITHUB_OWNER");
pub const GITHUB_REPO: &str = env!("BRAND_GITHUB_REPO");
pub const URL_SCHEME: &str = env!("BRAND_URL_SCHEME");
pub const ENV_PREFIX: &str = env!("BRAND_ENV_PREFIX");
pub const TAGLINE: &str = env!("BRAND_APP_SUMMARY");
pub const VERSION: &str = env!("BRAND_VERSION");

pub fn user_agent() -> String {
    format!("{APP_NAME}-Linux/{VERSION}")
}

pub fn repo_url() -> String {
    format!("https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}")
}

pub fn releases_page() -> String {
    format!("{}/releases/latest", repo_url())
}

pub fn latest_release_api() -> String {
    format!("https://api.github.com/repos/{GITHUB_OWNER}/{GITHUB_REPO}/releases/latest")
}

pub fn env_var(name: &str) -> Option<String> {
    std::env::var(format!("{ENV_PREFIX}_{name}")).ok().filter(|v| !v.is_empty())
}

pub fn env_key(name: &str) -> String {
    format!("{ENV_PREFIX}_{name}")
}

pub fn arch() -> &'static str {
    std::env::consts::ARCH
}

pub fn tarball_asset_name() -> String {
    format!("{APP_NAME}-linux-{}.tar.gz", arch())
}
