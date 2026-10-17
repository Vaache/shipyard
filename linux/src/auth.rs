use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::{http_client, ApiError, ApiResult};
use crate::git_accounts::{GitCredential, GitProvider};
use crate::host::{self, HostCmd};

pub fn gh_available() -> bool {
    host::gh_binary().is_some()
}

pub fn cli_available(provider: GitProvider) -> bool {
    match provider.cli_name() {
        Some("gh") => host::gh_binary().is_some(),
        Some("glab") => host::glab_binary().is_some(),
        _ => false,
    }
}

fn cli_output(bin: &str, args: &[&str], empty_message: String) -> ApiResult<String> {
    let (out, err, ok) = HostCmd::new(bin).args(args.iter().copied()).output();
    let token = out.trim().to_string();
    if !ok || token.is_empty() {
        let e = err.trim();
        return Err(ApiError::new(if e.is_empty() { empty_message } else { e.to_string() }));
    }
    Ok(token)
}

pub fn gh_token_blocking(host_name: &str) -> ApiResult<String> {
    let bin = host::gh_binary().ok_or_else(|| ApiError::new("GitHub CLI (gh) not found"))?;
    cli_output(&bin, &["auth", "token", "--hostname", host_name], format!("gh returned no token. Run gh auth login --hostname {host_name}"))
}

pub fn glab_token_blocking(host_name: &str) -> ApiResult<String> {
    let bin = host::glab_binary().ok_or_else(|| ApiError::new("GitLab CLI (glab) not found"))?;
    cli_output(&bin, &["config", "get", "token", "--host", host_name], format!("glab returned no token. Run glab auth login --hostname {host_name}"))
}

pub async fn gh_token(host_name: String) -> ApiResult<String> {
    crate::rt::blocking(move || gh_token_blocking(&host_name)).await
}

pub async fn cli_token(provider: GitProvider, host_name: String) -> ApiResult<String> {
    crate::rt::blocking(move || if provider == GitProvider::Gitlab { glab_token_blocking(&host_name) } else { gh_token_blocking(&host_name) }).await
}

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone)]
pub struct DeviceToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
}

