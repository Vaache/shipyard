using System.Text.Json.Nodes;
using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.Core.Auth;

public static class CliTool
{
    public static string? Find(string name, params string[] installDirs)
    {
        var candidates = new List<string?> { Proc.FindOnPath(name) };
        foreach (var dir in installDirs)
        {
            candidates.Add(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), dir, name + ".exe"));
            candidates.Add(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86), dir, name + ".exe"));
            candidates.Add(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", dir, name + ".exe"));
        }
        candidates.Add(Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "scoop", "shims", name + ".exe"));
        return candidates.FirstOrDefault(c => c is not null && File.Exists(c));
    }

    public static async Task<string> Output(string bin, string[] args, string emptyMessage)
    {
        var r = await Proc.Run(bin, args, timeout: TimeSpan.FromSeconds(20)).ConfigureAwait(false);
        var output = r.Out.Trim();
        if (!r.Ok || output.Length == 0) throw new ApiException(string.IsNullOrWhiteSpace(r.Err) ? emptyMessage : r.Err.Trim());
        return output;
    }
}

public static class GhCli
{
    public static string? Binary => CliTool.Find("gh", "GitHub CLI");

    public static async Task<string> Token(string host = GitProviders.GitHubHost)
    {
        if (Binary is not { } bin) throw new ApiException("GitHub CLI (gh) not found");
        return await CliTool.Output(bin, new[] { "auth", "token", "--hostname", host }, $"gh returned no token. Run gh auth login --hostname {host}").ConfigureAwait(false);
    }
}

public static class GLabCli
{
    public static string? Binary => CliTool.Find("glab", "glab", "GitLab CLI");

    public static async Task<string> Token(string host = "gitlab.com")
    {
        if (Binary is not { } bin) throw new ApiException("GitLab CLI (glab) not found");
        return await CliTool.Output(bin, new[] { "config", "get", "token", "--host", host }, $"glab returned no token. Run glab auth login --hostname {host}").ConfigureAwait(false);
    }
}

public sealed record DeviceCode(string Code, string UserCode, string VerificationUri, int ExpiresIn, int Interval);

public sealed record DeviceToken(string AccessToken, string? RefreshToken, int? ExpiresIn)
{
    public GitCredential Credential(string? clientId = null) => new()
    {
        Token = AccessToken,
        RefreshToken = RefreshToken,
        ExpiresAt = ExpiresIn is { } s ? DateTimeOffset.Now.AddSeconds(s) : null,
        ClientId = clientId
    };
}

public sealed record DeviceEndpoint(string CodeUrl, string TokenUrl, string Scopes, string Service);

public static class DeviceFlow
{
    public const string Scopes = "repo read:org read:project project notifications workflow";

    private static readonly HttpClient SharedHttp = new() { Timeout = TimeSpan.FromSeconds(30) };

    public static DeviceEndpoint GitHub(string host = GitProviders.GitHubHost) =>
        new($"https://{host}/login/device/code", $"https://{host}/login/oauth/access_token", Scopes, host == GitProviders.GitHubHost ? "GitHub" : host);

    public static DeviceEndpoint GitLab(string host) =>
        new($"https://{host}/oauth/authorize_device", $"https://{host}/oauth/token", "api", host);

    private static async Task<JsonObject> Post(HttpClient http, string url, Dictionary<string, string> form, string service, CancellationToken ct)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var target)) throw new ApiException($"Invalid address: {url}");
        using var req = new HttpRequestMessage(HttpMethod.Post, target) { Content = new FormUrlEncodedContent(form) };
        req.Headers.Accept.ParseAdd("application/json");
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        using var response = await http.SendAsync(req, ct).ConfigureAwait(false);
        var text = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        try
        {
            if (JsonNode.Parse(text) is JsonObject o) return o;
        }
        catch (System.Text.Json.JsonException)
        {
        }
        throw new ApiException($"Invalid response from {service}", (int)response.StatusCode);
    }

    public static async Task<DeviceCode> Start(string clientId, DeviceEndpoint? endpoint = null, CancellationToken ct = default)
    {
        var e = endpoint ?? GitHub();
        var o = await Post(SharedHttp, e.CodeUrl, new() { ["client_id"] = clientId, ["scope"] = e.Scopes }, e.Service, ct).ConfigureAwait(false);
        if ((o.Str("error_description") ?? o.Str("error")) is { } error) throw new ApiException(error);
        if (o.Str("device_code") is not { } dc || o.Str("user_code") is not { } uc || o.Str("verification_uri") is not { } uri)
            throw new ApiException("Device flow is not available for this Client ID");
        return new DeviceCode(dc, uc, uri, o.Int("expires_in") ?? 900, o.Int("interval") ?? 5);
    }

    public static async Task<DeviceToken> Poll(string clientId, DeviceCode code, DeviceEndpoint? endpoint = null, CancellationToken ct = default)
    {
        var e = endpoint ?? GitHub();
        var interval = Math.Max(code.Interval, 5);
        var deadline = DateTimeOffset.Now.AddSeconds(code.ExpiresIn);
        while (DateTimeOffset.Now < deadline)
        {
            await Task.Delay(TimeSpan.FromSeconds(interval), ct).ConfigureAwait(false);
            var o = await Post(SharedHttp, e.TokenUrl, new()
            {
                ["client_id"] = clientId,
                ["device_code"] = code.Code,
                ["grant_type"] = "urn:ietf:params:oauth:grant-type:device_code"
            }, e.Service, ct).ConfigureAwait(false);
            if (o.Str("access_token") is { } token) return new DeviceToken(token, o.Str("refresh_token"), o.Int("expires_in"));
            switch (o.Str("error"))
            {
                case "authorization_pending":
                    continue;
                case "slow_down":
                    interval += 5;
                    continue;
                case { } error:
                    throw new ApiException(o.Str("error_description") ?? error);
                default:
                    throw new ApiException($"Unexpected response from {e.Service}");
            }
        }
        throw new ApiException("Timed out");
    }

    public static async Task<DeviceToken> Refresh(string clientId, string refreshToken, DeviceEndpoint endpoint, HttpClient? http = null, CancellationToken ct = default)
    {
        var o = await Post(http ?? SharedHttp, endpoint.TokenUrl, new()
        {
            ["client_id"] = clientId,
            ["refresh_token"] = refreshToken,
            ["grant_type"] = "refresh_token"
        }, endpoint.Service, ct).ConfigureAwait(false);
        if (o.Str("access_token") is not { } token) throw new ApiException(o.Str("error_description") ?? "Sign-in expired. Sign in again", 401);
        return new DeviceToken(token, o.Str("refresh_token") ?? refreshToken, o.Int("expires_in"));
    }
}
