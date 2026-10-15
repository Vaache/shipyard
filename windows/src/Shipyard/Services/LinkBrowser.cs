using System.Diagnostics;
using Microsoft.Win32;
using Shipyard.Core.Util;

namespace Shipyard.Services;

public sealed record BrowserApp(string Id, string Name, string Command);

public static class LinkBrowser
{
    public const string SystemDefault = "system";

    public static List<BrowserApp> Installed
    {
        get
        {
            var result = new Dictionary<string, BrowserApp>(StringComparer.OrdinalIgnoreCase);
            foreach (var hive in new[] { Registry.CurrentUser, Registry.LocalMachine })
            {
                try
                {
                    using var key = hive.OpenSubKey(@"SOFTWARE\Clients\StartMenuInternet");
                    if (key is null) continue;
                    foreach (var id in key.GetSubKeyNames())
                    {
                        using var sub = key.OpenSubKey(id);
                        var name = sub?.GetValue(null) as string ?? id;
                        using var cmdKey = sub?.OpenSubKey(@"shell\open\command");
                        var cmd = cmdKey?.GetValue(null) as string;
                        if (cmd is null || result.ContainsKey(id)) continue;
                        result[id] = new BrowserApp(id, name, cmd.Trim());
                    }
                }
                catch
                {
                }
            }
            return result.Values.OrderBy(b => b.Name, StringComparer.OrdinalIgnoreCase).ToList();
        }
    }

    public static string Selected
    {
        get => Settings.Shared.GetString(Keys.LinkBrowser) ?? SystemDefault;
        set => Settings.Shared.Set(Keys.LinkBrowser, value);
    }

    public static void Open(string? url)
    {
        if (string.IsNullOrWhiteSpace(url)) return;
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        if (uri.Scheme is not ("http" or "https" or "mailto"))
        {
            return;
        }
        var selected = Selected;
        if (selected != SystemDefault && uri.Scheme is "http" or "https" && Installed.FirstOrDefault(b => b.Id == selected) is { } browser)
        {
            var exe = ExtractExe(browser.Command);
            if (exe is not null && File.Exists(exe))
            {
                try
                {
                    var psi = new ProcessStartInfo(exe) { UseShellExecute = false };
                    psi.ArgumentList.Add(uri.AbsoluteUri);
                    Process.Start(psi);
                    return;
                }
                catch
                {
                }
            }
        }
        Proc.OpenUrl(uri.AbsoluteUri);
    }

    private static string? ExtractExe(string command)
    {
        var c = command.Trim();
        if (c.StartsWith('"'))
        {
            var end = c.IndexOf('"', 1);
            return end > 1 ? c[1..end] : null;
        }
        var exe = c.IndexOf(".exe", StringComparison.OrdinalIgnoreCase);
        return exe > 0 ? c[..(exe + 4)] : c;
    }
}
