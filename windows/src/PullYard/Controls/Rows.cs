using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using PullYard.Core.Api;
using PullYard.Core.Claude;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;
using VisualState = PullYard.Core.Api.VisualState;

namespace PullYard.Controls;

public abstract class CodeRow : ContentControl
{
    protected CodeRow()
    {
        HorizontalContentAlignment = HorizontalAlignment.Stretch;
        HorizontalAlignment = HorizontalAlignment.Stretch;
        IsTabStop = false;
        DataContextChanged += (_, e) => Content = e.NewValue is null ? null : Build(e.NewValue);
    }

    protected abstract UIElement? Build(object item);
}

public sealed class GroupHeaderItem
{
    public RepoGroup Group { get; init; } = new("", new());
    public bool Expanded { get; init; }
}

public sealed class FeedRow : CodeRow
{
    public static bool ShowRepoDefault { get; set; } = true;

    protected override UIElement? Build(object item) => item switch
    {
        SearchItem s => SearchRow(s, true),
        GroupedSearchItem g => SearchRow(g.Item, false),
        GroupHeaderItem h => GroupHeader(h),
        _ => null
    };

    public static UIElement SearchRow(SearchItem item, bool showRepo)
    {
        var meta = Ui.H(5,
            Ui.Caption(showRepo ? $"{item.Repository.NameWithOwner}#{item.Number}" : $"#{item.Number}"),
            Ui.Caption("·"),
            Ui.Caption(Fmt.Rel(item.UpdatedAt)));
        var who = Ui.H(6, Ui.Avatar(item.Author?.AvatarUrl, 14), Ui.Caption(item.Author?.Login ?? ""));
        if (item.IsPR)
        {
            if (Ui.CiIcon(item.CiState, 11) is { } ci) who.Children.Add(ci);
            switch (item.ReviewDecision)
            {
                case "APPROVED":
                    var a = Ui.Glyph("", 11, "green");
                    Ui.Tip(a, L.T("Review_Approved", "Approved"));
                    who.Children.Add(a);
                    break;
                case "CHANGES_REQUESTED":
                    var c = Ui.Glyph("", 11, "red");
                    Ui.Tip(c, L.T("Review_ChangesRequested", "Changes requested"));
                    who.Children.Add(c);
                    break;
                case "REVIEW_REQUIRED":
                    var r = Ui.Glyph("", 11, "muted");
                    Ui.Tip(r, L.T("Review_Required", "Review required"));
                    who.Children.Add(r);
                    break;
            }
            if (item.Additions is { } add && item.Deletions is { } del) who.Children.Add(Ui.DiffStat(add, del, 11.5));
        }
        if (item.Comments.TotalCount > 0) who.Children.Add(Ui.H(3, Ui.Glyph("", 11), Ui.Caption(item.Comments.TotalCount.ToString())));
        var body = Ui.V(3, Ui.Text(item.Title, 13.5, "text", FontWeights.SemiBold, maxLines: 2), meta, who);
        if (item.StackedOn is { } baseRef)
        {
            var s = Ui.H(4, Ui.Glyph("", 11, "indigo"), Ui.Text(L.F("Item_StackedOn", "stacked on {0}", baseRef), 12, "indigo"));
            Ui.Tip(s, baseRef);
            body.Children.Add(s);
        }
        if (item.Labels?.Items is { Count: > 0 } labels)
        {
            var row = Ui.H(4);
            foreach (var l in labels.Take(4)) row.Children.Add(Ui.LabelChip(l));
            body.Children.Add(row);
        }
        var grid = Ui.Columns((Ui.StateBadge(item.VisualState, true), Ui.Auto), (body, Ui.Star));
        grid.ColumnSpacing = 10;
        grid.Padding = new Thickness(0, 6, 0, 6);
        return grid;
    }

    private static UIElement GroupHeader(GroupHeaderItem h)
    {
        var color = Palette.ProjectKey(h.Group.Id);
        var count = new Border
        {
            Background = Palette.Brush(color, 0.14),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(6, 0, 6, 1),
            Child = Ui.Text(h.Group.Items.Count.ToString(), 11, color, FontWeights.SemiBold),
            VerticalAlignment = VerticalAlignment.Center
        };
        var grid = Ui.Columns(
            (Ui.Glyph(h.Expanded ? "" : "", 10, "dim"), Ui.Auto),
            (Ui.IconTile("", color, 24), Ui.Auto),
            (Ui.V(0, Ui.Text(h.Group.Name, 13, "text", FontWeights.SemiBold), Ui.Text(h.Group.Owner, 11, "dim")), Ui.Star),
            (count, Ui.Auto));
        grid.Padding = new Thickness(0, 6, 0, 4);
        Ui.Tip(grid, h.Group.Id);
        return grid;
    }
}

