use serde::{Deserialize, Serialize};

use crate::api::models::RestUser;
use crate::api::ApiError;
use crate::app::PrFilter;
use crate::{secret, settings};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "camelCase")]
pub enum GitProvider {
    Github,
    GithubEnterprise,
    Gitlab,
    Bitbucket,
    Gitea,
}

impl GitProvider {
    pub const ALL: [GitProvider; 5] = [GitProvider::Github, GitProvider::GithubEnterprise, GitProvider::Gitlab, GitProvider::Bitbucket, GitProvider::Gitea];

    pub fn key(&self) -> &'static str {
        match self {
            GitProvider::Github => "github",
            GitProvider::GithubEnterprise => "githubEnterprise",
            GitProvider::Gitlab => "gitlab",
            GitProvider::Bitbucket => "bitbucket",
            GitProvider::Gitea => "gitea",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == k)
    }

    pub fn title(&self) -> &'static str {
        match self {
            GitProvider::Github => "GitHub",
            GitProvider::GithubEnterprise => "GitHub Enterprise",
            GitProvider::Gitlab => "GitLab",
            GitProvider::Bitbucket => "Bitbucket",
            GitProvider::Gitea => "Gitea / Forgejo",
        }
    }

    pub fn short_title(&self) -> &'static str {
        match self {
            GitProvider::GithubEnterprise => "GHE",
            GitProvider::Gitea => "Gitea",
            _ => self.title(),
        }
    }

    pub fn default_host(&self) -> &'static str {
        match self {
            GitProvider::Github => "github.com",
            GitProvider::GithubEnterprise => "",
            GitProvider::Gitlab => "gitlab.com",
            GitProvider::Bitbucket => "bitbucket.org",
            GitProvider::Gitea => "codeberg.org",
        }
    }

    pub fn host_editable(&self) -> bool {
        !matches!(self, GitProvider::Github | GitProvider::Bitbucket)
    }

    pub fn host_placeholder(&self) -> &'static str {
        match self {
            GitProvider::GithubEnterprise => "github.example.com",
            GitProvider::Gitlab => "gitlab.com or gitlab.example.com",
            GitProvider::Gitea => "codeberg.org or git.example.com",
            _ => self.default_host(),
        }
    }

    pub fn is_github(&self) -> bool {
        matches!(self, GitProvider::Github | GitProvider::GithubEnterprise)
    }

    pub fn requests_title(&self) -> &'static str {
        if *self == GitProvider::Gitlab {
            "Merge requests"
        } else {
            "Pull requests"
        }
    }

    pub fn request_title(&self) -> &'static str {
        if *self == GitProvider::Gitlab {
            "merge request"
        } else {
            "pull request"
        }
    }

    pub fn request_short(&self) -> &'static str {
        if *self == GitProvider::Gitlab {
            "MR"
        } else {
            "PR"
        }
    }

    pub fn number_prefix(&self) -> &'static str {
        if *self == GitProvider::Gitlab {
            "!"
        } else {
            "#"
        }
    }

    pub fn supported_filters(&self) -> Vec<PrFilter> {
        match self {
            GitProvider::Github | GitProvider::GithubEnterprise | GitProvider::Gitea => PrFilter::ALL.to_vec(),
            GitProvider::Gitlab => vec![PrFilter::ReviewRequested, PrFilter::Mine, PrFilter::Assigned],
            GitProvider::Bitbucket => vec![PrFilter::ReviewRequested, PrFilter::Mine],
        }
    }

    pub fn token_title(&self) -> &'static str {
        if *self == GitProvider::Bitbucket {
            "API token"
        } else {
            "Personal Access Token"
        }
    }

    pub fn token_placeholder(&self) -> &'static str {
        match self {
            GitProvider::Github | GitProvider::GithubEnterprise => "ghp_… or github_pat_…",
            GitProvider::Gitlab => "glpat-…",
            GitProvider::Bitbucket => "Atlassian API token",
            GitProvider::Gitea => "Access token",
        }
    }

    pub fn token_hint(&self) -> &'static str {
        match self {
            GitProvider::Github | GitProvider::GithubEnterprise => "Required scopes: repo, read:org, read:project, project, notifications, workflow.",
            GitProvider::Gitlab => "Required scope: api (or read_api for read-only).",
            GitProvider::Bitbucket => "Use your Atlassian account email and an API token with Bitbucket read scopes for account, repositories and pull requests.",
            GitProvider::Gitea => "Required permissions: read for user, repository and issue.",
        }
    }

    pub fn token_create_url(&self, host: &str, app_name: &str) -> Option<String> {
        let scopes = "repo,read:org,read:project,project,notifications,workflow";
        match self {
            GitProvider::Github => Some(format!("https://github.com/settings/tokens/new?scopes={scopes}&description={app_name}")),
            GitProvider::GithubEnterprise if !host.is_empty() => Some(format!("https://{host}/settings/tokens/new?scopes={scopes}&description={app_name}")),
            GitProvider::Gitlab if !host.is_empty() => Some(format!("https://{host}/-/user_settings/personal_access_tokens?name={app_name}&scopes=api")),
            GitProvider::Bitbucket => Some("https://id.atlassian.com/manage-profile/security/api-tokens".into()),
            GitProvider::Gitea if !host.is_empty() => Some(format!("https://{host}/user/settings/applications")),
            _ => None,
        }
    }

    pub fn token_create_label(&self, host: &str) -> String {
        if *self == GitProvider::Bitbucket {
            "Create an API token at id.atlassian.com".into()
        } else {
            format!("Create a token on {host}")
        }
    }

    pub fn cli_name(&self) -> Option<&'static str> {
        match self {
            GitProvider::Github | GitProvider::GithubEnterprise => Some("gh"),
            GitProvider::Gitlab => Some("glab"),
            _ => None,
        }
    }

    pub fn supports_device_flow(&self) -> bool {
        self.is_github() || *self == GitProvider::Gitlab
    }

    pub fn device_help(&self) -> &'static str {
        if *self == GitProvider::Gitlab {
            "Requires GitLab 17.1 or later and the Application ID of your own OAuth application (User settings → Applications): not confidential, scope api, “Device authorization grant” enabled."
        } else {
            "Requires the Client ID of your own OAuth App (Settings → Developer settings → OAuth Apps, enable “Enable Device Flow”)."
        }
    }

    pub fn client_id_placeholder(&self) -> &'static str {
        if *self == GitProvider::Gitlab {
            "Application ID"
        } else {
            "Client ID"
        }
    }

    pub fn effective_host(&self, raw: &str) -> String {
        if self.host_editable() {
            normalize_host(raw)
        } else {
            self.default_host().to_string()
        }
    }
}

