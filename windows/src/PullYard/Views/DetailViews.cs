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
using VisualState = PullYard.Core.Api.VisualState;

namespace PullYard.Views;

public abstract class LiveDetail : Grid
{
    protected readonly AppViewModel App = AppViewModel.Shared;
    private bool subscribed;

    protected LiveDetail()
    {
        Loaded += (_, _) =>
        {
            if (subscribed) return;
            subscribed = true;
            App.RefreshRequested += OnRefresh;
            App.LiveTick += OnLive;
            OnAttached();
        };
        Unloaded += (_, _) =>
        {
            subscribed = false;
            App.RefreshRequested -= OnRefresh;
            App.LiveTick -= OnLive;
        };
    }

    protected virtual void OnAttached()
    {
    }

    protected virtual void OnRefresh()
    {
    }

    protected virtual void OnLive()
    {
    }

    protected static StackPanel Toolbar(params UIElement?[] items) => Ui.H(2, items);
}

public sealed class IssueDetailView : LiveDetail
{
    private readonly ItemRef itemRef;
    private IssueDetailViewModel? model;
    private readonly ContentControl host = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private HtmlView? html;
    private string lastHtml = "";
    private int lastToken;
    private readonly RefreshFooter status = new(compact: true);
    private readonly StackPanel header = Ui.V(8);
    private readonly StackPanel tools = Ui.H(2);

    public IssueDetailView(ItemRef r)
    {
        itemRef = r;
        Children.Add(host);
        host.Content = Ui.Loading();
    }

    protected override void OnAttached()
    {
        if (model is null && App.Api is { } api)
        {
            model = new IssueDetailViewModel(itemRef, api);
            model.PropertyChanged += OnModel;
        }
        model?.Load().Forget();
    }

    protected override void OnRefresh() => model?.Load().Forget();

