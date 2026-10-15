using System.Text.Json.Nodes;
using Shipyard.Core.Api;
using Shipyard.Core.Util;

namespace Shipyard.Core.Update;

public sealed record Release(string Version, string Name, string Notes, string PageUrl, DateTimeOffset? PublishedAt, string? InstallerUrl, long InstallerSize, string? PortableUrl, long PortableSize);

public sealed class UpdateException : Exception
{
    public UpdateException(string message) : base(message)
    {
    }
}

public static class Releases
{
    private static readonly HttpClient Http = new() { Timeout = TimeSpan.FromSeconds(20) };

    public static string Arch => System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture switch
    {
        System.Runtime.InteropServices.Architecture.Arm64 => "arm64",
        System.Runtime.InteropServices.Architecture.X86 => "x86",
        _ => "x64"
    };

    public static async Task<Release> FetchLatest(CancellationToken ct = default)
    {
        using var req = new HttpRequestMessage(HttpMethod.Get, AppInfo.LatestReleaseApi);
        req.Headers.Accept.ParseAdd("application/vnd.github+json");
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        using var response = await Http.SendAsync(req, ct).ConfigureAwait(false);
        var code = (int)response.StatusCode;
        if (code == 404) throw new UpdateException("No releases found.");
        if (code is < 200 or >= 300)
            throw new UpdateException(code is 403 or 429 ? "GitHub rate limit reached. Try again later." : $"GitHub responded with error {code}.");
        return Parse(await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false), Arch);
    }

    public static Release Parse(string json, string arch)
    {
        var o = JsonNode.Parse(json) as JsonObject ?? throw new UpdateException("Unexpected response from GitHub.");
        var assets = o.Arr("assets").Objects().ToList();
        JsonObject? Find(string name) => assets.FirstOrDefault(a => string.Equals(a.Str("name"), name, StringComparison.OrdinalIgnoreCase));
        var installer = Find(AppInfo.InstallerAssetName(arch));
        var portable = Find(AppInfo.PortableAssetName(arch));
        if (installer is null && portable is null) throw new UpdateException($"The release has no Windows {arch} download.");
        var version = (o.Str("tag_name") ?? "").Trim();
        if (version.StartsWith('v') || version.StartsWith('V')) version = version[1..];
        var name = (o.Str("name") ?? "").Trim();
        DateTimeOffset? published = DateTimeOffset.TryParse(o.Str("published_at"), out var p) ? p : null;
        return new Release(
            version,
            name.Length == 0 ? $"{AppInfo.DisplayName} {version}" : name,
            o.Str("body") ?? "",
            o.Str("html_url") ?? AppInfo.ReleasesPage,
            published,
            installer?.Str("browser_download_url"),
            (long)(installer?.Dbl("size") ?? 0),
            portable?.Str("browser_download_url"),
            (long)(portable?.Dbl("size") ?? 0));
    }

    public static int Compare(string a, string b)
    {
        var pa = Components(a);
        var pb = Components(b);
        for (var i = 0; i < Math.Max(pa.Count, pb.Count); i++)
        {
            var x = i < pa.Count ? pa[i] : 0;
            var y = i < pb.Count ? pb[i] : 0;
            if (x != y) return x < y ? -1 : 1;
        }
        return 0;
    }

    public static List<int> Components(string v)
    {
        var s = v.Trim();
        if (s.StartsWith('v') || s.StartsWith('V')) s = s[1..];
        var core = s.Split('-', '+')[0];
        return core.Split('.').Select(part =>
        {
            var digits = new string(part.TakeWhile(char.IsDigit).ToArray());
            return int.TryParse(digits, out var n) ? n : 0;
        }).ToList();
    }

    public static async Task<string> Download(string url, string destination, long expectedSize, IProgress<double>? progress, CancellationToken ct = default)
    {
        using var http = new HttpClient { Timeout = TimeSpan.FromMinutes(30) };
        using var req = new HttpRequestMessage(HttpMethod.Get, url);
        req.Headers.UserAgent.ParseAdd(AppInfo.UserAgent);
        req.Headers.Accept.ParseAdd("application/octet-stream");
        using var response = await http.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false);
        if (!response.IsSuccessStatusCode) throw new UpdateException($"Couldn't download the update: HTTP {(int)response.StatusCode}");
        var total = response.Content.Headers.ContentLength ?? expectedSize;
        Directory.CreateDirectory(Path.GetDirectoryName(destination)!);
        await using (var input = await response.Content.ReadAsStreamAsync(ct).ConfigureAwait(false))
        await using (var output = File.Create(destination))
        {
            var buffer = new byte[81920];
            long written = 0;
            double last = -1;
            int read;
            while ((read = await input.ReadAsync(buffer, ct).ConfigureAwait(false)) > 0)
            {
                await output.WriteAsync(buffer.AsMemory(0, read), ct).ConfigureAwait(false);
                written += read;
                if (total > 0)
                {
                    var f = Math.Min(1, (double)written / total);
                    if (f - last >= 0.005 || f >= 1)
                    {
                        last = f;
                        progress?.Report(f);
                    }
                }
            }
        }
        progress?.Report(1);
        return destination;
    }
}
