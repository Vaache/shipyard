using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PullYard.Controls;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Views;

public abstract class ListPane : Grid
{
    protected readonly AppViewModel Model = AppViewModel.Shared;
    protected readonly TextBlock TitleText;
    protected readonly TextBlock CountText = Ui.Text("", 13, "muted");
    protected readonly StackPanel Tools = Ui.H(2);
    protected readonly StackPanel FilterRow = Ui.V(6);
    protected readonly ProgressBar LoadingBar = new() { IsIndeterminate = true, Visibility = Visibility.Collapsed, Height = 3, MinHeight = 3 };
    protected readonly RefreshContainer Refresh = new();
    protected readonly ListView List;
    protected readonly ContentControl Overlay = new() { HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch, IsTabStop = false, IsHitTestVisible = true };
    protected readonly RefreshFooter Footer = new();
    private bool subscribed;

    protected ListPane(string title, string template)
    {
        TitleText = Ui.Text(title, 15, "text", FontWeights.SemiBold);
        List = ListSupport.Create(template);
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });

        var header = Ui.Columns((Ui.H(8, TitleText, CountText), Ui.Star), (Tools, Ui.Auto));
        header.Padding = new Thickness(14, 10, 8, 6);
        Children.Add(header);

        FilterRow.Padding = new Thickness(12, 0, 12, 6);
        SetRow(FilterRow, 1);
        Children.Add(FilterRow);

        SetRow(LoadingBar, 2);
        Children.Add(LoadingBar);

        Refresh.Content = List;
        Refresh.RefreshRequested += async (_, e) =>
        {
            var deferral = e.GetDeferral();
            try
            {
                await OnPullRefresh();
            }
            finally
            {
                deferral.Complete();
            }
        };
        var area = new Grid();
        area.Children.Add(Refresh);
        area.Children.Add(Overlay);
        SetRow(area, 3);
        Children.Add(area);

        Footer.Retry = () => OnPullRefresh().Forget();
        SetRow(Footer, 4);
        Children.Add(Footer);

        Loaded += (_, _) =>
        {
            if (subscribed) return;
            subscribed = true;
            Model.RefreshRequested += OnManualRefresh;
            Model.LiveTick += OnLiveTick;
            OnAttached();
        };
        Unloaded += (_, _) =>
        {
            subscribed = false;
            Model.RefreshRequested -= OnManualRefresh;
            Model.LiveTick -= OnLiveTick;
        };
    }

    protected virtual void OnAttached()
    {
    }

    private void OnManualRefresh() => OnPullRefresh().Forget();

    protected virtual void OnLiveTick()
    {
    }

    protected abstract Task OnPullRefresh();

    protected void ShowOverlay(FrameworkElement? element)
    {
        Overlay.Content = element;
        Overlay.Visibility = element is null ? Visibility.Collapsed : Visibility.Visible;
    }

    protected void SetLoading(bool loading, bool hasItems)
    {
        LoadingBar.Visibility = loading && hasItems ? Visibility.Visible : Visibility.Collapsed;
        List.Opacity = loading && hasItems ? 0.6 : 1;
    }

    protected static SelectorBar StateBar(StateFilter current, Action<StateFilter> changed)
    {
        var bar = new SelectorBar();
        foreach (var s in Enum.GetValues<StateFilter>())
        {
            var item = new SelectorBarItem { Text = s.Title(), Tag = s };
            bar.Items.Add(item);
            if (s == current) bar.SelectedItem = item;
        }
        bar.SelectionChanged += (sender, _) =>
        {
            if (sender.SelectedItem?.Tag is StateFilter f) changed(f);
        };
        return bar;
    }

    protected static TextBox SearchBox(string placeholder, Action<string> changed, int debounceMs = 350)
    {
        var box = new TextBox { PlaceholderText = placeholder };
        DispatcherTimer? timer = null;
        box.TextChanged += (_, _) =>
        {
            timer?.Stop();
            timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(debounceMs) };
            timer.Tick += (t, _) =>
            {
                ((DispatcherTimer)t!).Stop();
                changed(box.Text);
            };
            timer.Start();
        };
        return box;
    }

    protected static AppBarToggleButton Toggle(string glyph, string tooltip, bool on, Action<bool> changed)
    {
        var b = new AppBarToggleButton { Icon = new FontIcon { Glyph = glyph, FontFamily = Ui.Icons }, IsChecked = on, LabelPosition = CommandBarLabelPosition.Collapsed, Width = 40, Height = 36 };
        ToolTipService.SetToolTip(b, tooltip);
        b.Click += (_, _) => changed(b.IsChecked == true);
        return b;
    }
}
