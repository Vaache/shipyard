using Microsoft.Windows.ApplicationModel.Resources;
using PullYard.Core.Util;

namespace PullYard.Services;

public static class L
{
    private static ResourceLoader? loader;
    private static bool unavailable;

    public static bool Available
    {
        get
        {
            Load();
            return loader is not null;
        }
    }

    private static void Load()
    {
        if (loader is not null || unavailable) return;
        try
        {
            loader = new ResourceLoader();
        }
        catch
        {
            unavailable = true;
        }
    }

    public static string Lookup(string key)
    {
        Load();
        if (loader is null) return "";
        try
        {
            return loader.GetString(key) ?? "";
        }
        catch
        {
            return "";
        }
    }

    public static string T(string key, string fallback)
    {
        var s = Lookup(key);
        return string.IsNullOrEmpty(s) ? fallback : s;
    }

    public static string F(string key, string fallback, params object?[] args) => string.Format(Fmt.Culture, T(key, fallback), args);
}
