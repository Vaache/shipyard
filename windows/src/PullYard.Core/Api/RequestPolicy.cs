using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace PullYard.Core.Api;

public static class RetryPolicy
{
    private static readonly Regex MutationRx = new(@"^\s*mutation\b", RegexOptions.Compiled);

    internal static TimeSpan Delay = TimeSpan.FromMilliseconds(500);

    public static bool IsRead(HttpMethod method, string? graphQLQuery) =>
        graphQLQuery is not null ? !MutationRx.IsMatch(graphQLQuery) : method == HttpMethod.Get || method == HttpMethod.Head;

    public static bool IsTransient(int status) => status is 502 or 503 or 504;

    public static bool IsTransient(Exception e, CancellationToken ct, TimeSpan elapsed, TimeSpan total) => e switch
    {
        OperationCanceledException => !ct.IsCancellationRequested && elapsed < total - TimeSpan.FromSeconds(1),
        HttpRequestException h => IsConnectionFailure(h.HttpRequestError) || h.InnerException is HttpIOException { HttpRequestError: HttpRequestError.ResponseEnded },
        HttpIOException io => io.HttpRequestError == HttpRequestError.ResponseEnded,
        _ => false
    };

    private static bool IsConnectionFailure(HttpRequestError error) => error is HttpRequestError.ConnectionError or HttpRequestError.ResponseEnded;
}

public sealed class RateLimitPause
{
    public static readonly TimeSpan Fallback = TimeSpan.FromSeconds(60);
    public static readonly TimeSpan Longest = TimeSpan.FromHours(1);

    private readonly object gate = new();
    private DateTimeOffset? until;

    public event Action<DateTimeOffset>? Paused;

    public DateTimeOffset? Until
    {
        get
        {
            lock (gate) return until;
        }
    }

    public bool IsPaused(DateTimeOffset now) => Until is { } u && u > now;

    public void Observe(HttpResponseMessage response, byte[] data, bool graphQL, DateTimeOffset now)
    {
        if (Evaluate(response, data, graphQL, now) is { } next) PauseUntil(next);
    }

    public void PauseUntil(DateTimeOffset next)
    {
        lock (gate)
        {
            if (until is { } current && current >= next) return;
            until = next;
        }
        Paused?.Invoke(next);
    }

    public static DateTimeOffset? Evaluate(HttpResponseMessage response, byte[] data, bool graphQL, DateTimeOffset now)
    {
        var status = (int)response.StatusCode;
        var remaining = Header(response, "X-RateLimit-Remaining");
        var exhausted = remaining == "0";
        var limited = status == 429
            || status == 403 && (exhausted || response.Headers.RetryAfter is not null || Mentions(data, "rate limit") || Mentions(data, "Rate limit"))
            || graphQL && status is >= 200 and < 300 && Mentions(data, "\"RATE_LIMITED\"");
        if (!limited)
        {
            if (exhausted && status is >= 200 and < 300 && ResetAt(response) is { } reset && reset > now) return Clamp(reset, now);
            return null;
        }
        if (response.Headers.RetryAfter is { } retry)
        {
            if (retry.Delta is { } delta) return Clamp(now + delta, now);
            if (retry.Date is { } date) return Clamp(date, now);
        }
        if (ResetAt(response) is { } at && at > now) return Clamp(at, now);
        return now + Fallback;
    }

    private static DateTimeOffset Clamp(DateTimeOffset at, DateTimeOffset now) => at <= now ? now + TimeSpan.FromSeconds(1) : at - now > Longest ? now + Longest : at;

    private static DateTimeOffset? ResetAt(HttpResponseMessage response) =>
        long.TryParse(Header(response, "X-RateLimit-Reset"), NumberStyles.Integer, CultureInfo.InvariantCulture, out var s) ? DateTimeOffset.FromUnixTimeSeconds(s) : null;

    private static string? Header(HttpResponseMessage response, string name) =>
        response.Headers.TryGetValues(name, out var values) ? values.FirstOrDefault() : null;

    private static bool Mentions(byte[] data, string text) => data.AsSpan().IndexOf(Encoding.UTF8.GetBytes(text)) >= 0;
}
