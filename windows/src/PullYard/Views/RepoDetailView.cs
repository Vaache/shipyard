using System.ComponentModel;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using PullYard.Controls;
using PullYard.Core.Api;
using PullYard.Core.Html;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;
using Windows.System;

namespace PullYard.Views;

public sealed class RepoDetailView : LiveDetail
{
    private readonly RepoRef repo;
    private RepoInfo? info;
    private bool infoLoaded;
    private string tab = "code";
    private StateFilter stateFilter = StateFilter.Open;
    private readonly SearchListViewModel listModel = new();
    private RepoCodeViewModel? code;
    private readonly StackPanel headerCard = Ui.V(4);
    private readonly TabStrip tabs = new("code");
    private readonly ContentControl body = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly ContentControl stateHost = new() { IsTabStop = false };
    private RepoCodeView? codeView;
    private ListView? itemsList;
    private RowAppear? itemsAppear;
    private readonly RefreshFooter footer = new();

    public RepoDetailView(RepoRef repo)
    {
        this.repo = repo;
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var tools = Toolbar(
            Ui.IconButton("", L.T("Repos_CopyClone", "Copy “gh repo clone”"), () => WindowInterop.Copy($"gh repo clone {repo.Full}"), "text"),
            Ui.IconButton("", L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(code?.WebUrl ?? $"https://github.com/{repo.Full}"), "text"));
        var card = Ui.Card(headerCard, 12, "raised", 12);
        var top = Ui.Columns((card, Ui.Star), (tools, Ui.Auto));
        top.Padding = new Thickness(16, 12, 12, 4);
        ((FrameworkElement)top.Children[1]).VerticalAlignment = VerticalAlignment.Top;
        Children.Add(top);
        var tabRow = Ui.Columns((tabs, Ui.Star), (stateHost, Ui.Auto));
        tabRow.Padding = new Thickness(0, 4, 12, 0);
        SetRow(tabRow, 1);
        Children.Add(tabRow);
        var line = Ui.Line();
        SetRow(line, 2);
        Children.Add(line);
        SetRow(body, 3);
        Children.Add(body);
        tabs.SelectionChanged += id =>
        {
            tab = id;
            ShowTab();
        };
        stateHost.Content = ListPaneStateBar();
        listModel.PropertyChanged += OnList;
        RenderHeader();
        RenderTabs();
        ShowTab();
    }

    private FrameworkElement ListPaneStateBar()
    {
        var combo = new ComboBox { MinWidth = 120 };
        foreach (var s in Enum.GetValues<StateFilter>()) combo.Items.Add(new ComboBoxItem { Content = s.Title(), Tag = s });
        combo.SelectedIndex = 0;
        combo.SelectionChanged += (_, _) =>
        {
            if ((combo.SelectedItem as ComboBoxItem)?.Tag is StateFilter f)
            {
                stateFilter = f;
                LoadItems().Forget();
            }
        };
        return combo;
    }

    private string Query => string.Join(" ", new[] { $"repo:{repo.Full}", $"is:{(tab == "issue" ? "issue" : "pr")}", stateFilter.Query(), "sort:updated-desc" }.Where(s => s.Length > 0));

    protected override void OnAttached()
    {
        if (App.Api is { } api && code is null)
        {
            code = new RepoCodeViewModel(repo, api);
            ShowTab();
        }
        LoadInfo().Forget();
    }

    protected override void OnRefresh()
    {
        if (tab == "code") code?.OpenDir(code.Dir).Forget();
        else LoadItems().Forget();
    }

    protected override void OnLive()
    {
        if (tab != "code") listModel.Load(App.Api, Query, true).Forget();
    }

    private async Task LoadInfo()
    {
        try
        {
            info = await App.Api!.RepoInfo(repo);
        }
        catch
        {
            info = null;
        }
        RenderHeader();
        RenderTabs();
        infoLoaded = true;
        if (code is not null && codeView is not null) await code.Start(info?.DefaultBranchRef?.Name ?? "HEAD");
    }

