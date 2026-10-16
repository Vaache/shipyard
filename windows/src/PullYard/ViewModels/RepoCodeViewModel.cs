using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core.Api;
using PullYard.Core.Util;

namespace PullYard.ViewModels;

public enum FileContentKind { Text, Markdown, Image, Binary }

public sealed record FileContent(FileContentKind Kind, string Text = "", bool Truncated = false, string Html = "", byte[]? Bytes = null, long Size = 0);

public sealed partial class RepoCodeViewModel : ObservableObject
{
    public RepoRef Repo { get; }
    public GitHubApi Api { get; }

    [ObservableProperty] private List<BranchInfo> branches = new();
    [ObservableProperty] private string currentRef = "";
    [ObservableProperty] private string dir = "";
    [ObservableProperty] private List<TreeEntry> entries = new();
    [ObservableProperty] private bool loadingDir;
    [ObservableProperty] private string? dirError;
    [ObservableProperty] private string? readme;
    [ObservableProperty] private string? selectedFile;
    [ObservableProperty] private FileContent? file;
    [ObservableProperty] private long? fileSize;
    [ObservableProperty] private bool loadingFile;
    [ObservableProperty] private string? fileError;
    [ObservableProperty] private List<string>? allPaths;
    [ObservableProperty] private bool pathsTruncated;
    [ObservableProperty] private bool loadingPaths;
    [ObservableProperty] private bool blameOn;
    [ObservableProperty] private List<BlameRange>? blame;
    [ObservableProperty] private bool loadingBlame;
    [ObservableProperty] private bool blameFailed;

    private readonly Dictionary<string, (List<TreeEntry> Entries, string? Readme)> dirCache = new();
    private readonly Dictionary<string, List<BlameRange>> blameCache = new();
    private string? blameKey;

    private static readonly HashSet<string> ImageExts = new(StringComparer.OrdinalIgnoreCase) { "png", "jpg", "jpeg", "gif", "webp", "bmp", "ico", "tiff", "svg" };
    private static readonly HashSet<string> MarkdownExts = new(StringComparer.OrdinalIgnoreCase) { "md", "markdown", "mdx" };

    public RepoCodeViewModel(RepoRef repo, GitHubApi api)
    {
        Repo = repo;
        Api = api;
    }

    public List<(string Name, string Path)> Crumbs
    {
        get
        {
            var result = new List<(string, string)> { (Repo.Name, "") };
            var acc = "";
            foreach (var part in Dir.Split('/', StringSplitOptions.RemoveEmptyEntries))
            {
                acc = acc.Length == 0 ? part : acc + "/" + part;
                result.Add((part, acc));
            }
            return result;
        }
    }

    partial void OnDirChanged(string value) => OnPropertyChanged(nameof(Crumbs));

    public async Task Start(string? defaultBranch)
    {
        if (CurrentRef.Length > 0) return;
        CurrentRef = defaultBranch ?? "HEAD";
        var b = LoadBranches();
        await OpenDir("");
        await b;
    }

    private async Task LoadBranches()
    {
        try
        {
            var list = await Api.Branches(Repo);
            var current = CurrentRef;
            Branches = list.OrderBy(b => b.Name == current ? 0 : 1).ThenBy(b => b.Name, StringComparer.OrdinalIgnoreCase).ToList();
        }
        catch
        {
        }
    }

    public async Task OpenAt(string @ref, string path, bool blame)
    {
        var first = CurrentRef.Length == 0;
        if (@ref != CurrentRef)
        {
            CurrentRef = @ref;
            AllPaths = null;
            ResetBlame();
        }
        if (first) LoadBranches().Forget();
        BlameOn = blame;
        await Reveal(path);
    }

    public string RefLabel => CurrentRef.Length == 40 && CurrentRef.All(Uri.IsHexDigit) ? CurrentRef[..7] : CurrentRef;

    partial void OnCurrentRefChanged(string value) => OnPropertyChanged(nameof(RefLabel));

