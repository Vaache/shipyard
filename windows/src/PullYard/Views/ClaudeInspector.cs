using System.ComponentModel;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Controls;
using PullYard.Core.Api;
using PullYard.Core.Claude;
using PullYard.Core.Html;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;
using VisualState = PullYard.Core.Api.VisualState;

namespace PullYard.Views;

public sealed class ClaudeInspector : Grid
{
    private readonly ClaudeTab tab;
    private readonly TabStrip tabs = new("changes");
    private readonly ContentControl host = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private ChangesPanel? changes;
    private LocalFilesPanel? files;
    private BranchPRPanel? prs;

    public ClaudeInspector(ClaudeTab tab)
    {
        this.tab = tab;
        Background = ThemeService.IsBrand ? Palette.Brush("panel") : null;
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var top = Ui.V(0, tabs, Ui.Line());
        top.Margin = new Thickness(0, 4, 0, 0);
        Children.Add(top);
        SetRow(host, 1);
        Children.Add(host);
        tabs.SelectionChanged += _ => Show();
        tab.Git.PropertyChanged += OnGit;
        Unloaded += (_, _) => tab.Git.PropertyChanged -= OnGit;
        Loaded += (_, _) =>
        {
            tab.Git.PropertyChanged -= OnGit;
            tab.Git.PropertyChanged += OnGit;
        };
        RenderTabs();
        Show();
    }

    private void OnGit(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(Core.Git.GitWorkingCopy.Changes)) RenderTabs();
    }

    private void RenderTabs()
    {
        var count = tab.Git.Changes.Count;
        tabs.SetTabs(new[]
        {
            new TabSpec("changes", L.T("Inspector_Changes", "Changes"), "", count > 0 ? count.ToString() : null, "amber"),
            new TabSpec("files", L.T("Inspector_Files", "Files"), ""),
            new TabSpec("pr", L.T("Inspector_PR", "PR"), "")
        });
    }

    private void Show()
    {
        host.Content = tabs.Selected switch
        {
            "files" => files ??= new LocalFilesPanel(tab.Folder),
            "pr" => prs ??= new BranchPRPanel(tab),
            _ => changes ??= new ChangesPanel(tab)
        };
    }
}

public sealed class ChangesPanel : Grid
{
    private readonly ClaudeTab tab;
    private readonly StackPanel header = Ui.V(4);
    private readonly HtmlView html = new();
    private readonly ContentControl empty = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly HashSet<string> collapsed = new();

    public ChangesPanel(ClaudeTab tab)
    {
        this.tab = tab;
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        header.Padding = new Thickness(10, 6, 6, 6);
        Children.Add(header);
        var area = new Grid();
        area.Children.Add(html);
        area.Children.Add(empty);
        SetRow(area, 1);
        Children.Add(area);
        html.ActionInvoked += (type, id) =>
        {
            switch (type)
            {
                case "open":
                    PathGuard.OpenWithinWorkspace(tab.Folder, id.Replace('/', Path.DirectorySeparatorChar));
                    break;
                case "copy":
                    WindowInterop.Copy(id);
                    break;
                case "collapse":
                    collapsed.Add(id);
                    break;
                case "expand":
                    collapsed.Remove(id);
                    if (tab.Git.Changes.FirstOrDefault(f => f.Id == id) is { } file)
                        html.Send(new System.Text.Json.Nodes.JsonObject { ["op"] = "fileBody", ["path"] = id, ["html"] = DiffHtml.Body(file, new DiffRenderOptions()) });
                    break;
            }
        };
        tab.Git.PropertyChanged += OnGit;
        Unloaded += (_, _) => tab.Git.PropertyChanged -= OnGit;
        Loaded += (_, _) =>
        {
            tab.Git.PropertyChanged -= OnGit;
            tab.Git.PropertyChanged += OnGit;
            tab.Git.Refresh().Forget();
        };
        Render();
    }

