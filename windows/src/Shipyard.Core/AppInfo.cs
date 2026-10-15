using System.Reflection;

namespace Shipyard.Core;

public static class AppInfo
{
    private static readonly Dictionary<string, string> Meta = typeof(AppInfo).Assembly
        .GetCustomAttributes<AssemblyMetadataAttribute>()
        .Where(a => a.Value is not null)
        .GroupBy(a => a.Key)
        .ToDictionary(g => g.Key, g => g.First().Value!);

    private static string Get(string key, string fallback) => Meta.TryGetValue(key, out var v) && !string.IsNullOrWhiteSpace(v) ? v : fallback;

    public static string DisplayName => Get("DisplayName", "Shipyard");
    public static string Tagline => Get("Tagline", "Pull requests, without the browser");
    public static string AppId => Get("AppId", "Shipyard.Shipyard");
    public static string ExeName => Get("ExeName", "Shipyard");
    public static string UrlScheme => Get("UrlScheme", "shipyard");
    public static string EnvPrefix => Get("EnvPrefix", "SHIPYARD");
    public static string Publisher => Get("Publisher", "Vaache");
    public static string GitHubOwner => Get("GitHubOwner", "Vaache");
    public static string GitHubRepo => Get("GitHubRepo", "shipyard");

    public static string Version
    {
        get
        {
            var info = typeof(AppInfo).Assembly.GetCustomAttribute<AssemblyInformationalVersionAttribute>()?.InformationalVersion;
            if (string.IsNullOrEmpty(info)) return typeof(AppInfo).Assembly.GetName().Version?.ToString(3) ?? "0.0.0";
            var plus = info.IndexOf('+');
            return plus >= 0 ? info[..plus] : info;
        }
    }

    public static string UserAgent => $"{ExeName}-Windows/{Version}";
    public static string RepoSlug => $"{GitHubOwner}/{GitHubRepo}";
    public static string RepoUrl => $"https://github.com/{RepoSlug}";
    public static string ReleasesPage => $"{RepoUrl}/releases/latest";
    public static string LatestReleaseApi => $"https://api.github.com/repos/{RepoSlug}/releases/latest";
    public static string InstallerAssetName(string arch) => $"{ExeName}-windows-{arch}-setup.exe";
    public static string PortableAssetName(string arch) => $"{ExeName}-windows-{arch}-portable.zip";

    public static string Env(string suffix) => Environment.GetEnvironmentVariable($"{EnvPrefix}_{suffix}") ?? "";

    public static string DataDirectory
    {
        get
        {
            var dir = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), DisplayName);
            Directory.CreateDirectory(dir);
            return dir;
        }
    }
}