    protected override void OnLive() => model?.Load().Forget();

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        if (model is null) return;
        switch (e.PropertyName)
        {
            case nameof(IssueDetailViewModel.IsPR):
                if (model.IsPR) host.Content = new PRDetailView(itemRef);
                break;
            case nameof(IssueDetailViewModel.Issue):
            case nameof(IssueDetailViewModel.Error):
                Render();
                break;
            case nameof(IssueDetailViewModel.Html):
                RenderHtml();
                break;
            case nameof(IssueDetailViewModel.ScrollToBottomToken):
                if (model.ScrollToBottomToken != lastToken)
                {
                    lastToken = model.ScrollToBottomToken;
                    html?.ScrollToBottom();
                }
                break;
            case nameof(IssueDetailViewModel.Loading):
            case nameof(IssueDetailViewModel.LastUpdated):
            case nameof(IssueDetailViewModel.RefreshError):
                status.Update(model.LastUpdated, model.Loading, model.RefreshError);
                break;
            case nameof(IssueDetailViewModel.ActionError):
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
        if (model is null || model.IsPR) return;
        if (model.Issue is not { } issue)
        {
            host.Content = model.Error is { } e ? Ui.Error(e, () => model.Load().Forget()) : Ui.Loading();
            return;
        }
        if (html is null)
        {
            var grid = new Grid();
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
            grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
            var top = Ui.Columns((header, Ui.Star), (Ui.H(4, status, tools), Ui.Auto));
            top.Padding = new Thickness(16, 12, 12, 10);
            ((FrameworkElement)top.Children[1]).VerticalAlignment = VerticalAlignment.Top;
            grid.Children.Add(top);
            html = new HtmlView();
            html.LinkClicked += url => HtmlView.OpenLink(url);
            SetRow(html, 1);
            grid.Children.Add(html);
            var composer = PRContent.Composer(L.T("PR_CommentPlaceholder", "Comment (Markdown) — Ctrl+Enter to send"), L.T("PR_Comment", "Comment"), model.Comment);
            SetRow(composer, 2);
            grid.Children.Add(composer);
            host.Content = grid;
        }
        header.Children.Clear();
        var titleText = Ui.Text("", 20, "text", FontWeights.Bold, wrap: true, select: true);
        titleText.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = issue.Title + "  " });
        titleText.Inlines.Add(new Microsoft.UI.Xaml.Documents.Run { Text = $"#{issue.Number}", Foreground = Palette.Brush("dim"), FontWeight = FontWeights.Normal });
        header.Children.Add(titleText);
        var time = Ui.Caption(Fmt.Rel(issue.CreatedAt));
        Ui.Tip(time, Fmt.Full(issue.CreatedAt));
        header.Children.Add(new WrapPanel(8, Ui.StateBadge(issue.VisualState), Ui.Avatar(issue.Author?.AvatarUrl, 18), Ui.Text(issue.Author?.Login ?? "ghost", 13, "text", FontWeights.SemiBold),
            Ui.Chip(itemRef.Repo.Full, "teal", ""), Ui.Chip(Fmt.Plural(issue.Comments.TotalCount, "comment", "comments"), "sky", ""), time));
        if (issue.Labels.Items.Count > 0 || issue.Assignees.Items.Count > 0)
        {
            var row = new WrapPanel(6);
            foreach (var l in issue.Labels.Items) row.Children.Add(Ui.LabelChip(l));
            if (issue.Assignees.Items.Count > 0)
            {
                row.Children.Add(Ui.Caption(L.T("PR_Assignees", "Assignees:")));
                foreach (var a in issue.Assignees.Items)
                {
                    var av = Ui.Avatar(a.AvatarUrl, 18);
                    Ui.Tip(av, a.Login);
                    row.Children.Add(av);
                }
            }
            header.Children.Add(row);
        }
        tools.Children.Clear();
        if (issue.ViewerCanUpdate)
        {
            tools.Children.Add(issue.IssueState == "OPEN"
                ? Ui.IconButton("", L.T("Issue_Close", "Close issue"), () => model.SetOpen(false).Forget(), "text")
                : Ui.IconButton("", L.T("Issue_Reopen", "Reopen issue"), () => model.SetOpen(true).Forget(), "text"));
        }
        tools.Children.Add(Ui.IconButton("", L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(issue.Url), "text"));
        tools.Children.Add(Ui.IconButton("", L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(issue.Url), "text"));
        RenderHtml();
        status.Update(model.LastUpdated, model.Loading, model.RefreshError);
    }

    private void RenderHtml()
    {
        if (html is null || model is null || model.Html == lastHtml) return;
        var first = lastHtml.Length == 0;
        lastHtml = model.Html;
        html.Render(model.Html, "conversation", first ? "top" : null);
    }
}

public sealed class CommitDetailView : LiveDetail
{
    private readonly RepoRef repo;
    private readonly string sha;
    private readonly ContentControl host = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private bool loaded;

    public CommitDetailView(RepoRef repo, string sha)
    {
        this.repo = repo;
        this.sha = sha;
        Children.Add(host);
        host.Content = Ui.Loading();
    }

    protected override void OnAttached()
    {
        if (!loaded) Load().Forget();
    }

    private async Task Load()
    {
        if (App.Api is not { } api) return;
        try
        {
            var c = await api.Commit(repo, sha);
            var files = await Task.Run(() => (c.Files ?? new()).Select(f => new DiffFile(f)).ToList());
            loaded = true;
            Render(c, files);
        }
        catch (Exception e)
        {
            host.Content = Ui.Error(e.Message, () => Load().Forget());
        }
    }

