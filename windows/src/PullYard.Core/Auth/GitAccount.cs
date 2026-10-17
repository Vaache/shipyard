using System.Text.Json;
using System.Text.Json.Serialization;
using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.Core.Auth;

[JsonConverter(typeof(JsonStringEnumConverter<GitProvider>))]
public enum GitProvider { GitHub, GitHubEnterprise, GitLab, Bitbucket, Gitea }

public enum ForgeFilter { ReviewRequested, Mine, Assigned, Mentioned, Reviewed }

public enum ForgeState { Open, Closed, All }

public static class GitProviders
{
    public const string GitHubHost = "github.com";

    public static IReadOnlyList<GitProvider> All { get; } = Enum.GetValues<GitProvider>();

    public static string Title(this GitProvider p) => p switch
    {
        GitProvider.GitHub => "GitHub",
        GitProvider.GitHubEnterprise => "GitHub Enterprise",
        GitProvider.GitLab => "GitLab",
        GitProvider.Bitbucket => "Bitbucket",
        _ => "Gitea / Forgejo"
    };

    public static string ShortTitle(this GitProvider p) => p switch
    {
        GitProvider.GitHubEnterprise => "GHE",
        GitProvider.Gitea => "Gitea",
        _ => p.Title()
    };

    public static string DefaultHost(this GitProvider p) => p switch
    {
        GitProvider.GitHub => GitHubHost,
        GitProvider.GitHubEnterprise => "",
        GitProvider.GitLab => "gitlab.com",
        GitProvider.Bitbucket => "bitbucket.org",
        _ => "codeberg.org"
    };

    public static bool HostEditable(this GitProvider p) => p is not (GitProvider.GitHub or GitProvider.Bitbucket);

    public static bool IsGitHub(this GitProvider p) => p is GitProvider.GitHub or GitProvider.GitHubEnterprise;

    public static bool UsesMergeRequests(this GitProvider p) => p == GitProvider.GitLab;

    public static string NumberPrefix(this GitProvider p) => p == GitProvider.GitLab ? "!" : "#";

    public static bool SupportsDeviceFlow(this GitProvider p) => p.IsGitHub() || p == GitProvider.GitLab;

    public static string TokenPlaceholder(this GitProvider p) => p switch
    {
        GitProvider.GitHub or GitProvider.GitHubEnterprise => "ghp_… or github_pat_…",
        GitProvider.GitLab => "glpat-…",
        GitProvider.Bitbucket => "Atlassian API token",
        _ => "Access token"
    };

    public static string? TokenCreateUrl(this GitProvider p, string host) => p switch
    {
        GitProvider.GitHub => $"https://{GitHubHost}/settings/tokens/new?scopes=repo,read:org,read:project,project,notifications,workflow&description={Uri.EscapeDataString(AppInfo.DisplayName)}",
        GitProvider.GitHubEnterprise => host.Length == 0 ? null : $"https://{host}/settings/tokens/new?scopes=repo,read:org,read:project,project,notifications,workflow&description={Uri.EscapeDataString(AppInfo.DisplayName)}",
        GitProvider.GitLab => host.Length == 0 ? null : $"https://{host}/-/user_settings/personal_access_tokens?name={Uri.EscapeDataString(AppInfo.DisplayName)}&scopes=api",
        GitProvider.Bitbucket => "https://id.atlassian.com/manage-profile/security/api-tokens",
        _ => host.Length == 0 ? null : $"https://{host}/user/settings/applications"
    };

    public static string NormalizeHost(string? raw)
    {
        var s = (raw ?? "").Trim().ToLowerInvariant();
        foreach (var prefix in new[] { "https://", "http://" })
            if (s.StartsWith(prefix, StringComparison.Ordinal)) s = s[prefix.Length..];
        return s.TrimEnd('/');
    }

    public static string EffectiveHost(this GitProvider p, string? raw) => p.HostEditable() ? NormalizeHost(raw) : p.DefaultHost();
}

public sealed class GitAccount
{
    public string Id { get; set; } = "";
    public GitProvider Provider { get; set; }
    public string Host { get; set; } = "";
    public string Login { get; set; } = "";
    public string? Name { get; set; }
    public string? AvatarUrl { get; set; }
    public string? Email { get; set; }
    public string? UserId { get; set; }
    public string CredentialKey { get; set; } = "";

    [JsonIgnore]
    public RestUser User => new() { Login = Login, Name = Name, AvatarUrl = AvatarUrl };

    [JsonIgnore]
    public string CacheKey => Provider == GitProvider.GitHub ? Login : $"{Login}-{Provider}-{Host}";

    [JsonIgnore]
    public string Subtitle => Provider == GitProvider.GitHub ? "@" + Login : $"@{Login} · {Host}";

    [JsonIgnore]
    public string Identity => $"{Provider}|{Host}|{Login.ToLowerInvariant()}";

    public bool Same(GitAccount other) => Identity == other.Identity;

    public GitAccount Copy() => (GitAccount)MemberwiseClone();

    public GitAccount With(RestUser user, string? userId = null)
    {
        var a = Copy();
        a.Login = user.Login;
        a.Name = user.Name;
        a.AvatarUrl = user.AvatarUrl;
        if (userId is not null) a.UserId = userId;
        return a;
    }

