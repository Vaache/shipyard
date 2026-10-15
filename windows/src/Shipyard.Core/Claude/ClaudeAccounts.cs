using System.Diagnostics;
using System.Text;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using CommunityToolkit.Mvvm.ComponentModel;
using Shipyard.Core.Util;

namespace Shipyard.Core.Claude;

public sealed class ClaudeProfile
{
    public string Id { get; set; } = "";
    public string Label { get; set; } = "";
    public string? ConfigDir { get; set; }
    public string? Email { get; set; }
    public string? Plan { get; set; }

    public bool IsDefault => ConfigDir is null;
    public string Initial => (Label.Length > 0 ? Label : Email ?? "C")[..1].ToUpperInvariant();
    public string MenuTitle => Label + (Email is { } e ? $" — {e}" : "");
}

public sealed partial class ClaudeProfiles : ObservableObject
{
    public static ClaudeProfiles Shared { get; } = new();
    public const string DefaultId = "default";

    private static readonly string[] SharedItems = { "settings.json", "CLAUDE.md", "RTK.md", "skills", "commands", "plugins", "agents", "output-styles", "keybindings.json" };

    [ObservableProperty] private List<ClaudeProfile> profiles;
    [ObservableProperty] private string activeId;

    private ClaudeProfiles()
    {
        var list = Settings.Shared.GetObject<List<ClaudeProfile>>(Keys.ClaudeProfiles) ?? new();
        if (!list.Any(p => p.Id == DefaultId)) list.Insert(0, new ClaudeProfile { Id = DefaultId, Label = "Main" });
        profiles = list;
        var saved = Settings.Shared.GetString(Keys.ClaudeActiveProfile) ?? DefaultId;
        activeId = list.Any(p => p.Id == saved) ? saved : DefaultId;
    }

    public ClaudeProfile Active => Profiles.FirstOrDefault(p => p.Id == ActiveId) ?? Profiles[0];

    public static string BaseDir => Path.Combine(AppInfo.DataDirectory, "ClaudeAccounts");

    public static string DefaultConfigDir
    {
        get
        {
            var env = Environment.GetEnvironmentVariable("CLAUDE_CONFIG_DIR");
            return string.IsNullOrEmpty(env) ? Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".claude") : env;
        }
    }

    public string ConfigDir(ClaudeProfile? profile = null) => (profile ?? Active).ConfigDir ?? DefaultConfigDir;

    public string? ActiveOverride => Active.ConfigDir;

    private void Save()
    {
        Settings.Shared.Set(Keys.ClaudeProfiles, Profiles);
        Settings.Shared.Set(Keys.ClaudeActiveProfile, ActiveId);
        OnPropertyChanged(nameof(Active));
    }

    public void UpdateCachedInfo(string? email, string? plan)
    {
        var p = Profiles.FirstOrDefault(x => x.Id == ActiveId);
        if (p is null || (p.Email == email && p.Plan == plan)) return;
        p.Email = email;
        p.Plan = plan;
        Profiles = Profiles.ToList();
        Save();
    }

    public ClaudeProfile Add(string label, bool shareSettings)
    {
        var id = Guid.NewGuid().ToString().ToLowerInvariant();
        var dir = Path.Combine(BaseDir, id);
        Directory.CreateDirectory(dir);
        if (shareSettings)
        {
            foreach (var item in SharedItems)
            {
                var src = Path.Combine(DefaultConfigDir, item);
                var dst = Path.Combine(dir, item);
                try
                {
                    if (Directory.Exists(src))
                    {
                        try
                        {
                            Directory.CreateSymbolicLink(dst, src);
                        }
                        catch
                        {
                            CopyDirectory(src, dst);
                        }
                    }
                    else if (File.Exists(src))
                    {
                        try
                        {
                            File.CreateSymbolicLink(dst, src);
                        }
                        catch
                        {
                            File.Copy(src, dst, true);
                        }
                    }
                }
                catch
                {
                }
            }
        }
        var name = label.Trim();
        var profile = new ClaudeProfile { Id = id, Label = name.Length == 0 ? $"Account {Profiles.Count + 1}" : name, ConfigDir = dir };
        Profiles = Profiles.Append(profile).ToList();
        Save();
        return profile;
    }

    private static void CopyDirectory(string src, string dst)
    {
        Directory.CreateDirectory(dst);
        foreach (var f in Directory.GetFiles(src)) File.Copy(f, Path.Combine(dst, Path.GetFileName(f)), true);
        foreach (var d in Directory.GetDirectories(src)) CopyDirectory(d, Path.Combine(dst, Path.GetFileName(d)));
    }

    public void Rename(string id, string label)
    {
        var name = label.Trim();
        if (name.Length == 0 || Profiles.FirstOrDefault(p => p.Id == id) is not { } p) return;
        p.Label = name;
        Profiles = Profiles.ToList();
        Save();
    }

    public void Remove(string id)
    {
        if (id == DefaultId) return;
        Profiles = Profiles.Where(p => p.Id != id).ToList();
        if (ActiveId == id) ActiveId = DefaultId;
        Save();
    }

    public void SetActive(string id)
    {
        if (!Profiles.Any(p => p.Id == id)) return;
        ActiveId = id;
        Save();
    }
}