    private void Render(CommitDetailRest c, List<DiffFile> files)
    {
        var lines = c.Commit.Message.Replace("\r\n", "\n").Split('\n');
        var header = Ui.V(8, Ui.Text(lines.FirstOrDefault() ?? "", 17, "text", FontWeights.SemiBold, wrap: true, select: true));
        var rest = string.Join("\n", lines.Skip(1)).Trim();
        if (rest.Length > 0) header.Children.Add(Ui.Text(rest, 12.5, "muted", wrap: true, mono: true, select: true));
        var meta = Ui.H(8, Ui.Avatar(c.Author?.AvatarUrl, 18), Ui.Text(c.Author?.Login ?? c.Commit.Author?.Name ?? "", 13, "text", FontWeights.SemiBold), Ui.Caption(Fmt.Rel(c.Commit.Author?.Date)), Ui.Text(c.Sha, 12, "muted", mono: true, select: true));
        if (c.Stats is { } s) meta.Children.Add(Ui.DiffStat(s.Additions, s.Deletions));
        header.Children.Add(meta);
        var tools = Toolbar(
            Ui.IconButton("", L.T("Commit_CopySha", "Copy SHA"), () => WindowInterop.Copy(sha), "text"),
            Ui.IconButton("", L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(c.HtmlUrl), "text"));
        var top = Ui.Columns((header, Ui.Star), (tools, Ui.Auto));
        top.Padding = new Thickness(16);
        var html = new HtmlView();
        html.LinkClicked += url => HtmlView.OpenLink(url);
        var collapsed = files.Where(f => f.Lines.Count > 1500).Select(f => f.Id).ToHashSet();
        html.ActionInvoked += (type, id) =>
        {
            if (type == "copy") WindowInterop.Copy(id);
            if (type == "expand" && files.FirstOrDefault(f => f.Id == id) is { } file)
                html.Send(new System.Text.Json.Nodes.JsonObject { ["op"] = "fileBody", ["path"] = id, ["html"] = DiffHtml.Body(file, new DiffRenderOptions()) });
        };
        html.Render(DiffHtml.Render(files, new DiffRenderOptions { Collapsed = collapsed }), "diff", "top", "flush");
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        grid.Children.Add(top);
        var line = Ui.Line();
        SetRow(line, 1);
        grid.Children.Add(line);
        SetRow(html, 2);
        grid.Children.Add(html);
        host.Content = grid;
    }
}