    public bool SameValues(GitAccount other) => JsonSerializer.Serialize(this, Json.Plain) == JsonSerializer.Serialize(other, Json.Plain);
}

public sealed class GitCredential
{
    public string Token { get; set; } = "";
    public string? RefreshToken { get; set; }
    public DateTimeOffset? ExpiresAt { get; set; }
    public string? ClientId { get; set; }

    public static GitCredential Of(string token) => new() { Token = token };

    public static GitCredential Decode(string raw)
    {
        if (raw.StartsWith('{'))
        {
            try
            {
                if (JsonSerializer.Deserialize<GitCredential>(raw, Json.Plain) is { Token.Length: > 0 } c) return c;
            }
            catch (JsonException)
            {
            }
        }
        return Of(raw);
    }

    [JsonIgnore]
    public string Encoded => RefreshToken is null && ExpiresAt is null && ClientId is null ? Token : JsonSerializer.Serialize(this, Json.Plain);
}

public interface ICredentialVault
{
    string? Load(string key);
    void Save(string key, string value);
    bool Delete(string key);
}

public sealed class GitAccounts
{
    public const string DefaultCredentialKey = "github-token";
    private const string ListKey = "git.accounts";
    private const string ActiveKey = "git.activeAccount";

    private readonly Settings settings;
    private readonly ICredentialVault vault;
    private readonly object gate = new();
    private List<GitAccount> accounts;

    public string? ActiveId { get; private set; }

    public event Action? Changed;

    public GitAccounts(Settings settings, ICredentialVault vault)
    {
        this.settings = settings;
        this.vault = vault;
        accounts = settings.GetObject<List<GitAccount>>(ListKey) ?? new();
        ActiveId = settings.GetString(ActiveKey);
        if (accounts.Count == 0 && vault.Load(DefaultCredentialKey) is not null)
        {
            var legacy = new GitAccount { Id = NewId(), Provider = GitProvider.GitHub, Host = GitProviders.GitHubHost, CredentialKey = DefaultCredentialKey };
            accounts.Add(legacy);
            ActiveId = legacy.Id;
            Save();
        }
        if (ActiveId is null || accounts.All(a => a.Id != ActiveId)) ActiveId = accounts.FirstOrDefault()?.Id;
    }

    public static string NewId() => Guid.NewGuid().ToString().ToLowerInvariant();

    public IReadOnlyList<GitAccount> All
    {
        get
        {
            lock (gate) return accounts.Select(a => a.Copy()).ToList();
        }
    }

    public GitAccount? Active
    {
        get
        {
            lock (gate) return accounts.FirstOrDefault(a => a.Id == ActiveId)?.Copy();
        }
    }

    public GitAccount? Find(string id)
    {
        lock (gate) return accounts.FirstOrDefault(a => a.Id == id)?.Copy();
    }

    public GitCredential? Credential(GitAccount account) => vault.Load(account.CredentialKey) is { } raw ? GitCredential.Decode(raw) : null;

    public void Store(GitCredential credential, GitAccount account) => vault.Save(account.CredentialKey, credential.Encoded);

    public bool HasCredential(GitAccount account) => vault.Load(account.CredentialKey) is not null;

    public void ForgetCredential(string id)
    {
        if (Find(id) is { } account) vault.Delete(account.CredentialKey);
    }

    public GitAccount Upsert(GitAccount candidate, GitCredential credential)
    {
        var account = candidate.Copy();
        lock (gate)
        {
            if (accounts.FirstOrDefault(a => a.Same(candidate)) is { } existing)
            {
                account.Id = existing.Id;
                account.CredentialKey = existing.CredentialKey;
            }
            else if (account.Provider == GitProvider.GitHub && accounts.FirstOrDefault(a => a.Provider == GitProvider.GitHub && a.Login.Length == 0) is { } legacy)
            {
                account.Id = legacy.Id;
                account.CredentialKey = legacy.CredentialKey;
            }
        }
        Store(credential, account);
        lock (gate)
        {
            var i = accounts.FindIndex(a => a.Id == account.Id);
            if (i >= 0) accounts[i] = account.Copy();
            else accounts.Add(account.Copy());
        }
        Save();
        return account;
    }

    public void Update(GitAccount account)
    {
        lock (gate)
        {
            var i = accounts.FindIndex(a => a.Id == account.Id);
            if (i < 0 || accounts[i].SameValues(account)) return;
            accounts[i] = account.Copy();
        }
        Save();
    }

    public void SetActive(string? id)
    {
        ActiveId = id;
        Save();
    }

    public void Remove(string id)
    {
        GitAccount? removed;
        lock (gate)
        {
            removed = accounts.FirstOrDefault(a => a.Id == id);
            if (removed is null) return;
            accounts.Remove(removed);
        }
        vault.Delete(removed.CredentialKey);
        if (ActiveId == id) ActiveId = null;
        Save();
    }

    private void Save()
    {
        List<GitAccount> snapshot;
        lock (gate) snapshot = accounts.Select(a => a.Copy()).ToList();
        settings.Set(ListKey, snapshot);
        settings.Set(ActiveKey, ActiveId);
        Changed?.Invoke();
    }
}