    private void RenderHeader()
    {
        headerCard.Children.Clear();
        var title = Ui.H(8, Ui.Text(repo.Full, 19, "text", FontWeights.SemiBold, select: true));
        if (info?.IsPrivate == true) title.Children.Add(Ui.Chip(L.T("Repo_Private", "private"), "amber", ""));
        var stack = Ui.V(4, title);
        if (!string.IsNullOrEmpty(info?.Description)) stack.Children.Add(Ui.Text(info.Description, 13, "muted", maxLines: 2));
        if (info is not null)
        {
            var chips = Ui.H(6, Ui.Chip(info.StargazerCount.ToString(), "amber", ""), Ui.Chip(info.ForkCount.ToString(), "sky", ""));
            if (info.DefaultBranchRef?.Name is { } b) chips.Children.Add(Ui.Chip(b, "teal", "", mono: true));
            stack.Children.Add(chips);
        }
        headerCard.Children.Add(Ui.Columns((Ui.IconTile(info?.IsPrivate == true ? "" : "", "teal", 36), Ui.Auto), (stack, Ui.Star)));
    }

    private void RenderTabs()
    {
        tabs.SetTabs(new[]
        {
            new TabSpec("code", L.T("Repo_Code", "Code"), ""),
            new TabSpec("pr", L.T("Nav_PullRequests", "Pull requests"), "", info?.PullRequests.TotalCount.ToString()),
            new TabSpec("issue", L.T("Nav_Issues", "Issues"), "", info?.Issues.TotalCount.ToString())
        });
        tabs.Selected = tab;
    }

    private void ShowTab()
    {
        stateHost.Visibility = tab == "code" ? Visibility.Collapsed : Visibility.Visible;
        if (tab == "code")
        {
            if (code is null)
            {
                body.Content = Ui.Loading();
                return;
            }
            if (codeView is null)
            {
                codeView = new RepoCodeView(code);
                if (infoLoaded) code.Start(info?.DefaultBranchRef?.Name ?? "HEAD").Forget();
            }
            body.Content = codeView;
            return;
        }
        if (itemsList is null)
        {
            itemsList = ListSupport.Create("FeedRowTemplate", ListViewSelectionMode.None);
            itemsAppear = new RowAppear(itemsList, i => (i as SearchItem)?.Id);
            itemsList.ItemClick += (_, e) =>
            {
                if (e.ClickedItem is SearchItem s) App.Open(s.Route, true);
            };
            itemsList.ContainerContentChanging += (_, e) =>
            {
                if (e.Item is SearchItem s && listModel.Items.Count > 0 && ReferenceEquals(s, listModel.Items[^1])) listModel.LoadMore(App.Api).Forget();
            };
            itemsList.ContextRequested += (_, args) =>
            {
                if ((args.OriginalSource as FrameworkElement)?.DataContext is not SearchItem s) return;
                args.Handled = true;
                SearchListView.ItemMenu(s).ShowAt((FrameworkElement)args.OriginalSource);
            };
        }
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        if (itemsList.Parent is Panel p) p.Children.Remove(itemsList);
        if (footer.Parent is Panel fp) fp.Children.Remove(footer);
        grid.Children.Add(itemsList);
        SetRow(footer, 1);
        grid.Children.Add(footer);
        footer.Retry = () => LoadItems().Forget();
        body.Content = grid;
        LoadItems().Forget();
    }

    private Task LoadItems() => listModel.Load(App.Api, Query);

    private void OnList(object? sender, PropertyChangedEventArgs e)
    {
        if (itemsList is null) return;
        if (e.PropertyName == nameof(SearchListViewModel.Items))
        {
            itemsAppear?.Ingest(listModel.Items.Select(i => i.Id), listModel.AppearMode);
            itemsList.ItemsSource = listModel.Items;
        }
        footer.Update(listModel.LastUpdated, listModel.Refreshing, listModel.Error);
    }
}

public sealed class RepoCodeView : Grid
{
    private readonly RepoCodeViewModel model;
    private readonly DropDownButton branchButton = new();
    private readonly StackPanel crumbs = Ui.H(2);
    private readonly ListView entries;
    private readonly ContentControl content = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly HtmlView html = new();
    private bool markdownSource;
    private bool wrap;
    private bool syncing;

