using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Shipyard.Services;

namespace Shipyard.Controls;

public sealed record TabSpec(string Id, string Title, string Glyph, string? Badge = null, string BadgeColor = "muted");

public sealed class TabStrip : StackPanel
{
    private string selected;
    private List<TabSpec> specs = new();

    public event Action<string>? SelectionChanged;

    public TabStrip(string selected)
    {
        this.selected = selected;
        Orientation = Orientation.Horizontal;
        Spacing = 2;
        Padding = new Thickness(8, 0, 8, 0);
    }

    public string Selected
    {
        get => selected;
        set
        {
            if (selected == value) return;
            selected = value;
            Render();
        }
    }

    public void SetTabs(IEnumerable<TabSpec> tabs)
    {
        specs = tabs.ToList();
        Render();
    }

    private void Render()
    {
        Children.Clear();
        foreach (var spec in specs)
        {
            var isSelected = spec.Id == selected;
            var content = Ui.H(6, Ui.Glyph(spec.Glyph, 12, isSelected ? "text" : "muted"), Ui.Text(spec.Title, 13, isSelected ? "text" : "muted", isSelected ? FontWeights.SemiBold : FontWeights.Normal));
            if (spec.Badge is { } badge)
            {
                content.Children.Add(new Border
                {
                    Background = isSelected ? Palette.Brush("accent", 0.16) : Palette.Brush("text", 0.08),
                    CornerRadius = new CornerRadius(8),
                    Padding = new Thickness(6, 0, 6, 1),
                    Child = Ui.Text(badge, 11.5, spec.BadgeColor, FontWeights.SemiBold),
                    VerticalAlignment = VerticalAlignment.Center
                });
            }
            var underline = new Border { Height = 2, CornerRadius = new CornerRadius(1), Background = isSelected ? Palette.Brush("accent") : new SolidColorBrush(Microsoft.UI.Colors.Transparent), VerticalAlignment = VerticalAlignment.Bottom };
            var grid = new Grid { Padding = new Thickness(12, 8, 12, 8) };
            grid.Children.Add(content);
            var host = new Grid();
            host.Children.Add(grid);
            host.Children.Add(underline);
            var button = new Button
            {
                Content = host,
                Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                BorderThickness = new Thickness(0),
                Padding = new Thickness(0),
                CornerRadius = new CornerRadius(6)
            };
            var id = spec.Id;
            button.Click += (_, _) =>
            {
                Selected = id;
                SelectionChanged?.Invoke(id);
            };
            Children.Add(button);
        }
    }
}
