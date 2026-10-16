using System.Collections.ObjectModel;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using PullYard.Controls;
using PullYard.Core;
using PullYard.Core.Util;
using PullYard.Services;

namespace PullYard.Views;

public sealed class DiagnosticsWindow : Window
{
    private static DiagnosticsWindow? current;

    private readonly Grid root = new();
    private readonly StackPanel header = Ui.V(4);
    private readonly Border columns = new();
    private readonly ListView list = ListSupport.Create("GenericRowTemplate", ListViewSelectionMode.None);
    private readonly TextBox filter = new() { PlaceholderText = L.T("Diagnostics_Filter", "Filter by name"), MinWidth = 220 };
    private readonly DispatcherQueueTimer timer;
    private readonly ObservableCollection<object> items = new();
    private long shownVersion = -1;
    private bool grouped;
    private RequestEntry? newestShown;

    public static void Show()
    {
        if (current is null)
        {
            current = new DiagnosticsWindow();
            current.Closed += (_, _) => current = null;
        }
        current.Activate();
        WindowInterop.BringToFront(current);
    }

    private DiagnosticsWindow()
    {
        Title = L.F("Diagnostics_WindowTitle", "{0} Diagnostics", AppInfo.DisplayName);
        try
        {
            SystemBackdrop = new MicaBackdrop();
        }
        catch
        {
        }
        AppWindow.SetIcon(Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico"));
        AppWindow.Resize(new Windows.Graphics.SizeInt32(1100, 760));

        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        root.RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });

        var card = Ui.Card(header, 10, "raised", 12);
        card.Margin = new Thickness(14, 14, 14, 8);
        root.Children.Add(card);

        var mode = new SelectorBar();
        var listItem = new SelectorBarItem { Text = L.T("Diagnostics_Requests", "Requests"), Tag = false };
        var groupItem = new SelectorBarItem { Text = L.T("Diagnostics_ByName", "By name"), Tag = true };
        mode.Items.Add(listItem);
        mode.Items.Add(groupItem);
        mode.SelectedItem = listItem;
        mode.SelectionChanged += (s, _) =>
        {
            grouped = s.SelectedItem?.Tag is true;
            Render(true, true);
        };
        filter.TextChanged += (_, _) => Render(true, true);
        var tools = Ui.H(8,
            Ui.Button(L.T("Diagnostics_Clear", "Clear"), () =>
            {
                Diagnostics.Shared.Clear();
                Render(true, true);
            }, glyph: ""),
            Ui.Button(L.T("Diagnostics_Export", "Export"), () => Export().Forget(), glyph: ""),
            Ui.Button(L.T("Diagnostics_CopySummary", "Copy summary"), () => WindowInterop.Copy(Diagnostics.Shared.SummaryText()), glyph: ""));
        var toolbar = Ui.Columns((mode, Ui.Auto), (filter, Ui.Star), (tools, Ui.Auto));
        toolbar.ColumnSpacing = 12;
        toolbar.Padding = new Thickness(14, 0, 14, 8);
        Grid.SetRow(toolbar, 1);
        root.Children.Add(toolbar);

        columns.Padding = new Thickness(26, 4, 26, 4);
        Grid.SetRow(columns, 2);
        root.Children.Add(columns);

        list.ItemsSource = items;
        Grid.SetRow(list, 3);
        root.Children.Add(list);

        Content = root;
        ApplyTheme();
        ThemeService.Changed += OnThemeChanged;
        DiagnosticsService.Changed += OnEnabledChanged;