    public RepoCodeView(RepoCodeViewModel model)
    {
        this.model = model;
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(0.32, GridUnitType.Star), MinWidth = 190, MaxWidth = 320 });
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1) });
        ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(0.68, GridUnitType.Star) });

        var side = new Grid { Background = ThemeService.IsBrand ? Palette.Brush("list") : null };
        side.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        side.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        side.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        branchButton.Padding = new Thickness(8, 4, 8, 4);
        var quick = Ui.IconButton("", L.T("Code_GoToFile", "Go to file (Ctrl+P)"), () => QuickOpen().Forget());
        var topRow = Ui.Columns((branchButton, Ui.Star), (quick, Ui.Auto));
        topRow.Padding = new Thickness(10, 8, 6, 4);
        side.Children.Add(topRow);
        var crumbScroll = new ScrollViewer { Content = crumbs, HorizontalScrollBarVisibility = ScrollBarVisibility.Hidden, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled, HorizontalScrollMode = ScrollMode.Enabled, VerticalScrollMode = ScrollMode.Disabled, Padding = new Thickness(10, 0, 10, 6) };
        SetRow(crumbScroll, 1);
        side.Children.Add(crumbScroll);
        entries = ListSupport.Create("GenericRowTemplate");
        entries.ItemClick += (_, e) =>
        {
            switch (e.ClickedItem)
            {
                case EntryItem item:
                    model.Select(item.Entry).Forget();
                    break;
                case UpItem:
                    var dir = model.Dir;
                    model.OpenDir(dir.Contains('/') ? dir[..dir.LastIndexOf('/')] : "").Forget();
                    break;
            }
        };
        entries.ContextRequested += (_, args) =>
        {
            if ((args.OriginalSource as FrameworkElement)?.DataContext is not EntryItem item) return;
            args.Handled = true;
            Ui.Menu(
                Ui.MenuItem(L.T("Code_CopyPath", "Copy path"), () => WindowInterop.Copy(item.Entry.Path)),
                Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open($"https://github.com/{model.Repo.Full}/{(item.Entry.IsDir ? "tree" : "blob")}/{model.CurrentRef}/{item.Entry.Path}"))).ShowAt((FrameworkElement)args.OriginalSource);
        };
        SetRow(entries, 2);
        side.Children.Add(entries);
        Children.Add(side);
        var line = Ui.Line(vertical: true);
        SetColumn(line, 1);
        Children.Add(line);
        SetColumn(content, 2);
        Children.Add(content);
        html.LinkClicked += OpenLink;

        var acc = new Microsoft.UI.Xaml.Input.KeyboardAccelerator { Key = VirtualKey.P, Modifiers = VirtualKeyModifiers.Control };
        acc.Invoked += (_, e) =>
        {
            e.Handled = true;
            QuickOpen().Forget();
        };
        KeyboardAccelerators.Add(acc);

        model.PropertyChanged += OnModel;
        Unloaded += (_, _) => model.PropertyChanged -= OnModel;
        Loaded += (_, _) =>
        {
            model.PropertyChanged -= OnModel;
            model.PropertyChanged += OnModel;
            RenderAll();
        };
        RenderAll();
    }

    private void RenderAll()
    {
        RenderBranches();
        RenderCrumbs();
        RenderEntries();
        RenderContent();
    }

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(RepoCodeViewModel.Branches):
            case nameof(RepoCodeViewModel.CurrentRef):
                RenderBranches();
                break;
            case nameof(RepoCodeViewModel.Dir):
                RenderCrumbs();
                break;
            case nameof(RepoCodeViewModel.Entries):
            case nameof(RepoCodeViewModel.DirError):
            case nameof(RepoCodeViewModel.SelectedFile):
                RenderEntries();
                if (e.PropertyName == nameof(RepoCodeViewModel.SelectedFile)) RenderContent();
                break;
            case nameof(RepoCodeViewModel.Readme):
            case nameof(RepoCodeViewModel.File):
            case nameof(RepoCodeViewModel.FileError):
            case nameof(RepoCodeViewModel.LoadingDir):
                RenderContent();
                break;
        }
    }

    private void RenderBranches()
    {
        branchButton.Content = Ui.H(6, Ui.Glyph("", 12), Ui.Text(model.CurrentRef.Length == 0 ? "…" : model.CurrentRef, 13, "text", FontWeights.Medium, mono: true));
        var menu = new MenuFlyout();
        foreach (var b in model.Branches.Take(300))
        {
            var name = b.Name;
            menu.Items.Add(Ui.CheckItem(name, name == model.CurrentRef, () => model.SwitchRef(name).Forget()));
        }
        branchButton.Flyout = menu;
    }

    private void RenderCrumbs()
    {
        crumbs.Children.Clear();
        var list = model.Crumbs;
        for (var i = 0; i < list.Count; i++)
        {
            if (i > 0) crumbs.Children.Add(Ui.Text("/", 13, "dim"));
            var (name, path) = list[i];
            var last = i == list.Count - 1;
            var b = new HyperlinkButton { Content = name, Padding = new Thickness(2, 0, 2, 0), FontWeight = i == 0 ? FontWeights.SemiBold : FontWeights.Normal, Foreground = Palette.Brush(last ? "text" : "accent") };
            b.Click += (_, _) =>
            {
                model.SelectedFile = null;
                model.File = null;
                model.OpenDir(path).Forget();
            };
            crumbs.Children.Add(b);
        }
    }

    private void RenderEntries()
    {
        syncing = true;
        var items = new List<object>();
        if (model.Dir.Length > 0) items.Add(new UpItem());
        if (model.DirError is { } e) items.Add(new TextRowItem(e));
        items.AddRange(model.Entries.Select(x => new EntryItem(x)));
        entries.ItemsSource = items;
        entries.SelectedItem = items.OfType<EntryItem>().FirstOrDefault(x => x.Entry.Path == model.SelectedFile);
        syncing = false;
    }

    private void RenderContent()
    {
        if (model.SelectedFile is { } path)
        {
            var grid = new Grid();
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
            grid.Children.Add(FileHeader(path));
            FrameworkElement bodyElement;
            if (model.FileError is { } err) bodyElement = Ui.Error(err, () => model.OpenFile(path).Forget());
            else if (model.File is { } file) bodyElement = FileBody(file, path);
            else bodyElement = Ui.Loading();
            SetRow(bodyElement, 1);
            grid.Children.Add(bodyElement);
            Detach(html);
            content.Content = grid;
            return;
        }
        if (model.Readme is { } readme)
        {
            Detach(html);
            var wrapper = new Border { Margin = new Thickness(10), CornerRadius = new CornerRadius(10), Background = Palette.Brush("panel"), Child = html };
            html.Render(Wrap(Resolve(readme, model.Dir)), "conversation", "top", "readme");
            content.Content = wrapper;
            return;
        }
        if (model.LoadingDir)
        {
            content.Content = Ui.Loading();
            return;
        }
        content.Content = Ui.Empty("", "teal", L.T("Code_Pick", "Pick a file on the left or press Ctrl+P"));
    }

    private static void Detach(FrameworkElement element)
    {
        switch (element.Parent)
        {
            case Panel p:
                p.Children.Remove(element);
                break;
            case Border b:
                b.Child = null;
                break;
            case ContentControl c:
                c.Content = null;
                break;
        }
    }

    private static readonly System.Text.RegularExpressions.Regex RelativeAttr = new(@"\b(src|href)=""(?![a-zA-Z][a-zA-Z0-9+.-]*:|//|/|#)([^""]+)""", System.Text.RegularExpressions.RegexOptions.Compiled);

    private string Resolve(string html, string dir)
    {
        var prefix = dir.Length == 0 ? "" : dir.TrimEnd('/') + "/";
        return RelativeAttr.Replace(html, m =>
        {
            var target = prefix + m.Groups[2].Value.TrimStart('.', '/');
            var url = m.Groups[1].Value == "src"
                ? $"https://raw.githubusercontent.com/{model.Repo.Full}/{model.CurrentRef}/{target}"
                : $"https://github.com/{model.Repo.Full}/blob/{model.CurrentRef}/{target}";
            return $"{m.Groups[1].Value}=\"{url}\"";
        });
    }

    private static string Wrap(string inner) => $"<div class=\"tl\"><div class=\"md\">{inner}</div></div>";

    private FrameworkElement FileHeader(string path)
    {
        var left = Ui.H(10, Ui.IconTile("", "sky", 20), Ui.Text(path, 13, "text", FontWeights.Medium, mono: true, select: true));
        if (model.FileSize is { } size) left.Children.Add(Ui.Chip(Fmt.Bytes(size)));
        if (model.File is { Kind: FileContentKind.Text } t) left.Children.Add(Ui.Chip(Fmt.Plural(t.Text.Count(c => c == '\n') + 1, "line", "lines")));
        var right = Ui.H(6);
        if (model.File is { Kind: FileContentKind.Markdown })
        {
            var toggle = new ToggleSwitch { OnContent = L.T("Code_Source", "Source"), OffContent = L.T("Code_Preview", "Preview"), IsOn = markdownSource, MinWidth = 0 };
            toggle.Toggled += (_, _) =>
            {
                markdownSource = toggle.IsOn;
                RenderContent();
            };
            right.Children.Add(toggle);
        }
        if (model.File is { Kind: FileContentKind.Text })
        {
            var w = new CheckBox { Content = L.T("Code_Wrap", "Wrap"), IsChecked = wrap, MinWidth = 0 };
            w.Checked += (_, _) => { wrap = true; RenderContent(); };
            w.Unchecked += (_, _) => { wrap = false; RenderContent(); };
            right.Children.Add(w);
        }
        var more = Ui.IconButton("", L.T("Common_More", "More"), () => { }, "text");
        var menu = Ui.Menu(Ui.MenuItem(L.T("Code_CopyPath", "Copy path"), () => WindowInterop.Copy(path)));
        if (model.File is { Kind: FileContentKind.Text or FileContentKind.Markdown } f) menu.Items.Add(Ui.MenuItem(L.T("Code_CopyContents", "Copy contents"), () => WindowInterop.Copy(f.Text)));
        menu.Items.Add(Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(model.WebUrl)));
        menu.Items.Add(Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(model.WebUrl)));
        more.Flyout = menu;
        right.Children.Add(more);
        var grid = Ui.Columns((left, Ui.Star), (right, Ui.Auto));
        grid.Padding = new Thickness(12, 8, 8, 8);
        return grid;
    }

    private FrameworkElement FileBody(FileContent file, string path)
    {
        var ext = RepoCodeViewModel.Extension(path);
        switch (file.Kind)
        {
            case FileContentKind.Text:
            {
                var panel = new Grid();
                panel.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
                panel.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
                if (file.Truncated) panel.Children.Add(Ui.H(6, Ui.Glyph("", 12, "amber"), Ui.Text(L.T("Code_Truncated", "File is too large — showing only part of it"), 12, "amber")));
                var border = new Border { Margin = new Thickness(10, 0, 10, 10), CornerRadius = new CornerRadius(10), Background = Palette.Brush("sunken"), Child = html };
                SetRow(border, 1);
                panel.Children.Add(border);
                html.Render(CodeHtml.Render(file.Text, ext, wrap), "code", "top", "flush code-body");
                return panel;
            }
            case FileContentKind.Markdown:
            {
                var border = new Border { Margin = new Thickness(10, 0, 10, 10), CornerRadius = new CornerRadius(10), Background = Palette.Brush(markdownSource ? "sunken" : "panel"), Child = html };
                if (markdownSource) html.Render(CodeHtml.Render(file.Text, ext, true), "code", "top", "flush code-body");
                else html.Render(Wrap(Resolve(file.Html, path.Contains('/') ? path[..path.LastIndexOf('/')] : "")), "conversation", "top", "readme");
                return border;
            }
            case FileContentKind.Image:
            {
                var image = new Image { Stretch = Stretch.None, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center, Margin = new Thickness(20) };
                LoadImage(image, file.Bytes ?? Array.Empty<byte>(), ext).Forget();
                return new Border
                {
                    Margin = new Thickness(10, 0, 10, 10),
                    CornerRadius = new CornerRadius(10),
                    Background = Palette.Brush("sunken"),
                    Child = new ScrollViewer { Content = image, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, ZoomMode = ZoomMode.Enabled }
                };
            }
            default:
            {
                var panel = Ui.V(8, Ui.IconTile("", "slate", 48), Ui.Text(L.T("Code_Binary", "Binary file") + " · " + Fmt.Bytes(file.Size), 13, "muted"), Ui.Button(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(model.WebUrl)));
                panel.HorizontalAlignment = HorizontalAlignment.Center;
                panel.VerticalAlignment = VerticalAlignment.Center;
                return panel;
            }
        }
    }

    private static async Task LoadImage(Image image, byte[] data, string ext)
    {
        try
        {
            using var stream = new Windows.Storage.Streams.InMemoryRandomAccessStream();
            using (var writer = new Windows.Storage.Streams.DataWriter(stream.GetOutputStreamAt(0)))
            {
                writer.WriteBytes(data);
                await writer.StoreAsync();
                await writer.FlushAsync();
                writer.DetachStream();
            }
            stream.Seek(0);
            if (ext.Equals("svg", StringComparison.OrdinalIgnoreCase))
            {
                var svg = new SvgImageSource();
                await svg.SetSourceAsync(stream);
                image.Source = svg;
            }
            else
            {
                var bmp = new BitmapImage();
                await bmp.SetSourceAsync(stream);
                image.Source = bmp;
                image.MaxWidth = Math.Min(bmp.PixelWidth, 1200);
                image.Stretch = Stretch.Uniform;
            }
        }
        catch
        {
        }
    }

    private void OpenLink(string url)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        var parts = uri.AbsolutePath.Split('/', StringSplitOptions.RemoveEmptyEntries);
        if (uri.Host == "github.com" && parts.Length >= 4 && parts[0].Equals(model.Repo.Owner, StringComparison.OrdinalIgnoreCase) && parts[1].Equals(model.Repo.Name, StringComparison.OrdinalIgnoreCase) && parts[2] is "blob" or "tree")
        {
            var path = string.Join("/", parts.Skip(4).Select(Uri.UnescapeDataString));
            if (parts[2] == "blob") model.Reveal(path).Forget();
            else model.OpenDir(path).Forget();
            return;
        }
        if (GitHubUrl.Parse(url, allowShort: false) is { Kind: RouteKind.Repo }) LinkBrowser.Open(url);
        else HtmlView.OpenLink(url);
    }

    private async Task QuickOpen()
    {
        if (XamlRoot is null) return;
        var box = new AutoSuggestBox { PlaceholderText = L.T("Code_GoToFilePlaceholder", "Go to file…"), QueryIcon = new SymbolIcon(Symbol.Find), MinWidth = 560 };
        var note = Ui.Text("", 12, "amber");
        var panel = Ui.V(8, box, note);
        var d = Dialogs.Create(XamlRoot, L.T("Code_GoToFileTitle", "Go to file"), panel, close: L.T("Common_Close", "Close"), maxWidth: 680);
        void Update()
        {
            var paths = model.AllPaths ?? new List<string>();
            box.ItemsSource = RepoCodeViewModel.FuzzyMatch(box.Text, paths, 50);
            note.Text = model.PathsTruncated ? L.T("Code_Incomplete", "Large repository — file list is incomplete") : model.AllPaths is null ? L.T("Common_Loading", "Loading…") : "";
        }
        box.TextChanged += (_, e) =>
        {
            if (e.Reason == AutoSuggestionBoxTextChangeReason.UserInput) Update();
        };
        box.QuerySubmitted += (_, e) =>
        {
            var pick = e.ChosenSuggestion as string ?? RepoCodeViewModel.FuzzyMatch(box.Text, model.AllPaths ?? new List<string>(), 1).FirstOrDefault();
            if (pick is null) return;
            d.Hide();
            model.Reveal(pick).Forget();
        };
        LoadPaths().Forget();
        async Task LoadPaths()
        {
            await model.LoadAllPaths();
            Update();
            box.IsSuggestionListOpen = true;
        }
        d.Opened += (_, _) => box.Focus(FocusState.Programmatic);
        await Dialogs.ShowAsync(d);
    }
}

