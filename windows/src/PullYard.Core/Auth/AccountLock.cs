using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using PullYard.Core.Util;

namespace PullYard.Core.Auth;

public sealed class LockRecord
{
    public byte[] Salt { get; set; } = Array.Empty<byte>();
    public byte[] Hash { get; set; } = Array.Empty<byte>();
    public int Iterations { get; set; }
    public int Failures { get; set; }
    public DateTimeOffset? LockedUntil { get; set; }
    public bool Hello { get; set; }
}

public interface ILockStore
{
    LockRecord? Load(string id);
    void Save(LockRecord record, string id);
    void Delete(string id);
}

public sealed class MemoryLockStore : ILockStore
{
    private readonly Dictionary<string, string> records = new();

    public LockRecord? Load(string id) => records.TryGetValue(id, out var json) ? JsonSerializer.Deserialize<LockRecord>(json, Json.Plain) : null;

    public void Save(LockRecord record, string id) => records[id] = JsonSerializer.Serialize(record, Json.Plain);

    public void Delete(string id) => records.Remove(id);
}

public sealed class VaultLockStore : ILockStore
{
    private readonly ICredentialVault vault;

    public VaultLockStore(ICredentialVault vault) => this.vault = vault;

    private static string Key(string id) => "lock-" + id;

    public LockRecord? Load(string id)
    {
        if (vault.Load(Key(id)) is not { } raw) return null;
        try
        {
            return JsonSerializer.Deserialize<LockRecord>(raw, Json.Plain);
        }
        catch (JsonException)
        {
            return null;
        }
    }

    public void Save(LockRecord record, string id) => vault.Save(Key(id), JsonSerializer.Serialize(record, Json.Plain));

    public void Delete(string id) => vault.Delete(Key(id));
}

public enum PinResult { Ok, Wrong, Wait, Blocked }

public sealed record PinCheck(PinResult Result, int AttemptsLeft = 0, DateTimeOffset? Until = null)
{
    public static PinCheck Ok { get; } = new(PinResult.Ok);
    public static PinCheck Blocked { get; } = new(PinResult.Blocked);
}

public sealed class AccountLock
{
    public const int MinLength = 4;
    public const int MaxLength = 6;
    public const int FreeAttempts = 5;
    public const int MaxAttempts = 10;
    public static readonly TimeSpan BaseDelay = TimeSpan.FromSeconds(60);
    public static readonly TimeSpan MaxDelay = TimeSpan.FromMinutes(15);

    private readonly ILockStore store;
    private readonly int iterations;

    public Func<DateTimeOffset> Now { get; set; } = () => DateTimeOffset.Now;

    public AccountLock(ILockStore store, int iterations = 200_000)
    {
        this.store = store;
        this.iterations = iterations;
    }

    public bool IsProtected(string id) => store.Load(id) is not null;

    public bool HelloEnabled(string id) => store.Load(id) is { Hello: true } r && r.Failures < MaxAttempts;

    public static bool IsValidPin(string? pin) => pin is { Length: >= MinLength and <= MaxLength } && pin.All(c => c is >= '0' and <= '9');

    public static string Digits(string? text) => new string((text ?? "").Where(c => c is >= '0' and <= '9').Take(MaxLength).ToArray());

    public void SetPin(string pin, string id)
    {
        if (!IsValidPin(pin)) throw new ArgumentException("Use 4 to 6 digits.", nameof(pin));
        var salt = RandomNumberGenerator.GetBytes(16);
        store.Save(new LockRecord { Salt = salt, Hash = Derive(pin, salt, iterations), Iterations = iterations }, id);
    }

    public void RemoveLock(string id) => store.Delete(id);

    public PinCheck? State(string id)
    {
        if (store.Load(id) is not { } r) return null;
        if (r.Failures >= MaxAttempts) return PinCheck.Blocked;
        if (r.LockedUntil is { } until && until > Now()) return new PinCheck(PinResult.Wait, Until: until);
        return null;
    }

    public PinCheck Verify(string pin, string id)
    {
        if (store.Load(id) is not { } r) return PinCheck.Ok;
        if (State(id) is { } pending) return pending;
        if (CryptographicOperations.FixedTimeEquals(Derive(pin, r.Salt, r.Iterations), r.Hash))
        {
            r.Failures = 0;
            r.LockedUntil = null;
            store.Save(r, id);
            return PinCheck.Ok;
        }
        r.Failures++;
        PinCheck result;
        if (r.Failures >= MaxAttempts)
        {
            r.Hello = false;
            result = PinCheck.Blocked;
        }
        else if (r.Failures >= FreeAttempts)
        {
            var delay = TimeSpan.FromSeconds(Math.Min(BaseDelay.TotalSeconds * Math.Pow(2, r.Failures - FreeAttempts), MaxDelay.TotalSeconds));
            var until = Now() + delay;
            r.LockedUntil = until;
            result = new PinCheck(PinResult.Wait, Until: until);
        }
        else
        {
            result = new PinCheck(PinResult.Wrong, FreeAttempts - r.Failures);
        }
        store.Save(r, id);
        return result;
    }

    public void SetHello(bool on, string id)
    {
        if (store.Load(id) is not { } r) return;
        r.Hello = on;
        store.Save(r, id);
    }

    public bool HelloSucceeded(string id)
    {
        if (store.Load(id) is not { Hello: true } r || r.Failures >= MaxAttempts) return false;
        r.Failures = 0;
        r.LockedUntil = null;
        store.Save(r, id);
        return true;
    }

    private static byte[] Derive(string pin, byte[] salt, int iterations) =>
        Rfc2898DeriveBytes.Pbkdf2(Encoding.UTF8.GetBytes(pin), salt, iterations, HashAlgorithmName.SHA256, 32);
}
