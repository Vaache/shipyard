using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Windows.Foundation;

namespace PullYard.Controls;

public sealed class WrapPanel : Panel
{
    public double HorizontalSpacing { get; set; } = 6;
    public double VerticalSpacing { get; set; } = 4;

    public WrapPanel(double spacing = 6, params UIElement?[] children)
    {
        HorizontalSpacing = spacing;
        foreach (var c in children)
            if (c is not null) Children.Add(c);
    }

    protected override Size MeasureOverride(Size availableSize)
    {
        double x = 0, y = 0, lineHeight = 0, width = 0;
        foreach (var child in Children)
        {
            child.Measure(new Size(availableSize.Width, double.PositiveInfinity));
            var s = child.DesiredSize;
            if (s.Width == 0 && s.Height == 0) continue;
            if (x > 0 && x + s.Width > availableSize.Width)
            {
                y += lineHeight + VerticalSpacing;
                x = 0;
                lineHeight = 0;
            }
            x += s.Width + HorizontalSpacing;
            width = Math.Max(width, x - HorizontalSpacing);
            lineHeight = Math.Max(lineHeight, s.Height);
        }
        return new Size(double.IsInfinity(availableSize.Width) ? width : Math.Min(width, availableSize.Width), y + lineHeight);
    }

    protected override Size ArrangeOverride(Size finalSize)
    {
        double x = 0, y = 0, lineHeight = 0;
        var line = new List<UIElement>();
        var positions = new Dictionary<UIElement, double>();
        foreach (var child in Children)
        {
            var s = child.DesiredSize;
            if (s.Width == 0 && s.Height == 0)
            {
                child.Arrange(new Rect(0, 0, 0, 0));
                continue;
            }
            if (x > 0 && x + s.Width > finalSize.Width)
            {
                Place(line, positions, y, lineHeight);
                line.Clear();
                y += lineHeight + VerticalSpacing;
                x = 0;
                lineHeight = 0;
            }
            positions[child] = x;
            line.Add(child);
            x += s.Width + HorizontalSpacing;
            lineHeight = Math.Max(lineHeight, s.Height);
        }
        Place(line, positions, y, lineHeight);
        return new Size(finalSize.Width, y + lineHeight);
    }

    private static void Place(List<UIElement> line, Dictionary<UIElement, double> positions, double y, double lineHeight)
    {
        foreach (var c in line)
        {
            var s = c.DesiredSize;
            c.Arrange(new Rect(positions[c], y + (lineHeight - s.Height) / 2, Math.Min(s.Width, double.MaxValue), s.Height));
        }
    }
}
