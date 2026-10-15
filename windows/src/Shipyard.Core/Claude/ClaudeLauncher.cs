using System.Diagnostics;
using Shipyard.Core.Util;

namespace Shipyard.Core.Claude;

public sealed record ClaudeCommand(string File, IReadOnlyList<string> PrefixArgs)
{
    public string CommandLine(IEnumerable<string> args) =>
        string.Join(" ", new[] { Proc.QuoteArg(File) }.Concat(PrefixArgs.Concat(args).Select(Proc.QuoteArg)));

    public string Directory => Path.GetDirectoryName(File) ?? "";
}

public enum ClaudeTarget { App, Terminal, Desktop, Copy }

public enum ClaudePermissionMode { Standard, AcceptEdits, Plan }

public enum ClaudeModelOption { Standard, Opus, Sonnet, Haiku, Fable }

public enum ClaudeEffort { Standard, Low, Medium, High, XHigh, Max }

public enum ClaudeMode { Standard, AcceptEdits, Plan, Auto }

public enum ClaudeTabKind { Chat, Terminal }

public static class ClaudeEnums
{
    public static string Raw(this ClaudeModelOption m) => m switch
    {
        ClaudeModelOption.Opus => "opus",
        ClaudeModelOption.Sonnet => "sonnet",
        ClaudeModelOption.Haiku => "haiku",
        ClaudeModelOption.Fable => "fable",
        _ => ""
    };

    public static string Title(this ClaudeModelOption m) => m == ClaudeModelOption.Standard ? "Default" : m.ToString();

    public static ClaudeModelOption ParseModel(string? raw) =>
        Enum.GetValues<ClaudeModelOption>().FirstOrDefault(m => m.Raw() == (raw ?? ""));

    public static string Raw(this ClaudeEffort e) => e switch
    {
        ClaudeEffort.Low => "low",
        ClaudeEffort.Medium => "medium",
        ClaudeEffort.High => "high",
        ClaudeEffort.XHigh => "xhigh",
        ClaudeEffort.Max => "max",
        _ => ""
    };

    public static string Title(this ClaudeEffort e) => e == ClaudeEffort.Standard ? "Default" : e.Raw();

    public static ClaudeEffort ParseEffort(string? raw) =>
        Enum.GetValues<ClaudeEffort>().FirstOrDefault(e => e.Raw() == (raw ?? ""));

    public static string Raw(this ClaudeMode m) => m switch
    {
        ClaudeMode.AcceptEdits => "acceptEdits",
        ClaudeMode.Plan => "plan",
        ClaudeMode.Auto => "auto",
        _ => "default"
    };

    public static string Title(this ClaudeMode m) => m switch
    {
        ClaudeMode.AcceptEdits => "Accept edits",
        ClaudeMode.Plan => "Plan",
        ClaudeMode.Auto => "Auto",
        _ => "Ask"
    };

    public static ClaudeMode ParseMode(string? raw) =>
        Enum.GetValues<ClaudeMode>().FirstOrDefault(m => m.Raw() == (raw ?? "default"));

    public static string Raw(this ClaudePermissionMode m) => m switch
    {
        ClaudePermissionMode.AcceptEdits => "acceptEdits",
        ClaudePermissionMode.Plan => "plan",
        _ => "default"
    };

    public static string Title(this ClaudePermissionMode m) => m switch
    {
        ClaudePermissionMode.AcceptEdits => "Accept edits",
        ClaudePermissionMode.Plan => "Plan first",
        _ => "Default (ask)"
    };

    public static string Title(this ClaudeTarget t) => t switch
    {
        ClaudeTarget.App => "In this app",
        ClaudeTarget.Terminal => ClaudeLauncher.WindowsTerminal is not null ? "Windows Terminal" : "PowerShell",
        ClaudeTarget.Desktop => "Claude Desktop",
        _ => "Copy only"
    };
}

public static class ClaudeLauncher
{
    private static ClaudeCommand? cached;
    private static DateTimeOffset cachedAt;

    public static ClaudeCommand? Command
    {
        get
        {
            if (cached is not null && DateTimeOffset.Now - cachedAt < TimeSpan.FromSeconds(30) && File.Exists(cached.File)) return cached;
            cached = Resolve();
            cachedAt = DateTimeOffset.Now;
            return cached;
        }
    }

    public static bool Available => Command is not null;