    private void OnGit(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Core.Git.GitWorkingCopy.Changes) or nameof(Core.Git.GitWorkingCopy.Unpushed) or nameof(Core.Git.GitWorkingCopy.Loading)) Render();
    }

    private void Render()
    {
        var git = tab.Git;
        header.Children.Clear();
        var summary = Ui.H(8);
        if (git.Loading) summary.Children.Add(Ui.Spinner(12));
        summary.Children.Add(Ui.Caption(git.Changes.Count == 0 ? L.T("Changes_None", "No uncommitted changes") : Fmt.Plural(git.Changes.Count, "file changed", "files changed")));
        if (git.Changes.Count > 0) summary.Children.Add(Ui.DiffStat(git.TotalAdditions, git.TotalDeletions));
        header.Children.Add(Ui.Columns((summary, Ui.Star), (Ui.IconButton("", L.T("Common_Refresh", "Refresh"), () => git.Refresh().Forget()), Ui.Auto)));
        if (git.Unpushed.Count > 0)
        {
            header.Children.Add(Ui.Text(L.F("Changes_Unpushed", "Unpushed commits · {0}", git.Unpushed.Count), 12, "muted", FontWeights.SemiBold));
            foreach (var c in git.Unpushed.Take(6)) header.Children.Add(Ui.H(6, Ui.Text(c.Short, 12, "muted", mono: true), Ui.Text(c.Subject, 12)));
        }
        if (git.Changes.Count == 0)
        {
            empty.Content = Ui.Empty("", "green", L.T("Changes_Help", "Changes Claude makes will show up here automatically"));
            empty.Visibility = Visibility.Visible;
            html.Visibility = Visibility.Collapsed;
            return;
        }
        empty.Visibility = Visibility.Collapsed;
        html.Visibility = Visibility.Visible;
        foreach (var f in git.Changes.Where(f => f.Lines.Count > 1500)) collapsed.Add(f.Id);
        html.Render(DiffHtml.Render(git.Changes, new DiffRenderOptions { ShowOpen = true, Collapsed = collapsed }), "diff", null, "flush");
    }
}

public sealed class LocalFilesPanel : Grid
{
    private readonly string root;
    private string dir = "";
    private string? file;
    private readonly TextBlock pathText = Ui.Text("", 12, "muted", mono: true);
    private readonly ListView list = ListSupport.Create("GenericRowTemplate");
    private readonly HtmlView html = new();
    private readonly ContentControl preview = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };

    public LocalFilesPanel(string root)
    {
        this.root = root;
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star), MinHeight = 140 });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1) });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1.2, GridUnitType.Star), MinHeight = 160 });
        var back = Ui.IconButton("", L.T("Common_Back", "Back"), () =>
        {
            if (dir.Length == 0) return;
            dir = dir.Contains('/') ? dir[..dir.LastIndexOf('/')] : "";
            Load();
        });
        var top = Ui.Columns((back, Ui.Auto), (pathText, Ui.Star));
        top.Padding = new Thickness(6, 4, 6, 4);
        Children.Add(top);
        list.ItemClick += (_, e) =>
        {
            if (e.ClickedItem is not EntryItem item) return;
            if (item.Entry.IsDir)
            {
                dir = item.Entry.Path;
                Load();
            }
            else
            {
                file = item.Entry.Path;
                ShowFile().Forget();
            }
        };
        SetRow(list, 1);
        Children.Add(list);
        var line = Ui.Line();
        SetRow(line, 2);
        Children.Add(line);
        SetRow(preview, 3);
        Children.Add(preview);
        preview.Content = Ui.Empty("", "slate", L.T("Files_Pick", "Pick a file"));
        Load();
    }

    private void Load()
    {
        pathText.Text = dir.Length == 0 ? Path.GetFileName(root.TrimEnd('\\', '/')) : dir;
        var basePath = dir.Length == 0 ? root : Path.Combine(root, dir.Replace('/', Path.DirectorySeparatorChar));
        var entries = new List<TreeEntry>();
        try
        {
            foreach (var d in Directory.GetDirectories(basePath))
            {
                var name = Path.GetFileName(d);
                if (name is ".git") continue;
                entries.Add(new TreeEntry { Name = name, Path = dir.Length == 0 ? name : $"{dir}/{name}", Type = "tree" });
            }
            foreach (var f in Directory.GetFiles(basePath))
            {
                var name = Path.GetFileName(f);
                if (name is "desktop.ini" or "Thumbs.db" or ".DS_Store") continue;
                long size = 0;
                try
                {
                    size = new FileInfo(f).Length;
                }
                catch
                {
                }
                entries.Add(new TreeEntry { Name = name, Path = dir.Length == 0 ? name : $"{dir}/{name}", Type = "blob", Object = new BlobMeta { ByteSize = (int)Math.Min(size, int.MaxValue) } });
            }
        }
        catch
        {
        }
        list.ItemsSource = entries.OrderBy(e => e.IsDir ? 0 : 1).ThenBy(e => e.Name, StringComparer.OrdinalIgnoreCase).Select(e => new EntryItem(e)).ToList();
    }

    private async Task ShowFile()
    {
        if (file is not { } path) return;
        var full = Path.Combine(root, path.Replace('/', Path.DirectorySeparatorChar));
        var text = await Task.Run(() =>
        {
            try
            {
                var info = new FileInfo(full);
                if (!info.Exists || info.Length > 2_000_000) return null;
                var content = File.ReadAllText(full);
                return content.Contains('\0') ? null : content;
            }
            catch
            {
                return null;
            }
        });
        if (file != path) return;
        if (text is null)
        {
            preview.Content = Ui.Empty("", "slate", L.T("Files_Binary", "Binary or large file"));
            return;
        }
        html.Render(CodeHtml.Render(text, RepoCodeViewModel.Extension(path), false), "code", "top", "flush code-body");
        preview.Content = html;
    }
}

