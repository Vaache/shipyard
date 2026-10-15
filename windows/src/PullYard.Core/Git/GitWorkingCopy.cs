using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.Core.Git;

public sealed partial class GitWorkingCopy : ObservableObject, IDisposable
{
    public string Path { get; }
    private readonly SynchronizationContext? context;
    private FileSystemWatcher? watcher;
    private Timer? debounce;
    private int refreshing;

    [ObservableProperty] private bool isRepo;
    [ObservableProperty] private string? branch;
    [ObservableProperty] private RepoRef? repo;
    [ObservableProperty] private IReadOnlyList<DiffFile> changes = Array.Empty<DiffFile>();
    [ObservableProperty] private IReadOnlyList<GitCommitLine> unpushed = Array.Empty<GitCommitLine>();
    [ObservableProperty] private int ahead;
    [ObservableProperty] private bool loading;
    [ObservableProperty] private DateTimeOffset? lastRefresh;

    public event Action? Refreshed;

    public GitWorkingCopy(string path)
    {
        Path = path;
        context = SynchronizationContext.Current;
    }

    public int TotalAdditions => Changes.Sum(c => c.Additions);
    public int TotalDeletions => Changes.Sum(c => c.Deletions);

    public async Task Refresh()
    {
        if (Interlocked.Exchange(ref refreshing, 1) == 1) return;
        Loading = true;
        try
        {
            var dir = Path;
            var result = await Task.Run(() => Load(dir));
            IsRepo = result.IsRepo;
            Branch = result.Branch;
            Repo = result.Repo;
            Changes = result.Files;
            Unpushed = result.Commits;
            Ahead = result.Commits.Count;
            LastRefresh = DateTimeOffset.Now;
            OnPropertyChanged(nameof(TotalAdditions));
            OnPropertyChanged(nameof(TotalDeletions));
            Refreshed?.Invoke();
        }
        finally
        {
            Loading = false;
            Interlocked.Exchange(ref refreshing, 0);
        }
    }

    private sealed record LoadResult(bool IsRepo, string? Branch, RepoRef? Repo, List<DiffFile> Files, List<GitCommitLine> Commits);

    private static async Task<LoadResult> Load(string dir)
    {
        if (!Directory.Exists(dir) || !(await GitCli.Run(new[] { "rev-parse", "--is-inside-work-tree" }, dir).ConfigureAwait(false)).Ok)
            return new LoadResult(false, null, null, new(), new());
        var branch = (await GitCli.Run(new[] { "rev-parse", "--abbrev-ref", "HEAD" }, dir).ConfigureAwait(false)).Out.Trim();
        var repo = await GitCli.RemoteRepo(dir).ConfigureAwait(false);
        var files = GitCli.ParseUnifiedDiff((await GitCli.Run(new[] { "diff", "HEAD", "--no-color", "-M" }, dir).ConfigureAwait(false)).Out);
        if (files.Count == 0) files = GitCli.ParseUnifiedDiff((await GitCli.Run(new[] { "diff", "--no-color", "-M" }, dir).ConfigureAwait(false)).Out);
        var untracked = (await GitCli.Run(new[] { "ls-files", "--others", "--exclude-standard" }, dir).ConfigureAwait(false)).Out
            .Split('\n', StringSplitOptions.RemoveEmptyEntries).Select(s => s.TrimEnd('\r')).Take(200);
        foreach (var name in untracked)
        {
            var full = System.IO.Path.Combine(dir, name.Replace('/', System.IO.Path.DirectorySeparatorChar));
            string? patch = null;
            var adds = 0;
            try
            {
                var info = new FileInfo(full);
                if (info.Exists && info.Length < 300_000)
                {
                    var text = await File.ReadAllTextAsync(full).ConfigureAwait(false);
                    if (!text.Contains('\0'))
                    {
                        var lines = text.Replace("\r\n", "\n").Split('\n');
                        adds = lines.Length;
                        patch = $"@@ -0,0 +1,{lines.Length} @@\n" + string.Join("\n", lines.Select(l => "+" + l));
                    }
                }
            }
            catch
            {
            }
            files.Add(new PRFile { Filename = name, Status = "added", Additions = adds, Patch = patch });
        }
        var commits = new List<GitCommitLine>();
        var log = await GitCli.Run(new[] { "log", "--format=%H%x09%h%x09%s", "-n", "30", "@{upstream}..HEAD" }, dir).ConfigureAwait(false);
        if (log.Ok)
        {
            foreach (var line in log.Out.Split('\n', StringSplitOptions.RemoveEmptyEntries))
            {
                var parts = line.TrimEnd('\r').Split('\t', 3);
                if (parts.Length == 3) commits.Add(new GitCommitLine(parts[0], parts[1], parts[2]));
            }
        }
        return new LoadResult(true, branch.Length == 0 ? null : branch, repo, files.Select(f => new DiffFile(f)).ToList(), commits);
    }

    public void StartWatching()
    {
        if (watcher is not null || !Directory.Exists(Path)) return;
        try
        {
            watcher = new FileSystemWatcher(Path)
            {
                IncludeSubdirectories = true,
                NotifyFilter = NotifyFilters.FileName | NotifyFilters.DirectoryName | NotifyFilters.LastWrite | NotifyFilters.Size,
                InternalBufferSize = 64 * 1024
            };
            watcher.Changed += OnFsEvent;
            watcher.Created += OnFsEvent;
            watcher.Deleted += OnFsEvent;
            watcher.Renamed += OnFsEvent;
            watcher.EnableRaisingEvents = true;
        }
        catch
        {
            watcher = null;
        }
    }

    private static readonly string[] Ignored =
    {
        $"{System.IO.Path.DirectorySeparatorChar}.git{System.IO.Path.DirectorySeparatorChar}objects",
        $"{System.IO.Path.DirectorySeparatorChar}.git{System.IO.Path.DirectorySeparatorChar}logs",
        $"{System.IO.Path.DirectorySeparatorChar}node_modules{System.IO.Path.DirectorySeparatorChar}",
        $"{System.IO.Path.DirectorySeparatorChar}.dart_tool{System.IO.Path.DirectorySeparatorChar}",
        $"{System.IO.Path.DirectorySeparatorChar}build{System.IO.Path.DirectorySeparatorChar}",
        $"{System.IO.Path.DirectorySeparatorChar}bin{System.IO.Path.DirectorySeparatorChar}",
        $"{System.IO.Path.DirectorySeparatorChar}obj{System.IO.Path.DirectorySeparatorChar}"
    };

    private void OnFsEvent(object sender, FileSystemEventArgs e)
    {
        var p = e.FullPath;
        if (Ignored.Any(i => p.Contains(i, StringComparison.OrdinalIgnoreCase))) return;
        debounce?.Dispose();
        debounce = new Timer(FireRefresh, null, 800, Timeout.Infinite);
    }

    private void FireRefresh(object? state)
    {
        if (context is not null) context.Post(RefreshFromContext, null);
        else Refresh().Forget();
    }

    private void RefreshFromContext(object? state) => Refresh().Forget();

    public void Dispose()
    {
        watcher?.Dispose();
        debounce?.Dispose();
    }
}
