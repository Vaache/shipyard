using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.System;

namespace PullYard.Controls;

public interface IShortcutHost
{
    bool HandleShortcut(string id);
}

public static class Shortcuts
{
    public const string ShortLink = "shortLink";
    public const string Blame = "blame";

    private static DateTimeOffset last;
    private static string lastId = "";

    public static void Dispatch(DependencyObject from, string id)
    {
        var now = DateTimeOffset.Now;
        if (id == lastId && now - last < TimeSpan.FromMilliseconds(250)) return;
        last = now;
        lastId = id;
        for (var node = from; node is not null; node = VisualTreeHelper.GetParent(node))
            if (node is IShortcutHost host && host.HandleShortcut(id)) return;
    }

    public static void Register(UIElement element, string id, VirtualKey key, VirtualKeyModifiers modifiers)
    {
        var acc = new KeyboardAccelerator { Key = key, Modifiers = modifiers };
        acc.Invoked += (_, e) =>
        {
            e.Handled = true;
            Dispatch(element, id);
        };
        element.KeyboardAccelerators.Add(acc);
    }

    public static void ShortLinkKey(UIElement element) => Register(element, ShortLink, VirtualKey.C, VirtualKeyModifiers.Control | VirtualKeyModifiers.Shift);

    public static void BlameKey(UIElement element) => Register(element, Blame, VirtualKey.B, VirtualKeyModifiers.Control | VirtualKeyModifiers.Menu);
}
