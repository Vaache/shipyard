using Microsoft.UI.Xaml;
using PullYard.Core.Html;
using PullYard.Core.Util;

namespace PullYard.Services;

public enum ThemeStyle { PullYard, System }

public enum AppearanceMode { Auto, Light, Dark }

public static class ThemeService
{
    private static FrameworkElement? root;
    private static Windows.UI.ViewManagement.AccessibilitySettings? accessibility;
    private static bool? highContrast;

    public static event Action? Changed;

    public static ThemeStyle Style
    {
        get => Settings.Shared.GetString(Keys.ThemeStyle) == "system" ? ThemeStyle.System : ThemeStyle.PullYard;
        set
        {
            Settings.Shared.Set(Keys.ThemeStyle, value == ThemeStyle.System ? "system" : "brand");
            Apply();
        }
    }

    public static AppearanceMode Appearance
    {
        get => Settings.Shared.GetString(Keys.ThemeAppearance) switch
        {
            "light" => AppearanceMode.Light,
            "dark" => AppearanceMode.Dark,
            _ => AppearanceMode.Auto
        };
        set
        {
            Settings.Shared.Set(Keys.ThemeAppearance, value switch
            {
                AppearanceMode.Light => "light",
                AppearanceMode.Dark => "dark",
                _ => "auto"
            });
            Apply();
        }
    }

    public static bool IsBrand => Style == ThemeStyle.PullYard && !IsHighContrast;

    public static bool IsHighContrast
    {
        get
        {
            if (highContrast is { } known) return known;
            try
            {
                accessibility ??= new Windows.UI.ViewManagement.AccessibilitySettings();
                highContrast = accessibility.HighContrast;
            }
            catch
            {
                highContrast = false;
            }
            return highContrast.Value;
        }
    }

    public static bool IsDark
    {
        get
        {
            if (root is not null) return root.ActualTheme == ElementTheme.Dark;
            return Application.Current?.RequestedTheme == ApplicationTheme.Dark;
        }
    }

    public static void Attach(FrameworkElement element)
    {
        root = element;
        element.ActualThemeChanged += (_, _) => Changed?.Invoke();
        try
        {
            accessibility ??= new Windows.UI.ViewManagement.AccessibilitySettings();
            var queue = element.DispatcherQueue;
            accessibility.HighContrastChanged += (sender, _) =>
            {
                var value = sender.HighContrast;
                queue.TryEnqueue(() =>
                {
                    if (highContrast == value) return;
                    highContrast = value;
                    Apply();
                });
            };
        }
        catch
        {
        }
        Apply();
    }

    public static void Apply()
    {
        ConversationHtml.Brand = IsBrand;
        var app = Application.Current;
        if (app is not null)
        {
            var dictionaries = app.Resources.MergedDictionaries;
            var source = new Uri(IsBrand ? "ms-appx:///Themes/Brand.xaml" : "ms-appx:///Themes/System.xaml");
            var index = -1;
            for (var i = 0; i < dictionaries.Count; i++)
            {
                var s = dictionaries[i].Source?.ToString() ?? "";
                if (s.EndsWith("Themes/Brand.xaml", StringComparison.OrdinalIgnoreCase) || s.EndsWith("Themes/System.xaml", StringComparison.OrdinalIgnoreCase)) index = i;
            }
            if (index < 0 || dictionaries[index].Source?.ToString() != source.ToString())
            {
                var next = new ResourceDictionary { Source = source };
                if (index >= 0) dictionaries[index] = next;
                else dictionaries.Add(next);
            }
        }
        if (root is not null)
        {
            var target = Appearance switch
            {
                AppearanceMode.Light => ElementTheme.Light,
                AppearanceMode.Dark => ElementTheme.Dark,
                _ => ElementTheme.Default
            };
            root.RequestedTheme = target == ElementTheme.Light ? ElementTheme.Dark : ElementTheme.Light;
            root.RequestedTheme = target;
        }
        Changed?.Invoke();
    }

    public static string WebVars()
    {
        string V(string name, string key) => $"--{name}:{Palette.CssHex(key)};";
        var dark = IsDark;
        var sb = new System.Text.StringBuilder(":root{");
        sb.Append(V("fg", "text")).Append(V("muted", "muted")).Append(V("dim", "dim")).Append(V("link", "sky")).Append(V("green", "green")).Append(V("red", "red"))
            .Append(V("amber", "amber")).Append(V("sky", "sky")).Append(V("accent", "accent")).Append(V("coral", "coral")).Append(V("purple", "violet")).Append(V("gutter", "gutter"));
        if (IsBrand)
        {
            sb.Append(V("raised", "raised")).Append(V("sunken", "sunken"));
            sb.Append(dark ? "--border:rgba(160,174,196,.12);--hover:rgba(255,255,255,.05);--button:#1b2740;" : "--border:rgba(26,35,51,.10);--hover:rgba(0,0,0,.04);--button:#f3f1ec;");
        }
        else
        {
            sb.Append(dark ? "--raised:rgba(255,255,255,.06);--sunken:rgba(0,0,0,.25);--border:rgba(255,255,255,.10);--hover:rgba(255,255,255,.05);--button:rgba(255,255,255,.06);"
                : "--raised:rgba(255,255,255,.75);--sunken:rgba(0,0,0,.04);--border:rgba(0,0,0,.10);--hover:rgba(0,0,0,.04);--button:rgba(255,255,255,.8);");
        }
        sb.Append(dark ? "--onaccent:#0c1220;--add:rgba(93,190,138,.12);--del:rgba(232,116,106,.12);--coralbg:rgba(238,111,95,.08);color-scheme:dark;"
            : "--onaccent:#ffffff;--add:rgba(46,138,87,.11);--del:rgba(194,65,47,.10);--coralbg:rgba(216,81,63,.06);color-scheme:light;");
        sb.Append('}');
        return sb.ToString();
    }
}
