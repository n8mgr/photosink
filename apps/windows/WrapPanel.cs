using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System;
using Windows.Foundation;

namespace Photosink;

public sealed partial class WrapPanel : Panel
{
    public double HorizontalSpacing { get; set; } = 8;

    public double VerticalSpacing { get; set; } = 4;

    private Size Layout(double width, bool arrange)
    {
        double x = 0, y = 0, line = 0, used = 0;
        foreach (var child in Children)
        {
            if (child.Visibility == Visibility.Collapsed)
            {
                continue;
            }
            if (!arrange)
            {
                child.Measure(new Size(width, double.PositiveInfinity));
            }
            var size = child.DesiredSize;
            if (x > 0 && x + size.Width > width)
            {
                y += line + VerticalSpacing;
                x = 0;
                line = 0;
            }
            if (arrange)
            {
                child.Arrange(new Rect(x, y, Math.Min(size.Width, width), size.Height));
            }
            used = Math.Max(used, x + size.Width);
            x += size.Width + HorizontalSpacing;
            line = Math.Max(line, size.Height);
        }
        return new Size(used, y + line);
    }

    protected override Size MeasureOverride(Size availableSize) => Layout(availableSize.Width, false);

    protected override Size ArrangeOverride(Size finalSize)
    {
        Layout(finalSize.Width, true);
        return finalSize;
    }
}