public sealed class BranchPRPanel : Grid
{
    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly ClaudeTab tab;
    private List<SearchItem> branchPRs = new();
    private SearchItem? linked;
    private List<SearchItem> list = new();
    private string scope = "open";
    private bool branchLoading;
    private bool listLoading;
    private string? branchError;
    private string? error;
    private DateTimeOffset? lastUpdated;
    private readonly ContentControl host = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly ListView listView = ListSupport.Create("GenericRowTemplate", ListViewSelectionMode.None);
    private readonly RefreshFooter status = new(compact: true);
    private ItemRef? selected;
    private string loadedKey = "";
    private readonly LiveRefresh live;

    public BranchPRPanel(ClaudeTab tab)
    {
        this.tab = tab;
        live = new LiveRefresh(this, OnLive);
        Children.Add(host);
        listView.ItemClick += (_, e) =>
        {
            switch (e.ClickedItem)
            {
                case PRRowItem p:
                    selected = p.Item.Ref;
                    Render();
                    break;
                case ActionRowItem a:
                    a.Action();
                    break;
            }
        };
        listView.ContextRequested += (_, args) =>
        {
            if ((args.OriginalSource as FrameworkElement)?.DataContext is not PRRowItem p) return;
            args.Handled = true;
            Ui.Menu(
                Ui.MenuItem(L.T("Inspector_OpenInPRs", "Open in PR section"), () => { app.Section = Section.Prs(PRFilter.Mine); app.Open(p.Item.Route); }),
                Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(p.Item.Url)),
                Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(p.Item.Url))).ShowAt((FrameworkElement)args.OriginalSource);
        };
        status.Retry = () => Load().Forget();
        Loaded += (_, _) =>
        {
            tab.Git.PropertyChanged -= OnGit;
            tab.Git.PropertyChanged += OnGit;
            tab.PropertyChanged -= OnTab;
            tab.PropertyChanged += OnTab;
            app.LiveTick -= live.Tick;
            app.LiveTick += live.Tick;
            app.RefreshRequested -= OnLive;
            app.RefreshRequested += OnLive;
            Load().Forget();
        };
        Unloaded += (_, _) =>
        {
            tab.Git.PropertyChanged -= OnGit;
            tab.PropertyChanged -= OnTab;
            app.LiveTick -= live.Tick;
            app.RefreshRequested -= OnLive;
        };
        Render();
    }

    private RepoRef? Repo => tab.Git.Repo;

    private void OnLive()
    {
        if (selected is null) Load().Forget();
    }

    private void OnGit(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(Core.Git.GitWorkingCopy.Repo) or nameof(Core.Git.GitWorkingCopy.Branch) or nameof(Core.Git.GitWorkingCopy.Ahead))
        {
            var key = $"{Repo?.Full}|{tab.Git.Branch}|{tab.Git.Ahead}";
            if (key == loadedKey) return;
            Load().Forget();
        }
    }

    private void OnTab(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName == nameof(ClaudeTab.State) && !tab.Busy && selected is null) LoadBranch().Forget();
    }

    private async Task Load()
    {
        loadedKey = $"{Repo?.Full}|{tab.Git.Branch}|{tab.Git.Ahead}";
        var b = LoadBranch();
        var l = LoadList();
        await Task.WhenAll(b, l);
    }

    private async Task LoadBranch()
    {
        if (app.Api is not { } api || Repo is not { } repo) return;
        var branch = tab.Git.Branch;
        branchLoading = true;
        Render();
        try
        {
            var found = new List<SearchItem>();
            if (branch is not null && branch != "HEAD") found = await api.RepoPullRequests(repo, branch, new[] { "OPEN", "MERGED", "CLOSED" }, 5);
            SearchItem? linkedItem = null;
            if (app.Claude.AllSessions.FirstOrDefault(s => s.Id == tab.SessionId)?.PrNumber is { } number && !found.Any(f => f.Number == number))
            {
                try
                {
                    linkedItem = await api.PullRequestItem(new ItemRef(repo, number));
                }
                catch
                {
                }
            }
            if (branch != tab.Git.Branch || repo != Repo) return;
            branchPRs = found;
            linked = linkedItem;
            branchError = null;
            lastUpdated = DateTimeOffset.Now;
        }
        catch (Exception e)
        {
            branchError = e.Message;
        }
        finally
        {
            branchLoading = false;
            Render();
        }
    }

    private async Task LoadList()
    {
        if (app.Api is not { } api || Repo is not { } repo) return;
        var s = scope;
        listLoading = true;
        Render();
        try
        {
            var items = s switch
            {
                "merged" => await api.RepoPullRequests(repo, states: new[] { "MERGED" }),
                "mine" => (await api.Search($"repo:{repo.Full} is:pr author:@me sort:updated-desc")).Nodes,
                _ => await api.RepoPullRequests(repo, states: new[] { "OPEN" })
            };
            if (s != scope || repo != Repo) return;
            list = items;
            error = null;
            lastUpdated = DateTimeOffset.Now;
        }
        catch (Exception e)
        {
            error = e.Message;
        }
        finally
        {
            listLoading = false;
            Render();
        }
    }

    private void Render()
    {
        if (selected is { } r)
        {
            if (host.Content is not InspectorPRDetail d || d.Ref != r)
                host.Content = new InspectorPRDetail(tab, r, () =>
                {
                    selected = null;
                    Render();
                });
            return;
        }
        if (Repo is null)
        {
            host.Content = Ui.Empty("", "slate", L.T("Inspector_NoRepo", "Folder isn't linked to a GitHub repository"));
            return;
        }
        var items = new List<object>();
        var branchName = tab.Git.Branch ?? "?";
        items.Add(new HeaderRowItem(L.T("Inspector_ThisBranch", "This branch"), branchName));
        if (branchError is { } be && branchPRs.Count == 0) items.Add(new TextRowItem(be));
        else if (branchPRs.Count == 0 && linked is null && !branchLoading)
        {
            items.Add(new TextRowItem(L.F("Inspector_NoPR", "No PR for branch {0}", branchName)));
            if (tab.Git.Branch is { } b && b != "HEAD" && Repo is { } repo)
                items.Add(new ActionRowItem(L.T("Inspector_CreatePR", "Create PR in browser"), () => LinkBrowser.Open($"https://github.com/{repo.Full}/pull/new/{b}")));
        }
        items.AddRange(branchPRs.Select(p => new PRRowItem(p, null)));
        if (linked is { } l && !branchPRs.Any(p => p.Number == l.Number)) items.Add(new PRRowItem(l, L.T("Inspector_FromSession", "from this session")));
        var bar = new SelectorBar();
        foreach (var (id, title) in new[] { ("open", L.T("State_Open", "Open")), ("merged", L.T("State_Merged", "Merged")), ("mine", L.T("Inspector_Mine", "Mine")) })
        {
            var item = new SelectorBarItem { Text = title, Tag = id };
            bar.Items.Add(item);
            if (id == scope) bar.SelectedItem = item;
        }
        bar.SelectionChanged += (s, _) =>
        {
            if (s.SelectedItem?.Tag is string id && id != scope)
            {
                scope = id;
                LoadList().Forget();
            }
        };
        if (error is { } err) items.Add(new TextRowItem(err));
        if (list.Count == 0 && !listLoading && error is null) items.Add(new TextRowItem(L.T("Common_NothingHere", "Nothing here")));
        items.AddRange(list.Select(p => new PRRowItem(p, null)));
        listView.ItemsSource = items;
        status.Update(lastUpdated, branchLoading || listLoading, branchError ?? error);
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var top = Ui.V(4, Ui.Columns((Ui.Text(Repo.Full, 12, "muted", FontWeights.SemiBold), Ui.Star), (status, Ui.Auto)), bar);
        top.Padding = new Thickness(10, 6, 10, 4);
        grid.Children.Add(top);
        if (listView.Parent is Panel p) p.Children.Remove(listView);
        SetRow(listView, 1);
        grid.Children.Add(listView);
        host.Content = grid;
    }
}

