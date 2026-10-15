using System.Text.Json.Nodes;
using Shipyard.Core.Api;
using Shipyard.Core.Util;

namespace Shipyard.Core.Auth;

public static class GhCli
{
    public static string? Binary
    {
        get
        {
            var candidates = new[]
            {
                Proc.FindOnPath("gh"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "GitHub CLI", "gh.exe"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86), "GitHub CLI", "gh.exe"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", "GitHub CLI", "gh.exe"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "scoop", "shims", "gh.exe")
            };
            return candidates.FirstOrDefault(c => c is not null && File.Exists(c));
        }
    }

    public static async Task<string> Token()
    {
        if (Binary is not { } bin) throw new ApiException("GitHub CLI (gh) not found");
        var r = await Proc.Run(bin, new[] { "auth", "token", "--hostname", "github.com" }, timeout: TimeSpan.FromSeconds(20)).ConfigureAwait(false);
        var token = r.Out.Trim();
        if (!r.Ok || token.Length == 0) throw new ApiException(string.IsNullOrWhiteSpace(r.Err) ? "gh returned no token. Run gh auth login" : r.Err.Trim());
        return token;
    }
}

public sealed record DeviceCode(string Code, string UserCode, string VerificationUri, int ExpiresIn, int Interval);

public static class DeviceFlow
{
    public const string Scopes = "repo read:org read:project project notifications workflow";

    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(30) };

    private static async Task<JsonObject> Post(string url, Dictionary<string, string> form, CancellationToken ct)
    {
        using var req = new HttpRequestMessage(HttpMethod.Post, url) { Content = new FormUrlEncodedContent(form) };
        req.Headers.Accept.ParseAdd("application/json");
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        using var response = await Http.SendAsync(req, ct).ConfigureAwait(false);
        var text = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        return JsonNode.Parse(text) as JsonObject ?? throw new ApiException("Invalid response from GitHub");
    }

    public static async Task<DeviceCode> Start(string clientId, CancellationToken ct = default)
    {
        var o = await Post("https://github.com/login/device/code", new() { ["client_id"] = clientId, ["scope"] = Scopes }, ct).ConfigureAwait(false);
        if ((o.Str("error_description") ?? o.Str("error")) is { } e) throw new ApiException(e);
        if (o.Str("device_code") is not { } dc || o.Str("user_code") is not { } uc || o.Str("verification_uri") is not { } uri)
            throw new ApiException("Device flow is not available for this Client ID");
        return new DeviceCode(dc, uc, uri, o.Int("expires_in") ?? 900, o.Int("interval") ?? 5);
    }

    public static async Task<string> Poll(string clientId, DeviceCode code, CancellationToken ct = default)
    {
        var interval = Math.Max(code.Interval, 5);
        var deadline = DateTimeOffset.Now.AddSeconds(code.ExpiresIn);
        while (DateTimeOffset.Now < deadline)
        {
            await Task.Delay(TimeSpan.FromSeconds(interval), ct).ConfigureAwait(false);
            var o = await Post("https://github.com/login/oauth/access_token", new()
            {
                ["client_id"] = clientId,
                ["device_code"] = code.Code,
                ["grant_type"] = "urn:ietf:params:oauth:grant-type:device_code"
            }, ct).ConfigureAwait(false);
            if (o.Str("access_token") is { } token) return token;
            switch (o.Str("error"))
            {
                case "authorization_pending":
                    continue;
                case "slow_down":
                    interval += 5;
                    continue;
                case { } e:
                    throw new ApiException(o.Str("error_description") ?? e);
                default:
                    throw new ApiException("Unexpected response from GitHub");
            }
        }
        throw new ApiException("Timed out");
    }
}
