using System.Diagnostics;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace PullYard.Core.Util;

public sealed record RequestEntry(
    DateTimeOffset Timestamp,
    string Kind,
    string Name,
    string Origin,
    double DurationMs,
    long Bytes,
    int Status,
    string? Error,
    int? RateLimitRemaining,
    long? RateLimitReset,
    string? RateLimitResource);

public sealed record TickEntry(DateTimeOffset Timestamp, double DurationMs, int Requests, bool Finished);

public sealed record SlowUiEntry(DateTimeOffset Timestamp, double DurationMs);

public sealed record RequestGroup(string Name, int Count, double AvgMs, double P95Ms, long TotalBytes);

public sealed record RateLimitState(string Resource, int Remaining, DateTimeOffset? Reset);

public sealed record DiagnosticsSummary(
    int Requests,
    long TotalBytes,
    double AvgMs,
    double P95Ms,
    RateLimitState? LastRateLimit,
    IReadOnlyList<RateLimitState> RateLimits,
    IReadOnlyList<TickEntry> RecentTicks,
    int SlowUiCount,
    double SlowUiMaxMs);

public sealed class LiveTick
{
    private readonly long started = Stopwatch.GetTimestamp();
    private long ended;
    private int pending = 1;
    private int requests;

    internal LiveTick(LiveTick? previous)
    {
        Previous = previous;
    }

    internal LiveTick? Previous { get; }

    public DateTimeOffset Timestamp { get; } = DateTimeOffset.Now;

    internal void Enter()
    {
        Interlocked.Increment(ref requests);
        Interlocked.Increment(ref pending);
    }

    internal void Leave()
    {
        if (Interlocked.Decrement(ref pending) == 0) Volatile.Write(ref ended, Stopwatch.GetTimestamp());
    }

    public TickEntry Snapshot()
    {
        var end = Volatile.Read(ref ended);
        var finished = end != 0;
        var ms = Stopwatch.GetElapsedTime(started, finished ? end : Stopwatch.GetTimestamp()).TotalMilliseconds;
        return new TickEntry(Timestamp, ms, Volatile.Read(ref requests), finished);
    }
}

public readonly struct RequestProbe
{
    private readonly long started;
    private readonly LiveTick? tick;

    private RequestProbe(long started, LiveTick? tick)
    {
        this.started = started;
        this.tick = tick;
    }

    public bool Active => started != 0;

    public static RequestProbe Start()
    {
        if (!Diagnostics.Enabled) return default;
        var tick = Diagnostics.CurrentTick;
        tick?.Enter();
        return new RequestProbe(Stopwatch.GetTimestamp(), tick);
    }

    public void Finish(string kind, string name, HttpResponseMessage? response, long bytes, Exception? error)
    {
        if (started == 0) return;
        var ms = Stopwatch.GetElapsedTime(started).TotalMilliseconds;
        var entry = new RequestEntry(
            DateTimeOffset.Now,
            kind,
            name,
            tick is null ? Diagnostics.OriginUser : Diagnostics.OriginLiveTick,
            ms,
            bytes,
            response is null ? 0 : (int)response.StatusCode,
            error is null ? null : Diagnostics.Describe(error),
            response is null ? null : Diagnostics.IntHeader(response, "X-RateLimit-Remaining"),
            response is null ? null : Diagnostics.LongHeader(response, "X-RateLimit-Reset"),
            response is null ? null : Diagnostics.Header(response, "X-RateLimit-Resource"));
        Diagnostics.Shared.Add(entry);
        tick?.Leave();
    }
}

public sealed class Diagnostics
{
    public const int RequestCapacity = 1000;
    public const int TickCapacity = 200;
    public const int SlowUiCapacity = 200;
    public const string KindGraphQL = "graphql";
    public const string KindRest = "rest";
    public const string OriginUser = "user";
    public const string OriginLiveTick = "live-tick";