public sealed record HeaderRowItem(string Title, string Branch) : IRowContent
{
    public UIElement BuildRow()
    {
        var p = Ui.H(6, Ui.Text(Title, 12, "muted", FontWeights.SemiBold), SessionRow.BranchTag(Branch));
        p.Margin = new Thickness(0, 6, 0, 2);
        return p;
    }
}

public sealed record PRRowItem(SearchItem Item, string? Note) : IRowContent
{
    public UIElement BuildRow()
    {
        var i = Item;
        var meta = Ui.H(6, Ui.Caption($"#{i.Number}"), Ui.Caption(i.Author?.Login ?? ""));
        if (Ui.CiIcon(i.CiState, 11) is { } ci) meta.Children.Add(ci);
        if (i.Additions is { } a && i.Deletions is { } d) meta.Children.Add(Ui.DiffStat(a, d, 11.5));
        if (Note is { } n) meta.Children.Add(Ui.Caption(n, "violet"));
        var grid = Ui.Columns((Ui.StateBadge(i.VisualState, true), Ui.Auto), (Ui.V(3, Ui.Text(i.Title, 13, "text", FontWeights.Medium, maxLines: 2), Ui.Columns((meta, Ui.Star), (Ui.Caption(Fmt.Rel(i.UpdatedAt), "dim"), Ui.Auto))), Ui.Star));
        grid.Padding = new Thickness(0, 4, 0, 4);
        return grid;
    }
}

