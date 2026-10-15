use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::{http_client, ApiError, ApiResult};
use crate::host::{self, HostCmd};

pub fn gh_available() -> bool {
    host::gh_binary().is_some()
}

pub fn gh_token_blocking() -> ApiResult<String> {
    let bin = host::gh_binary().ok_or_else(|| ApiError::new("GitHub CLI (gh) not found"))?;
    let (out, err, ok) = HostCmd::new(&bin).args(["auth", "token", "--hostname", "github.com"]).output();
    let token = out.trim().to_string();
    if !ok || token.is_empty() {
        let e = err.trim();
        return Err(ApiError::new(if e.is_empty() { "gh returned no token. Run gh auth login".to_string() } else { e.to_string() }));
    }
    Ok(token)
}

pub async fn gh_token() -> ApiResult<String> {
    crate::rt::blocking(gh_token_blocking).await
}

#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

pub const SCOPES: &str = "repo read:org read:project project notifications workflow";

async fn post(url: &str, params: &[(&str, &str)]) -> ApiResult<Value> {
    let resp = http_client().post(url).header("Accept", "application/json").form(params).send().await?;
    let bytes = resp.bytes().await?;
    serde_json::from_slice::<Value>(&bytes).map_err(|_| ApiError::new("Invalid response from GitHub"))
}

pub async fn device_start(client_id: String) -> ApiResult<DeviceCode> {
    let obj = post("https://github.com/login/device/code", &[("client_id", &client_id), ("scope", SCOPES)]).await?;
    if let Some(e) = obj.get("error_description").and_then(|v| v.as_str()).or_else(|| obj.get("error").and_then(|v| v.as_str())) {
        return Err(ApiError::new(e));
    }
    let s = |k: &str| obj.get(k).and_then(|v| v.as_str()).map(|s| s.to_string());
    match (s("device_code"), s("user_code"), s("verification_uri")) {
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

pub async fn device_poll(client_id: String, code: DeviceCode) -> ApiResult<String> {
    let mut interval = code.interval.max(5);
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let obj = post(
            "https://github.com/login/oauth/access_token",
            &[("client_id", &client_id), ("device_code", &code.device_code), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code")],
        )
        .await?;
        if let Some(token) = obj.get("access_token").and_then(|v| v.as_str()) {
            return Ok(token.to_string());
        }
        match obj.get("error").and_then(|v| v.as_str()) {
            Some("authorization_pending") => continue,
            Some("slow_down") => interval += 5,
            Some(e) => return Err(ApiError::new(obj.get("error_description").and_then(|v| v.as_str()).unwrap_or(e))),
            None => return Err(ApiError::new("Unexpected response from GitHub")),
        }
    }
    Err(ApiError::new("Timed out"))
}
