using System.Net;
using System.Net.Http.Headers;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization;
using PullYard.Core.Api;
using PullYard.Core.Auth;
using PullYard.Core.Util;

namespace PullYard.Core.Forges;

public sealed record ForgeUser(string Login, string? Name, string? AvatarUrl, string? Id)
{
    public RestUser User => new() { Login = Login, Name = Name, AvatarUrl = AvatarUrl };
}

public enum ForgeIssueFilter { Assigned, Mine, Mentioned }

public enum ForgeInbox { None, ToDoList, Notifications }

public sealed record ForgeCapabilities(IReadOnlyList<ForgeFilter> PullFilters, IReadOnlyList<ForgeIssueFilter> IssueFilters, ForgeInbox Inbox, bool Repositories)
{
    public static ForgeCapabilities Initial { get; } = new(new[] { ForgeFilter.ReviewRequested, ForgeFilter.Mine }, Array.Empty<ForgeIssueFilter>(), ForgeInbox.None, false);

    public bool HasInbox => Inbox != ForgeInbox.None;

    public bool Same(ForgeCapabilities other) =>
        PullFilters.SequenceEqual(other.PullFilters) && IssueFilters.SequenceEqual(other.IssueFilters) && Inbox == other.Inbox && Repositories == other.Repositories;
}

public sealed record ForgeNotification(string Id, string Title, string Repo, string Kind, string Reason, string Url, DateTimeOffset UpdatedAt, Actor? Author, bool Unread);

public sealed record ForgeRepo(string FullName, string? Description, string Url, bool IsPrivate, DateTimeOffset? UpdatedAt, string? DefaultBranch, int? Stars);

public interface IForgeClient
{
    GitProvider Provider { get; }
    string Host { get; }
    Task<ForgeUser> CurrentUser(CancellationToken ct = default);
    Task<ForgeCapabilities> Capabilities(CancellationToken ct = default);
    Task<List<SearchItem>> PullRequests(ForgeFilter filter, ForgeState state, CancellationToken ct = default);
    Task<List<SearchItem>> Issues(ForgeIssueFilter filter, ForgeState state, CancellationToken ct = default) => Task.FromResult(new List<SearchItem>());
    Task<List<ForgeNotification>> Notifications(CancellationToken ct = default) => Task.FromResult(new List<ForgeNotification>());
    Task MarkDone(ForgeNotification notification, CancellationToken ct = default) => Task.CompletedTask;
    Task<List<ForgeRepo>> Repositories(CancellationToken ct = default) => Task.FromResult(new List<ForgeRepo>());
}

public static class ForgeClients
{
    public static IForgeClient? Create(GitAccount account, GitCredential credential, Action<GitCredential>? persist = null, HttpClient? http = null) => account.Provider switch
    {
        GitProvider.GitLab => new GitLabClient(account, credential, persist, http),
        GitProvider.Bitbucket => new BitbucketClient(account, credential, http),
        GitProvider.Gitea => new GiteaClient(account, credential, http),
        _ => null
    };
}

public sealed class ForgeHttp
{
    private static readonly HttpClient SharedHttp = new(new SocketsHttpHandler
    {
        AutomaticDecompression = DecompressionMethods.All,
        PooledConnectionLifetime = TimeSpan.FromMinutes(5),
        ConnectTimeout = TimeSpan.FromSeconds(10)
    })
    { Timeout = TimeSpan.FromSeconds(40) };

    private readonly HttpClient http;
    private readonly SemaphoreSlim refreshGate = new(1, 1);

    public string Base { get; }
    public string Service { get; }
    public string Authorization { get; private set; }
    public Func<CancellationToken, Task<string>>? Refresh { get; set; }

    public ForgeHttp(string baseUrl, string service, string authorization, HttpClient? http = null)
    {
        Base = baseUrl;
        Service = service;
        Authorization = authorization;
        this.http = http ?? SharedHttp;
    }

    public static string Query(IEnumerable<(string Key, string Value)> query) =>
        string.Join("&", query.Select(q => Uri.EscapeDataString(q.Key) + "=" + Uri.EscapeDataString(q.Value)));

    public Uri Url(string path, IEnumerable<(string Key, string Value)>? query = null)
    {
        var raw = path.StartsWith("http", StringComparison.OrdinalIgnoreCase) ? path : Base + path;
        var q = query is null ? "" : Query(query);
        if (q.Length > 0) raw += (raw.Contains('?') ? "&" : "?") + q;
        return Uri.TryCreate(raw, UriKind.Absolute, out var u) && (u.Scheme == Uri.UriSchemeHttps || u.Scheme == Uri.UriSchemeHttp)
            ? u
            : throw new ApiException($"Invalid address: {Base}{path}");
    }

    public async Task<T> Get<T>(string path, IEnumerable<(string Key, string Value)>? query = null, CancellationToken ct = default)
    {
        var data = await Call(HttpMethod.Get, path, query, ct).ConfigureAwait(false);
        try
        {
            return JsonSerializer.Deserialize<T>(data, Json.Rest) ?? throw new ApiException($"Empty response from {Service}");
        }
        catch (JsonException e)
        {
            throw new ApiException($"Failed to parse response from {Service}: {e.Message}");
        }
    }

    public async Task Post(string path, IEnumerable<(string Key, string Value)>? query = null, string method = "POST", CancellationToken ct = default) =>
        await Call(new HttpMethod(method), path, query, ct).ConfigureAwait(false);