    private static ClaudeCommand? Resolve()
    {
        var home = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
        var appData = Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData);
        var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        foreach (var exe in new[]
                 {
                     Path.Combine(home, ".local", "bin", "claude.exe"),
                     Path.Combine(home, ".claude", "local", "claude.exe"),
                     Path.Combine(local, "Programs", "claude", "claude.exe"),
                     Proc.FindOnPath("claude.exe")
                 })
        {
            if (exe is not null && File.Exists(exe)) return new ClaudeCommand(exe, Array.Empty<string>());
        }
        foreach (var shim in new[] { Path.Combine(appData, "npm", "claude.cmd"), Proc.FindOnPath("claude.cmd") })
        {
            if (shim is null || !File.Exists(shim)) continue;
            var cli = Path.Combine(Path.GetDirectoryName(shim)!, "node_modules", "@anthropic-ai", "claude-code", "cli.js");
            var node = Proc.FindOnPath("node.exe") ?? Path.Combine(Path.GetDirectoryName(shim)!, "node.exe");
            if (File.Exists(cli) && File.Exists(node)) return new ClaudeCommand(node, new[] { cli });
            var cmd = Environment.GetEnvironmentVariable("ComSpec") ?? "cmd.exe";
            return new ClaudeCommand(cmd, new[] { "/d", "/c", shim });
        }
        if (!OperatingSystem.IsWindows())
        {
            foreach (var p in new[] { Path.Combine(home, ".local/bin/claude"), "/opt/homebrew/bin/claude", "/usr/local/bin/claude" })
                if (File.Exists(p)) return new ClaudeCommand(p, Array.Empty<string>());
        }
        return null;
    }

    public static string? WindowsTerminal
    {
        get
        {
            var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            var alias = Path.Combine(local, "Microsoft", "WindowsApps", "wt.exe");
            return File.Exists(alias) ? alias : Proc.FindOnPath("wt.exe");
        }
    }

    public static bool DesktopInstalled
    {
        get
        {
            var local = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
            return Directory.Exists(Path.Combine(local, "AnthropicClaude")) || Directory.Exists(Path.Combine(local, "Programs", "Claude"));
        }
    }

    public static bool TargetAvailable(ClaudeTarget t) => t switch
    {
        ClaudeTarget.App => Available,
        ClaudeTarget.Desktop => DesktopInstalled && Available,
        _ => true
    };

    public static Dictionary<string, string?> ProcessEnvironment(string? configDir)
    {
        var env = new Dictionary<string, string?>();
        foreach (System.Collections.DictionaryEntry e in System.Environment.GetEnvironmentVariables())
        {
            var key = (string)e.Key;
            if (key == $"{AppInfo.EnvPrefix}_TOKEN" || key == "CLAUDECODE" || key.StartsWith("CLAUDE_CODE_", StringComparison.OrdinalIgnoreCase))
                env[key] = null;
        }
        if (configDir is not null) env["CLAUDE_CONFIG_DIR"] = configDir;
        if (Command is { } c && c.Directory.Length > 0)
            env["PATH"] = c.Directory + Path.PathSeparator + (System.Environment.GetEnvironmentVariable("PATH") ?? "");
        return env;
    }

    public static void Launch(string prompt, string folder, ClaudeTarget target, ClaudePermissionMode mode, string sessionName, string? resumeId, string? configDir, Action<string> copy)
    {
        if (target == ClaudeTarget.Copy)
        {
            copy(prompt);
            return;
        }
        var command = Command ?? throw new InvalidOperationException("claude CLI not found");
        if (target == ClaudeTarget.Desktop)
        {
            copy(prompt);
            var psi = new ProcessStartInfo(command.File) { UseShellExecute = false, CreateNoWindow = true, WorkingDirectory = folder };
            foreach (var a in command.PrefixArgs) psi.ArgumentList.Add(a);
            psi.ArgumentList.Add("--desktop");
            Apply(psi, configDir);
            Process.Start(psi);
            return;
        }
        var args = new List<string>();
        if (mode != ClaudePermissionMode.Standard) args.AddRange(new[] { "--permission-mode", mode.Raw() });
        if (resumeId is not null) args.AddRange(new[] { "--resume", resumeId });
        else args.AddRange(new[] { "-n", sessionName });
        if (!string.IsNullOrWhiteSpace(prompt)) args.Add(prompt);
        var line = command.CommandLine(args);
        if (WindowsTerminal is { } wt)
        {
            var psi = new ProcessStartInfo(wt) { UseShellExecute = false };
            psi.ArgumentList.Add("-d");
            psi.ArgumentList.Add(folder);
            psi.ArgumentList.Add("--title");
            psi.ArgumentList.Add(sessionName);
            psi.ArgumentList.Add(command.File);
            foreach (var a in command.PrefixArgs.Concat(args)) psi.ArgumentList.Add(a);
            Apply(psi, configDir);
            Process.Start(psi);
            return;
        }
        var shell = new ProcessStartInfo("cmd.exe") { UseShellExecute = true, WorkingDirectory = folder };
        shell.Arguments = $"/k \"{line}\"";
        if (configDir is not null) shell.Arguments = $"/k \"set \"CLAUDE_CONFIG_DIR={configDir}\" && {line}\"";
        Process.Start(shell);
    }

    private static void Apply(ProcessStartInfo psi, string? configDir)
    {
        foreach (var (k, v) in ProcessEnvironment(configDir))
        {
            if (v is null) psi.Environment.Remove(k);
            else psi.Environment[k] = v;
        }
    }
}
