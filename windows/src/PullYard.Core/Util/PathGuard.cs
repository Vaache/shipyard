namespace PullYard.Core.Util;

public static class PathGuard
{
    private static readonly HashSet<string> ExecutableExtensions = new(StringComparer.OrdinalIgnoreCase)
    {
        ".exe", ".bat", ".cmd", ".com", ".ps1", ".vbs", ".vbe", ".js", ".jse", ".wsf", ".wsh", ".msi", ".msp", ".lnk", ".scr", ".hta", ".cpl", ".reg", ".url", ".pif", ".jar"
    };

    public static bool IsExecutableExtension(string path) => ExecutableExtensions.Contains(Path.GetExtension(path));

    public static bool IsInside(string root, string path)
    {
        if (string.IsNullOrWhiteSpace(root) || string.IsNullOrWhiteSpace(path)) return false;
        try
        {
            var full = Path.GetFullPath(path);
            var baseDir = Path.TrimEndingDirectorySeparator(Path.GetFullPath(root)) + Path.DirectorySeparatorChar;
            var comparison = OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
            return full.StartsWith(baseDir, comparison);
        }
        catch (Exception)
        {
            return false;
        }
    }

    public static void OpenWithinWorkspace(string workspace, string path)
    {
        if (string.IsNullOrWhiteSpace(path)) return;
        string full;
        try
        {
            full = Path.GetFullPath(Path.IsPathRooted(path) ? path : Path.Combine(workspace, path));
        }
        catch (Exception)
        {
            return;
        }
        if (!IsInside(workspace, full)) return;
        if (IsExecutableExtension(full)) Proc.Reveal(full);
        else Proc.OpenUrl(full);
    }
}
