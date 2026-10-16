using System.Text;
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

public sealed record CiExcerpt(string Text, bool Truncated, bool FoundMarker);

public sealed record CiFailure(
    CheckContext Check,
    CiExcerpt? Log,
    IReadOnlyList<CheckRunAnnotation> Annotations,
    CheckRunOutput? Output,
    string? LogError)
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
    private static readonly Regex Marker = new(@"##\[error\]|(?<![\w-])(?i:error)(?:\[[A-Za-z]*\d+\])?:|\bFAILED\b|\bpanicked\b|Process completed with exit code [1-9]", RegexOptions.Compiled);

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
        var lines = cleanLog.Split('\n').Select(Shorten).ToList();
        while (lines.Count > 0 && lines[^1].Trim().Length == 0) lines.RemoveAt(lines.Count - 1);
        var markers = new List<int>();
        for (var i = 0; i < lines.Count; i++)
            if (IsMarker(lines[i])) markers.Add(i);

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
            var from = Math.Max(0, first - ContextLines);
            head = lines.GetRange(from, first - from);
            cutBefore = from > 0;
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
        int Size(IEnumerable<string> part) => part.Sum(l => l.Length + 1);
        var budget = Math.Max(0, maxChars);
        var errorSize = Size(errors);
        if (errorSize > budget)
        {
            var kept = new List<string>();
            var used = 0;
            foreach (var l in errors)
            {
                if (used + l.Length + 1 > budget - 16) break;
                kept.Add(l);
                used += l.Length + 1;
            }
            kept.Add("… (truncated)");
            errors = kept;
            head.Clear();
            truncated = true;
            cutBefore = true;
        }
        else
        {
            var room = budget - errorSize;
            var headSize = Size(head);
            var skip = 0;
            while (skip < head.Count && headSize > room)
            {
                headSize -= head[skip].Length + 1;
                skip++;
            }
            if (skip > 0)
            {
                head.RemoveRange(0, skip);
                truncated = true;
                cutBefore = true;
            }
        }

        var sb = new StringBuilder();
        if (cutBefore && head.Count > 0) sb.Append("…\n");
        foreach (var l in head) sb.Append(l).Append('\n');
        foreach (var l in errors) sb.Append(l).Append('\n');
        return new CiExcerpt(sb.ToString().TrimEnd('\n'), truncated, markers.Count > 0);
    }

    public static string TrimStart(string text, int maxChars)
    {
        if (text.Length <= maxChars) return text;
        var cut = text.Length - Math.Max(0, maxChars - 2);
        var newline = text.IndexOf('\n', cut);
        var start = newline >= 0 && newline < text.Length - 1 ? newline + 1 : cut;
        return "…\n" + text[start..];
    }

    private static string Shorten(string line) => line.Length > MaxLineLength ? line[..MaxLineLength] + " …" : line;
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

    public Task<string> JobLog(long jobId, CancellationToken ct) => api.JobLog(repo, jobId, ct);

    public Task<List<CheckRunAnnotation>> Annotations(long checkRunId, CancellationToken ct) => api.CheckRunAnnotations(repo, checkRunId, ct);

    public Task<CheckRunDetail> CheckRun(long checkRunId, CancellationToken ct) => api.CheckRun(repo, checkRunId, ct);
}

public static class CiFailures
{
    public const int MaxCharsPerJob = 40_000;
    public const int MaxCharsTotal = 120_000;
    public const int MaxChecks = 12;
    public const int MaxAnnotations = 20;
    public const int Parallelism = 4;

    public static List<CheckContext> Failing(IEnumerable<CheckContext> checks) =>
        checks.Where(c => c.Outcome == CheckOutcome.Failure).OrderBy(c => c.FullTitle, StringComparer.OrdinalIgnoreCase).ToList();

    public static Task<List<CiFailure>> Collect(GitHubApi api, RepoRef repo, IReadOnlyList<CheckContext> checks, CancellationToken ct = default) =>
        Diagnostics.WithNote("ci-fix", () => Collect(new GitHubCiSource(api, repo), checks, ct));

    public static async Task<List<CiFailure>> Collect(ICiSource source, IReadOnlyList<CheckContext> checks, CancellationToken ct = default)
    {
        var picked = checks.Take(MaxChecks).ToList();
        using var gate = new SemaphoreSlim(Parallelism);
        var tasks = picked.Select(async c =>
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
            var output = string.IsNullOrWhiteSpace(check.Description) ? null : new CheckRunOutput { Summary = check.Description };
            return new CiFailure(check, null, Array.Empty<CheckRunAnnotation>(), output, null);
        }
        var annotationsTask = Annotations(source, id, ct);
        CiExcerpt? log = null;
        CheckRunOutput? detail = null;
        string? logError = null;
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
        return new CiFailure(check, log, annotations, Clip(detail), logError);
    }

    public static string DescribeLogError(Exception e) => e switch
    {
        ApiException { Status: 410 } => "the log has expired",
        ApiException { Status: 404 } => "the log was not found",
        ApiException { Status: 403 } => "no permission to read the log",
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
                a.Message = a.Message.Length > 2000 ? a.Message[..2000] + " …" : a.Message;
                a.RawDetails = a.RawDetails is { Length: > 2000 } r ? r[..2000] + " …" : a.RawDetails;
                return a;
            })
            .ToList();
    }

    private static CheckRunOutput? Clip(CheckRunOutput? o)
    {
        if (o is null) return null;
        string? Cut(string? s, int max) => s is null ? null : s.Length > max ? s[..max] + "\n… (truncated)" : s;
        var summary = Cut(o.Summary, MaxCharsPerJob / 2);
        var text = Cut(o.Text, MaxCharsPerJob / 2);
        if (string.IsNullOrWhiteSpace(o.Title) && string.IsNullOrWhiteSpace(summary) && string.IsNullOrWhiteSpace(text)) return null;
        return new CheckRunOutput { Title = o.Title, Summary = summary, Text = text };
    }
}
