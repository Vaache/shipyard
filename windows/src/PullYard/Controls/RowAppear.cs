using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Animation;

namespace PullYard.Controls;

public enum RowAppearMode { Silent, Refresh, Cascade }

public sealed class RowAppear
{
    private static readonly TimeSpan Window = TimeSpan.FromSeconds(1.5);
    private static readonly Duration Length = new(TimeSpan.FromMilliseconds(220));

    private readonly Func<object, string?> key;
    private HashSet<string> known = new();
    private readonly HashSet<string> pending = new();
    private DateTimeOffset until;
    private bool cascade;

    public RowAppear(ListViewBase list, Func<object, string?> key)
    {
        this.key = key;
        list.ItemContainerTransitions = new TransitionCollection();
        list.ContainerContentChanging += OnContainer;
    }

    public void Ingest(IEnumerable<string> ids, RowAppearMode mode)
    {
        var next = ids.ToHashSet();
        pending.Clear();
        if (mode == RowAppearMode.Cascade) pending.UnionWith(next);
        else if (mode == RowAppearMode.Refresh && known.Count > 0) pending.UnionWith(next.Where(id => !known.Contains(id)));
        known = next;
        cascade = mode == RowAppearMode.Cascade;
        until = DateTimeOffset.Now + Window;
    }

    private void OnContainer(ListViewBase sender, ContainerContentChangingEventArgs e)
    {
        if (e.InRecycleQueue || pending.Count == 0 || e.Item is null) return;
        if (key(e.Item) is not { } id || !pending.Remove(id) || DateTimeOffset.Now > until) return;
        Play(e.ItemContainer, cascade ? TimeSpan.FromMilliseconds(Math.Min(e.ItemIndex, 12) * 25) : TimeSpan.Zero);
    }

    private static void Play(UIElement row, TimeSpan delay)
    {
        var shift = new TranslateTransform { Y = 8 };
        row.RenderTransform = shift;
        row.Opacity = 0;
        var ease = new CubicEase { EasingMode = EasingMode.EaseOut };
        var fade = new DoubleAnimation { From = 0, To = 1, Duration = Length, BeginTime = delay, EasingFunction = ease };
        Storyboard.SetTarget(fade, row);
        Storyboard.SetTargetProperty(fade, "Opacity");
        var slide = new DoubleAnimation { From = 8, To = 0, Duration = Length, BeginTime = delay, EasingFunction = ease };
        Storyboard.SetTarget(slide, shift);
        Storyboard.SetTargetProperty(slide, "Y");
        var board = new Storyboard();
        board.Children.Add(fade);
        board.Children.Add(slide);
        board.Completed += (_, _) =>
        {
            row.Opacity = 1;
            shift.Y = 0;
            board.Stop();
        };
        board.Begin();
    }
}
