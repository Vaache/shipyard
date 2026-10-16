using System.Text.RegularExpressions;
using PullYard.Core.Util;

namespace PullYard.Core.Api;

public sealed class CheckRunAnnotation
{
    public string Path { get; set; } = "";
    public int? StartLine { get; set; }
    public int? EndLine { get; set; }
    public string? AnnotationLevel { get; set; }
    public string? Title { get; set; }
    public string Message { get; set; } = "";
    public string? RawDetails { get; set; }

    public string Location => Path.Length == 0 || Path == ".github" ? "" : StartLine is { } l and > 0 ? $"{Path}:{l}" : Path;
}

public sealed class CheckRunOutput
{
    public string? Title { get; set; }
    public string? Summary { get; set; }
    public string? Text { get; set; }
}

public sealed class CheckRunDetail
{
    public long Id { get; set; }
    public string? Name { get; set; }
    public string? Conclusion { get; set; }
    public string? HtmlUrl { get; set; }
    public string? DetailsUrl { get; set; }
    public CheckRunOutput? Output { get; set; }
}

public enum CiLogError { Expired, NotFound, Forbidden, Other }

public sealed record CiExcerpt(string Context, string Errors, bool Truncated, bool FoundMarker)
{
    public string Text => Context.Length == 0 ? Errors : Errors.Length == 0 ? Context : Context + "\n" + Errors;
}

public sealed record CiFailure(
    CheckContext Check,
    CiExcerpt? Log,
    IReadOnlyList<CheckRunAnnotation> Annotations,
    CheckRunOutput? Output,
    string? LogError,
    CiLogError? LogErrorKind = null)
{
    public string Conclusion => (Check.Typename == "StatusContext" ? Check.State : Check.Conclusion)?.ToLowerInvariant() ?? "failure";
}

public static class CiLog
{
    public const int ContextLines = 150;
    public const int AfterLines = 3;
    public const int MaxLineLength = 2000;

    private static readonly Regex Ansi = new(@"\x1B(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1B]*(?:\x07|\x1B\\)?|[@-_])", RegexOptions.Compiled);
    private static readonly Regex Timestamp = new(@"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2}) ?", RegexOptions.Compiled);
    private static readonly Regex Marker = new(
        @"##\[error\]|(?<![\w-])(?i:error)(?:\[[A-Za-z]*\d+\])?:|\berror TS\d+:|^\s*(?:--- )?FAIL\b|\bFAILED\b|\bpanicked\b|\bERR!|^\s*● |Process completed with exit code [1-9]",
        RegexOptions.Compiled);
    private static readonly Regex StepEnd = new(@"##\[error\]|Process completed with exit code [1-9]", RegexOptions.Compiled);

    public static string Clean(string raw)
    {
        var lines = raw.Replace("\r\n", "\n").Replace('\r', '\n').Split('\n');
        for (var i = 0; i < lines.Length; i++)
        {
            var l = lines[i];
            if (l.Length > 0 && l[0] == '﻿') l = l[1..];
            l = Timestamp.Replace(l, "", 1);
            if (l.IndexOf('\x1B') >= 0) l = Ansi.Replace(l, "");
            lines[i] = l;
        }
        return string.Join("\n", lines);
    }

    public static bool IsMarker(string line) => Marker.IsMatch(line);