        timer = DispatcherQueue.CreateTimer();
        timer.Interval = TimeSpan.FromSeconds(1);
        timer.IsRepeating = true;
        timer.Tick += (_, _) => Render(false);
        timer.Start();
        Closed += (_, _) =>
        {
            timer.Stop();
            ThemeService.Changed -= OnThemeChanged;
            DiagnosticsService.Changed -= OnEnabledChanged;
        };
        Render(true, true);
    }

    private void OnThemeChanged()
    {
        ApplyTheme();
        Render(true, true);
    }

    private void OnEnabledChanged() => Render(true);

    private void ApplyTheme()
    {
        var target = ThemeService.Appearance switch
        {
            AppearanceMode.Light => ElementTheme.Light,
            AppearanceMode.Dark => ElementTheme.Dark,
            _ => ElementTheme.Default
        };
        root.RequestedTheme = root.ActualTheme == ElementTheme.Dark ? ElementTheme.Light : ElementTheme.Dark;
        root.RequestedTheme = target;
        root.Background = ThemeService.IsBrand ? Palette.Brush("window") : new SolidColorBrush(Microsoft.UI.Colors.Transparent);
    }

    private void Render(bool force, bool rebuild = false)
    {
        var version = Diagnostics.Shared.Version;
        if (!force && version == shownVersion) return;
        shownVersion = version;
        var requests = Diagnostics.Shared.Requests();
        RenderHeader(Diagnostics.Summarize(requests, Diagnostics.Shared.Ticks(), Diagnostics.Shared.SlowUi()));
        var q = filter.Text.Trim();
        var shown = q.Length == 0 ? requests : requests.Where(r => r.Name.Contains(q, StringComparison.OrdinalIgnoreCase) || r.Path.Contains(q, StringComparison.OrdinalIgnoreCase)).ToList();
        if (rebuild)
        {
            columns.Child = grouped ? GroupRowItem.Header() : RequestRowItem.Header();
            items.Clear();
            newestShown = null;
        }
        if (grouped) SyncGroups(Diagnostics.Group(shown));
        else SyncRequests(shown);
    }

    private void SyncRequests(List<RequestEntry> shown)
    {
        var known = newestShown is null ? -1 : shown.FindIndex(r => ReferenceEquals(r, newestShown));
        if (known < 0)
        {
            items.Clear();
            foreach (var r in shown) items.Add(new RequestRowItem(r));
        }
        else
        {
            for (var i = known - 1; i >= 0; i--) items.Insert(0, new RequestRowItem(shown[i]));
            while (items.Count > shown.Count) items.RemoveAt(items.Count - 1);
        }
        newestShown = shown.FirstOrDefault();
    }

    private void SyncGroups(List<RequestGroup> groups)
    {
        for (var i = 0; i < groups.Count; i++)
        {
            var row = new GroupRowItem(groups[i]);
            if (i >= items.Count) items.Add(row);
            else if (!row.Equals(items[i])) items[i] = row;
        }
        while (items.Count > groups.Count) items.RemoveAt(items.Count - 1);
    }

    private void RenderHeader(DiagnosticsSummary s)
    {
        header.Children.Clear();
        if (!Diagnostics.Enabled)
            header.Children.Add(Ui.Text(L.T("Diagnostics_Off", "Recording is off. Turn on Diagnostics in Settings to record requests."), 12.5, "amber", wrap: true));
        header.Children.Add(Ui.Text(L.F("Diagnostics_Totals", "{0} requests · {1} · avg {2} ms · p95 {3} ms", s.Requests, Fmt.Bytes(s.TotalBytes), Ms(s.AvgMs), Ms(s.P95Ms)), 14, "text", FontWeights.SemiBold));
        var limits = s.RateLimits.Count == 0
            ? "—"
            : string.Join("   ", s.RateLimits.Select(l => L.F("Diagnostics_RateLimitEntry", "{0}: {1} left, resets {2}", l.Resource.Length == 0 ? "?" : l.Resource, l.Remaining, l.Reset is { } r ? r.ToLocalTime().ToString("HH:mm:ss", Fmt.Culture) : "—")));
        header.Children.Add(Ui.Text(L.F("Diagnostics_RateLimit", "Rate limit: {0}", limits), 12.5, "muted", wrap: true));
        var ticks = s.RecentTicks.Count == 0 ? "—" : string.Join(", ", s.RecentTicks.Select(t => $"{Ms(t.DurationMs)} ms/{t.Requests}"));
        var average = s.TickCount == 0 ? "—" : L.F("Diagnostics_TickAverage", "avg {0} ms, {1} requests over {2} ticks", Ms(s.TickAvgMs), s.TickAvgRequests.ToString("0.#", Fmt.Culture), s.TickCount);
        header.Children.Add(Ui.Text(L.F("Diagnostics_TicksSummary", "Live ticks with requests: {0}", average), 12.5, "muted", wrap: true));
        header.Children.Add(Ui.Text(L.F("Diagnostics_Ticks", "Live ticks (duration/requests, newest first): {0}", ticks), 12.5, "muted", wrap: true));
        header.Children.Add(Ui.Text(s.SlowUiCount == 0
            ? L.F("Diagnostics_SlowUiNone", "Slow UI (over {0} ms): none", Ms(DiagnosticsService.SlowUiThresholdMs))
            : L.F("Diagnostics_SlowUi", "Slow UI (over {0} ms): {1}, max {2} ms", Ms(DiagnosticsService.SlowUiThresholdMs), s.SlowUiCount, Ms(s.SlowUiMaxMs)), 12.5, "muted", wrap: true));
    }

    private async Task Export()
    {
        var name = $"{AppInfo.ExeName}-diagnostics-{DateTimeOffset.Now.ToString("yyyyMMdd-HHmmss", Fmt.Culture)}";
        if (await WindowInterop.PickSaveFile(this, name, L.T("Diagnostics_JsonLines", "JSON lines"), ".jsonl") is not { } path) return;
        try
        {
            await File.WriteAllTextAsync(path, Diagnostics.Shared.ExportJsonLines());
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, "diagnostics export failed");
        }
    }

    internal static string Ms(double ms) => ms.ToString("0", Fmt.Culture);
}

