using Microsoft.UI.Input;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Windows.System;
using Windows.UI.Core;

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

    public static bool Dispatch(DependencyObject from, string id)
    {
        var now = DateTimeOffset.Now;
        if (id == lastId && now - last < TimeSpan.FromMilliseconds(250)) return true;
        for (var node = from; node is not null; node = VisualTreeHelper.GetParent(node))
        {
            if (node is IShortcutHost host && host.HandleShortcut(id))
            {
                last = now;
                lastId = id;
                return true;
            }
        }
        return false;
    }

    public static void Register(UIElement element, string id, VirtualKey key, VirtualKeyModifiers modifiers, bool skipInText = false)
    {
        var acc = new KeyboardAccelerator { Key = key, Modifiers = modifiers };
        acc.Invoked += (_, e) =>
        {
            if (modifiers.HasFlag(VirtualKeyModifiers.Menu) && IsDown(VirtualKey.RightMenu)) return;
            if (skipInText && element.XamlRoot is { } root && FocusManager.GetFocusedElement(root) is TextBox or AutoSuggestBox or RichEditBox or PasswordBox) return;
            e.Handled = Dispatch(element, id);
        };
        element.KeyboardAccelerators.Add(acc);
    }

    private static bool IsDown(VirtualKey key) => InputKeyboardSource.GetKeyStateForCurrentThread(key).HasFlag(CoreVirtualKeyStates.Down);

    public static void ShortLinkKey(UIElement element) => Register(element, ShortLink, VirtualKey.C, VirtualKeyModifiers.Control | VirtualKeyModifiers.Shift);

    public static void BlameKey(UIElement element) => Register(element, Blame, VirtualKey.B, VirtualKeyModifiers.Control | VirtualKeyModifiers.Menu, skipInText: true);
}