public sealed record UpItem : IRowContent
{
    public UIElement BuildRow()
    {
        var p = Ui.H(7, Ui.Glyph("", 12), Ui.Text("..", 13, "muted"));
        p.Margin = new Thickness(0, 4, 0, 4);
        return p;
    }
}

public sealed record EntryItem(TreeEntry Entry) : IRowContent
{
    public UIElement BuildRow()
    {
        var e = Entry;
        var ext = RepoCodeViewModel.Extension(e.Name).ToLowerInvariant();
        var (glyph, color) = e.IsDir ? ("", "sky") : e.IsSubmodule ? ("", "indigo") : ext switch
        {
            "md" or "markdown" or "txt" => ("", "muted"),
            "png" or "jpg" or "jpeg" or "gif" or "webp" or "svg" or "ico" => ("", "muted"),
            "json" or "yaml" or "yml" or "toml" or "xml" or "plist" => ("", "muted"),
            "sh" or "bash" or "zsh" or "ps1" => ("", "muted"),
            "lock" => ("", "muted"),
            _ => ("", "muted")
        };
        var right = !e.IsDir && e.Object?.ByteSize is { } size ? Ui.Text(Fmt.Bytes(size), 11, "dim") : null;
        var grid = Ui.Columns((Ui.Glyph(glyph, 13, color), new GridLength(18)), (Ui.Text(e.Name, 13, "text", e.IsDir ? FontWeights.Medium : FontWeights.Normal), Ui.Star), (right, Ui.Auto));
        grid.ColumnSpacing = 7;
        grid.Padding = new Thickness(0, 3, 0, 3);
        return grid;
    }
}