    public static CiExcerpt Extract(string cleanLog, int maxChars)
    {
        var lines = cleanLog.Split('\n').Select(l => l.Length > MaxLineLength ? Cut(l, MaxLineLength) + " …" : l).ToList();
        while (lines.Count > 0 && lines[^1].Trim().Length == 0) lines.RemoveAt(lines.Count - 1);
        var markers = new List<int>();
        for (var i = 0; i < lines.Count; i++)
            if (IsMarker(lines[i])) markers.Add(i);

        var lastError = -1;
        for (var i = lines.Count - 1; i >= 0; i--)
        {
            if (!StepEnd.IsMatch(lines[i])) continue;
            lastError = i;
            break;
        }
        var stepStart = 0;
        if (lastError >= 0)
        {
            for (var i = lastError - 1; i >= 0; i--)
            {
                if (!lines[i].StartsWith("##[group]Run ", StringComparison.Ordinal)) continue;
                stepStart = i;
                break;
            }
            markers = markers.Where(m => m >= stepStart && m <= lastError).ToList();
        }

        List<string> head;
        var errors = new List<string>();
        bool cutBefore;
        if (markers.Count == 0)
        {
            var from = Math.Max(0, lines.Count - ContextLines);
            head = lines.GetRange(from, lines.Count - from);
            cutBefore = from > 0;
        }
        else
        {
            var first = markers[0];
            var from = Math.Max(stepStart, first - ContextLines);
            head = lines.GetRange(from, first - from);
            cutBefore = from > stepStart;
            var last = first - 1;
            foreach (var m in markers)
            {
                var start = Math.Max(m, last + 1);
                var end = Math.Min(lines.Count - 1, m + AfterLines);
                if (end < start) continue;
                if (start > last + 1 && errors.Count > 0) errors.Add("…");
                for (var j = start; j <= end; j++) errors.Add(lines[j]);
                last = end;
            }
        }

        var truncated = false;
        var budget = Math.Max(0, maxChars);
        var errorText = string.Join("\n", errors);
        if (errorText.Length > budget)
        {
            errorText = CutLines(errorText, budget);
            head.Clear();
            truncated = true;
        }
        var contextText = string.Join("\n", head);
        var room = Math.Max(0, budget - errorText.Length - 1);
        if (contextText.Length > room)
        {
            contextText = TrimStart(contextText, room);
            truncated = true;
        }
        else if (cutBefore && contextText.Length > 0 && contextText.Length + 2 <= room)
        {
            contextText = "…\n" + contextText;
        }
        return new CiExcerpt(contextText, errorText, truncated, markers.Count > 0);
    }

    public static string Cut(string s, int max)
    {
        if (s.Length <= max) return s;
        if (max <= 0) return "";
        var n = max;
        if (char.IsHighSurrogate(s[n - 1])) n--;
        return s[..n];
    }

    public static string CutLines(string text, int maxChars)
    {
        const string tail = "\n… (truncated)";
        if (text.Length <= maxChars) return text;
        if (maxChars <= tail.Length) return "";
        var keep = Cut(text, maxChars - tail.Length);
        var newline = keep.LastIndexOf('\n');
        if (newline > 0) keep = keep[..newline];
        return keep + tail;
    }

    public static string TrimStart(string text, int maxChars)
    {
        if (text.Length <= maxChars) return text;
        if (maxChars <= 2) return "";
        var cut = text.Length - (maxChars - 2);
        var newline = text.IndexOf('\n', cut);
        var start = newline >= 0 && newline < text.Length - 1 ? newline + 1 : cut;
        if (start < text.Length && char.IsLowSurrogate(text[start])) start++;
        return "…\n" + text[start..];
    }
}

public interface ICiSource
{
    Task<string> JobLog(long jobId, CancellationToken ct);
    Task<List<CheckRunAnnotation>> Annotations(long checkRunId, CancellationToken ct);
    Task<CheckRunDetail> CheckRun(long checkRunId, CancellationToken ct);
}

public sealed class GitHubCiSource : ICiSource
{
    private readonly GitHubApi api;
    private readonly RepoRef repo;

    public GitHubCiSource(GitHubApi api, RepoRef repo)
    {
        this.api = api;
        this.repo = repo;
    }

    public async Task<string> JobLog(long jobId, CancellationToken ct) => (await api.JobLogTail(repo, jobId, CiFailures.MaxLogBytes, ct).ConfigureAwait(false)).Text;

