using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using PullYard.Core.Util;

namespace PullYard.Core.Api;

public sealed record ListSnapshot(List<SearchItem> Items, int Total, DateTimeOffset Updated);

public sealed class ListSnapshotStore
{
    public const int Version = 1;
    public const int MaxItems = 40;
    public const int MaxEntries = 60;
    public static readonly TimeSpan MaxAge = TimeSpan.FromDays(7);
    public static readonly TimeSpan RewriteAfter = TimeSpan.FromMinutes(10);

    public static ListSnapshotStore Shared { get; } = new(Path.Combine(AppInfo.DataDirectory, "Cache", "Lists"));

    private static readonly JsonSerializerOptions Options = new(Json.GraphQL)
    {
        IgnoreReadOnlyProperties = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull
    };

    private const string AccountFile = "account.json";

    private readonly string root;
    private readonly Func<DateTimeOffset> now;
    private readonly object gate = new();
    private readonly Dictionary<string, long> generations = new();
    private readonly Dictionary<string, (string Hash, DateTimeOffset At)> written = new();
    private long accountGeneration;

    public ListSnapshotStore(string root, Func<DateTimeOffset>? now = null)
    {
        this.root = root;
        this.now = now ?? (() => DateTimeOffset.Now);
    }

    private sealed class Entry
    {
        public int V { get; set; }
        public string Query { get; set; } = "";
        public DateTimeOffset Updated { get; set; }
        public int Total { get; set; }
        public List<SearchItem> Items { get; set; } = new();
    }

    private sealed class RememberedEntry
    {
        public string Login { get; set; } = "";
        public string Fingerprint { get; set; } = "";
    }

    public static string? AccountKey(string? login)
    {
        if (string.IsNullOrWhiteSpace(login)) return null;
        var key = new string(login.Trim().ToLowerInvariant().Where(c => c is >= 'a' and <= 'z' or >= '0' and <= '9' or '-' or '_').ToArray());
        return key.Length == 0 ? null : key;
    }