public sealed record GroupedSearchItem(SearchItem Item);

public sealed class NotificationRow : CodeRow
{
    protected override UIElement? Build(object item)
    {
        if (item is not NotificationThread n) return null;
        var (glyph, color) = n.Subject.Type switch
        {
            "PullRequest" => ("", "violet"),
            "Issue" => ("", "green"),
            "Release" => ("", "teal"),
            "Discussion" => ("", "indigo"),
            "CheckSuite" => ("", "red"),
            "Commit" => ("", "sky"),
            _ => ("", "amber")
        };
        var reasonColor = n.Reason switch
        {
            "review_requested" => "sky",
            "mention" or "team_mention" => "amber",
            "security_alert" => "red",
            _ => "muted"
        };
        var reason = n.Reason switch
        {
            "review_requested" => "review requested",
            "team_mention" => "team mention",
            "assign" => "assigned",
            "state_change" => "state change",
            "ci_activity" => "CI",
            "manual" => "subscribed",
            "security_alert" => "security alert",
            var r => r.Replace('_', ' ')
        };
        var dot = new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 7, Height = 7, Fill = n.Unread ? Palette.Brush("sky") : new SolidColorBrush(Microsoft.UI.Colors.Transparent), Margin = new Thickness(0, 7, 0, 0), VerticalAlignment = VerticalAlignment.Top };
        var tile = Ui.IconTile(glyph, color, 22);
        tile.VerticalAlignment = VerticalAlignment.Top;
        var title = Ui.Text(n.Subject.Title, 13.5, n.Unread ? "text" : "muted", n.Unread ? FontWeights.SemiBold : FontWeights.Normal, maxLines: 2);
        var meta = Ui.H(5, Ui.Avatar(n.Repository.Owner?.AvatarUrl, 13), Ui.Caption(n.Repository.FullName), Ui.Caption("·", "dim"), Ui.Caption(reason, reasonColor), Ui.Caption("·", "dim"), Ui.Caption(Fmt.Rel(n.UpdatedAt), "dim"));
        var grid = Ui.Columns((dot, Ui.Auto), (tile, Ui.Auto), (Ui.V(3, title, meta), Ui.Star));
        grid.Padding = new Thickness(0, 5, 0, 5);
        return grid;
    }
}

public sealed class RepoRow : CodeRow
{
    protected override UIElement? Build(object item)
    {
        if (item is not RepoSummary repo) return null;
        var tile = Ui.IconTile(repo.IsPrivate ? "" : "", repo.IsArchived ? "slate" : "teal", 22);
        tile.VerticalAlignment = VerticalAlignment.Top;
        var title = Ui.H(5, Ui.Text(repo.NameWithOwner, 13.5, "text", FontWeights.SemiBold));
        if (repo.IsArchived) title.Children.Add(Ui.Chip(L.T("Repo_Archived", "archived"), "amber"));
        if (repo.IsFork) title.Children.Add(Ui.Glyph("", 10, "dim"));
        var body = Ui.V(3, title);
        if (!string.IsNullOrEmpty(repo.Description)) body.Children.Add(Ui.Text(repo.Description, 12, "muted", maxLines: 2));
        var stats = Ui.H(10);
        if (repo.PrimaryLanguage is { } lang)
            stats.Children.Add(Ui.H(3, new Microsoft.UI.Xaml.Shapes.Ellipse { Width = 8, Height = 8, Fill = Palette.HexBrush(lang.Color ?? "#888") }, Ui.Caption(lang.Name)));
        if (repo.PullRequests.TotalCount > 0) stats.Children.Add(Ui.H(3, Ui.Glyph("", 11, "violet"), Ui.Caption(repo.PullRequests.TotalCount.ToString(), "violet")));
        if (repo.Issues.TotalCount > 0) stats.Children.Add(Ui.H(3, Ui.Glyph("", 11, "green"), Ui.Caption(repo.Issues.TotalCount.ToString(), "green")));
        if (repo.StargazerCount > 0) stats.Children.Add(Ui.H(3, Ui.Glyph("", 11, "amber"), Ui.Caption(repo.StargazerCount.ToString(), "amber")));
        var line = Ui.Columns((stats, Ui.Star), (Ui.Caption(Fmt.Rel(repo.PushedAt), "dim"), Ui.Auto));
        body.Children.Add(line);
        var grid = Ui.Columns((tile, Ui.Auto), (body, Ui.Star));
        grid.Padding = new Thickness(0, 5, 0, 5);
        return grid;
    }
}