    public Task<List<CheckRunAnnotation>> Annotations(long checkRunId, CancellationToken ct) => api.CheckRunAnnotations(repo, checkRunId, ct);

    public Task<CheckRunDetail> CheckRun(long checkRunId, CancellationToken ct) => api.CheckRun(repo, checkRunId, ct);
}

public sealed class PreloadedCiSource : ICiSource
{
    private readonly ICiSource inner;
    private readonly IReadOnlyDictionary<long, string> logs;

    public PreloadedCiSource(ICiSource inner, IReadOnlyDictionary<long, string> logs)
    {
        this.inner = inner;
        this.logs = logs;
    }

    public Task<string> JobLog(long jobId, CancellationToken ct) => logs.TryGetValue(jobId, out var log) ? Task.FromResult(log) : inner.JobLog(jobId, ct);

    public Task<List<CheckRunAnnotation>> Annotations(long checkRunId, CancellationToken ct) => inner.Annotations(checkRunId, ct);

    public Task<CheckRunDetail> CheckRun(long checkRunId, CancellationToken ct) => inner.CheckRun(checkRunId, ct);
}

public static class CiFailures
{
    public const int MaxCharsPerJob = 40_000;
    public const int MaxCharsTotal = 120_000;
    public const int MaxOutputChars = 8_000;
    public const int MaxLogBytes = 8 * 1024 * 1024;
    public const int MaxChecks = 12;
    public const int MaxAnnotations = 20;
    public const int Parallelism = 3;

    public static bool IsFixable(CheckContext c) => c.Outcome == CheckOutcome.Failure && c.Conclusion != "ACTION_REQUIRED";

    public static List<CheckContext> Failing(IEnumerable<CheckContext> checks) =>
        checks.Where(IsFixable).OrderBy(c => c.FullTitle, StringComparer.OrdinalIgnoreCase).ToList();

    public static List<CheckContext> Limit(IReadOnlyList<CheckContext> failing, CheckContext? keep, int max = MaxChecks)
    {
        var list = failing.ToList();
        if (keep is not null && list.All(c => c.Id != keep.Id)) list.Insert(0, keep);
        if (list.Count <= max) return list;
        var picked = list.Take(max).ToList();
        if (keep is not null && picked.All(c => c.Id != keep.Id)) picked[^1] = list.First(c => c.Id == keep.Id);
        return list.Where(picked.Contains).ToList();
    }

    public static Task<List<CiFailure>> Collect(GitHubApi api, RepoRef repo, IReadOnlyList<CheckContext> checks, IReadOnlyDictionary<long, string>? knownLogs = null, CancellationToken ct = default)
    {
        ICiSource source = new GitHubCiSource(api, repo);
        if (knownLogs is { Count: > 0 }) source = new PreloadedCiSource(source, knownLogs);
        return Diagnostics.WithNote("ci-fix", () => Collect(source, checks, ct));
    }

    public static async Task<List<CiFailure>> Collect(ICiSource source, IReadOnlyList<CheckContext> checks, CancellationToken ct = default)
    {
        using var gate = new SemaphoreSlim(Parallelism);
        var tasks = checks.Select(async c =>
        {
            await gate.WaitAsync(ct).ConfigureAwait(false);
            try
            {
                return await CollectOne(source, c, ct).ConfigureAwait(false);
            }
            finally
            {
                gate.Release();
            }
        }).ToList();
        return (await Task.WhenAll(tasks).ConfigureAwait(false)).ToList();
    }

