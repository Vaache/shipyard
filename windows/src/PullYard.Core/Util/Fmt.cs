using System.Globalization;

namespace PullYard.Core.Util;

public static class Fmt
{
    public static readonly CultureInfo Culture = CultureInfo.GetCultureInfo("en-US");

    public static string Rel(DateTimeOffset? date) => date is { } d ? Rel(d, DateTimeOffset.Now) : "";

    public static string Rel(DateTimeOffset date, DateTimeOffset now)
    {
        var diff = now - date;
        var future = diff < TimeSpan.Zero;
        var secs = Math.Abs(diff.TotalSeconds);
        if (secs < 45) return "just now";
        string body;
        if (secs < 3600) body = $"{Math.Max(1, (int)Math.Round(secs / 60))} min.";
        else if (secs < 86400) body = $"{(int)(secs / 3600)} hr.";
        else if (secs < 7 * 86400) body = Plural((int)(secs / 86400), "day", "days");
        else if (secs < 31 * 86400) body = $"{(int)(secs / (7 * 86400))} wk.";
        else if (secs < 365 * 86400) body = $"{(int)(secs / (30 * 86400))} mo.";
        else body = $"{(int)(secs / (365 * 86400))} yr.";
        return future ? $"in {body}" : $"{body} ago";
    }

    public static string Full(DateTimeOffset? date) => date is { } d ? d.ToLocalTime().ToString("MMM d, yyyy 'at' h:mm tt", Culture) : "";

    public static string Plural(int n, string one, string many) => $"{n} {(n == 1 ? one : many)}";

    public static string Ago(DateTimeOffset updated, DateTimeOffset now)
    {
        var s = Math.Max(0, (int)(now - updated).TotalSeconds);
        if (s < 10) return "just now";
        if (s < 60) return $"{s / 5 * 5}s ago";
        if (s < 3600) return $"{s / 60}m ago";
        return $"{s / 3600}h ago";
    }

    public static string Bytes(long bytes)
    {
        if (bytes < 1000) return $"{bytes} bytes";
        double v = bytes;
        string[] units = { "KB", "MB", "GB" };
        var i = -1;
        while (v >= 1000 && i < units.Length - 1)
        {
            v /= 1000;
            i++;
        }
        return v >= 100 ? $"{v:0} {units[i]}" : $"{v:0.#} {units[i]}";
    }

    public static string Memory(long bytes)
    {
        var mb = bytes / 1048576.0;
        return mb >= 1024 ? $"{mb / 1024:0.0} GB" : $"{Math.Round(mb):0} MB";
    }

    public static string Tokens(int n)
    {
        if (n >= 1_000_000) return n % 1_000_000 == 0 ? $"{n / 1_000_000}M" : $"{n / 1_000_000.0:0.0}M";
        if (n >= 1000) return $"{n / 1000.0:0.0}k";
        return n.ToString(Culture);
    }

    public static string HomeRelative(string path)
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        return !string.IsNullOrEmpty(home) && path.StartsWith(home, StringComparison.OrdinalIgnoreCase) ? "~" + path[home.Length..] : path;
    }
}
