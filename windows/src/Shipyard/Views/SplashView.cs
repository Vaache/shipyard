using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using Shipyard.Controls;
using Shipyard.Core;

namespace Shipyard.Views;

public sealed class SplashView : Grid
{
    public SplashView()
    {
        var icon = new Image { Source = new BitmapImage(new Uri("ms-appx:///Assets/AppIcon-256.png")), Width = 136, Height = 136 };
        var title = Ui.Text(AppInfo.DisplayName, 32, "text", FontWeights.Bold);
        title.HorizontalAlignment = HorizontalAlignment.Center;
        var tagline = Ui.Text(AppInfo.Tagline, 14, "muted");
        tagline.HorizontalAlignment = HorizontalAlignment.Center;
        var ring = Ui.Spinner(18);
        ring.HorizontalAlignment = HorizontalAlignment.Center;
        ring.Opacity = 0.7;
        var stack = Ui.V(20, icon, Ui.V(6, title, tagline), ring);
        stack.HorizontalAlignment = HorizontalAlignment.Center;
        stack.VerticalAlignment = VerticalAlignment.Center;
        Children.Add(stack);
    }
}
