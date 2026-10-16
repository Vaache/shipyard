using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace PullYard.Controls;

public sealed class LiveRefresh
{
    private readonly FrameworkElement owner;
    private readonly Action reload;
    private bool stale;

    public LiveRefresh(FrameworkElement owner, Action reload)
    {
        this.owner = owner;
        this.reload = reload;
        owner.EffectiveViewportChanged += (_, _) =>
        {
            if (!stale || !IsShown(owner)) return;
            stale = false;
            reload();
        };
    }

    public void Tick()
    {
        if (!IsShown(owner))
        {
            stale = true;
            return;
        }
        stale = false;
        reload();
    }

    public static bool IsShown(FrameworkElement element)
    {
        if (!element.IsLoaded) return false;
        DependencyObject? node = element;
        while (node is not null)
        {
            if (node is UIElement { Visibility: Visibility.Collapsed }) return false;
            node = VisualTreeHelper.GetParent(node);
        }
        return true;
    }
}