    public async Task SwitchRef(string name)
    {
        if (name == CurrentRef) return;
        CurrentRef = name;
        AllPaths = null;
        SelectedFile = null;
        File = null;
        ResetBlame();
        BlameOn = false;
        var keep = Dir;
        await OpenDir(keep);
        if (DirError is not null && keep.Length > 0) await OpenDir("");
    }

    public async Task OpenDir(string path)
    {
        var key = $"{CurrentRef}|{path}";
        var switching = path != Dir;
        Dir = path;
        if (dirCache.TryGetValue(key, out var cached))
        {
            Entries = cached.Entries;
            Readme = cached.Readme;
            DirError = null;
        }
        else if (switching)
        {
            Entries = new();
            Readme = null;
            DirError = null;
        }
        LoadingDir = true;
        try
        {
            var obj = await Api.GitObject(Repo, CurrentRef, path);
            if (path != Dir || key != $"{CurrentRef}|{Dir}") return;
            if (obj?.Entries is not { } list)
            {
                Entries = new();
                Readme = null;
                DirError = "Folder not found on this branch";
                return;
            }
            var sorted = list.OrderBy(e => e.IsDir ? 0 : 1).ThenBy(e => e.Name, StringComparer.OrdinalIgnoreCase).ToList();
            Entries = sorted;
            DirError = null;
            string? html = null;
            if (sorted.Any(e => !e.IsDir && e.Name.StartsWith("readme", StringComparison.OrdinalIgnoreCase)))
            {
                try
                {
                    html = await Api.ReadmeHtml(Repo, CurrentRef, path);
                }
                catch
                {
                }
                if (path != Dir || key != $"{CurrentRef}|{Dir}") return;
            }
            dirCache[key] = (sorted, html);
            Readme = html;
        }
        catch (Exception e)
        {
            if (path == Dir) DirError = e.Message;
        }
        finally
        {
            if (path == Dir) LoadingDir = false;
        }
    }

    public async Task Select(TreeEntry entry)
    {
        if (entry.IsDir)
        {
            SelectedFile = null;
            File = null;
            await OpenDir(entry.Path);
        }
        else if (!entry.IsSubmodule)
        {
            await OpenFile(entry.Path);
        }
    }

    public async Task Reveal(string path)
    {
        var slash = path.LastIndexOf('/');
        var parent = slash < 0 ? "" : path[..slash];
        if (parent != Dir) await OpenDir(parent);
        await OpenFile(path);
    }

    public static string Extension(string path)
    {
        var name = path.Contains('/') ? path[(path.LastIndexOf('/') + 1)..] : path;
        var dot = name.LastIndexOf('.');
        return dot > 0 ? name[(dot + 1)..] : name;
    }

    public async Task OpenFile(string path)
    {
        SelectedFile = path;
        File = null;
        FileError = null;
        ResetBlame();
        LoadingFile = true;
        var ext = Extension(path);
        try
        {
            if (ImageExts.Contains(ext))
            {
                var data = await Api.RawFile(Repo, CurrentRef, path) ?? throw new ApiException("File not found");
                if (SelectedFile != path) return;
                FileSize = data.Length;
                File = new FileContent(FileContentKind.Image, Bytes: data, Size: data.Length);
                return;
            }
            var obj = await Api.GitObject(Repo, CurrentRef, path) ?? throw new ApiException("File not found");
            if (SelectedFile != path) return;
            FileSize = obj.ByteSize;
            if (obj.IsBinary == true)
            {
                File = new FileContent(FileContentKind.Binary, Size: obj.ByteSize ?? 0);
            }
            else if (MarkdownExts.Contains(ext))
            {
                string? html = null;
                try
                {
                    html = await Api.RenderedFile(Repo, CurrentRef, path);
                }
                catch
                {
                }
                File = html is not null
                    ? new FileContent(FileContentKind.Markdown, Text: obj.Text ?? "", Html: html)
                    : new FileContent(FileContentKind.Text, Text: obj.Text ?? "", Truncated: obj.IsTruncated ?? false);
            }
            else
            {
                File = new FileContent(FileContentKind.Text, Text: obj.Text ?? "", Truncated: obj.IsTruncated ?? false);
            }
        }
        catch (Exception e)
        {
            FileError = e.Message;
        }
        finally
        {
            LoadingFile = false;
        }
        if (SelectedFile == path && BlameOn) LoadBlame().Forget();
    }

