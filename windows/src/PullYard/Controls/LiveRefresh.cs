using System.Runtime.CompilerServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;

namespace PullYard.Controls;

public sealed class LiveRefresh
{
    private static readonly ConditionalWeakTable<FrameworkElement, LiveRefresh> Registry = new();

    private readonly FrameworkElement owner;
    private readonly Action reload;
    private bool stale;

    public LiveRefresh(FrameworkElement owner, Action reload)
    {
        this.owner = owner;
        this.reload = reload;
        Registry.AddOrUpdate(owner, this);
        owner.EffectiveViewportChanged += (_, _) => Resume();
    }

    private void Resume()
    {
        if (!stale || !IsShown(owner)) return;
        stale = false;
        reload();
    }

    public static void Reveal(DependencyObject root)
    {
        if (root is FrameworkElement element && Registry.TryGetValue(element, out var live)) live.Resume();
        var count = VisualTreeHelper.GetChildrenCount(root);
        for (var i = 0; i < count; i++) Reveal(VisualTreeHelper.GetChild(root, i));
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