    private static string Hash(string text) => Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(text))).ToLowerInvariant();

    private static string Fingerprint(string token) => Hash("pullyard-account:" + token)[..16];

    private string AccountDirectory(string key) => Path.Combine(root, key);

    private string SnapshotPath(string key, string query) => Path.Combine(AccountDirectory(key), Hash(query)[..32] + ".json");

    private long Generation(string key) => generations.TryGetValue(key, out var g) ? g : 0;

    public ListSnapshot? Load(string? account, string query)
    {
        if (AccountKey(account) is not { } key) return null;
        var path = SnapshotPath(key, query);
        try
        {
            if (!File.Exists(path)) return null;
            var entry = JsonSerializer.Deserialize<Entry>(ReadShared(path), Options);
            if (entry is null || entry.V != Version || entry.Query != query) return null;
            if (now() - entry.Updated > MaxAge)
            {
                TryDelete(path);
                return null;
            }
            return new ListSnapshot(entry.Items, entry.Total, entry.Updated);
        }
        catch (JsonException)
        {
            TryDelete(path);
            return null;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or NotSupportedException)
        {
            return null;
        }
    }

    private static byte[] ReadShared(string path)
    {
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
        using var buffer = new MemoryStream();
        stream.CopyTo(buffer);
        return buffer.ToArray();
    }

    private static string ContentHash(Entry entry) =>
        Hash(Encoding.UTF8.GetString(JsonSerializer.SerializeToUtf8Bytes(new Entry { V = entry.V, Query = entry.Query, Total = entry.Total, Items = entry.Items }, Options)));

    public Task<ListSnapshot?> LoadAsync(string? account, string query) => Task.Run(() => Load(account, query));

    public Task Save(string? account, string query, ListSnapshot snapshot)
    {
        if (AccountKey(account) is not { } key) return Task.CompletedTask;
        long generation;
        lock (gate) generation = Generation(key);
        var entry = new Entry { V = Version, Query = query, Updated = snapshot.Updated, Total = snapshot.Total, Items = snapshot.Items.Take(MaxItems).ToList() };
        return Task.Run(() => Write(key, generation, SnapshotPath(key, query), entry));
    }

    private void Write(string key, long generation, string path, Entry entry)
    {
        try
        {
            var bytes = JsonSerializer.SerializeToUtf8Bytes(entry, Options);
            var hash = ContentHash(entry);
            lock (gate)
            {
                if (Generation(key) != generation) return;
                if (written.TryGetValue(path, out var last) && last.Hash == hash && entry.Updated - last.At < RewriteAfter && File.Exists(path)) return;
                var dir = Path.GetDirectoryName(path)!;
                Directory.CreateDirectory(dir);
                ReplaceFile(path, bytes);
                written[path] = (hash, entry.Updated);
                Trim(dir);
            }
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, "list cache save failed");
        }
    }

    private static void ReplaceFile(string path, byte[] bytes)
    {
        var tmp = $"{path}.{Guid.NewGuid():N}.tmp";
        try
        {
            File.WriteAllBytes(tmp, bytes);
            File.Move(tmp, path, true);
        }
        finally
        {
            TryDelete(tmp);
        }
    }

    private static void Trim(string dir)
    {
        var files = new DirectoryInfo(dir).GetFiles("*.json");
        if (files.Length <= MaxEntries) return;
        foreach (var file in files.OrderByDescending(f => f.LastWriteTimeUtc).Skip(MaxEntries)) TryDelete(file.FullName);
    }

    public void Clear(string? account)
    {
        if (AccountKey(account) is not { } key) return;
        lock (gate)
        {
            generations[key] = Generation(key) + 1;
            foreach (var path in written.Keys.Where(p => p.StartsWith(AccountDirectory(key) + Path.DirectorySeparatorChar, StringComparison.Ordinal)).ToList()) written.Remove(path);
            try
            {
                if (Directory.Exists(AccountDirectory(key))) Directory.Delete(AccountDirectory(key), true);
            }
            catch (Exception e)
            {
                ErrorLog.Write(e, "list cache clear failed");
            }
        }
    }

    public void Prune()
    {
        lock (gate)
        {
            try
            {
                if (!Directory.Exists(root)) return;
                var cutoff = now() - MaxAge;
                foreach (var dir in Directory.EnumerateDirectories(root))
                {
                    foreach (var file in Directory.EnumerateFiles(dir))
                        if (file.EndsWith(".tmp", StringComparison.Ordinal) || File.GetLastWriteTimeUtc(file) < cutoff.UtcDateTime) TryDelete(file);
                    if (!Directory.EnumerateFileSystemEntries(dir).Any()) Directory.Delete(dir);
                }
            }
            catch (Exception e)
            {
                ErrorLog.Write(e, "list cache prune failed");
            }
        }
    }

    public string? RememberedAccount(string token)
    {
        try
        {
            var path = Path.Combine(root, AccountFile);
            if (!File.Exists(path)) return null;
            var entry = JsonSerializer.Deserialize<RememberedEntry>(ReadShared(path), Options);
            return entry is not null && entry.Fingerprint == Fingerprint(token) && AccountKey(entry.Login) is not null ? entry.Login : null;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or JsonException or NotSupportedException)
        {
            return null;
        }
    }

    public long AccountGeneration
    {
        get
        {
            lock (gate) return accountGeneration;
        }
    }

    public void RememberAccount(string token, string login, long? generation = null)
    {
        if (AccountKey(login) is null) return;
        try
        {
            lock (gate)
            {
                if (generation is { } g && g != accountGeneration) return;
                Directory.CreateDirectory(root);
                ReplaceFile(Path.Combine(root, AccountFile), JsonSerializer.SerializeToUtf8Bytes(new RememberedEntry { Login = login, Fingerprint = Fingerprint(token) }, Options));
            }
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, "account remember failed");
        }
    }

    public void ForgetAccount()
    {
        lock (gate)
        {
            accountGeneration++;
            TryDelete(Path.Combine(root, AccountFile));
        }
    }

    private static void TryDelete(string path)
    {
        try
        {
            if (File.Exists(path)) File.Delete(path);
        }
        catch
        {
        }
    }
}