public sealed class ProjectRow : CodeRow
{
    protected override UIElement? Build(object item)
    {
        if (item is not ProjectSummary p) return null;
        var tile = Ui.IconTile("", p.Closed ? "slate" : "indigo", 22);
        tile.VerticalAlignment = VerticalAlignment.Top;
        var title = Ui.H(5, Ui.Text(p.Title, 13.5, "text", FontWeights.SemiBold));
        if (p.Closed) title.Children.Add(Ui.Chip(L.T("Project_Closed", "closed"), "slate"));
        var body = Ui.V(3, title);
        if (!string.IsNullOrEmpty(p.ShortDescription)) body.Children.Add(Ui.Text(p.ShortDescription, 12, "muted", maxLines: 2));
        var meta = Ui.H(6, Ui.Caption($"{p.Owner?.Login ?? ""} · #{p.Number}"), Ui.Caption("·", "dim"), Ui.Caption(Fmt.Plural(p.Items.TotalCount, "item", "items"), "indigo"));
        body.Children.Add(Ui.Columns((meta, Ui.Star), (Ui.Caption(Fmt.Rel(p.UpdatedAt), "dim"), Ui.Auto)));
        var grid = Ui.Columns((tile, Ui.Auto), (body, Ui.Star));
        grid.Padding = new Thickness(0, 5, 0, 5);
        return grid;
    }
}

public sealed class SessionRow : CodeRow
{
    protected override UIElement? Build(object item)
    {
        return item switch
        {
            SessionListItem s => Session(s),
            ProjectHeaderItem p => ProjectHeader(p),
            OpenTabItem t => OpenTab(t),
            SectionLabelItem l => Ui.Text(l.Text, 11.5, "muted", FontWeights.SemiBold),
            _ => null
        };
    }

    private static UIElement Session(SessionListItem item)
    {
        var s = item.Session;
        var bar = new Border { Width = 3, CornerRadius = new CornerRadius(1.5), Background = Palette.Brush(Palette.ProjectKey(ClaudeSessionParser.ProjectRoot(s.Cwd)), item.Open ? 1 : 0.45) };
        var title = Ui.H(6);
        if (item.Open) title.Children.Add(Ui.Glyph("", 10, "green"));
        title.Children.Add(Ui.Text(s.Title, 13, "text", item.Open ? FontWeights.SemiBold : FontWeights.Normal, maxLines: 2));
        var meta = Ui.H(5);
        if (s.Branch is { } b) meta.Children.Add(BranchTag(b));
        if (s.WorktreeName is { } w) meta.Children.Add(Ui.H(3, Ui.Glyph("", 10, "teal"), Ui.Text(w, 12, "teal")));
        if (s.PrNumber is { } n) meta.Children.Add(Ui.Chip($"#{n}", "violet", ""));
        var right = Ui.H(5);
        if (s.HasDiff) right.Children.Add(Ui.DiffStat(s.LinesAdded ?? 0, s.LinesRemoved ?? 0, 11.5));
        right.Children.Add(Ui.Caption(Fmt.Rel(s.Modified), "dim"));
        var body = Ui.V(4, title, Ui.Columns((meta, Ui.Star), (right, Ui.Auto)));
        var grid = Ui.Columns((bar, Ui.Auto), (body, Ui.Star));
        grid.Padding = new Thickness(0, 3, 0, 3);
        return grid;
    }

    public static Border BranchTag(string name)
    {
        var c = Palette.BranchKey(name);
        return Ui.Chip(name, c, "");
    }

