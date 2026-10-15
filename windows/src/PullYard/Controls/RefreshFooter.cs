using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Core.Util;
using PullYard.Services;

namespace PullYard.Controls;

public sealed class RefreshFooter : Grid
{
    private readonly TextBlock label = Ui.Caption("");
    private readonly FontIcon icon = Ui.Glyph("", 11);
    private readonly ProgressRing ring = Ui.Spinner(11);
    private readonly HyperlinkButton retry;
    private readonly DispatcherTimer timer = new() { Interval = TimeSpan.FromSeconds(5) };
    private DateTimeOffset? updated;
    private bool refreshing;
    private string? error;
    private readonly bool compact;

    public Action? Retry { get; set; }

    public RefreshFooter(bool compact = false, bool line = true)
    {
        this.compact = compact;
        retry = Ui.Link(L.T("Common_Retry", "Retry"), () => Retry?.Invoke(), 12);
        retry.Visibility = Visibility.Collapsed;
        ring.Visibility = Visibility.Collapsed;
        var row = Ui.H(5, ring, icon, label, retry);
        row.HorizontalAlignment = compact ? HorizontalAlignment.Left : HorizontalAlignment.Center;
        row.VerticalAlignment = VerticalAlignment.Center;
        if (!compact) Height = 26;
        if (line && !compact)
        {
            var l = Ui.Line();
            l.VerticalAlignment = VerticalAlignment.Top;
            Children.Add(l);
        }
        Children.Add(row);
        timer.Tick += (_, _) => Render();
        Loaded += (_, _) => timer.Start();
        Unloaded += (_, _) => timer.Stop();
        Render();
    }

    public void Update(DateTimeOffset? updated, bool refreshing, string? error = null)
    {
        this.updated = updated;
        this.refreshing = refreshing;
        this.error = error;
        Render();
    }

    private void Render()
    {
        ring.Visibility = refreshing ? Visibility.Visible : Visibility.Collapsed;
        icon.Visibility = refreshing ? Visibility.Collapsed : Visibility.Visible;
        icon.Glyph = error is not null ? "" : "";
        icon.Foreground = Palette.Brush(error is not null ? "amber" : "muted");
        retry.Visibility = error is not null && !refreshing && Retry is not null ? Visibility.Visible : Visibility.Collapsed;
        string text;
        if (refreshing) text = L.T("Refresh_Updating", "Updating…");
        else if (error is not null) text = compact ? L.T("Refresh_Offline", "Offline") : L.T("Refresh_Failed", "Couldn't refresh");
        else if (updated is not { } u) text = compact ? "" : L.T("Refresh_NotYet", "Not updated yet");
        else
        {
            var ago = Fmt.Ago(u, DateTimeOffset.Now);
            text = compact ? ago : L.F("Refresh_Updated", "Updated {0}", ago);
        }
        label.Text = text;
        ToolTipService.SetToolTip(this, error is not null ? L.F("Refresh_LastFailed", "Last refresh failed: {0}", error) : L.T("Refresh_Hint", "Pull down to refresh (Ctrl+R)"));
    }
}
