using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;
using PullYard.Core;

namespace PullYard.Services;

public static class WebViewEnvironment
{
    public const string Host = "app.local";
    public const string Origin = "https://app.local/";
    private static Task<CoreWebView2Environment>? environment;

    public static string UserDataFolder => Path.Combine(AppInfo.DataDirectory, "WebView2");

    public static string WebRoot => Path.Combine(AppContext.BaseDirectory, "Web");

    public static void Prepare()
    {
        Environment.SetEnvironmentVariable("WEBVIEW2_USER_DATA_FOLDER", UserDataFolder);
    }

    public static Task<CoreWebView2Environment> Shared()
    {
        environment ??= CoreWebView2Environment.CreateWithOptionsAsync(null, UserDataFolder, new CoreWebView2EnvironmentOptions()).AsTask();
        return environment;
    }

    public static async Task Initialize(WebView2 view)
    {
        var env = await Shared();
        await view.EnsureCoreWebView2Async(env);
        var core = view.CoreWebView2;
        core.SetVirtualHostNameToFolderMapping(Host, WebRoot, CoreWebView2HostResourceAccessKind.Allow);
        core.Settings.AreDevToolsEnabled = Environment.GetEnvironmentVariable($"{AppInfo.EnvPrefix}_DEVTOOLS") == "1";
        core.Settings.AreDefaultContextMenusEnabled = true;
        core.Settings.IsStatusBarEnabled = false;
        core.Settings.IsZoomControlEnabled = true;
        core.Settings.AreBrowserAcceleratorKeysEnabled = false;
        core.Settings.IsGeneralAutofillEnabled = false;
        core.Settings.IsPasswordAutosaveEnabled = false;
        ApplyScheme(view);
        core.NewWindowRequested += (_, e) =>
        {
            e.Handled = true;
            LinkBrowser.Open(e.Uri);
        };
        core.NavigationStarting += (_, e) =>
        {
            if (!e.Uri.StartsWith(Origin, StringComparison.OrdinalIgnoreCase) && !e.Uri.StartsWith("about:", StringComparison.OrdinalIgnoreCase))
            {
                e.Cancel = true;
                LinkBrowser.Open(e.Uri);
            }
        };
    }

    public static void ApplyScheme(WebView2 view)
    {
        try
        {
            if (view.CoreWebView2 is { } core)
                core.Profile.PreferredColorScheme = ThemeService.IsDark ? CoreWebView2PreferredColorScheme.Dark : CoreWebView2PreferredColorScheme.Light;
        }
        catch
        {
        }
    }

    public static string? AvailableVersion()
    {
        try
        {
            return CoreWebView2Environment.GetAvailableBrowserVersionString();
        }
        catch
        {
            return null;
        }
    }
}
