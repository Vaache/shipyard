namespace PullYard.Core.Util;

public static class ErrorLog
{
    private static readonly object Gate = new();

    public static void Write(string message)
    {
        try
        {
            lock (Gate)
            {
                Directory.CreateDirectory(AppInfo.DataDirectory);
                File.AppendAllText(Path.Combine(AppInfo.DataDirectory, "errors.log"), $"{DateTimeOffset.Now:o} {message}\n");
            }
        }
        catch
        {
        }
    }

    public static void Write(Exception e, string? context = null) => Write(context is null ? e.ToString() : $"{context}: {e}");
}