    public bool CanBlame => File is { Kind: FileContentKind.Text or FileContentKind.Markdown };

    partial void OnFileChanged(FileContent? value) => OnPropertyChanged(nameof(CanBlame));

    public void ToggleBlame() => SetBlame(!BlameOn);

    public void SetBlame(bool on)
    {
        if (BlameOn == on) return;
        BlameOn = on;
        if (on) LoadBlame().Forget();
    }

    public void RetryBlame()
    {
        ResetBlame();
        LoadBlame().Forget();
    }

    private void ResetBlame()
    {
        blameKey = null;
        Blame = null;
        BlameFailed = false;
        LoadingBlame = false;
    }

    public async Task LoadBlame()
    {
        if (!BlameOn || SelectedFile is not { } path || File is not { } file || !CanBlame) return;
        var key = $"{CurrentRef}|{path}";
        if (blameKey == key && (Blame is not null || LoadingBlame)) return;
        blameKey = key;
        BlameFailed = false;
        if (blameCache.TryGetValue(key, out var cached))
        {
            Blame = cached;
            return;
        }
        LoadingBlame = true;
        var @ref = CurrentRef;
        try
        {
            var lines = PullYard.Core.Api.Blame.LineCount(file.Text);
            var ranges = await Task.Run(async () => PullYard.Core.Api.Blame.Map(await Api.BlameRanges(Repo, @ref, path).ConfigureAwait(false), lines));
            if (blameKey != key) return;
            if (ranges.Count == 0)
            {
                BlameFailed = true;
                return;
            }
            blameCache[key] = ranges;
            Blame = ranges;
        }
        catch
        {
            if (blameKey == key) BlameFailed = true;
        }
        finally
        {
            if (blameKey == key) LoadingBlame = false;
        }
    }

    public async Task LoadAllPaths()
    {
        if (AllPaths is not null || LoadingPaths) return;
        LoadingPaths = true;
        try
        {
            var sha = Branches.FirstOrDefault(b => b.Name == CurrentRef)?.Commit.Sha;
            if (sha is null)
            {
                await LoadBranches();
                sha = Branches.FirstOrDefault(b => b.Name == CurrentRef)?.Commit.Sha;
            }
            var (paths, truncated) = await Api.AllPaths(Repo, sha ?? CurrentRef);
            AllPaths = paths;
            PathsTruncated = truncated;
        }
        catch
        {
            AllPaths = new();
        }
        finally
        {
            LoadingPaths = false;
        }
    }

    public string WebUrl
    {
        get
        {
            var @base = $"https://github.com/{Repo.Full}";
            if (SelectedFile is { } f) return $"{@base}/blob/{CurrentRef}/{f}";
            return Dir.Length == 0 ? $"{@base}/tree/{CurrentRef}" : $"{@base}/tree/{CurrentRef}/{Dir}";
        }
    }

    public static List<string> FuzzyMatch(string query, IReadOnlyList<string> paths, int limit = 200)
    {
        var q = new string(query.ToLowerInvariant().Where(c => !char.IsWhiteSpace(c)).ToArray());
        if (q.Length == 0) return paths.Take(limit).ToList();
        var scored = new List<(string, int)>();
        foreach (var p in paths)
        {
            var lower = p.ToLowerInvariant();
            var score = 0;
            var qi = 0;
            var last = -2;
            for (var i = 0; i < lower.Length && qi < q.Length; i++)
            {
                if (lower[i] != q[qi]) continue;
                score += last == i - 1 ? 3 : 1;
                last = i;
                qi++;
            }
            if (qi < q.Length) continue;
            var name = lower.Contains('/') ? lower[(lower.LastIndexOf('/') + 1)..] : lower;
            if (name.Contains(q)) score += 20;
            if (name.StartsWith(q)) score += 10;
            score -= lower.Length / 20;
            scored.Add((p, score));
        }
        return scored.OrderByDescending(s => s.Item2).Take(limit).Select(s => s.Item1).ToList();
    }
}
