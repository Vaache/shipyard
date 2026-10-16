pub mod endpoints;
pub mod models;
pub mod queries;

use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED};
use reqwest::{Client, Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::brand;
use crate::diagnostics::{self, Probe};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const RETRY_DELAY: Duration = Duration::from_millis(750);
const RATE_LIMIT_FALLBACK_SECS: i64 = 60;
const RATE_LIMIT_MAX_PAUSE_SECS: i64 = 3600;
const LIVE_RESOURCES: [&str; 2] = ["core", "graphql"];
const RETRY_NOTE: &str = "retry";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorKind {
    #[default]
    Other,
    Network,
    Schema,
    RateLimited,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub message: String,
    pub status: u16,
    pub kind: ErrorKind,
}

impl ApiError {
    pub fn new(message: impl Into<String>) -> Self {
        ApiError { message: message.into(), status: 0, kind: ErrorKind::Other }
    }

    pub fn with_status(message: impl Into<String>, status: u16) -> Self {
        ApiError { message: message.into(), status, kind: ErrorKind::Other }
    }

    pub fn with_kind(mut self, kind: ErrorKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn is_transient(&self) -> bool {
        self.kind == ErrorKind::Network || matches!(self.status, 502..=504)
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

fn connection_dropped(e: &reqwest::Error) -> bool {
    let mut source = Some(e as &(dyn std::error::Error + 'static));
    while let Some(err) = source {
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            if matches!(io.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted | std::io::ErrorKind::BrokenPipe) {
                return true;
            }
        }
        let text = err.to_string().to_ascii_lowercase();
        if text.contains("connection reset") || text.contains("connection closed before message completed") {
            return true;
        }
        source = err.source();
    }
    false
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_connect() {
            return ApiError::new(if e.is_timeout() { "The request timed out" } else { "Could not connect to GitHub" }).with_kind(ErrorKind::Network);
        }
        if e.is_timeout() {
            return ApiError::new("The request timed out");
        }
        let kind = if connection_dropped(&e) { ErrorKind::Network } else { ErrorKind::Other };
        ApiError::new(e.to_string()).with_kind(kind)
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Clone, Default)]
struct Validators {
    last_modified: Option<String>,
    etag: Option<String>,
}

struct Polled {
    validators: Validators,
    body: Vec<u8>,
    checked: Instant,
    interval: Duration,
}

#[derive(Default)]
struct PollCache {
    generation: u64,
    entries: HashMap<String, Polled>,
}

struct Inner {
    token: String,
    client: Client,
    no_redirect: Client,
    plain: Client,
    polled: Mutex<PollCache>,
}

#[derive(Clone)]
pub struct GitHubApi {
    inner: Arc<Inner>,
}

struct Exchange {
    status: StatusCode,
    bytes: Vec<u8>,
    headers: HeaderMap,
    probe: Probe,
}

static RATE_LIMITED_UNTIL: AtomicI64 = AtomicI64::new(0);

pub fn rate_limited_until() -> Option<DateTime<Utc>> {
    let until = RATE_LIMITED_UNTIL.load(Ordering::Relaxed);
    if until > Utc::now().timestamp() {
        DateTime::from_timestamp(until, 0)
    } else {
        None
    }
}

pub fn reset_rate_limit() {
    RATE_LIMITED_UNTIL.store(0, Ordering::Relaxed);
}

fn pause_until(ts: i64) {
    let now = Utc::now().timestamp();
    let ts = ts.min(now + RATE_LIMIT_MAX_PAUSE_SECS);
    let _ = RATE_LIMITED_UNTIL.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| if cur > now && cur >= ts { None } else { Some(ts) });
}

fn header_i64(headers: &HeaderMap, key: &str) -> Option<i64> {
    headers.get(key).and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse().ok())
}

fn mentions_rate_limit(body: &[u8]) -> bool {
    String::from_utf8_lossy(body).to_ascii_lowercase().contains("rate limit")
}

pub fn rate_limit_resume(status: u16, headers: &HeaderMap, body: &[u8], now: i64) -> Option<i64> {
    let remaining = header_i64(headers, "x-ratelimit-remaining");
    let reset = header_i64(headers, "x-ratelimit-reset").filter(|r| *r > now);
    let retry_after = header_i64(headers, "retry-after");
    let live_resource = headers.get("x-ratelimit-resource").and_then(|v| v.to_str().ok()).is_none_or(|r| LIVE_RESOURCES.contains(&r.trim()));
    let exhausted = remaining == Some(0);
    if exhausted && !live_resource && retry_after.is_none() {
        return None;
    }
    let limited = match status {
        429 => true,
        403 => exhausted || retry_after.is_some() || mentions_rate_limit(body),
        _ => false,
    };
    let resume = if !limited {
        if exhausted {
            reset
        } else {
            None
        }
    } else {
        Some(match (retry_after, exhausted, reset) {
            (Some(secs), _, _) => now + secs.max(1),
            (None, true, Some(r)) => r,
            _ => now + RATE_LIMIT_FALLBACK_SECS,
        })
    };
    resume.map(|ts| ts.min(now + RATE_LIMIT_MAX_PAUSE_SECS))
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

fn checked(status: StatusCode, headers: &HeaderMap, body: &[u8]) -> ApiResult<()> {
    let resume = rate_limit_resume(status.as_u16(), headers, body, Utc::now().timestamp());
    if let Some(ts) = resume {
        pause_until(ts);
    }
    check_status(status, body).map_err(|e| if resume.is_some() && matches!(e.status, 403 | 429) { e.with_kind(ErrorKind::RateLimited) } else { e })
}

fn describe_graphql_error(e: &Value) -> String {
    let message = e.get("message").and_then(|m| m.as_str()).unwrap_or("GraphQL error").to_string();
    if e.get("type").and_then(|t| t.as_str()) == Some("INSUFFICIENT_SCOPES") {
        format!("Insufficient token scopes: {message}")
    } else {
        message
    }
}

const SCHEMA_CODES: [&str; 5] = ["undefinedField", "undefinedType", "argumentNotAccepted", "missingRequiredArguments", "argumentLiteralsIncompatible"];

pub fn graphql_error_kind(e: &Value) -> ErrorKind {
    if e.get("type").and_then(|t| t.as_str()) == Some("RATE_LIMITED") {
        return ErrorKind::RateLimited;
    }
    let code = e.get("extensions").and_then(|x| x.get("code")).and_then(|c| c.as_str());
    let message = e.get("message").and_then(|m| m.as_str()).unwrap_or("");
    if code.is_some_and(|c| SCHEMA_CODES.contains(&c)) || message.contains("doesn't exist on type") {
        ErrorKind::Schema
    } else {
        ErrorKind::Other
    }
}

fn graphql_error(e: &Value, headers: &HeaderMap) -> ApiError {
    let kind = graphql_error_kind(e);
    if kind == ErrorKind::RateLimited {
        let now = Utc::now().timestamp();
        pause_until(header_i64(headers, "x-ratelimit-reset").filter(|r| *r > now).unwrap_or(now + RATE_LIMIT_FALLBACK_SECS));
    }
    ApiError::new(describe_graphql_error(e)).with_kind(kind)
}

pub fn scrub(mut e: reqwest::Error) -> reqwest::Error {
    if let Some(url) = e.url_mut() {
        url.set_query(None);
        url.set_fragment(None);
    }
    e
}

fn net<T>(probe: &mut Probe, result: Result<T, reqwest::Error>) -> ApiResult<T> {
    result.map_err(|e| {
        let err = ApiError::from(scrub(e));
        probe.fail(&err);
        err
    })
}

fn header_string(headers: &HeaderMap, name: HeaderName) -> Option<String> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

fn strip_nulls(vars: Value) -> Value {
    match vars {
        Value::Object(map) => Value::Object(map.into_iter().filter(|(_, v)| !v.is_null()).collect()),
        other => other,
    }
}

fn retry_probe(mut probe: Probe, retry: bool) -> Probe {
    if retry {
        probe.note_with(|| RETRY_NOTE.to_string());
    }
    probe
}

impl GitHubApi {
    pub fn new(token: &str) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert("X-GitHub-Api-Version", HeaderValue::from_static("2022-11-28"));
        let agent = brand::user_agent();
        let client = Client::builder()
            .user_agent(agent.clone())
            .default_headers(headers.clone())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(40))
            .build()
            .expect("http client");
        let no_redirect = Client::builder()
            .user_agent(agent.clone())
            .default_headers(headers)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(40))
            .build()
            .expect("http client");
        let plain = Client::builder().user_agent(agent).connect_timeout(CONNECT_TIMEOUT).timeout(Duration::from_secs(60)).build().expect("http client");
        GitHubApi { inner: Arc::new(Inner { token: token.to_string(), client, no_redirect, plain, polled: Mutex::new(PollCache::default()) }) }
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

    fn polled(&self) -> MutexGuard<'_, PollCache> {
        self.inner.polled.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub async fn graphql<T: DeserializeOwned>(&self, name: &'static str, query: &str, variables: Value, strict: bool) -> ApiResult<T> {
        let body = serde_json::json!({ "query": query, "variables": strip_nulls(variables) });
        match self.graphql_probed(diagnostics::begin_graphql(|| name.to_string()), &body, strict).await {
            Err(e) if e.is_transient() => {
                tokio::time::sleep(RETRY_DELAY).await;
                self.graphql_probed(retry_probe(diagnostics::begin_graphql(|| name.to_string()), true), &body, strict).await
            }
            other => other,
        }
    }

    async fn graphql_probed<T: DeserializeOwned>(&self, mut probe: Probe, body: &Value, strict: bool) -> ApiResult<T> {
        let resp = net(
            &mut probe,
            self.inner
                .client
                .post("https://api.github.com/graphql")
                .header(AUTHORIZATION, self.auth())
                .header(ACCEPT, "application/vnd.github+json")
                .header(CONTENT_TYPE, "application/json")
                .json(body)
                .send()
                .await,
        )?;
        let status = resp.status();
        let headers = resp.headers().clone();
        probe.response(status.as_u16(), &headers);
        let bytes = net(&mut probe, resp.bytes().await)?;
        probe.body(bytes.len());
        probe.track(checked(status, &headers, &bytes))?;
        let value: Value = probe.track(serde_json::from_slice(&bytes).map_err(|e| ApiError::new(format!("Failed to parse response: {e}"))))?;
        let errors = value.get("errors").and_then(|e| e.as_array()).cloned().unwrap_or_default();
        let data = value.get("data").cloned().unwrap_or(Value::Null);
        if let Some(first) = errors.first() {
            if strict || data.is_null() {
                let err = graphql_error(first, &headers);
                probe.fail(&err);
                return Err(err);
            }
            probe.note_with(|| format!("partial: {}", describe_graphql_error(first)));
        }
        if data.is_null() {
            return probe.track(Err(ApiError::new("Empty response from GitHub")));
        }
        probe.track(serde_json::from_value::<T>(data).map_err(|e| match errors.first() {
            Some(first) => graphql_error(first, &headers),
            None => ApiError::new(format!("Failed to parse response: {e}")),
        }))
    }

    pub async fn mutate(&self, query: &str, vars: Value) -> ApiResult<()> {
        let probe = diagnostics::begin_graphql(|| diagnostics::graphql_name(query));
        let body = serde_json::json!({ "query": query, "variables": strip_nulls(vars) });
        let _: Value = self.graphql_probed(probe, &body, true).await?;
        Ok(())
    }

    async fn attempt(&self, method: &Method, path: &str, body: Option<&Value>, since: Option<&Validators>, retry: bool) -> ApiResult<Exchange> {
        let mut probe = retry_probe(diagnostics::begin_rest(method.as_str(), path), retry);
        let mut req = self.inner.client.request(method.clone(), Self::url(path)).header(AUTHORIZATION, self.auth()).header(ACCEPT, "application/vnd.github+json");
        if let Some(etag) = since.and_then(|v| v.etag.as_deref()) {
            req = req.header(IF_NONE_MATCH, etag);
        }
        if let Some(modified) = since.and_then(|v| v.last_modified.as_deref()) {
            req = req.header(IF_MODIFIED_SINCE, modified);
        }
        if let Some(body) = body {
            req = req.json(body);
        }
        let resp = net(&mut probe, req.send().await)?;
        let status = resp.status();
        let headers = resp.headers().clone();
        probe.response(status.as_u16(), &headers);
        let bytes = net(&mut probe, resp.bytes().await)?.to_vec();
        probe.body(bytes.len());
        if status != StatusCode::NOT_MODIFIED {
            probe.track(checked(status, &headers, &bytes))?;
        }
        Ok(Exchange { status, bytes, headers, probe })
    }

    async fn exchange(&self, method: Method, path: &str, body: Option<Value>, since: Option<Validators>) -> ApiResult<Exchange> {
        let read = method == Method::GET;
        match self.attempt(&method, path, body.as_ref(), since.as_ref(), false).await {
            Err(e) if read && e.is_transient() => {
                tokio::time::sleep(RETRY_DELAY).await;
                self.attempt(&method, path, body.as_ref(), since.as_ref(), true).await
            }
            other => other,
        }
    }

    pub async fn rest_raw(&self, method: Method, path: &str, body: Option<Value>) -> ApiResult<(Vec<u8>, HeaderMap)> {
        let ex = self.exchange(method, path, body, None).await?;
        Ok((ex.bytes, ex.headers))
    }

    pub async fn rest<T: DeserializeOwned>(&self, method: Method, path: &str, body: Option<Value>) -> ApiResult<T> {
        let mut ex = self.exchange(method, path, body, None).await?;
        ex.probe.track(serde_json::from_slice(&ex.bytes).map_err(|e| ApiError::new(format!("Failed to parse response: {e}"))))
    }

    pub async fn rest_polled(&self, path: &str, force: bool) -> ApiResult<Vec<u8>> {
        let (generation, fresh, since) = {
            let polled = self.polled();
            let entry = polled.entries.get(path).filter(|_| !force);
            let fresh = entry.filter(|p| p.checked.elapsed() < p.interval).map(|p| p.body.clone());
            let since = entry.map(|p| p.validators.clone()).filter(|v| v.last_modified.is_some() || v.etag.is_some());
            (polled.generation, fresh, since)
        };
        if let Some(body) = fresh {
            return Ok(body);
        }
        let mut ex = self.exchange(Method::GET, path, None, since).await?;
        let interval = Duration::from_secs(header_i64(&ex.headers, "x-poll-interval").unwrap_or(0).clamp(0, 3600) as u64);
        if ex.status == StatusCode::NOT_MODIFIED {
            let cached = self.polled().entries.get_mut(path).map(|p| {
                p.checked = Instant::now();
                p.interval = interval;
                p.body.clone()
            });
            if let Some(body) = cached {
                return Ok(body);
            }
            drop(ex);
            ex = self.exchange(Method::GET, path, None, None).await?;
        }
        let validators = Validators { last_modified: header_string(&ex.headers, LAST_MODIFIED), etag: header_string(&ex.headers, ETAG) };
        let mut polled = self.polled();
        if polled.generation == generation {
            polled.entries.insert(path.to_string(), Polled { validators, body: ex.bytes.clone(), checked: Instant::now(), interval });
        }
        drop(polled);
        Ok(ex.bytes)
    }

    pub fn invalidate_polled(&self) {
        let mut polled = self.polled();
        polled.generation += 1;
        polled.entries.clear();
    }

    pub async fn send(&self, method: Method, path: &str, body: Option<Value>) -> ApiResult<()> {
        self.rest_raw(method, path, body).await.map(|_| ())
    }

    pub async fn redirect_target(&self, path: &str) -> ApiResult<String> {
        let mut probe = diagnostics::begin_rest("GET", path);
        let resp = net(
            &mut probe,
            self.inner
                .no_redirect
                .get(Self::url(path))
                .header(AUTHORIZATION, self.auth())
                .header(ACCEPT, "application/vnd.github+json")
                .send()
                .await,
        )?;
        let status = resp.status();
        let headers = resp.headers().clone();
        probe.response(status.as_u16(), &headers);
        if status.is_redirection() {
            if let Some(loc) = headers.get("location").and_then(|v| v.to_str().ok()) {
                probe.finish();
                return Ok(loc.to_string());
            }
        }
        let bytes = net(&mut probe, resp.bytes().await)?;
        probe.body(bytes.len());
        probe.track(checked(status, &headers, &bytes))?;
        probe.track(Err(ApiError::new("Log unavailable")))
    }

    pub async fn plain(&self, url: &str) -> ApiResult<String> {
        let mut probe = diagnostics::begin_rest("GET", url);
        let resp = net(&mut probe, self.inner.plain.get(url).send().await)?;
        let status = resp.status();
        probe.response(status.as_u16(), resp.headers());
        let bytes = net(&mut probe, resp.bytes().await)?;
        probe.body(bytes.len());
        probe.track(check_status(status, &bytes))?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    pub async fn fetch(&self, path: &str, accept: &str) -> ApiResult<Option<Vec<u8>>> {
        match self.fetch_attempt(path, accept, false).await {
            Err(e) if e.is_transient() => {
                tokio::time::sleep(RETRY_DELAY).await;
                self.fetch_attempt(path, accept, true).await
            }
            other => other,
        }
    }

    async fn fetch_attempt(&self, path: &str, accept: &str, retry: bool) -> ApiResult<Option<Vec<u8>>> {
        let mut probe = retry_probe(diagnostics::begin_rest("GET", path), retry);
        let resp = net(&mut probe, self.inner.client.get(Self::url(path)).header(AUTHORIZATION, self.auth()).header(ACCEPT, accept).send().await)?;
        let status = resp.status();
        let headers = resp.headers().clone();
        probe.response(status.as_u16(), &headers);
        if status == StatusCode::NOT_FOUND {
            probe.finish();
            return Ok(None);
        }
        if !status.is_success() {
            let bytes = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
            let err = match checked(status, &headers, &bytes) {
                Err(e) if e.kind == ErrorKind::RateLimited => e,
                _ => ApiError::with_status(format!("GitHub {}", status.as_u16()), status.as_u16()),
            };
            return probe.track(Err(err));
        }
        let bytes = net(&mut probe, resp.bytes().await)?.to_vec();
        probe.body(bytes.len());
        Ok(Some(bytes))
    }
}

static SHARED: std::sync::LazyLock<Client> =
    std::sync::LazyLock::new(|| Client::builder().user_agent(brand::user_agent()).connect_timeout(CONNECT_TIMEOUT).timeout(Duration::from_secs(30)).build().expect("http client"));

pub fn http_client() -> Client {
    SHARED.clone()
}

pub async fn probed_get(probe: &mut Probe, request: reqwest::RequestBuilder) -> Result<(StatusCode, Vec<u8>), ApiError> {
    let resp = net(probe, request.send().await)?;
    let status = resp.status();
    probe.response(status.as_u16(), resp.headers());
    let bytes = net(probe, resp.bytes().await)?.to_vec();
    probe.body(bytes.len());
    Ok((status, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(*k, HeaderValue::from_str(v).expect("header"));
        }
        h
    }

    #[test]
    fn primary_limit_waits_for_reset() {
        let h = headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1000")]);
        assert_eq!(rate_limit_resume(403, &h, b"{}", 900), Some(1000));
        assert_eq!(rate_limit_resume(200, &h, b"{}", 900), Some(1000));
    }

    #[test]
    fn secondary_limit_honors_retry_after() {
        let h = headers(&[("retry-after", "30"), ("x-ratelimit-remaining", "12")]);
        assert_eq!(rate_limit_resume(403, &h, b"{}", 900), Some(930));
        assert_eq!(rate_limit_resume(429, &headers(&[]), b"", 900), Some(900 + RATE_LIMIT_FALLBACK_SECS));
        assert_eq!(rate_limit_resume(403, &headers(&[]), br#"{"message":"You have exceeded a secondary rate limit"}"#, 900), Some(900 + RATE_LIMIT_FALLBACK_SECS));
    }

    #[test]
    fn other_resources_do_not_pause_live_polling() {
        let search = headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1000"), ("x-ratelimit-resource", "search")]);
        assert_eq!(rate_limit_resume(403, &search, br#"{"message":"API rate limit exceeded"}"#, 900), None);
        assert_eq!(rate_limit_resume(200, &search, b"", 900), None);
        let graphql = headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "1000"), ("x-ratelimit-resource", "graphql")]);
        assert_eq!(rate_limit_resume(200, &graphql, b"", 900), Some(1000));
    }

    #[test]
    fn pause_is_capped() {
        let h = headers(&[("x-ratelimit-remaining", "0"), ("x-ratelimit-reset", "999999")]);
        assert_eq!(rate_limit_resume(403, &h, b"", 900), Some(900 + RATE_LIMIT_MAX_PAUSE_SECS));
        assert_eq!(rate_limit_resume(429, &headers(&[("retry-after", "86400")]), b"", 900), Some(900 + RATE_LIMIT_MAX_PAUSE_SECS));
    }

    #[test]
    fn ordinary_errors_do_not_pause() {
        let h = headers(&[("x-ratelimit-remaining", "4000"), ("x-ratelimit-reset", "1000")]);
        assert_eq!(rate_limit_resume(403, &h, br#"{"message":"Resource not accessible by integration"}"#, 900), None);
        assert_eq!(rate_limit_resume(502, &h, b"", 900), None);
        assert_eq!(rate_limit_resume(200, &h, b"", 900), None);
    }

    #[test]
    fn checked_marks_rate_limited() {
        let h = headers(&[("retry-after", "5")]);
        let err = checked(StatusCode::FORBIDDEN, &h, br#"{"message":"secondary rate limit"}"#).expect_err("limited");
        assert_eq!(err.kind, ErrorKind::RateLimited);
        assert!(!err.is_transient());
        assert!(rate_limited_until().is_some());
        reset_rate_limit();
    }

    #[test]
    fn transient_errors() {
        assert!(ApiError::with_status("x", 502).is_transient());
        assert!(ApiError::with_status("x", 504).is_transient());
        assert!(ApiError::new("x").with_kind(ErrorKind::Network).is_transient());
        assert!(!ApiError::with_status("x", 500).is_transient());
        assert!(!ApiError::with_status("x", 404).is_transient());
    }

    #[test]
    fn graphql_error_kinds() {
        let schema = serde_json::json!({ "message": "Field 'stack' doesn't exist on type 'PullRequest'", "extensions": { "code": "undefinedField" } });
        assert_eq!(graphql_error_kind(&schema), ErrorKind::Schema);
        let limited = serde_json::json!({ "type": "RATE_LIMITED", "message": "API rate limit exceeded" });
        assert_eq!(graphql_error_kind(&limited), ErrorKind::RateLimited);
        let other = serde_json::json!({ "type": "NOT_FOUND", "message": "Could not resolve to a Repository" });
        assert_eq!(graphql_error_kind(&other), ErrorKind::Other);
    }
}