public sealed class InspectorPRDetail : Grid
{
    private readonly AppViewModel app = AppViewModel.Shared;
    private readonly ClaudeTab tab;
    public ItemRef Ref { get; }
    private readonly Action onBack;
    private PRDetailViewModel? model;
    private readonly StackPanel body = Ui.V(12);
    private readonly RefreshFooter status = new(compact: true);
    private readonly LiveRefresh live;
    private bool showFiles;

    public InspectorPRDetail(ClaudeTab tab, ItemRef r, Action onBack)
    {
        this.tab = tab;
        live = new LiveRefresh(this, OnLive);
        Ref = r;
        this.onBack = onBack;
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var back = Ui.Button(L.T("Common_Back", "Back"), onBack, glyph: "");
        var openFull = Ui.Button(L.T("Inspector_OpenFull", "Open full"), () => { app.Section = Section.Prs(PRFilter.Mine); app.Open(Route.Pr(r)); });
        var top = Ui.Columns((back, Ui.Auto), (new Grid(), Ui.Star), (status, Ui.Auto), (Ui.IconButton("", L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open($"https://github.com/{r.Repo.Full}/pull/{r.Number}")), Ui.Auto), (openFull, Ui.Auto));
        top.Padding = new Thickness(8);
        Children.Add(top);
        body.Padding = new Thickness(12);
        var scroll = new ScrollViewer { Content = body };
        SetRow(scroll, 1);
        Children.Add(scroll);
        body.Children.Add(Ui.Loading());
        Loaded += (_, _) =>
        {
            model ??= app.PrModel(r);
            if (model is null) return;
            model.PropertyChanged -= OnModel;
            model.PropertyChanged += OnModel;
            app.LiveTick -= live.Tick;
            app.LiveTick += live.Tick;
            model.FilesInInspector = showFiles;
            status.Retry = () => model.Load(true).Forget();
            model.Load().Forget();
            Render();
        };
        Unloaded += (_, _) =>
        {
            if (model is not null)
            {
                model.PropertyChanged -= OnModel;
                model.FilesInInspector = false;
            }
            app.LiveTick -= live.Tick;
        };
    }

