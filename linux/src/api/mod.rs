pub mod endpoints;
pub mod models;
pub mod queries;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::brand;

#[derive(Debug, Clone)]
pub struct ApiError {
    pub message: String,
    pub status: u16,
}

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        ApiError { message: message.into(), status: 0 }
    }

    pub fn with_status(message: impl Into<String>, status: u16) -> Self {
        ApiError { message: message.into(), status }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            return ApiError::new("The request timed out");
        }
        if e.is_connect() {
            return ApiError::new("Could not connect to GitHub");
        }
        ApiError::new(e.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

struct Inner {
    token: String,
    client: Client,
    no_redirect: Client,
    plain: Client,
}

#[derive(Clone)]
pub struct GitHubApi {
    inner: Arc<Inner>,
}

fn reason(status: StatusCode) -> String {
    status.canonical_reason().unwrap_or("error").to_string()
}

pub fn check_status(status: StatusCode, body: &[u8]) -> ApiResult<()> {
    if status.is_success() {
        return Ok(());
    }
    if status == StatusCode::UNAUTHORIZED {
        return Err(ApiError::with_status("Token is invalid or revoked (401)", 401));
    }
    let msg = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(|m| m.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| reason(status));
    Err(ApiError::with_status(format!("GitHub {}: {}", status.as_u16(), msg), status.as_u16()))
}

fn describe_graphql_error(e: &Value) -> String {
    let message = e.get("message").and_then(|m| m.as_str()).unwrap_or("GraphQL error").to_string();
    if e.get("type").and_then(|t| t.as_str()) == Some("INSUFFICIENT_SCOPES") {
        format!("Insufficient token scopes: {message}")
    } else {
        message
    }
}

fn strip_nulls(vars: Value) -> Value {
    match vars {
        Value::Object(map) => Value::Object(map.into_iter().filter(|(_, v)| !v.is_null()).collect()),
        other => other,
    }
}

impl GitHubApi {
    pub fn new(token: &str) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("X-GitHub-Api-Version", HeaderValue::from_static("2022-11-28"));
        let agent = brand::user_agent();
        let client = Client::builder()
            .user_agent(agent.clone())
            .default_headers(headers.clone())
            .timeout(Duration::from_secs(40))
            .build()
            .expect("http client");
        let no_redirect = Client::builder()
            .user_agent(agent.clone())
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(40))
            .build()
            .expect("http client");
        let plain = Client::builder().user_agent(agent).timeout(Duration::from_secs(60)).build().expect("http client");
        GitHubApi { inner: Arc::new(Inner { token: token.to_string(), client, no_redirect, plain }) }
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.inner.token)
    }

    fn url(path: &str) -> String {
        if path.starts_with("http") {
            path.to_string()
        } else {
            format!("https://api.github.com{path}")
        }
    }

    pub async fn graphql<T: DeserializeOwned>(&self, query: &str, variables: Value, strict: bool) -> ApiResult<T> {
        let body = serde_json::json!({ "query": query, "variables": strip_nulls(variables) });
        let resp = self
            .inner
            .client
            .post("https://api.github.com/graphql")
            .header(AUTHORIZATION, self.auth())
            .header(ACCEPT, "application/vnd.github+json")
            .header(CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        check_status(status, &bytes)?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| ApiError::new(format!("Failed to parse response: {e}")))?;
        let errors = value.get("errors").and_then(|e| e.as_array()).cloned().unwrap_or_default();
        let data = value.get("data").cloned().unwrap_or(Value::Null);
        if let Some(first) = errors.first() {
            if strict || data.is_null() {
                return Err(ApiError::new(describe_graphql_error(first)));
            }
        }
        if data.is_null() {
            return Err(ApiError::new("Empty response from GitHub"));
        }
        serde_json::from_value::<T>(data).map_err(|e| match errors.first() {
            Some(first) => ApiError::new(describe_graphql_error(first)),
            None => ApiError::new(format!("Failed to parse response: {e}")),
        })
    }

    pub async fn mutate(&self, query: &str, vars: Value) -> ApiResult<()> {
        let _: Value = self.graphql(query, vars, true).await?;
        Ok(())
    }

    pub async fn rest_raw(&self, method: Method, path: &str, body: Option<Value>) -> ApiResult<(Vec<u8>, HeaderMap)> {
        let mut req = self.inner.client.request(method, Self::url(path)).header(AUTHORIZATION, self.auth()).header(ACCEPT, "application/vnd.github+json");
        if let Some(body) = body {
            req = req.json(&body);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let bytes = resp.bytes().await?.to_vec();
        check_status(status, &bytes)?;
        Ok((bytes, headers))
    }

    pub async fn rest<T: DeserializeOwned>(&self, method: Method, path: &str, body: Option<Value>) -> ApiResult<T> {
        let (bytes, _) = self.rest_raw(method, path, body).await?;
        serde_json::from_slice(&bytes).map_err(|e| ApiError::new(format!("Failed to parse response: {e}")))
    }

    pub async fn send(&self, method: Method, path: &str, body: Option<Value>) -> ApiResult<()> {
        self.rest_raw(method, path, body).await.map(|_| ())
    }

    pub async fn redirect_target(&self, path: &str) -> ApiResult<String> {
        let resp = self
            .inner
            .no_redirect
            .get(Self::url(path))
            .header(AUTHORIZATION, self.auth())
            .header(ACCEPT, "application/vnd.github+json")
            .send()
            .await?;
        let status = resp.status();
        if status.is_redirection() {
            if let Some(loc) = resp.headers().get("location").and_then(|v| v.to_str().ok()) {
                return Ok(loc.to_string());
            }
        }
        let bytes = resp.bytes().await?;
        check_status(status, &bytes)?;
        Err(ApiError::new("Log unavailable"))
    }

    pub async fn plain(&self, url: &str) -> ApiResult<String> {
        let resp = self.inner.plain.get(url).send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        check_status(status, &bytes)?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    pub async fn fetch(&self, path: &str, accept: &str) -> ApiResult<Option<Vec<u8>>> {
        let resp = self.inner.client.get(Self::url(path)).header(AUTHORIZATION, self.auth()).header(ACCEPT, accept).send().await?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(ApiError::with_status(format!("GitHub {}", status.as_u16()), status.as_u16()));
        }
        Ok(Some(resp.bytes().await?.to_vec()))
    }
}

static SHARED: std::sync::LazyLock<Client> =
    std::sync::LazyLock::new(|| Client::builder().user_agent(brand::user_agent()).timeout(Duration::from_secs(30)).build().expect("http client"));

pub fn http_client() -> Client {
    SHARED.clone()
}