    public static Diagnostics Shared { get; } = new();

    private static volatile bool enabled;
    private static readonly AsyncLocal<LiveTick?> currentTick = new();
    private static readonly Regex OperationRx = new(@"^\s*(?:query|mutation|subscription)\s+([A-Za-z_][A-Za-z0-9_]*)", RegexOptions.Compiled);

    public static bool Enabled
    {
        get => enabled;
        set => enabled = value;
    }

    internal static LiveTick? CurrentTick => currentTick.Value;

    private readonly object gate = new();
    private readonly Ring<RequestEntry> requests = new(RequestCapacity);
    private readonly Ring<LiveTick> ticks = new(TickCapacity);
    private readonly Ring<SlowUiEntry> slowUi = new(SlowUiCapacity);
    private long version;

    public long Version => Interlocked.Read(ref version);

    public static LiveTick? BeginLiveTick()
    {
        if (!enabled) return null;
        var tick = new LiveTick(currentTick.Value);
        currentTick.Value = tick;
        Shared.Add(tick);
        return tick;
    }

    public static void EndLiveTick(LiveTick tick)
    {
        currentTick.Value = tick.Previous;
        tick.Leave();
    }

    public void Add(RequestEntry entry)
    {
        lock (gate) requests.Add(entry);
        Interlocked.Increment(ref version);
    }

    public void Add(LiveTick tick)
    {
        lock (gate) ticks.Add(tick);
        Interlocked.Increment(ref version);
    }

    public void AddSlowUi(double durationMs)
    {
        lock (gate) slowUi.Add(new SlowUiEntry(DateTimeOffset.Now, durationMs));
        Interlocked.Increment(ref version);
    }

    public void Clear()
    {
        lock (gate)
        {
            requests.Clear();
            ticks.Clear();
            slowUi.Clear();
        }
        Interlocked.Increment(ref version);
    }

    public List<RequestEntry> Requests()
    {
        lock (gate) return requests.NewestFirst();
    }

    public List<TickEntry> Ticks()
    {
        List<LiveTick> list;
        lock (gate) list = ticks.NewestFirst();
        return list.Select(t => t.Snapshot()).ToList();
    }

    public List<SlowUiEntry> SlowUi()
    {
        lock (gate) return slowUi.NewestFirst();
    }

    public DiagnosticsSummary Summary(int recentTicks = 10) => Summarize(Requests(), Ticks(), SlowUi(), recentTicks);

    public static string GraphQLName(string query, string caller)
    {
        var m = OperationRx.Match(query);
        if (m.Success) return m.Groups[1].Value;
        if (caller.Length == 0) return KindGraphQL;
        return char.ToLowerInvariant(caller[0]) + caller[1..];
    }

    public static string RestName(HttpMethod method, Uri? uri) => uri is null ? method.Method : method.Method + " " + (uri.IsAbsoluteUri ? uri.AbsolutePath : uri.OriginalString.Split('?', 2)[0]);

    internal static string Describe(Exception e) => e is OperationCanceledException ? "cancelled" : $"{e.GetType().Name}: {e.Message}";

    internal static string? Header(HttpResponseMessage response, string name) =>
        response.Headers.TryGetValues(name, out var values) ? values.FirstOrDefault() : null;

    internal static int? IntHeader(HttpResponseMessage response, string name) =>
        int.TryParse(Header(response, name), System.Globalization.NumberStyles.Integer, System.Globalization.CultureInfo.InvariantCulture, out var v) ? v : null;

    internal static long? LongHeader(HttpResponseMessage response, string name) =>
        long.TryParse(Header(response, name), System.Globalization.NumberStyles.Integer, System.Globalization.CultureInfo.InvariantCulture, out var v) ? v : null;