public sealed record ClaudeAccountInfo(string? Email, string? OrgName, string? Subscription, string? Method, string? Provider);

public enum ClaudeAccountStatus { Unknown, Checking, NotInstalled, LoggedOut, LoggedIn }

public enum ClaudeLoginMethod { ClaudeAi, Console, Sso }

public static class ClaudeLoginMethods
{
    public static string Title(this ClaudeLoginMethod m) => m switch
    {
        ClaudeLoginMethod.ClaudeAi => "Claude subscription (Pro, Max, Team)",
        ClaudeLoginMethod.Console => "Anthropic Console (API billing)",
        _ => "SSO"
    };

    public static string Flag(this ClaudeLoginMethod m) => m switch
    {
        ClaudeLoginMethod.ClaudeAi => "--claudeai",
        ClaudeLoginMethod.Console => "--console",
        _ => "--sso"
    };
}

public sealed partial class ClaudeAccountStore : ObservableObject
{
    public static ClaudeAccountStore Shared { get; } = new();

    [ObservableProperty] private ClaudeAccountStatus status = ClaudeAccountStatus.Unknown;
    [ObservableProperty] private ClaudeAccountInfo? info;
    [ObservableProperty] private bool loggingIn;
    [ObservableProperty] private string? fallbackUrl;
    [ObservableProperty] private string? loginError;
    [ObservableProperty] private DateTimeOffset? lastChecked;

    private SynchronizationContext? context;
    private Process? loginProcess;
    private readonly StringBuilder output = new();

    public bool IsLoggedIn => Status == ClaudeAccountStatus.LoggedIn;

    partial void OnStatusChanged(ClaudeAccountStatus value) => OnPropertyChanged(nameof(IsLoggedIn));

    public void Attach(SynchronizationContext ctx) => context = ctx;

    private void Ui(Action a)
    {
        if (context is null || SynchronizationContext.Current == context) a();
        else context.Post(_ => a(), null);
    }

    public async Task Refresh()
    {
        if (ClaudeLauncher.Command is not { } cmd)
        {
            Status = ClaudeAccountStatus.NotInstalled;
            Info = null;
            return;
        }
        if (Status == ClaudeAccountStatus.Unknown) Status = ClaudeAccountStatus.Checking;
        var env = ClaudeLauncher.ProcessEnvironment(ClaudeProfiles.Shared.ActiveOverride);
        var r = await Proc.Run(cmd.File, cmd.PrefixArgs.Concat(new[] { "auth", "status" }), env: env, timeout: TimeSpan.FromSeconds(30));
        LastChecked = DateTimeOffset.Now;
        JsonObject? json = null;
        try
        {
            var text = r.Out.Trim();
            var start = text.IndexOf('{');
            if (start >= 0) json = JsonNode.Parse(text[start..]) as JsonObject;
        }
        catch
        {
        }
        if (json is null || json.Bool("loggedIn") != true)
        {
            Status = ClaudeAccountStatus.LoggedOut;
            Info = null;
            return;
        }
        var sub = json.Str("subscriptionType");
        var plan = sub is null ? null : char.ToUpperInvariant(sub[0]) + sub[1..];
        if (!Demo.IsOn) ClaudeProfiles.Shared.UpdateCachedInfo(json.Str("email"), plan);
        Info = new ClaudeAccountInfo(Demo.IsOn ? Demo.Email : json.Str("email"), Demo.IsOn ? null : json.Str("orgName"), plan, json.Str("authMethod"), json.Str("apiProvider"));
        Status = ClaudeAccountStatus.LoggedIn;
    }