    private void OnLive() => model?.Load(live: true).Forget();

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        if (model is null) return;
        switch (e.PropertyName)
        {
            case nameof(PRDetailViewModel.Pr):
            case nameof(PRDetailViewModel.Files):
            case nameof(PRDetailViewModel.FilesLoaded):
            case nameof(PRDetailViewModel.FilesError):
            case nameof(PRDetailViewModel.Error):
                Render();
                break;
            case nameof(PRDetailViewModel.Loading):
            case nameof(PRDetailViewModel.LastUpdated):
            case nameof(PRDetailViewModel.RefreshError):
                status.Update(model.LastUpdated, model.Loading, model.RefreshError);
                break;
            case nameof(PRDetailViewModel.ActionError):
                if (model.ActionError is { } err)
                {
                    model.ActionError = null;
                    Dialogs.Alert(XamlRoot, L.T("Common_Error", "Error"), err).Forget();
                }
                break;
        }
    }

    private void Render()
    {
        if (model is null) return;
        body.Children.Clear();
        if (model.Pr is not { } pr)
        {
            body.Children.Add(model.Error is { } e ? Ui.Error(e, () => model.Load().Forget()) : Ui.Loading());
            return;
        }
        var unresolved = pr.ReviewThreads.Items.Where(t => !t.IsResolved).ToList();
        var fix = FixPromptExtractor.Latest(pr);
        body.Children.Add(Ui.V(6,
            Ui.Text(pr.Title, 15, "text", FontWeights.SemiBold, wrap: true, select: true),
            Ui.H(6, Ui.StateBadge(pr.VisualState), Ui.Caption($"#{pr.Number}"), Ui.DiffStat(pr.Additions, pr.Deletions)),
            Ui.H(4, SessionRow.BranchTag(pr.BaseRefName), Ui.Glyph("", 10), SessionRow.BranchTag(pr.HeadRefName))));
        if (pr.IsOpen)
        {
            if (PRContent.MergeStatusBar(model, app, () => { app.Section = Section.Prs(PRFilter.Mine); app.Open(Route.Pr(Ref)); }) is { } bar) body.Children.Add(bar);
            if (PRContent.ReReviewBanner(model, () => { app.Section = Section.Prs(PRFilter.Mine); app.Open(Route.Pr(Ref)); }) is { } rr) body.Children.Add(rr);
        }
        var checks = pr.Checks;
        var stats = Ui.H(14,
            Ui.H(4, Ui.Glyph("", 12, checks.Any(c => c.Outcome == CheckOutcome.Failure) ? "red" : "muted"), Ui.Caption($"{checks.Count(c => c.Outcome == CheckOutcome.Success)}/{checks.Count}")),
            Ui.H(4, Ui.Glyph("", 12, unresolved.Count == 0 ? "muted" : "amber"), Ui.Caption(unresolved.Count.ToString(), unresolved.Count == 0 ? "muted" : "amber")));
        foreach (var r in model.Reviewers) stats.Children.Add(PRContent.ReviewerBadge(r));
        body.Children.Add(stats);
        if (tab.Chat is not null || tab.Running)
        {
            var send = Ui.V(6, Ui.Text(L.T("Inspector_SendToSession", "Send to this session"), 12, "muted", FontWeights.SemiBold));
            if (fix is not null)
                send.Children.Add(Wide(Ui.Button(L.F("Fix_PromptBy", "Fix prompt by @{0}", fix.Author), () => app.Claude.Send(FixPromptExtractor.Wrap(fix, pr, new FixOptions()), tab), glyph: "")));
            var comments = Ui.Button(L.F("Inspector_Unresolved", "Unresolved comments ({0})", unresolved.Count), () =>
                app.Claude.Send(FixPrompt.Build(pr, Ref.Repo, unresolved, FixPrompt.GeneralFeedback(pr), new FixOptions()), tab), glyph: "");
            comments.IsEnabled = unresolved.Count > 0;
            send.Children.Add(Wide(comments));
            send.Children.Add(Wide(Ui.Button(L.T("Inspector_AskReview", "Ask Claude to review this PR"), () =>
                app.Claude.Send($"Review PR #{pr.Number} ({pr.Url}) and briefly summarize what it does and any problems.", tab), glyph: "")));
            body.Children.Add(send);
        }
        if (unresolved.Count > 0)
        {
            var list = Ui.V(8, Ui.Text(L.T("Fix_Unresolved", "Unresolved comments"), 12, "muted", FontWeights.SemiBold));
            foreach (var th in unresolved)
            {
                var item = Ui.V(3, Ui.Text(th.Location, 12, "muted", mono: true));
                if (th.Comments.Items.FirstOrDefault() is { } c)
                {
                    var text = Ui.Text(c.Body, 13, "text", wrap: true, maxLines: 4);
                    item.Children.Add(Ui.Columns((Ui.Avatar(c.Author?.AvatarUrl, 14), Ui.Auto), (text, Ui.Star)));
                }
                list.Children.Add(Ui.Card(item, 8, "sunken", 8));
            }
            body.Children.Add(list);
        }
        var filesHeader = new Expander
        {
            Header = L.F("Inspector_FilesChanged", "Files changed · {0}", pr.ChangedFiles),
            IsExpanded = showFiles,
            HorizontalAlignment = HorizontalAlignment.Stretch,
            HorizontalContentAlignment = HorizontalAlignment.Stretch
        };
        var filesPanel = Ui.V(3);
        if (showFiles && model.FilesError is null) model.LoadFiles().Forget();
        if (model.FilesLoaded)
        {
            foreach (var f in model.Files)
                filesPanel.Children.Add(Ui.Columns((Ui.Text(f.StatusLetter, 11, "muted", FontWeights.Bold, mono: true), Ui.Auto), (Ui.Text(f.Id, 12, "text", mono: true), Ui.Star), (Ui.DiffStat(f.Additions, f.Deletions, 11), Ui.Auto)));
        }
        else if (model.FilesError is { } filesError && !model.FilesLoading)
        {
            filesPanel.Children.Add(Ui.Error(filesError, () => model.LoadFiles().Forget()));
        }
        else
        {
            filesPanel.Children.Add(Ui.Spinner(14));
        }
        filesHeader.Content = filesPanel;
        filesHeader.Expanding += (_, _) =>
        {
            showFiles = true;
            model.FilesInInspector = true;
            model.LoadFiles().Forget();
        };
        filesHeader.Collapsed += (_, _) =>
        {
            showFiles = false;
            model.FilesInInspector = false;
        };
        body.Children.Add(filesHeader);
        status.Update(model.LastUpdated, model.Loading, model.RefreshError);
    }

    private static Button Wide(Button b)
    {
        b.HorizontalAlignment = HorizontalAlignment.Stretch;
        b.HorizontalContentAlignment = HorizontalAlignment.Left;
        return b;
    }
}
