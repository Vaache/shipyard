using System.Text.Json;
using System.Text.Json.Nodes;

namespace PullYard.Core.Util;

public sealed class Settings
{
    public static Settings Shared { get; } = new(Path.Combine(AppInfo.DataDirectory, "settings.json"));

    private readonly string path;
    private readonly object gate = new();
    private JsonObject data;

    public event Action<string>? Changed;

    public Settings(string path)
    {
        this.path = path;
        data = Load(path);
    }

    private static JsonObject Load(string path)
    {
        try
        {
            if (!File.Exists(path)) return new JsonObject();
            if (JsonNode.Parse(File.ReadAllText(path)) is JsonObject o) return o;
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, $"settings load failed for {path}");
        }
        Quarantine(path);
        return new JsonObject();
    }

    private static void Quarantine(string path)
    {
        try
        {
            if (File.Exists(path)) File.Move(path, path + ".corrupt", true);
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, $"settings quarantine failed for {path}");
        }
    }

    private void Save()
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            var tmp = path + ".tmp";
            var json = data.ToJsonString(new JsonSerializerOptions { WriteIndented = true });
            using (var stream = new FileStream(tmp, FileMode.Create, FileAccess.Write, FileShare.None))
            {
                var bytes = new System.Text.UTF8Encoding(false).GetBytes(json);
                stream.Write(bytes, 0, bytes.Length);
                stream.Flush(true);
            }
            if (File.Exists(path)) File.Replace(tmp, path, null);
            else File.Move(tmp, path);
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, $"settings save failed for {path}");
        }
    }

    public bool Contains(string key)
    {
        lock (gate) return data.ContainsKey(key);
    }

    public string? GetString(string key)
    {
        lock (gate) return data[key] is JsonValue v && v.TryGetValue<string>(out var s) ? s : null;
    }

    public string GetString(string key, string fallback) => GetString(key) ?? fallback;

    public bool GetBool(string key, bool fallback)
    {
        lock (gate) return data[key] is JsonValue v && v.TryGetValue<bool>(out var b) ? b : fallback;
    }

    public double GetDouble(string key, double fallback)
    {
        lock (gate) return data[key] is JsonValue v && v.TryGetValue<double>(out var d) ? d : fallback;
    }

    public int GetInt(string key, int fallback)
    {
        lock (gate)
        {
            if (data[key] is JsonValue v)
            {
                if (v.TryGetValue<int>(out var i)) return i;
                if (v.TryGetValue<double>(out var d)) return (int)d;
            }
            return fallback;
        }
    }

    public DateTimeOffset? GetDate(string key)
    {
        var s = GetString(key);
        return s is not null && DateTimeOffset.TryParse(s, out var d) ? d : null;
    }

    public T? GetObject<T>(string key)
    {
        lock (gate)
        {
            try
            {
                return data[key] is JsonNode n ? n.Deserialize<T>(Json.Plain) : default;
            }
            catch
            {
                return default;
            }
        }
    }

    public void Set(string key, object? value)
    {
        lock (gate)
        {
            data[key] = value switch
            {
                null => null,
                DateTimeOffset d => JsonValue.Create(d.ToString("o")),
                JsonNode n => n,
                _ => JsonSerializer.SerializeToNode(value, value.GetType(), Json.Plain)
            };
            Save();
        }
        Changed?.Invoke(key);
    }

    public void Remove(string key)
    {
        lock (gate)
        {
            data.Remove(key);
            Save();
        }
        Changed?.Invoke(key);
    }
}

public static class Keys
{
    public const string PollInterval = "pollInterval";
    public const string OAuthClientId = "oauthClientID";
    public const string ThemeStyle = "theme.style";
    public const string ThemeAppearance = "theme.appearance";
    public const string LinkBrowser = "linkBrowser";
    public const string UpdateAutoCheck = "update.autoCheck";
    public const string UpdateLastCheck = "update.lastCheck";
    public const string UpdateSkipped = "update.skippedVersion";
    public const string UpdateAnnounced = "update.announcedVersion";
    public const string ClaudeModel = "claude.model";
    public const string ClaudeEffort = "claude.effort";
    public const string ClaudeMode = "claude.mode";
    public const string ClaudeIdleMinutes = "claude.idleSleepMinutes";
    public const string ClaudeUi = "claude.ui";
    public const string ClaudeProfiles = "claude.profiles";
    public const string ClaudeActiveProfile = "claude.activeProfile";
    public const string ClaudeTarget = "claudeTarget.v2";
    public const string ClaudePermissionMode = "claudeMode";
    public const string LocalRepoPaths = "localRepoPaths";
    public const string CloseToTray = "window.closeToTray";
    public const string WindowBounds = "window.bounds";
}