    public void Login(ClaudeLoginMethod method, string? email)
    {
        if (LoggingIn || ClaudeLauncher.Command is not { } cmd) return;
        LoginError = null;
        FallbackUrl = null;
        output.Clear();
        var psi = new ProcessStartInfo(cmd.File)
        {
            UseShellExecute = false,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
            WorkingDirectory = Environment.GetFolderPath(Environment.SpecialFolder.UserProfile)
        };
        foreach (var a in cmd.PrefixArgs) psi.ArgumentList.Add(a);
        psi.ArgumentList.Add("auth");
        psi.ArgumentList.Add("login");
        psi.ArgumentList.Add(method.Flag());
        if (!string.IsNullOrWhiteSpace(email))
        {
            psi.ArgumentList.Add("--email");
            psi.ArgumentList.Add(email.Trim());
        }
        foreach (var (k, v) in ClaudeLauncher.ProcessEnvironment(ClaudeProfiles.Shared.ActiveOverride))
        {
            if (v is null) psi.Environment.Remove(k);
            else psi.Environment[k] = v;
        }
        var p = new Process { StartInfo = psi, EnableRaisingEvents = true };
        p.OutputDataReceived += (_, e) => { if (e.Data is not null) Ui(() => Consume(e.Data)); };
        p.ErrorDataReceived += (_, e) => { if (e.Data is not null) Ui(() => Consume(e.Data)); };
        p.Exited += (_, _) => Ui(() => LoginFinished(p.ExitCode));
        try
        {
            p.Start();
            p.BeginOutputReadLine();
            p.BeginErrorReadLine();
            loginProcess = p;
            LoggingIn = true;
        }
        catch (Exception e)
        {
            LoginError = e.Message;
        }
    }

    private static readonly Regex UrlRx = new(@"https://[^\s]+", RegexOptions.Compiled);

    private void Consume(string line)
    {
        output.AppendLine(line);
        if (FallbackUrl is null && UrlRx.Match(output.ToString()) is { Success: true } m) FallbackUrl = m.Value;
    }

    public void SubmitCode(string code)
    {
        var trimmed = code.Trim();
        if (trimmed.Length == 0 || loginProcess is null) return;
        try
        {
            loginProcess.StandardInput.WriteLine(trimmed);
            loginProcess.StandardInput.Flush();
        }
        catch
        {
        }
    }

    public void CancelLogin()
    {
        try
        {
            loginProcess?.Kill(true);
        }
        catch
        {
        }
    }

    private void LoginFinished(int code)
    {
        LoggingIn = false;
        loginProcess = null;
        if (code != 0 && code != 15 && code != -1)
        {
            var tail = string.Join("\n", output.ToString().Split('\n').Select(l => l.Trim()).Where(l => l.Length > 0 && !l.Contains("http")).TakeLast(3));
            LoginError = tail.Length == 0 ? $"Sign-in failed (code {code})" : tail;
        }
        _ = Refresh();
    }

    public async Task Logout(string? configDir = null)
    {
        if (ClaudeLauncher.Command is not { } cmd) return;
        var env = ClaudeLauncher.ProcessEnvironment(configDir ?? ClaudeProfiles.Shared.ActiveOverride);
        await Proc.Run(cmd.File, cmd.PrefixArgs.Concat(new[] { "auth", "logout" }), env: env, timeout: TimeSpan.FromSeconds(30));
        await Refresh();
    }
}

public sealed record RateWindow(string Key, double Utilization, DateTimeOffset? ResetsAt);

public sealed partial class ClaudeUsageStore : ObservableObject
{
    public static ClaudeUsageStore Shared { get; } = new();

    [ObservableProperty] private List<RateWindow> windows = new();
    [ObservableProperty] private string? subscription;
    [ObservableProperty] private string? overageNote;
    [ObservableProperty] private DateTimeOffset? updatedAt;

    public static string WindowTitle(string key) => key switch
    {
        "five_hour" => "Session limit",
        "seven_day" => "Weekly · all models",
        _ when key.StartsWith("seven_day") => "Weekly · " + Capitalize(key.Replace("seven_day_", "").Replace('_', ' ')),
        _ => Capitalize(key.Replace('_', ' '))
    };

    private static string Capitalize(string s) => string.Join(" ", s.Split(' ').Where(w => w.Length > 0).Select(w => char.ToUpperInvariant(w[0]) + w[1..]));

    public static string Resets(DateTimeOffset? date)
    {
        if (date is not { } d) return "";
        var interval = d - DateTimeOffset.Now;
        if (interval <= TimeSpan.Zero) return "Resetting";
        if (interval < TimeSpan.FromHours(24))
        {
            var h = (int)interval.TotalHours;
            var m = interval.Minutes;
            return h > 0 ? $"Resets in {h} hr {m} min" : $"Resets in {m} min";
        }
        return "Resets " + d.ToLocalTime().ToString("ddd h:mm tt", Fmt.Culture);
    }
}