    public static async Task<CiFailure> CollectOne(ICiSource source, CheckContext check, CancellationToken ct)
    {
        if (check.Typename != "CheckRun" || check.DatabaseId is not { } id)
        {
            var output = string.IsNullOrWhiteSpace(check.Description) ? null : new CheckRunOutput { Summary = CiLog.Cut(check.Description, MaxOutputChars) };
            return new CiFailure(check, null, Array.Empty<CheckRunAnnotation>(), output, null);
        }
        var annotationsTask = Annotations(source, id, ct);
        CiExcerpt? log = null;
        CheckRunOutput? detail = null;
        string? logError = null;
        CiLogError? logErrorKind = null;
        if (check.IsActionsJob)
        {
            try
            {
                var raw = await source.JobLog(id, ct).ConfigureAwait(false);
                log = CiLog.Extract(CiLog.Clean(raw), MaxCharsPerJob);
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                throw;
            }
            catch (Exception e)
            {
                logErrorKind = ClassifyLogError(e);
                logError = DescribeLogError(e);
            }
        }
        else
        {
            try
            {
                detail = (await source.CheckRun(id, ct).ConfigureAwait(false)).Output;
            }
            catch (OperationCanceledException) when (ct.IsCancellationRequested)
            {
                throw;
            }
            catch
            {
            }
        }
        var annotations = await annotationsTask.ConfigureAwait(false);
        if (log is not null) annotations = annotations.Where(a => !a.Message.StartsWith("Process completed with exit code", StringComparison.Ordinal)).ToList();
        return new CiFailure(check, log, annotations, Clip(detail), logError, logErrorKind);
    }

    public static CiLogError ClassifyLogError(Exception e) => e switch
    {
        ApiException { Status: 410 } => CiLogError.Expired,
        ApiException { Status: 404 } => CiLogError.NotFound,
        ApiException { Status: 403 } => CiLogError.Forbidden,
        _ => CiLogError.Other
    };

    public static string DescribeLogError(Exception e) => ClassifyLogError(e) switch
    {
        CiLogError.Expired => "the log has expired",
        CiLogError.NotFound => "the log was not found",
        CiLogError.Forbidden => "no permission to read the log",
        _ => e.Message
    };

    private static async Task<List<CheckRunAnnotation>> Annotations(ICiSource source, long id, CancellationToken ct)
    {
        try
        {
            var all = await source.Annotations(id, ct).ConfigureAwait(false);
            return PickAnnotations(all);
        }
        catch (OperationCanceledException) when (ct.IsCancellationRequested)
        {
            throw;
        }
        catch
        {
            return new();
        }
    }

    public static List<CheckRunAnnotation> PickAnnotations(IEnumerable<CheckRunAnnotation> all)
    {
        var list = all.Where(a => !string.IsNullOrWhiteSpace(a.Message)).ToList();
        var failures = list.Where(a => a.AnnotationLevel == "failure").ToList();
        var chosen = failures.Count > 0 ? failures : list.Where(a => a.AnnotationLevel == "warning").ToList();
        return chosen
            .OrderBy(a => a.Path == ".github" ? 1 : 0)
            .ThenBy(a => a.Path, StringComparer.Ordinal)
            .ThenBy(a => a.StartLine ?? 0)
            .Take(MaxAnnotations)
            .Select(a =>
            {
                a.Message = a.Message.Length > 2000 ? CiLog.Cut(a.Message, 2000) + " …" : a.Message;
                a.RawDetails = a.RawDetails is { Length: > 2000 } r ? CiLog.Cut(r, 2000) + " …" : a.RawDetails;
                a.Title = a.Title is { Length: > 300 } t ? CiLog.Cut(t, 300) + " …" : a.Title;
                return a;
            })
            .ToList();
    }

    private static CheckRunOutput? Clip(CheckRunOutput? o)
    {
        if (o is null) return null;
        string? Trim(string? s) => s is null ? null : CiLog.CutLines(s, MaxOutputChars);
        var summary = Trim(o.Summary);
        var text = Trim(o.Text);
        if (string.IsNullOrWhiteSpace(o.Title) && string.IsNullOrWhiteSpace(summary) && string.IsNullOrWhiteSpace(text)) return null;
        return new CheckRunOutput { Title = o.Title is null ? null : CiLog.Cut(o.Title, 500), Summary = summary, Text = text };
    }
}
