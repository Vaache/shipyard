using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Shipyard.Core.Api;
using Shipyard.Core.Util;
using Shipyard.Services;


namespace Shipyard.Controls;

public static class Ui
{
    public static FontFamily Mono { get; } = new("Cascadia Mono, Consolas");
    public static FontFamily Icons { get; } = new("Segoe Fluent Icons, Segoe MDL2 Assets");

    public static TextBlock Text(string? text, double size = 13, string color = "text", Windows.UI.Text.FontWeight? weight = null, bool wrap = false, int maxLines = 0, bool mono = false, bool select = false)
    {
        var t = new TextBlock
        {
            Text = text ?? "",
            FontSize = size,
            Foreground = Palette.Brush(color),
            TextTrimming = TextTrimming.CharacterEllipsis,
            TextWrapping = wrap || maxLines > 1 ? TextWrapping.WrapWholeWords : TextWrapping.NoWrap,
            IsTextSelectionEnabled = select,
            VerticalAlignment = VerticalAlignment.Center
        };
        if (maxLines > 0) t.MaxLines = maxLines;
        if (weight is { } w) t.FontWeight = w;
        if (mono) t.FontFamily = Mono;
        return t;
    }

    public static TextBlock Caption(string? text, string color = "muted") => Text(text, 12, color);

    public static FontIcon Glyph(string glyph, double size = 13, string color = "muted") => new()
    {
        Glyph = glyph,
        FontFamily = Icons,
        FontSize = size,
        Foreground = Palette.Brush(color),
        VerticalAlignment = VerticalAlignment.Center
    };

    public static StackPanel H(double spacing, params UIElement?[] children)
    {
        var p = new StackPanel { Orientation = Orientation.Horizontal, Spacing = spacing };
        foreach (var c in children)
            if (c is not null) p.Children.Add(c);
        return p;
    }

    public static StackPanel V(double spacing, params UIElement?[] children)
    {
        var p = new StackPanel { Orientation = Orientation.Vertical, Spacing = spacing };
        foreach (var c in children)
            if (c is not null) p.Children.Add(c);
        return p;
    }

    public static Border IconTile(string glyph, string color, double size = 20) => new()
    {
        Width = size,
        Height = size,
        CornerRadius = new CornerRadius(size * 0.28),
        Background = Palette.Brush(color, 0.14),
        Child = new FontIcon { Glyph = glyph, FontFamily = Icons, FontSize = size * 0.55, Foreground = Palette.Brush(color), HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
        VerticalAlignment = VerticalAlignment.Center
    };

    public static Border Chip(string text, string color = "muted", string? glyph = null, bool mono = false)
    {
        var content = H(4);
        if (glyph is not null) content.Children.Add(Glyph(glyph, 10, color));
        var t = Text(text, 11.5, color, FontWeights.Medium);
        if (mono) t.FontFamily = Mono;
        t.MaxWidth = 260;
        content.Children.Add(t);
        return new Border
        {
            Background = Palette.Brush(color, 0.12),
            CornerRadius = new CornerRadius(9),
            Padding = new Thickness(7, 1, 7, 2),
            Child = content,
            VerticalAlignment = VerticalAlignment.Center
        };
    }

    public static Border LabelChip(LabelInfo label)
    {
        var t = Text(label.Name, 11, "text", FontWeights.Medium);
        return new Border
        {
            Background = Palette.HexBrush(label.Color, 0.22),
            BorderBrush = Palette.HexBrush(label.Color, 0.7),
            BorderThickness = new Thickness(0.5),
            CornerRadius = new CornerRadius(9),
            Padding = new Thickness(7, 0, 7, 1),
            Child = t,
            VerticalAlignment = VerticalAlignment.Center
        };
    }

    public static FrameworkElement StateBadge(VisualState state, bool compact = false)
    {
        var key = Palette.StateKey(state);
        if (compact)
        {
            return new Border
            {
                Width = 22,
                Height = 22,
                CornerRadius = new CornerRadius(11),
                Background = Palette.Brush(key, 0.15),
                Child = new FontIcon { Glyph = state.Glyph(), FontFamily = Icons, FontSize = 11, Foreground = Palette.Brush(key), HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center },
                VerticalAlignment = VerticalAlignment.Top
            };
        }
        var white = new SolidColorBrush(Microsoft.UI.Colors.White);
        return new Border
        {
            Background = Palette.Brush(key),
            CornerRadius = new CornerRadius(11),
            Padding = new Thickness(9, 2, 10, 3),
            VerticalAlignment = VerticalAlignment.Center,
            Child = H(5,
                new FontIcon { Glyph = state.Glyph(), FontFamily = Icons, FontSize = 11, Foreground = white },
                new TextBlock { Text = state.Title(), FontSize = 12.5, FontWeight = FontWeights.SemiBold, Foreground = white })
        };
    }

    public static FrameworkElement Avatar(string? url, double size = 20)
    {
        var p = new PersonPicture { Width = size, Height = size, VerticalAlignment = VerticalAlignment.Center };
        if (!string.IsNullOrEmpty(url))
        {
            try
            {
                var sep = url.Contains('?') ? "&" : "?";
                var px = (int)(size * 2);
                p.ProfilePicture = new BitmapImage(new Uri($"{url}{sep}s={px}")) { DecodePixelWidth = px, DecodePixelType = DecodePixelType.Physical };
            }
            catch
            {
            }
        }
        return p;
    }

    public static StackPanel DiffStat(int additions, int deletions, double size = 12) =>
        H(4, Text($"+{additions}", size, "green"), Text($"−{deletions}", size, "red"));

    public static FrameworkElement? CiIcon(string? state, double size = 12)
    {
        if (Palette.CiIcon(state) is not { } ci) return null;
        return Glyph(ci.Glyph, size, ci.Key);
    }

    public static Border Card(UIElement child, double radius = 10, string fill = "raised", double padding = 12) => new()
    {
        Background = Palette.Brush(fill),
        CornerRadius = new CornerRadius(radius),
        Padding = new Thickness(padding),
        Child = child
    };

    public static Button Button(string text, Action onClick, bool accent = false, string? glyph = null, string? tooltip = null)
    {
        var b = new Button();
        if (glyph is not null) b.Content = H(6, Glyph(glyph, 12, accent ? "text" : "text"), new TextBlock { Text = text });
        else b.Content = text;
        if (accent) b.Style = (Style)Application.Current.Resources["AccentButtonStyle"];
        b.Click += (_, _) => onClick();
        if (tooltip is not null) ToolTipService.SetToolTip(b, tooltip);
        return b;
    }

    public static Button IconButton(string glyph, string tooltip, Action onClick, string color = "muted")
    {
        var b = new Button
        {
            Content = Glyph(glyph, 14, color),
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0),
            Padding = new Thickness(8, 5, 8, 5)
        };
        ToolTipService.SetToolTip(b, tooltip);
        b.Click += (_, _) => onClick();
        return b;
    }

