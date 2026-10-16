using System.Collections.Concurrent;
using System.Diagnostics;
using System.Net;
using System.Net.Http.Headers;
using System.Runtime.CompilerServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using PullYard.Core.Util;

namespace PullYard.Core.Api;

public sealed class ApiException : Exception
{
    public int Status { get; }
    public bool SchemaError { get; init; }

    public ApiException(string message, int status = 0) : base(message)
    {
        Status = status;
    }
}

public sealed partial class GitHubApi
{
    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromSeconds(10);
    private static readonly TimeSpan TotalTimeout = TimeSpan.FromSeconds(40);

    private static readonly HttpClient SharedHttp = new(new SocketsHttpHandler
    {
        AutomaticDecompression = DecompressionMethods.All,
        PooledConnectionLifetime = TimeSpan.FromMinutes(5),
        ConnectTimeout = ConnectTimeout
    })
    { Timeout = TotalTimeout };

    private static readonly HttpClient SharedNoRedirectHttp = new(new SocketsHttpHandler
    {
        AllowAutoRedirect = false,
        AutomaticDecompression = DecompressionMethods.All,
        ConnectTimeout = ConnectTimeout
    })
    { Timeout = TotalTimeout };

    private readonly HttpClient http;
    private readonly HttpClient noRedirectHttp;

    public string Token { get; }

    public RateLimitPause RateLimit { get; } = new();

    public GitHubApi(string token) : this(token, SharedHttp, SharedNoRedirectHttp)
    {
    }

    internal GitHubApi(string token, HttpClient http, HttpClient? noRedirectHttp = null)
    {
        Token = token;
        this.http = http;
        this.noRedirectHttp = noRedirectHttp ?? http;
    }

    private Task<(HttpResponseMessage Response, byte[] Data)> Send(HttpClient client, Func<HttpRequestMessage> make, CancellationToken ct, string? graphQLQuery = null, string caller = "", bool keepRedirectBody = true) =>
        SendWithRetry(client, make, ct, graphQLQuery, caller, keepRedirectBody, (response, data) => RateLimit.Observe(response, data, graphQLQuery is not null, DateTimeOffset.Now));

    private static async Task<(HttpResponseMessage Response, byte[] Data)> SendWithRetry(HttpClient client, Func<HttpRequestMessage> make, CancellationToken ct, string? graphQLQuery = null, string caller = "", bool keepRedirectBody = true, Action<HttpResponseMessage, byte[]>? observe = null)
    {
        for (var attempt = 0; ; attempt++)
        {
            using var req = make();
            var retryable = attempt == 0 && RetryPolicy.IsRead(req.Method, graphQLQuery);
            var started = Stopwatch.GetTimestamp();
            HttpResponseMessage response;
            byte[] data;
            try
            {
                (response, data) = await RecordedHttp.Send(client, req, ct, graphQLQuery, caller, keepRedirectBody, retry: attempt > 0).ConfigureAwait(false);
            }
            catch (Exception e) when (retryable && RetryPolicy.IsTransient(e, ct, Stopwatch.GetElapsedTime(started), client.Timeout))
            {
                await Backoff(ct).ConfigureAwait(false);
                continue;
            }
            observe?.Invoke(response, data);
            if (retryable && RetryPolicy.IsTransient((int)response.StatusCode))
            {
                response.Dispose();
                await Backoff(ct).ConfigureAwait(false);
                continue;
            }
            return (response, data);
        }
    }

    private static async Task Backoff(CancellationToken ct)
    {
        var hold = Diagnostics.HoldTick();
        try
        {
            await Task.Delay(RetryPolicy.Delay, ct).ConfigureAwait(false);
        }
        finally
        {
            hold?.Leave();
        }
    }

    private HttpRequestMessage Request(HttpMethod method, string url, string accept = "application/vnd.github+json", bool noCache = true)
    {
        var req = new HttpRequestMessage(method, url);
        if (Uri.TryCreate(url, UriKind.Absolute, out var target) && target.Scheme == Uri.UriSchemeHttps && target.Host.Equals("api.github.com", StringComparison.OrdinalIgnoreCase))
            req.Headers.Authorization = new AuthenticationHeaderValue("Bearer", Token);
        req.Headers.Accept.ParseAdd(accept);
        req.Headers.Add("X-GitHub-Api-Version", "2022-11-28");
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        if (noCache) req.Headers.CacheControl = new CacheControlHeaderValue { NoCache = true };
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
        public sealed class ErrorExtensions
        {
            public string? Code { get; set; }
        }

        public string Message { get; set; } = "";
        public string? Type { get; set; }
        public ErrorExtensions? Extensions { get; set; }

        public bool IsSchema => Extensions?.Code is "undefinedField" or "undefinedType" or "argumentNotAccepted" || Message.Contains("doesn't exist on type", StringComparison.Ordinal);
    }

    private sealed class GraphQLResponse<T>
    {
        public T? Data { get; set; }
        public List<GraphQLError>? Errors { get; set; }
    }

    public sealed class EmptyData
    {
    }

    public async Task<T> GraphQL<T>(string query, IDictionary<string, object?>? variables = null, bool strict = false, CancellationToken ct = default, [CallerMemberName] string caller = "")
    {
        var vars = new JsonObject();
        if (variables is not null)
            foreach (var (k, v) in variables)
                if (v is not null) vars[k] = JsonSerializer.SerializeToNode(v, v.GetType(), Json.Plain);
        var payload = new JsonObject { ["query"] = query, ["variables"] = vars }.ToJsonString();
        HttpRequestMessage Make()
        {
            var req = Request(HttpMethod.Post, "https://api.github.com/graphql");
            req.Content = new StringContent(payload, Encoding.UTF8, "application/json");
            return req;
        }
        var (response, data) = await Send(http, Make, ct, query, caller).ConfigureAwait(false);
        using (response) await Check(response, data);
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
                if (errs is { Count: > 0 }) throw new ApiException(Describe(errs[0])) { SchemaError = errs[0].IsSchema };
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
            throw new ApiException(Describe(errors[0])) { SchemaError = errors[0].IsSchema };
        if (decoded.Data is not { } value) throw new ApiException("Empty response from GitHub");
        return value;
    }