    public static double Percentile(IEnumerable<double> values, double p)
    {
        var sorted = values.OrderBy(v => v).ToList();
        if (sorted.Count == 0) return 0;
        var rank = (int)Math.Ceiling(p / 100 * sorted.Count);
        return sorted[Math.Clamp(rank - 1, 0, sorted.Count - 1)];
    }

    public static DiagnosticsSummary Summarize(IReadOnlyList<RequestEntry> requests, IReadOnlyList<TickEntry> ticks, IReadOnlyList<SlowUiEntry> slow, int recentTicks = 10)
    {
        var limits = new Dictionary<string, RateLimitState>();
        RateLimitState? last = null;
        foreach (var r in requests.OrderByDescending(r => r.Timestamp))
        {
            if (r.RateLimitRemaining is not { } remaining) continue;
            var reset = r.RateLimitReset is { } s ? DateTimeOffset.FromUnixTimeSeconds(s) : (DateTimeOffset?)null;
            var state = new RateLimitState(r.RateLimitResource ?? "", remaining, reset);
            last ??= state;
            limits.TryAdd(state.Resource, state);
        }
        return new DiagnosticsSummary(
            requests.Count,
            requests.Sum(r => r.Bytes),
            requests.Count == 0 ? 0 : requests.Average(r => r.DurationMs),
            Percentile(requests.Select(r => r.DurationMs), 95),
            last,
            limits.Values.OrderBy(l => l.Resource, StringComparer.Ordinal).ToList(),
            ticks.Where(t => t.Finished).OrderByDescending(t => t.Timestamp).Take(recentTicks).ToList(),
            slow.Count,
            slow.Count == 0 ? 0 : slow.Max(s => s.DurationMs));
    }

    public static List<RequestGroup> Group(IEnumerable<RequestEntry> requests) =>
        requests.GroupBy(r => r.Name)
            .Select(g => new RequestGroup(g.Key, g.Count(), g.Average(r => r.DurationMs), Percentile(g.Select(r => r.DurationMs), 95), g.Sum(r => r.Bytes)))
            .OrderByDescending(g => g.Count)
            .ThenBy(g => g.Name, StringComparer.Ordinal)
            .ToList();

    public string ExportJsonLines() => ExportJsonLines(Requests(), Ticks(), SlowUi());

    public static string ExportJsonLines(IEnumerable<RequestEntry> requests, IEnumerable<TickEntry> ticks, IEnumerable<SlowUiEntry> slow)
    {
        var lines = new List<(DateTimeOffset Ts, string Line)>();
        foreach (var r in requests)
            lines.Add((r.Timestamp, Line(w =>
            {
                w.WriteString("type", "request");
                w.WriteString("ts", Timestamp(r.Timestamp));
                w.WriteString("kind", r.Kind);
                w.WriteString("name", r.Name);
                w.WriteString("origin", r.Origin);
                w.WriteNumber("durationMs", Math.Round(r.DurationMs, 1));
                w.WriteNumber("bytes", r.Bytes);
                w.WriteNumber("status", r.Status);
                if (r.Error is { } e) w.WriteString("error", e);
                else w.WriteNull("error");
                if (r.RateLimitRemaining is { } rem) w.WriteNumber("rateLimitRemaining", rem);
                else w.WriteNull("rateLimitRemaining");
                if (r.RateLimitReset is { } reset) w.WriteNumber("rateLimitReset", reset);
                else w.WriteNull("rateLimitReset");
                if (r.RateLimitResource is { } res) w.WriteString("rateLimitResource", res);
                else w.WriteNull("rateLimitResource");
            })));
        foreach (var t in ticks.Where(t => t.Finished))
            lines.Add((t.Timestamp, Line(w =>
            {
                w.WriteString("type", "tick");
                w.WriteString("ts", Timestamp(t.Timestamp));
                w.WriteNumber("durationMs", Math.Round(t.DurationMs, 1));
                w.WriteNumber("requests", t.Requests);
            })));
        foreach (var s in slow)
            lines.Add((s.Timestamp, Line(w =>
            {
                w.WriteString("type", "slow-ui");
                w.WriteString("ts", Timestamp(s.Timestamp));
                w.WriteNumber("durationMs", Math.Round(s.DurationMs, 1));
            })));
        var sb = new StringBuilder();
        foreach (var (_, line) in lines.OrderBy(l => l.Ts)) sb.Append(line).Append('\n');
        return sb.ToString();
    }

