using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Shipyard.Core.Util;

namespace Shipyard.Core.Api;

public sealed class ApiException : Exception
{
    public int Status { get; }

    public ApiException(string message, int status = 0) : base(message)
    {
        Status = status;
    }
}

public sealed partial class GitHubApi
{
    private static readonly HttpClient Http = new(new SocketsHttpHandler
    {
        AutomaticDecompression = DecompressionMethods.All,
        PooledConnectionLifetime = TimeSpan.FromMinutes(5)
    })
    { Timeout = TimeSpan.FromSeconds(40) };

    private static readonly HttpClient NoRedirectHttp = new(new SocketsHttpHandler
    {
        AllowAutoRedirect = false,
        AutomaticDecompression = DecompressionMethods.All
    })
    { Timeout = TimeSpan.FromSeconds(40) };

    public string Token { get; }

    public GitHubApi(string token)
    {
        Token = token;
    }

    private HttpRequestMessage Request(HttpMethod method, string url, string accept = "application/vnd.github+json")
    {
        var req = new HttpRequestMessage(method, url);
        req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", Token);
        req.Headers.Accept.ParseAdd(accept);
        req.Headers.Add("X-GitHub-Api-Version", "2022-11-28");
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        req.Headers.CacheControl = new CacheControlHeaderValue { NoCache = true };
        return req;
    }

    private static string Url(string path) => path.StartsWith("http", StringComparison.OrdinalIgnoreCase) ? path : "https://api.github.com" + path;

    private static async Task Check(HttpResponseMessage response, byte[] data)
    {
        await Task.CompletedTask;
        var code = (int)response.StatusCode;
        if (code is >= 200 and < 300) return;
        string? msg = null;
        try
        {
            msg = JsonNode.Parse(data).Str("message");
        }
        catch
        {
        }
        msg ??= response.ReasonPhrase ?? code.ToString();
        if (code == 401) throw new ApiException("Token is invalid or revoked (401)", 401);
        throw new ApiException($"GitHub {code}: {msg}", code);
    }

    private sealed class GraphQLError
    {
        public string Message { get; set; } = "";
        public string? Type { get; set; }
    }

    private sealed class GraphQLResponse<T>
    {
        public T? Data { get; set; }
        public List<GraphQLError>? Errors { get; set; }
    }

    public sealed class EmptyData
    {
    }

    public async Task<T> GraphQL<T>(string query, IDictionary<string, object?>? variables = null, bool strict = false, CancellationToken ct = default)
    {
        using var req = Request(HttpMethod.Post, "https://api.github.com/graphql");
        var vars = new JsonObject();
        if (variables is not null)
            foreach (var (k, v) in variables)
                if (v is not null) vars[k] = JsonSerializer.SerializeToNode(v, v.GetType(), Json.Plain);
        var payload = new JsonObject { ["query"] = query, ["variables"] = vars };
        req.Content = new StringContent(payload.ToJsonString(), Encoding.UTF8, "application/json");
        using var response = await Http.SendAsync(req, ct).ConfigureAwait(false);
        var data = await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
        await Check(response, data);
        GraphQLResponse<T>? decoded;
        try
        {
            decoded = JsonSerializer.Deserialize<GraphQLResponse<T>>(data, Json.GraphQL);
        }
        catch (Exception e)
        {
            try
            {
                var errs = JsonSerializer.Deserialize<GraphQLResponse<EmptyData>>(data, Json.GraphQL)?.Errors;
                if (errs is { Count: > 0 }) throw new ApiException(Describe(errs[0]));
            }
            catch (ApiException)
            {
                throw;
            }
            catch
            {
            }
            throw new ApiException($"Failed to parse response: {e.Message}");
        }
        if (decoded is null) throw new ApiException("Empty response from GitHub");
        if (decoded.Errors is { Count: > 0 } errors && (strict || decoded.Data is null))
            throw new ApiException(Describe(errors[0]));
        if (decoded.Data is not { } data) throw new ApiException("Empty response from GitHub");
        return data;
    }

    private static string Describe(GraphQLError e) => e.Type == "INSUFFICIENT_SCOPES" ? $"Insufficient token scopes: {e.Message}" : e.Message;

    public async Task<(byte[] Data, HttpResponseMessage Response)> RestRaw(string method, string path, object? body = null, CancellationToken ct = default)
    {
        using var req = Request(new HttpMethod(method), Url(path));
        if (body is not null)
            req.Content = new StringContent(JsonSerializer.Serialize(body, body.GetType(), Json.Plain), Encoding.UTF8, "application/json");
        var response = await Http.SendAsync(req, ct).ConfigureAwait(false);
        var data = await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
        await Check(response, data);
        return (data, response);
    }

    public async Task<T> Rest<T>(string method, string path, object? body = null, CancellationToken ct = default)
    {
        var (data, response) = await RestRaw(method, path, body, ct).ConfigureAwait(false);
        response.Dispose();
        try
        {
            return JsonSerializer.Deserialize<T>(data, Json.Rest) ?? throw new ApiException("Empty response from GitHub");
        }
        catch (JsonException e)
        {
            throw new ApiException($"Failed to parse response: {e.Message}");
        }
    }

    public async Task Send(string method, string path, object? body = null, CancellationToken ct = default)
    {
        var (_, response) = await RestRaw(method, path, body, ct).ConfigureAwait(false);
        response.Dispose();
    }

    public async Task<Uri> RedirectTarget(string path, CancellationToken ct = default)
    {
        using var req = Request(HttpMethod.Get, Url(path));
        using var response = await NoRedirectHttp.SendAsync(req, ct).ConfigureAwait(false);
        var code = (int)response.StatusCode;
        if (code is >= 300 and < 400 && response.Headers.Location is { } loc) return loc;
        var data = await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
        await Check(response, data);
        throw new ApiException("Log unavailable");
    }

    public static async Task<string> Plain(Uri url, CancellationToken ct = default)
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, url);
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        using var response = await Http.SendAsync(req, ct).ConfigureAwait(false);
        var data = await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
        await Check(response, data);
        return Encoding.UTF8.GetString(data);
    }

    public async Task<byte[]?> Fetch(string path, string accept, CancellationToken ct = default)
    {
        using var req = Request(HttpMethod.Get, Url(path), accept);
        using var response = await Http.SendAsync(req, ct).ConfigureAwait(false);
        var code = (int)response.StatusCode;
        if (code == 404) return null;
        if (code is < 200 or >= 300) throw new ApiException($"GitHub {code}", code);
        return await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
    }

    internal static Dictionary<string, object?> V(params (string, object?)[] pairs) => pairs.ToDictionary(p => p.Item1, p => p.Item2);
}