    private static string Describe(GraphQLError e) => e.Type == "INSUFFICIENT_SCOPES" ? $"Insufficient token scopes: {e.Message}" : e.Message;

    public async Task<(byte[] Data, HttpResponseMessage Response)> RestRaw(string method, string path, object? body = null, CancellationToken ct = default)
    {
        var json = body is null ? null : JsonSerializer.Serialize(body, body.GetType(), Json.Plain);
        HttpRequestMessage Make()
        {
            var req = Request(new HttpMethod(method), Url(path));
            if (json is not null) req.Content = new StringContent(json, Encoding.UTF8, "application/json");
            return req;
        }
        var (response, data) = await Send(http, Make, ct).ConfigureAwait(false);
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
        var (response, data) = await Send(noRedirectHttp, () => Request(HttpMethod.Get, Url(path)), ct, keepRedirectBody: false).ConfigureAwait(false);
        using var _ = response;
        var code = (int)response.StatusCode;
        if (code is >= 300 and < 400 && response.Headers.Location is { } loc) return loc;
        await Check(response, data);
        throw new ApiException("Log unavailable");
    }

    public static async Task<string> Plain(Uri url, CancellationToken ct = default)
    {
        HttpRequestMessage Make()
        {
            var req = new HttpRequestMessage(HttpMethod.Get, url);
            req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
            return req;
        }
        var (response, data) = await SendWithRetry(SharedHttp, Make, ct).ConfigureAwait(false);
        using var _ = response;
        await Check(response, data);
        return Encoding.UTF8.GetString(data);
    }

    public static async Task<(string Text, bool Truncated)> PlainTail(Uri url, int maxBytes, CancellationToken ct = default)
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, url);
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        var (response, data, truncated) = await RecordedHttp.SendTail(SharedHttp, req, maxBytes, ct).ConfigureAwait(false);
        using var _ = response;
        await Check(response, data);
        return (Encoding.UTF8.GetString(data), truncated);
    }

    public async Task<byte[]?> Fetch(string path, string accept, CancellationToken ct = default)
    {
        var (response, data) = await Send(http, () => Request(HttpMethod.Get, Url(path), accept), ct).ConfigureAwait(false);
        using var _ = response;
        var code = (int)response.StatusCode;
        if (code == 404) return null;
        if (code is < 200 or >= 300) throw new ApiException($"GitHub {code}", code);
        return data;
    }

    private sealed record Validated(string? LastModified, string? ETag, byte[] Data, DateTimeOffset FetchedAt, TimeSpan PollInterval);

    private readonly ConcurrentDictionary<string, Validated> validated = new();
    private readonly object validatedGate = new();
    private long validatedGeneration;

    internal async Task<(byte[] Data, bool Changed)> Conditional(string path, bool live, CancellationToken ct = default)
    {
        var url = Url(path);
        long generation;
        Validated? prior;
        lock (validatedGate)
        {
            generation = validatedGeneration;
            validated.TryGetValue(url, out prior);
        }
        if (live && prior is not null && DateTimeOffset.Now - prior.FetchedAt < prior.PollInterval) return (prior.Data, false);
        var conditional = live ? prior : null;
        HttpRequestMessage Make()
        {
            var req = Request(HttpMethod.Get, url, noCache: false);
            if (conditional?.ETag is { } etag) req.Headers.TryAddWithoutValidation("If-None-Match", etag);
            if (conditional?.LastModified is { } since) req.Headers.TryAddWithoutValidation("If-Modified-Since", since);
            return req;
        }
        var (response, data) = await Send(http, Make, ct).ConfigureAwait(false);
        using var _ = response;
        var poll = PollInterval(response) ?? prior?.PollInterval ?? TimeSpan.Zero;
        if (response.StatusCode == HttpStatusCode.NotModified && conditional is not null)
        {
            Store(url, generation, conditional with { FetchedAt = DateTimeOffset.Now, PollInterval = poll });
            return (conditional.Data, false);
        }
        await Check(response, data);
        var lastModified = response.Content.Headers.TryGetValues("Last-Modified", out var values) ? values.FirstOrDefault() : null;
        var etag = response.Headers.ETag?.ToString();
        if (!Store(url, generation, new Validated(lastModified, etag, data, DateTimeOffset.Now, poll)) && live) return (data, false);
        return (data, true);
    }

    private bool Store(string url, long generation, Validated entry)
    {
        lock (validatedGate)
        {
            if (generation != validatedGeneration) return false;
            validated[url] = entry;
            return true;
        }
    }

    internal void Invalidate(string pathPrefix)
    {
        var prefix = Url(pathPrefix);
        lock (validatedGate)
        {
            validatedGeneration++;
            foreach (var key in validated.Keys)
                if (key.StartsWith(prefix, StringComparison.Ordinal)) validated.TryRemove(key, out _);
        }
    }

    private static TimeSpan? PollInterval(HttpResponseMessage response) =>
        response.Headers.TryGetValues("X-Poll-Interval", out var values) && int.TryParse(values.FirstOrDefault(), out var s) && s > 0 ? TimeSpan.FromSeconds(Math.Min(s, 600)) : null;

    internal static Dictionary<string, object?> V(params (string, object?)[] pairs) => pairs.ToDictionary(p => p.Item1, p => p.Item2);
}