    private static UIElement ProjectHeader(ProjectHeaderItem item)
    {
        var p = item.Project;
        var color = Palette.ProjectKey(p.Path);
        var count = new Border
        {
            Background = Palette.Brush(color, 0.14),
            CornerRadius = new CornerRadius(8),
            Padding = new Thickness(6, 0, 6, 1),
            Child = Ui.Text(p.Sessions.Count.ToString(), 11, color, FontWeights.SemiBold),
            VerticalAlignment = VerticalAlignment.Center
        };
        var plus = Ui.IconButton("", L.T("Claude_NewSession", "New session"), () => item.OnNew?.Invoke(), "coral");
        plus.IsEnabled = p.Exists;
        var grid = Ui.Columns(
            (Ui.Glyph(item.Expanded ? "" : "", 10, "dim"), Ui.Auto),
            (Ui.IconTile(p.Exists ? "" : "", p.Exists ? color : "slate", 24), Ui.Auto),
            (Ui.V(0, Ui.Text(p.Name, 13, "text", FontWeights.SemiBold), Ui.Text(Fmt.HomeRelative(p.Path), 11, "dim")), Ui.Star),
            (count, Ui.Auto),
            (plus, Ui.Auto));
        grid.Opacity = p.Exists ? 1 : 0.6;
        grid.Padding = new Thickness(0, 4, 0, 2);
        return grid;
    }

    private static UIElement OpenTab(OpenTabItem item)
    {
        var tab = item.Tab;
        var bar = new Border { Width = 3, CornerRadius = new CornerRadius(1.5), Background = Palette.Brush(Palette.ProjectKey(tab.Folder)) };
        var title = Ui.H(6, Ui.Glyph(tab.Kind == ClaudeTabKind.Chat ? "" : "", 10), Ui.Text(tab.Title, 13, "text", item.Selected ? FontWeights.SemiBold : FontWeights.Normal));
        var meta = Ui.Columns((StateBadge(tab.State, tab.MemoryBytes), Ui.Star), (Ui.Text(tab.ProjectName, 12, Palette.ProjectKey(tab.Folder)), Ui.Auto));
        var grid = Ui.Columns((bar, Ui.Auto), (Ui.V(3, title, meta), Ui.Star));
        grid.Padding = new Thickness(0, 3, 0, 3);
        return grid;
    }

    public static FrameworkElement StateBadge(ClaudeTabState state, long memory, bool compact = false)
    {
        var color = state.ColorKey();
        var glyph = state switch
        {
            ClaudeTabState.Working => "",
            ClaudeTabState.Waiting => "",
            ClaudeTabState.Active => "",
            ClaudeTabState.Sleeping => "",
            _ => ""
        };
        var p = Ui.H(4, Ui.Glyph(glyph, state == ClaudeTabState.Active ? 7 : 10, color));
        if (!compact)
        {
            p.Children.Add(Ui.Text(state.Title(), 12, color));
            if (memory > 0 && state != ClaudeTabState.Sleeping) p.Children.Add(Ui.Text("· " + Fmt.Memory(memory), 12, "muted"));
        }
        Ui.Tip(p, state.Help() + (memory > 0 ? " — " + Fmt.Memory(memory) : ""));
        return p;
    }
}

public sealed record SessionListItem(ClaudeSessionInfo Session, bool Open);

public sealed record ProjectHeaderItem(ClaudeProject Project, bool Expanded)
{
    public Action? OnNew { get; init; }
}

public sealed record OpenTabItem(ClaudeTab Tab, bool Selected);

public sealed record SectionLabelItem(string Text);

public interface IRowContent
{
    UIElement BuildRow();
}

public sealed class GenericRow : CodeRow
{
    protected override UIElement? Build(object item) => item is IRowContent c ? c.BuildRow() : null;
}

public static class ListSupport
{
    private static Style? stretch;

    public static DataTemplate Template(string key)
    {
        if (Application.Current.Resources.TryGetValue(key, out var v) && v is DataTemplate t) return t;
        foreach (var d in Application.Current.Resources.MergedDictionaries)
            if (d.TryGetValue(key, out var x) && x is DataTemplate dt) return dt;
        throw new InvalidOperationException($"Missing template {key}");
    }

    public static Style StretchItem
    {
        get
        {
            if (stretch is not null) return stretch;
            var s = new Style(typeof(ListViewItem));
            if (Application.Current.Resources.TryGetValue("DefaultListViewItemStyle", out var b) && b is Style basedOn) s.BasedOn = basedOn;
            s.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch));
            s.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(12, 0, 12, 0)));
            s.Setters.Add(new Setter(FrameworkElement.MinHeightProperty, 0d));
            stretch = s;
            return s;
        }
    }

    public static ListView Create(string template, ListViewSelectionMode mode = ListViewSelectionMode.Single)
    {
        var list = new ListView
        {
            ItemTemplate = Template(template),
            ItemContainerStyle = StretchItem,
            SelectionMode = mode,
            IsItemClickEnabled = true,
            Padding = new Thickness(0, 4, 0, 4)
        };
        return list;
    }
}