pub fn normalize_host(raw: &str) -> String {
    let mut s = raw.trim().to_lowercase();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    s.trim_end_matches('/').to_string()
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GitAccount {
    pub id: String,
    pub provider: GitProvider,
    pub host: String,
    pub login: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    pub secret_key: String,
}

impl GitAccount {
    pub fn draft(provider: GitProvider, host: &str, email: Option<String>) -> GitAccount {
        let id = uuid::Uuid::new_v4().to_string().to_lowercase();
        GitAccount {
            secret_key: format!("account-{id}"),
            id,
            provider,
            host: host.to_string(),
            login: String::new(),
            name: None,
            avatar_url: None,
            email: email.map(|e| e.trim().to_string()).filter(|e| !e.is_empty()),
            user_id: None,
        }
    }

    pub fn user(&self) -> RestUser {
        RestUser { login: self.login.clone(), name: self.name.clone(), avatar_url: self.avatar_url.clone() }
    }

    pub fn display_name(&self) -> String {
        self.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| self.login.clone())
    }

    pub fn cache_key(&self) -> String {
        if self.provider == GitProvider::Github {
            self.login.clone()
        } else {
            format!("{}-{}-{}", self.login, self.provider.key(), self.host)
        }
    }

    pub fn subtitle(&self) -> String {
        if self.provider == GitProvider::Github {
            format!("@{}", self.login)
        } else {
            format!("@{} · {}", self.login, self.host)
        }
    }

    pub fn identity(&self) -> String {
        format!("{}|{}|{}", self.provider.key(), self.host, self.login.to_lowercase())
    }

    pub fn same(&self, other: &GitAccount) -> bool {
        self.identity() == other.identity()
    }

    pub fn with_user(mut self, user: &RestUser) -> GitAccount {
        self.login = user.login.clone();
        self.name = user.name.clone();
        self.avatar_url = user.avatar_url.clone();
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct GitCredential {
    pub token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
}

impl GitCredential {
    pub fn token(token: impl Into<String>) -> GitCredential {
        GitCredential { token: token.into(), ..Default::default() }
    }

    pub fn decode(raw: &str) -> GitCredential {
        if raw.starts_with('{') {
            if let Ok(c) = serde_json::from_str::<GitCredential>(raw) {
                return c;
            }
        }
        GitCredential::token(raw)
    }

    pub fn encode(&self) -> String {
        if self.refresh_token.is_none() && self.expires_at.is_none() && self.client_id.is_none() {
            return self.token.clone();
        }
        serde_json::to_string(self).unwrap_or_else(|_| self.token.clone())
    }
}

pub fn resolve(existing: &[GitAccount], candidate: GitAccount) -> GitAccount {
    let mut account = candidate;
    if let Some(found) = existing.iter().find(|a| a.same(&account)) {
        account.id = found.id.clone();
        account.secret_key = found.secret_key.clone();
    } else if account.provider == GitProvider::Github {
        if let Some(legacy) = existing.iter().find(|a| a.provider == GitProvider::Github && a.login.is_empty()) {
            account.id = legacy.id.clone();
            account.secret_key = legacy.secret_key.clone();
        }
    }
    account
}

pub fn accounts() -> Vec<GitAccount> {
    settings::read(|s| s.git_accounts.clone())
}

pub fn active() -> Option<GitAccount> {
    let (list, id) = settings::read(|s| (s.git_accounts.clone(), s.git_active_account.clone()));
    id.and_then(|id| list.iter().find(|a| a.id == id).cloned()).or_else(|| list.first().cloned())
}

pub fn set_active(id: &str) {
    let id = id.to_string();
    if settings::read(|s| s.git_active_account.as_deref() != Some(id.as_str())) {
        settings::update(|s| s.git_active_account = Some(id));
    }
}

pub fn update(account: &GitAccount) {
    let changed = settings::read(|s| s.git_accounts.iter().any(|a| a.id == account.id && a != account));
    if changed {
        settings::update(|s| {
            if let Some(a) = s.git_accounts.iter_mut().find(|a| a.id == account.id) {
                *a = account.clone();
            }
        });
    }
}

pub async fn credential(account: &GitAccount) -> Option<GitCredential> {
    let key = account.secret_key.clone();
    crate::rt::run(secret::load(settings::config_dir(), key)).await.map(|raw| GitCredential::decode(&raw))
}

pub async fn has_credential(account: &GitAccount) -> bool {
    credential(account).await.is_some()
}

pub fn forget_credential(account: &GitAccount) {
    let key = account.secret_key.clone();
    crate::rt::spawn_local(async move { crate::rt::run(secret::delete(settings::config_dir(), key)).await });
}

pub async fn store(account: &GitAccount, credential: &GitCredential) -> secret::Saved {
    crate::rt::run(secret::save(settings::config_dir(), account.secret_key.clone(), credential.encode())).await
}

pub async fn upsert(candidate: GitAccount, credential: &GitCredential) -> Result<(GitAccount, secret::Saved), ApiError> {
    let account = resolve(&accounts(), candidate);
    let saved = store(&account, credential).await;
    if saved == secret::Saved::Failed {
        return Err(ApiError::new("Could not save the sign-in. Check that the keyring is available."));
    }
    let stored = account.clone();
    settings::update(move |s| match s.git_accounts.iter_mut().find(|a| a.id == stored.id) {
        Some(a) => *a = stored,
        None => s.git_accounts.push(stored),
    });
    Ok((account, saved))
}

pub fn remove(id: &str) -> Option<GitAccount> {
    let account = accounts().into_iter().find(|a| a.id == id)?;
    let id = id.to_string();
    settings::update(|s| {
        s.git_accounts.retain(|a| a.id != id);
        if s.git_active_account.as_deref() == Some(id.as_str()) {
            s.git_active_account = None;
        }
    });
    let key = account.secret_key.clone();
    crate::rt::spawn_local(async move { crate::rt::run(secret::delete(settings::config_dir(), key)).await });
    Some(account)
}

pub async fn migrate() {
    if !accounts().is_empty() {
        return;
    }
    let legacy = crate::rt::run(secret::load(settings::config_dir(), secret::DEFAULT_KEY.to_string())).await;
    let Some(token) = legacy else { return };
    let known = settings::read(|s| s.last_account.clone()).filter(|a| a.fingerprint == crate::list_cache::fingerprint(&token));
    let id = uuid::Uuid::new_v4().to_string().to_lowercase();
    let account = GitAccount {
        id: id.clone(),
        provider: GitProvider::Github,
        host: GitProvider::Github.default_host().into(),
        login: known.as_ref().map(|k| k.login.clone()).unwrap_or_default(),
        name: known.as_ref().and_then(|k| k.name.clone()),
        avatar_url: known.as_ref().and_then(|k| k.avatar_url.clone()),
        email: None,
        user_id: None,
        secret_key: secret::DEFAULT_KEY.into(),
    };
    settings::update(move |s| {
        if s.git_accounts.is_empty() {
            s.git_accounts.push(account);
            s.git_active_account = Some(id);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(provider: GitProvider, host: &str, login: &str) -> GitAccount {
        let mut a = GitAccount::draft(provider, host, None);
        a.login = login.into();
        a
    }

    #[test]
    fn hosts_are_normalized() {
        assert_eq!(normalize_host("  https://GitLab.Example.com/ "), "gitlab.example.com");
        assert_eq!(normalize_host("http://git.example.com//"), "git.example.com");
        assert_eq!(GitProvider::Github.effective_host("anything"), "github.com");
        assert_eq!(GitProvider::Bitbucket.effective_host(""), "bitbucket.org");
        assert_eq!(GitProvider::GithubEnterprise.effective_host("https://ghe.corp/"), "ghe.corp");
    }

    #[test]
    fn provider_keys_round_trip() {
        for p in GitProvider::ALL {
            assert_eq!(GitProvider::from_key(p.key()), Some(p));
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{}\"", p.key()));
        }
    }

    #[test]
    fn gitlab_wording_and_filters() {
        assert_eq!(GitProvider::Gitlab.requests_title(), "Merge requests");
        assert_eq!(GitProvider::Gitlab.request_short(), "MR");
        assert_eq!(GitProvider::Gitlab.number_prefix(), "!");
        assert_eq!(GitProvider::Github.number_prefix(), "#");
        assert_eq!(GitProvider::Bitbucket.supported_filters(), vec![PrFilter::ReviewRequested, PrFilter::Mine]);
        assert_eq!(GitProvider::Gitea.supported_filters().len(), PrFilter::ALL.len());
    }

    #[test]
    fn identity_and_cache_keys() {
        let gh = account(GitProvider::Github, "github.com", "Octo");
        assert_eq!(gh.cache_key(), "Octo");
        assert_eq!(gh.subtitle(), "@Octo");
        let gl = account(GitProvider::Gitlab, "gitlab.com", "octo");
        assert_eq!(gl.cache_key(), "octo-gitlab-gitlab.com");
        assert_eq!(gl.subtitle(), "@octo · gitlab.com");
        assert!(account(GitProvider::Github, "github.com", "octo").same(&gh));
        assert!(!gl.same(&gh));
    }

    #[test]
    fn credentials_encode_plain_tokens_as_is() {
        let plain = GitCredential::token("glpat-1");
        assert_eq!(plain.encode(), "glpat-1");
        assert_eq!(GitCredential::decode("glpat-1"), plain);
        let oauth = GitCredential { token: "a".into(), refresh_token: Some("r".into()), expires_at: Some(10), client_id: Some("c".into()) };
        let raw = oauth.encode();
        assert!(raw.starts_with('{'));
        assert_eq!(GitCredential::decode(&raw), oauth);
    }

    #[test]
    fn upsert_reuses_existing_and_legacy_records() {
        let existing = vec![account(GitProvider::Gitlab, "gitlab.com", "octo")];
        let again = resolve(&existing, account(GitProvider::Gitlab, "gitlab.com", "OCTO"));
        assert_eq!(again.id, existing[0].id);
        assert_eq!(again.secret_key, existing[0].secret_key);
        let mut legacy = account(GitProvider::Github, "github.com", "");
        legacy.secret_key = secret::DEFAULT_KEY.into();
        let resolved = resolve(&[legacy.clone()], account(GitProvider::Github, "github.com", "octo"));
        assert_eq!(resolved.id, legacy.id);
        assert_eq!(resolved.secret_key, secret::DEFAULT_KEY);
        let other = resolve(&[legacy.clone()], account(GitProvider::GithubEnterprise, "ghe.corp", "octo"));
        assert_ne!(other.id, legacy.id);
    }

    #[test]
    fn token_urls() {
        assert!(GitProvider::GithubEnterprise.token_create_url("", "PullYard").is_none());
        assert_eq!(GitProvider::Gitea.token_create_url("codeberg.org", "PullYard").as_deref(), Some("https://codeberg.org/user/settings/applications"));
        assert!(GitProvider::Gitlab.token_create_url("gitlab.com", "PullYard").unwrap().contains("scopes=api"));
    }
}
