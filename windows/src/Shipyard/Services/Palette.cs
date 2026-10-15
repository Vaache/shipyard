using Microsoft.UI;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Shipyard.Core.Api;
using Windows.UI;

namespace Shipyard.Services;

public static class Palette
{
    private static readonly Dictionary<string, (uint Light, uint Dark)> Brand = new()
    {
        ["window"] = (0xF7F5F0, 0x0C1220),
        ["sidebar"] = (0xEFECE5, 0x0A0F1B),
        ["list"] = (0xF3F1EC, 0x0E1526),
        ["panel"] = (0xFDFCFA, 0x101828),
        ["raised"] = (0xFFFFFF, 0x152036),
        ["sunken"] = (0xF1EEE8, 0x0A101C),
        ["bar"] = (0xF5F3EE, 0x111A2C),
        ["text"] = (0x1A2333, 0xE6EBF2),
        ["muted"] = (0x5F6B7E, 0x8693A8),
        ["dim"] = (0x939AA8, 0x5F6B80),
        ["accent"] = (0x2A8BC8, 0x5BB8EC),
        ["sky"] = (0x1F7FB8, 0x5BB8EC),
        ["violet"] = (0x6B55D6, 0x9D8CF0),
        ["coral"] = (0xD8513F, 0xEE6F5F),
        ["green"] = (0x2E8A57, 0x5DBE8A),
        ["red"] = (0xC2412F, 0xE8746A),
        ["amber"] = (0xB7791F, 0xE8B44C),
        ["teal"] = (0x23877A, 0x56C2B0),
        ["indigo"] = (0x4E5BC4, 0x8C97E8),
        ["slate"] = (0x6B7486, 0x939CB0),
        ["orange"] = (0xC7651A, 0xF0954A),
        ["gutter"] = (0x9AA2B0, 0x4A5670)
    };

    public static bool IsDark => ThemeService.IsDark;

    public static Color Hex(uint rgb, byte alpha = 255) => Color.FromArgb(alpha, (byte)((rgb >> 16) & 0xFF), (byte)((rgb >> 8) & 0xFF), (byte)(rgb & 0xFF));

    public static Color Hex(string? hex, byte alpha = 255)
    {
        var s = (hex ?? "").Trim('#');
        if (s.Length == 3) s = string.Concat(s.Select(c => $"{c}{c}"));
        return uint.TryParse(s, System.Globalization.NumberStyles.HexNumber, null, out var v) ? Hex(v, alpha) : Hex(0x888888u, alpha);
    }

    public static Color Get(string key, byte alpha = 255)
    {
        if (!Brand.TryGetValue(key, out var pair)) pair = Brand["muted"];
        return Hex(IsDark ? pair.Dark : pair.Light, alpha);
    }

    public static SolidColorBrush Brush(string key, double opacity = 1) => new(Get(key, (byte)Math.Round(255 * opacity)));

    public static SolidColorBrush HexBrush(string? hex, double opacity = 1) => new(Hex(hex, (byte)Math.Round(255 * opacity)));

    public static string CssHex(string key) => $"#{(IsDark ? Brand[key].Dark : Brand[key].Light):X6}";

    public static string StateKey(VisualState s) => s.ColorKey();

    private static readonly string[] ProjectColors = { "sky", "violet", "coral", "orange", "teal", "green", "indigo", "amber" };

    public static string ProjectKey(string path)
    {
        ulong h = 1469598103934665603;
        foreach (var b in System.Text.Encoding.UTF8.GetBytes(path))
            h = unchecked((h ^ b) * 1099511628211);
        return ProjectColors[(int)(h % (ulong)ProjectColors.Length)];
    }

    public static string BranchKey(string branch)
    {
        var b = branch.ToLowerInvariant();
        if (b is "main" or "master" or "develop" or "dev" or "staging" or "release") return "slate";
        if (b.StartsWith("feat")) return "green";
        if (b.StartsWith("fix") || b.StartsWith("hotfix") || b.StartsWith("bug")) return "amber";
        if (b.StartsWith("chore") || b.StartsWith("refactor") || b.StartsWith("docs")) return "teal";
        if (System.Text.RegularExpressions.Regex.IsMatch(b, "^[a-z]+-[0-9]+")) return "violet";
        return "muted";
    }

    public static string ProjectOptionKey(string name) => name switch
    {
        "BLUE" => "sky",
        "GREEN" => "green",
        "YELLOW" => "amber",
        "ORANGE" => "coral",
        "RED" => "red",
        "PINK" => "coral",
        "PURPLE" => "violet",
        _ => "slate"
    };

    public static string CheckKey(CheckOutcome o) => o switch
    {
        CheckOutcome.Success => "green",
        CheckOutcome.Failure => "red",
        CheckOutcome.Pending => "amber",
        _ => "muted"
    };

    public static string CheckGlyph(CheckOutcome o) => o switch
    {
        CheckOutcome.Success => "",
        CheckOutcome.Failure => "",
        CheckOutcome.Pending => "",
        CheckOutcome.Skipped => "",
        _ => ""
    };

    public static (string Glyph, string Key)? CiIcon(string? state) => state switch
    {
        "SUCCESS" => ("", "green"),
        "FAILURE" or "ERROR" => ("", "red"),
        "PENDING" or "EXPECTED" => ("", "amber"),
        _ => null
    };

    public static Color Transparent => Colors.Transparent;
}