    private static string Timestamp(DateTimeOffset ts) => ts.ToUniversalTime().ToString("yyyy-MM-dd'T'HH:mm:ss.fff'Z'", System.Globalization.CultureInfo.InvariantCulture);

    private static string Line(Action<Utf8JsonWriter> write)
    {
        using var stream = new MemoryStream();
        using (var w = new Utf8JsonWriter(stream))
        {
            w.WriteStartObject();
            write(w);
            w.WriteEndObject();
        }
        return Encoding.UTF8.GetString(stream.ToArray());
    }

    public string SummaryText() => SummaryText(Requests(), Ticks(), SlowUi());

    public static string SummaryText(IReadOnlyList<RequestEntry> requests, IReadOnlyList<TickEntry> ticks, IReadOnlyList<SlowUiEntry> slow)
    {
        var c = System.Globalization.CultureInfo.InvariantCulture;
        var s = Summarize(requests, ticks, slow);
        var sb = new StringBuilder();
        sb.Append(c, $"{AppInfo.DisplayName} {AppInfo.Version} diagnostics, {DateTimeOffset.Now.ToString("yyyy-MM-dd HH:mm:ss zzz", c)}\n");
        sb.Append(c, $"Requests: {s.Requests}, {Fmt.Bytes(s.TotalBytes)}, avg {s.AvgMs:0} ms, p95 {s.P95Ms:0} ms\n");
        var origins = requests.GroupBy(r => r.Origin).OrderBy(g => g.Key, StringComparer.Ordinal).Select(g => $"{g.Key} {g.Count()}");
        sb.Append(c, $"Origin: {string.Join(", ", origins)}\n");
        if (s.RateLimits.Count > 0)
            sb.Append(c, $"Rate limit: {string.Join(", ", s.RateLimits.Select(l => $"{(l.Resource.Length == 0 ? "?" : l.Resource)} {l.Remaining}{(l.Reset is { } r ? " (resets " + r.ToLocalTime().ToString("HH:mm:ss", c) + ")" : "")}"))}\n");
        if (s.RecentTicks.Count > 0)
            sb.Append(c, $"Live ticks (newest first): {string.Join(", ", s.RecentTicks.Select(t => $"{t.DurationMs:0} ms/{t.Requests} req"))}\n");
        sb.Append(c, $"Slow UI: {s.SlowUiCount}{(s.SlowUiCount > 0 ? $", max {s.SlowUiMaxMs:0} ms" : "")}\n");
        var groups = Group(requests);
        if (groups.Count > 0)
        {
            sb.Append("\nname\tcount\tavg ms\tp95 ms\tbytes\n");
            foreach (var g in groups) sb.Append(c, $"{g.Name}\t{g.Count}\t{g.AvgMs:0}\t{g.P95Ms:0}\t{g.TotalBytes}\n");
        }
        return sb.ToString();
    }

    private sealed class Ring<T>
    {
        private readonly T[] items;
        private int next;
        private int count;

        public Ring(int capacity)
        {
            items = new T[capacity];
        }

        public void Add(T item)
        {
            items[next] = item;
            next = (next + 1) % items.Length;
            if (count < items.Length) count++;
        }

        public List<T> NewestFirst()
        {
            var list = new List<T>(count);
            for (var i = 1; i <= count; i++) list.Add(items[(next - i + items.Length) % items.Length]);
            return list;
        }

        public void Clear()
        {
            Array.Clear(items);
            next = 0;
            count = 0;
        }
    }
}
