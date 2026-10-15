using Shipyard.Core.Api;

namespace Shipyard.Core.Util;

public static class Demo
{
    public static string? Repo { get; } = NonEmpty(AppInfo.Env("DEMO_REPO"));
    public static string? Folder { get; } = NonEmpty(AppInfo.Env("DEMO_FOLDER"));
    public static bool IsOn => Repo is not null || Folder is not null;

    public static RestUser User { get; } = new() { Login = "arivera", Name = "Alex Rivera" };
    public const string Email = "alex@example.com";

    public static string Query(string @base) => Repo is null ? @base : $"repo:{Repo}";

    private static string? NonEmpty(string s) => string.IsNullOrWhiteSpace(s) ? null : s;
}