    public static HyperlinkButton Link(string text, Action onClick, double size = 12)
    {
        var b = new HyperlinkButton { Content = text, Padding = new Thickness(0), FontSize = size };
        b.Click += (_, _) => onClick();
        return b;
    }

    public static Microsoft.UI.Xaml.Shapes.Rectangle Line(bool vertical = false)
    {
        var r = new Microsoft.UI.Xaml.Shapes.Rectangle { Fill = Palette.Brush("text", ThemeService.IsDark ? 0.10 : 0.08) };
        if (vertical) r.Width = 1;
        else r.Height = 1;
        return r;
    }

    public static Grid Columns(params (UIElement? Element, GridLength Width)[] cols)
    {
        var g = new Grid { ColumnSpacing = 8 };
        for (var i = 0; i < cols.Length; i++)
        {
            g.ColumnDefinitions.Add(new ColumnDefinition { Width = cols[i].Width });
            if (cols[i].Element is { } e)
            {
                Grid.SetColumn((FrameworkElement)e, i);
                g.Children.Add(e);
            }
        }
        return g;
    }

    public static GridLength Auto => GridLength.Auto;
    public static GridLength Star => new(1, GridUnitType.Star);

    public static ProgressRing Spinner(double size = 16) => new() { IsActive = true, Width = size, Height = size };

    public static FrameworkElement Error(string message, Action? retry = null)
    {
        var panel = V(10, IconTile("", "amber", 44), Text(message, 13, "muted", wrap: true, select: true));
        ((TextBlock)panel.Children[1]).TextAlignment = TextAlignment.Center;
        ((FrameworkElement)panel.Children[0]).HorizontalAlignment = HorizontalAlignment.Center;
        if (retry is not null)
        {
            var b = Button(L.T("Common_Retry", "Retry"), retry);
            b.HorizontalAlignment = HorizontalAlignment.Center;
            panel.Children.Add(b);
        }
        panel.HorizontalAlignment = HorizontalAlignment.Center;
        panel.VerticalAlignment = VerticalAlignment.Center;
        panel.MaxWidth = 520;
        panel.Margin = new Thickness(30);
        return panel;
    }

    public static FrameworkElement Loading()
    {
        var ring = Spinner(22);
        ring.HorizontalAlignment = HorizontalAlignment.Center;
        ring.VerticalAlignment = VerticalAlignment.Center;
        return ring;
    }

    public static FrameworkElement Empty(string glyph, string color, string message)
    {
        var tile = IconTile(glyph, color, 44);
        tile.HorizontalAlignment = HorizontalAlignment.Center;
        var t = Text(message, 13, "muted", wrap: true);
        t.TextAlignment = TextAlignment.Center;
        var p = V(10, tile, t);
        p.HorizontalAlignment = HorizontalAlignment.Center;
        p.VerticalAlignment = VerticalAlignment.Center;
        p.Margin = new Thickness(24);
        return p;
    }

    public static MenuFlyoutItem MenuItem(string text, Action onClick, string? glyph = null, bool enabled = true)
    {
        var item = new MenuFlyoutItem { Text = text, IsEnabled = enabled };
        if (glyph is not null) item.Icon = new FontIcon { Glyph = glyph, FontFamily = Icons };
        item.Click += (_, _) => onClick();
        return item;
    }

    public static ToggleMenuFlyoutItem CheckItem(string text, bool on, Action onClick)
    {
        var item = new ToggleMenuFlyoutItem { Text = text, IsChecked = on };
        item.Click += (_, _) => onClick();
        return item;
    }

    public static MenuFlyout Menu(params MenuFlyoutItemBase?[] items)
    {
        var m = new MenuFlyout();
        foreach (var i in items)
            if (i is not null) m.Items.Add(i);
        return m;
    }

    public static MenuFlyoutSeparator Separator() => new();

    public static void Tip(DependencyObject element, string? text)
    {
        if (!string.IsNullOrEmpty(text)) ToolTipService.SetToolTip(element, text);
    }

    public static string Plural(int n, string one, string many) => Fmt.Plural(n, one, many);
}