public sealed record RequestRowItem(RequestEntry Entry) : IRowContent
{
    private static readonly GridLength[] Widths = { new(92), new(58), new(1, GridUnitType.Star), new(72), new(64), new(70), new(80), new(64) };

    public static UIElement Header() => Row(
        Ui.Caption(L.T("Diagnostics_ColTime", "Time")),
        Ui.Caption(L.T("Diagnostics_ColKind", "Kind")),
        Ui.Caption(L.T("Diagnostics_ColName", "Name")),
        Ui.Caption(L.T("Diagnostics_ColOrigin", "Origin")),
        Ui.Caption(L.T("Diagnostics_ColStatus", "Status")),
        Right(Ui.Caption(L.T("Diagnostics_ColDuration", "ms"))),
        Right(Ui.Caption(L.T("Diagnostics_ColBytes", "Bytes"))),
        Right(Ui.Caption(L.T("Diagnostics_ColRemaining", "Left"))));

    public UIElement BuildRow()
    {
        var e = Entry;
        var failed = e.Error is not null || e.Status is 0 or >= 400;
        var status = Ui.Text(e.Error is null ? e.Status.ToString(Fmt.Culture) : L.T("Diagnostics_Error", "error"), 12, failed ? "red" : e.Note is null ? "green" : "amber", mono: true);
        if ((e.Error ?? e.Note) is { } detail) Ui.Tip(status, detail);
        var name = Ui.Text(e.Name, 12, "text", mono: true, maxLines: 1);
        Ui.Tip(name, e.Path.Length == 0 || e.Path == e.Name ? e.Name : e.Name + "\n" + e.Path);
        var row = Row(
            Ui.Text(e.Timestamp.ToLocalTime().ToString("HH:mm:ss.fff", Fmt.Culture), 12, "muted", mono: true),
            Ui.Text(e.Kind, 12, e.Kind == Diagnostics.KindGraphQL ? "violet" : "sky"),
            name,
            Ui.Text(e.Origin, 12, e.Origin == Diagnostics.OriginLiveTick ? "amber" : "muted"),
            status,
            Right(Ui.Text(DiagnosticsWindow.Ms(e.DurationMs), 12, "text", mono: true)),
            Right(Ui.Text(Fmt.Bytes(e.Bytes), 12, "muted", mono: true)),
            Right(Ui.Text(e.RateLimitRemaining?.ToString(Fmt.Culture) ?? "", 12, "muted", mono: true)));
        row.Padding = new Thickness(0, 3, 0, 3);
        return row;
    }

    private static TextBlock Right(TextBlock t)
    {
        t.HorizontalAlignment = HorizontalAlignment.Right;
        return t;
    }

    private static Grid Row(params UIElement[] cells) => Ui.Columns(cells.Select((c, i) => ((UIElement?)c, Widths[i])).ToArray());
}

public sealed record GroupRowItem(RequestGroup Group) : IRowContent
{
    private static readonly GridLength[] Widths = { new(1, GridUnitType.Star), new(64), new(72), new(72), new(90) };

    public static UIElement Header() => Row(
        Ui.Caption(L.T("Diagnostics_ColName", "Name")),
        Right(Ui.Caption(L.T("Diagnostics_ColCount", "Count"))),
        Right(Ui.Caption(L.T("Diagnostics_ColAvg", "Avg ms"))),
        Right(Ui.Caption(L.T("Diagnostics_ColP95", "p95 ms"))),
        Right(Ui.Caption(L.T("Diagnostics_ColTotalBytes", "Total bytes"))));

    public UIElement BuildRow()
    {
        var g = Group;
        var name = Ui.Text(g.Name, 12, "text", mono: true, maxLines: 1);
        Ui.Tip(name, g.Name);
        var row = Row(
            name,
            Right(Ui.Text(g.Count.ToString(Fmt.Culture), 12, "text", mono: true)),
            Right(Ui.Text(DiagnosticsWindow.Ms(g.AvgMs), 12, "text", mono: true)),
            Right(Ui.Text(DiagnosticsWindow.Ms(g.P95Ms), 12, "text", mono: true)),
            Right(Ui.Text(Fmt.Bytes(g.TotalBytes), 12, "muted", mono: true)));
        row.Padding = new Thickness(0, 3, 0, 3);
        return row;
    }

    private static TextBlock Right(TextBlock t)
    {
        t.HorizontalAlignment = HorizontalAlignment.Right;
        return t;
    }

    private static Grid Row(params UIElement[] cells) => Ui.Columns(cells.Select((c, i) => ((UIElement?)c, Widths[i])).ToArray());
}