impl DeviceToken {
    pub fn credential(&self, client_id: Option<String>) -> GitCredential {
        GitCredential {
            token: self.access_token.clone(),
            refresh_token: self.refresh_token.clone(),
            expires_at: self.expires_in.map(|s| chrono::Utc::now().timestamp() + s),
            client_id,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    pub code_url: String,
    pub token_url: String,
    pub scopes: String,
    pub service: String,
}

pub const SCOPES: &str = "repo read:org read:project project notifications workflow";

impl Endpoint {
    pub fn github(host_name: &str) -> Endpoint {
        Endpoint {
            code_url: format!("https://{host_name}/login/device/code"),
            token_url: format!("https://{host_name}/login/oauth/access_token"),
            scopes: SCOPES.into(),
            service: if host_name == "github.com" { "GitHub".into() } else { host_name.to_string() },
        }
    }

    pub fn gitlab(host_name: &str) -> Endpoint {
        Endpoint { code_url: format!("https://{host_name}/oauth/authorize_device"), token_url: format!("https://{host_name}/oauth/token"), scopes: "api".into(), service: host_name.to_string() }
    }

    pub fn for_provider(provider: GitProvider, host_name: &str) -> Endpoint {
        if provider == GitProvider::Gitlab {
            Endpoint::gitlab(host_name)
        } else {
            Endpoint::github(host_name)
        }
    }
}

async fn post(url: &str, params: &[(&str, &str)], service: &str) -> ApiResult<Value> {
    let resp = http_client().post(url).header("Accept", "application/json").form(params).send().await.map_err(crate::api::scrub)?;
    let bytes = resp.bytes().await?;
    serde_json::from_slice::<Value>(&bytes).map_err(|_| ApiError::new(format!("Invalid response from {service}")))
}

fn str_field(obj: &Value, k: &str) -> Option<String> {
    obj.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
}

pub async fn device_start(client_id: String, endpoint: Endpoint) -> ApiResult<DeviceCode> {
    let obj = post(&endpoint.code_url, &[("client_id", &client_id), ("scope", &endpoint.scopes)], &endpoint.service).await?;
    if let Some(e) = str_field(&obj, "error_description").or_else(|| str_field(&obj, "error")) {
        return Err(ApiError::new(e));
    }
    match (str_field(&obj, "device_code"), str_field(&obj, "user_code"), str_field(&obj, "verification_uri")) {
        (Some(device_code), Some(user_code), Some(verification_uri)) => Ok(DeviceCode {
            device_code,
            user_code,
            verification_uri,
            expires_in: obj.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900),
            interval: obj.get("interval").and_then(|v| v.as_u64()).unwrap_or(5),
        }),
        _ => Err(ApiError::new("Device flow is not available for this Client ID")),
    }
}

fn token_from(obj: &Value, fallback_refresh: Option<String>) -> Option<DeviceToken> {
    let access_token = str_field(obj, "access_token")?;
    Some(DeviceToken { access_token, refresh_token: str_field(obj, "refresh_token").or(fallback_refresh), expires_in: obj.get("expires_in").and_then(|v| v.as_i64()) })
}

pub async fn device_poll(client_id: String, code: DeviceCode, endpoint: Endpoint) -> ApiResult<DeviceToken> {
    let mut interval = code.interval.max(5);
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    let mut failures = 0;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let obj = match post(&endpoint.token_url, &[("client_id", &client_id), ("device_code", &code.device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")], &endpoint.service).await {
            Ok(obj) => {
                failures = 0;
                obj
            }
            Err(e) => {
                failures += 1;
                if failures >= 5 {
                    return Err(e);
                }
                continue;
            }
        };
        if let Some(token) = token_from(&obj, None) {
            return Ok(token);
        }
        match obj.get("error").and_then(|v| v.as_str()) {
            Some("authorization_pending") => continue,
            Some("slow_down") => interval += 5,
            Some(e) => return Err(ApiError::new(obj.get("error_description").and_then(|v| v.as_str()).unwrap_or(e))),
            None => return Err(ApiError::new(format!("Unexpected response from {}", endpoint.service))),
        }
    }
    Err(ApiError::new("Timed out"))
}

pub async fn refresh(client_id: &str, refresh_token: &str, endpoint: &Endpoint) -> ApiResult<DeviceToken> {
    let obj = post(&endpoint.token_url, &[("client_id", client_id), ("refresh_token", refresh_token), ("grant_type", "refresh_token")], &endpoint.service).await?;
    token_from(&obj, Some(refresh_token.to_string())).ok_or_else(|| ApiError::with_status(obj.get("error_description").and_then(|v| v.as_str()).unwrap_or("Sign-in expired. Sign in again"), 401))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_follow_the_host() {
        let gh = Endpoint::github("github.com");
        assert_eq!(gh.code_url, "https://github.com/login/device/code");
        assert_eq!(gh.service, "GitHub");
        let ghe = Endpoint::for_provider(GitProvider::GithubEnterprise, "ghe.corp");
        assert_eq!(ghe.token_url, "https://ghe.corp/login/oauth/access_token");
        let gl = Endpoint::for_provider(GitProvider::Gitlab, "gitlab.example.com");
        assert_eq!(gl.code_url, "https://gitlab.example.com/oauth/authorize_device");
        assert_eq!(gl.scopes, "api");
    }

    #[test]
    fn refresh_keeps_previous_refresh_token() {
        let obj = serde_json::json!({ "access_token": "new", "expires_in": 7200 });
        let t = token_from(&obj, Some("old".into())).unwrap();
        assert_eq!(t.refresh_token.as_deref(), Some("old"));
        let c = t.credential(Some("app".into()));
        assert_eq!(c.client_id.as_deref(), Some("app"));
        assert!(c.expires_at.unwrap() > chrono::Utc::now().timestamp());
        assert!(token_from(&serde_json::json!({ "error": "invalid_grant" }), None).is_none());
    }
}