    private async Task<byte[]> Call(HttpMethod method, string path, IEnumerable<(string Key, string Value)>? query, CancellationToken ct)
    {
        var target = Url(path, query);
        var used = Authorization;
        var (status, data) = await Send(method, target, used, ct).ConfigureAwait(false);
        if (status == 401 && Refresh is { } refresh)
        {
            await refreshGate.WaitAsync(ct).ConfigureAwait(false);
            try
            {
                if (Authorization == used) Authorization = await refresh(ct).ConfigureAwait(false);
            }
            finally
            {
                refreshGate.Release();
            }
            (status, data) = await Send(method, target, Authorization, ct).ConfigureAwait(false);
        }
        if (status is < 200 or >= 300)
        {
            if (status == 401) throw new ApiException("Token is invalid or revoked (401)", 401);
            throw new ApiException($"{Service} {status}: {Message(data) ?? ((HttpStatusCode)status).ToString()}", status);
        }
        return data;
    }

    private async Task<(int Status, byte[] Data)> Send(HttpMethod method, Uri url, string authorization, CancellationToken ct)
    {
        using var req = new HttpRequestMessage(method, url);
        req.Headers.TryAddWithoutValidation("Authorization", authorization);
        req.Headers.Accept.ParseAdd("application/json");
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        req.Headers.CacheControl = new CacheControlHeaderValue { NoCache = true };
        using var response = await http.SendAsync(req, ct).ConfigureAwait(false);
        var data = await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
        return ((int)response.StatusCode, data);
    }

    public static string? Message(byte[] data)
    {
        try
        {
            if (JsonNode.Parse(data) is not JsonObject o) return null;
            return o.Str("message") ?? o.Obj("error").Str("message") ?? o.Str("error_description") ?? o.Str("error");
        }
        catch (JsonException)
        {
            return null;
        }
    }

    public static string Basic(string user, string secret) => "Basic " + Convert.ToBase64String(Encoding.UTF8.GetBytes($"{user}:{secret}"));
}

public static class ForgeItems
{
    public static SearchItem Item(int number, string title, string url, string state, bool draft, DateTimeOffset created, DateTimeOffset updated, string? baseRef, string? author, string? avatar, string repo, int? comments, IEnumerable<LabelInfo> labels, string? review = null)
    {
        var labelList = labels.ToList();
        return new SearchItem
        {
            Typename = "PullRequest",
            Number = number,
            Title = title,
            Url = url,
            PrState = state,
            IsDraft = draft,
            CreatedAt = created,
            UpdatedAt = updated,
            BaseRefName = baseRef,
            Author = author is null ? null : new Actor { Login = author, AvatarUrl = avatar },
            Repository = new RepoName { NameWithOwner = repo },
            ReviewDecision = review,
            Comments = new Count { TotalCount = comments ?? 0 },
            Labels = labelList.Count == 0 ? null : new Nodes<LabelInfo> { Items = labelList }
        };
    }

    public static SearchItem Issue(int number, string title, string url, bool open, DateTimeOffset created, DateTimeOffset updated, string? author, string? avatar, string repo, int? comments, IEnumerable<LabelInfo> labels, bool completed = true)
    {
        var labelList = labels.ToList();
        return new SearchItem
        {
            Typename = "Issue",
            Number = number,
            Title = title,
            Url = url,
            IssueState = open ? "OPEN" : "CLOSED",
            StateReason = open ? null : completed ? "COMPLETED" : "NOT_PLANNED",
            CreatedAt = created,
            UpdatedAt = updated,
            Author = author is null ? null : new Actor { Login = author, AvatarUrl = avatar },
            Repository = new RepoName { NameWithOwner = repo },
            Comments = new Count { TotalCount = comments ?? 0 },
            Labels = labelList.Count == 0 ? null : new Nodes<LabelInfo> { Items = labelList }
        };
    }

    public static LabelInfo Label(string name, string? color) => new() { Name = name, Color = (color ?? "888888").Replace("#", "") };

    public static bool Matches(this ForgeState state, SearchItem item)
    {
        var open = item.IsPR ? item.PrState == "OPEN" : item.IssueState == "OPEN";
        return state switch
        {
            ForgeState.Open => open,
            ForgeState.Closed => !open,
            _ => true
        };
    }

    public static List<SearchItem> NewestFirst(this IEnumerable<SearchItem> items, int limit = 100)
    {
        var seen = new HashSet<string>();
        return items.Where(i => seen.Add(i.Id)).OrderByDescending(i => i.UpdatedAt).Take(limit).ToList();
    }
}

[JsonConverter(typeof(ForgeLabelConverter))]
public sealed class ForgeLabel
{
    public string Name { get; set; } = "";
    public string? Color { get; set; }

    public LabelInfo Info => ForgeItems.Label(Name, Color);
}

public sealed class ForgeLabelConverter : JsonConverter<ForgeLabel>
{
    public override ForgeLabel? Read(ref Utf8JsonReader reader, Type typeToConvert, JsonSerializerOptions options)
    {
        if (reader.TokenType == JsonTokenType.String) return new ForgeLabel { Name = reader.GetString() ?? "" };
        var node = JsonNode.Parse(ref reader);
        return new ForgeLabel { Name = node.Str("name") ?? "", Color = node.Str("color") };
    }

    public override void Write(Utf8JsonWriter writer, ForgeLabel value, JsonSerializerOptions options)
    {
        writer.WriteStartObject();
        writer.WriteString("name", value.Name);
        if (value.Color is { } color) writer.WriteString("color", color);
        writer.WriteEndObject();
    }
}