public sealed class ProjectBoardView : LiveDetail
{
    private readonly ProjectRef project;
    private ProjectBoardViewModel? model;
    private readonly ContentControl host = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false };
    private readonly StackPanel columns = Ui.H(12);
    private readonly ComboBox groupBy = new() { MinWidth = 180 };
    private readonly TextBox filter = new() { PlaceholderText = L.T("Board_Filter", "Filter cards"), Width = 240 };
    private readonly TextBlock count = Ui.Caption("");
    private readonly RefreshFooter footer = new();
    private bool built;
    private bool syncing;

    public ProjectBoardView(ProjectRef project)
    {
        this.project = project;
        Children.Add(host);
        host.Content = Ui.Loading();
    }

    protected override void OnAttached()
    {
        if (model is null && App.Api is { } api)
        {
            model = new ProjectBoardViewModel(project, api);
            model.PropertyChanged += OnModel;
        }
        model?.Load().Forget();
    }

    protected override void OnRefresh() => model?.Load().Forget();

    protected override void OnLive() => model?.Load().Forget();

    private void OnModel(object? sender, PropertyChangedEventArgs e)
    {
        if (model is null) return;
        switch (e.PropertyName)
        {
            case nameof(ProjectBoardViewModel.Detail):
            case nameof(ProjectBoardViewModel.Items):
            case nameof(ProjectBoardViewModel.FieldId):
            case nameof(ProjectBoardViewModel.Error):
                Render();
                break;
            case nameof(ProjectBoardViewModel.Loading):
            case nameof(ProjectBoardViewModel.LastUpdated):
                footer.Update(model.LastUpdated, model.Loading, model.Error);
                break;
            case nameof(ProjectBoardViewModel.ActionError):
                if (model.ActionError is { } err)
                {
                    model.ActionError = null;
                    Dialogs.Alert(XamlRoot, L.T("Common_Error", "Error"), err).Forget();
                }
                break;
        }
    }

    private void Build()
    {
        built = true;
        var grid = new Grid();
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        grid.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        grid.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        groupBy.SelectionChanged += (_, _) =>
        {
            if (syncing || model is null) return;
            if ((groupBy.SelectedItem as ComboBoxItem)?.Tag is string id) model.FieldId = id;
        };
        filter.TextChanged += (_, _) => RenderColumns();
        var title = Ui.Text(project.Title, 17, "text", FontWeights.SemiBold);
        var top = Ui.Columns((Ui.H(10, title, groupBy, filter), Ui.Star), (Ui.H(8, count, Ui.IconButton("", L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(project.Url), "text")), Ui.Auto));
        top.Padding = new Thickness(14, 10, 10, 8);
        grid.Children.Add(top);
        var line = Ui.Line();
        SetRow(line, 1);
        grid.Children.Add(line);
        columns.Padding = new Thickness(14);
        var scroll = new ScrollViewer
        {
            Content = columns,
            HorizontalScrollBarVisibility = ScrollBarVisibility.Auto,
            VerticalScrollBarVisibility = ScrollBarVisibility.Disabled,
            HorizontalScrollMode = ScrollMode.Enabled,
            VerticalScrollMode = ScrollMode.Disabled
        };
        SetRow(scroll, 2);
        grid.Children.Add(scroll);
        footer.Retry = () => model?.Load().Forget();
        SetRow(footer, 3);
        grid.Children.Add(footer);
        host.Content = grid;
    }

    private void Render()
    {
        if (model is null) return;
        if (model.Detail is null)
        {
            host.Content = model.Error is { } e ? Ui.Error(e, () => model.Load().Forget()) : Ui.Loading();
            built = false;
            return;
        }
        if (!built) Build();
        syncing = true;
        groupBy.Items.Clear();
        foreach (var f in model.GroupFields) groupBy.Items.Add(new ComboBoxItem { Content = f.Name, Tag = f.Id });
        groupBy.SelectedItem = groupBy.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (string?)i.Tag == model.FieldId);
        groupBy.Visibility = model.GroupFields.Count > 1 ? Visibility.Visible : Visibility.Collapsed;
        syncing = false;
        count.Text = Fmt.Plural(model.Items.Count, "item", "items");
        footer.Update(model.LastUpdated, model.Loading, model.Error);
        RenderColumns();
    }

    private List<ProjectColumn> Columns()
    {
        if (model is null) return new();
        var text = filter.Text.Trim();
        var items = model.Items.Where(i => text.Length == 0 || (i.Content?.Title ?? "").Contains(text, StringComparison.OrdinalIgnoreCase) || (i.Content?.Repository?.NameWithOwner ?? "").Contains(text, StringComparison.OrdinalIgnoreCase)).ToList();
        if (model.Field is not { Options: { } options } field || model.FieldId is not { } fieldId)
            return new List<ProjectColumn> { new("all", null, L.T("Board_All", "All"), "GRAY", items) };
        var cols = new List<ProjectColumn>();
        var known = options.Select(o => o.Id).ToHashSet();
        var none = items.Where(i => i.OptionId(fieldId) is not { } oid || !known.Contains(oid)).ToList();
        if (none.Count > 0) cols.Add(new ProjectColumn("none", null, L.F("Board_No", "No {0}", field.Name ?? ""), "GRAY", none));
        foreach (var o in options) cols.Add(new ProjectColumn(o.Id, o.Id, o.Name, o.Color, items.Where(i => i.OptionId(fieldId) == o.Id).ToList()));
        return cols;
    }

    private void RenderColumns()
    {
        if (model is null) return;
        columns.Children.Clear();
        var options = model.Field?.Options ?? new List<ProjectOption>();
        foreach (var col in Columns()) columns.Children.Add(Column(col, options));
    }

    private FrameworkElement Column(ProjectColumn col, List<ProjectOption> options)
    {
        var color = Palette.ProjectOptionKey(col.Color);
        var badge = new Border { Background = Palette.Brush(color, 0.14), CornerRadius = new CornerRadius(8), Padding = new Thickness(6, 0, 6, 1), Child = Ui.Text(col.Items.Count.ToString(), 11.5, color, FontWeights.Medium) };
        var ring = new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 10, Height = 10, Stroke = Palette.Brush(color), StrokeThickness = 2 };
        var head = Ui.H(6, ring, Ui.Text(col.Name, 13, "text", FontWeights.SemiBold), badge);
        head.Margin = new Thickness(10);
        var cards = Ui.V(8);
        cards.Padding = new Thickness(8, 0, 8, 8);
        foreach (var item in col.Items) cards.Children.Add(Card(item, options));
        var body = new Grid();
        body.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        body.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        body.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        var stripe = new Border { Height = 3, Background = Palette.Brush(color) };
        body.Children.Add(stripe);
        SetRow(head, 1);
        body.Children.Add(head);
        var scroll = new ScrollViewer { Content = cards, VerticalScrollBarVisibility = ScrollBarVisibility.Auto };
        SetRow(scroll, 2);
        body.Children.Add(scroll);
        var border = new Border
        {
            Width = 290,
            CornerRadius = new CornerRadius(12),
            Background = Palette.Brush("sunken"),
            Child = body,
            AllowDrop = model?.FieldId is not null,
            BorderThickness = new Thickness(2),
            BorderBrush = new SolidColorBrush(Microsoft.UI.Colors.Transparent)
        };
        border.DragOver += (_, e) =>
        {
            e.AcceptedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Move;
            border.BorderBrush = Palette.Brush("accent");
        };
        border.DragLeave += (_, _) => border.BorderBrush = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
        border.Drop += async (_, e) =>
        {
            border.BorderBrush = new SolidColorBrush(Microsoft.UI.Colors.Transparent);
            if (!e.DataView.Contains(Windows.ApplicationModel.DataTransfer.StandardDataFormats.Text) || model is null) return;
            var id = await e.DataView.GetTextAsync();
            await model.Move(id, col.OptionId);
        };
        return border;
    }

    private FrameworkElement Card(ProjectItem item, List<ProjectOption> options)
    {
        var c = item.Content;
        var stateIcon = c?.VisualState is { } vs ? Ui.Glyph(vs.Glyph(), 12, Palette.StateKey(vs)) : Ui.Glyph("", 12, "dim");
        var repoText = c?.Repository is { } r ? $"{r.NameWithOwner}{(c.Number is { } n ? $"#{n}" : "")}" : L.T("Board_Draft", "Draft");
        var avatars = Ui.H(2);
        foreach (var a in c?.Assignees?.Items ?? new())
        {
            var av = Ui.Avatar(a.AvatarUrl, 16);
            Ui.Tip(av, a.Login);
            avatars.Children.Add(av);
        }
        var top = Ui.Columns((stateIcon, Ui.Auto), (Ui.Caption(repoText), Ui.Star), (avatars, Ui.Auto));
        top.ColumnSpacing = 5;
        var title = Ui.Text(c?.Title ?? "—", 13, "text", FontWeights.Medium, maxLines: 3);
        var card = new Border { Background = Palette.Brush("raised"), CornerRadius = new CornerRadius(8), Padding = new Thickness(10), Child = Ui.V(5, top, title), CanDrag = model?.FieldId is not null };
        card.DragStarting += (_, e) =>
        {
            e.Data.SetText(item.Id);
            e.Data.RequestedOperation = Windows.ApplicationModel.DataTransfer.DataPackageOperation.Move;
        };
        card.Tapped += (_, _) =>
        {
            if (c?.Route is { } route) App.Open(route, true);
        };
        var menu = new MenuFlyout();
        var move = new MenuFlyoutSubItem { Text = L.T("Board_MoveTo", "Move to") };
        foreach (var o in options) move.Items.Add(Ui.MenuItem(o.Name, () => model?.Move(item.Id, o.Id).Forget()));
        move.Items.Add(Ui.Separator());
        move.Items.Add(Ui.MenuItem(L.T("Board_NoStatus", "No Status"), () => model?.Move(item.Id, null).Forget()));
        if (options.Count > 0) menu.Items.Add(move);
        if (c?.Url is { } url)
        {
            menu.Items.Add(Ui.MenuItem(L.T("Menu_OpenInBrowser", "Open in Browser"), () => LinkBrowser.Open(url)));
            menu.Items.Add(Ui.MenuItem(L.T("Menu_CopyLink", "Copy Link"), () => WindowInterop.Copy(url)));
        }
        card.ContextFlyout = menu;
        return card;
    }
}
