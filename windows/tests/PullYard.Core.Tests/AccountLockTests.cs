using PullYard.Core.Auth;
using Xunit;

namespace PullYard.Core.Tests;

public class AccountLockTests
{
    private readonly MemoryLockStore store = new();
    private readonly AccountLock lockout;
    private DateTimeOffset clock = DateTimeOffset.FromUnixTimeSeconds(1_000_000);

    public AccountLockTests()
    {
        lockout = new AccountLock(store, 1_000) { Now = () => clock };
    }

    [Theory]
    [InlineData("1234", true)]
    [InlineData("123456", true)]
    [InlineData("123", false)]
    [InlineData("1234567", false)]
    [InlineData("12a4", false)]
    [InlineData("١٢٣٤", false)]
    [InlineData("", false)]
    public void ValidatesPinFormat(string pin, bool valid) => Assert.Equal(valid, AccountLock.IsValidPin(pin));

    [Fact]
    public void DigitsKeepsOnlyAsciiDigits()
    {
        Assert.Equal("123456", AccountLock.Digits("1a2 3-4567"));
        Assert.Equal("", AccountLock.Digits(null));
    }

    [Fact]
    public void StoresHashNotPin()
    {
        lockout.SetPin("1234", "a");
        var record = store.Load("a")!;
        Assert.Equal(32, record.Hash.Length);
        Assert.NotEqual(System.Text.Encoding.UTF8.GetBytes("1234"), record.Hash);
        Assert.Equal(16, record.Salt.Length);
        Assert.Equal(1_000, record.Iterations);
        Assert.Throws<ArgumentException>(() => lockout.SetPin("12", "a"));
    }

    [Fact]
    public void CorrectPinUnlocksAndResetsFailures()
    {
        lockout.SetPin("1234", "a");
        Assert.Equal(new PinCheck(PinResult.Wrong, 4), lockout.Verify("0000", "a"));
        Assert.Equal(PinCheck.Ok, lockout.Verify("1234", "a"));
        Assert.Equal(0, store.Load("a")!.Failures);
    }

    [Fact]
    public void PausesAfterFiveFailuresAndGrows()
    {
        lockout.SetPin("1234", "a");
        for (var i = 0; i < 4; i++) lockout.Verify("0000", "a");
        Assert.Equal(new PinCheck(PinResult.Wait, Until: clock.AddSeconds(60)), lockout.Verify("0000", "a"));
        Assert.Equal(new PinCheck(PinResult.Wait, Until: clock.AddSeconds(60)), lockout.Verify("1234", "a"));
        clock = clock.AddSeconds(61);
        Assert.Equal(new PinCheck(PinResult.Wait, Until: clock.AddSeconds(120)), lockout.Verify("0000", "a"));
    }

    [Fact]
    public void PauseIsCappedAtFifteenMinutes()
    {
        lockout.SetPin("1234", "a");
        PinCheck last = PinCheck.Ok;
        for (var i = 0; i < 9; i++)
        {
            last = lockout.Verify("0000", "a");
            clock = clock.AddHours(1);
        }
        Assert.Equal(PinResult.Wait, last.Result);
        Assert.Equal(TimeSpan.FromMinutes(15), last.Until!.Value - clock.AddHours(-1));
    }

    [Fact]
    public void BlocksAfterTenFailures()
    {
        lockout.SetPin("1234", "a");
        lockout.SetHello(true, "a");
        Assert.True(lockout.HelloEnabled("a"));
        for (var i = 0; i < 10; i++)
        {
            lockout.Verify("0000", "a");
            clock = clock.AddHours(1);
        }
        Assert.Equal(PinCheck.Blocked, lockout.Verify("1234", "a"));
        Assert.Equal(PinCheck.Blocked, lockout.State("a"));
        Assert.False(lockout.HelloEnabled("a"));
        Assert.False(lockout.HelloSucceeded("a"));
    }

    [Fact]
    public void HelloClearsPauseButNeedsToBeOn()
    {
        lockout.SetPin("1234", "a");
        Assert.False(lockout.HelloSucceeded("a"));
        for (var i = 0; i < 5; i++) lockout.Verify("0000", "a");
        Assert.Equal(PinResult.Wait, lockout.State("a")!.Result);
        lockout.SetHello(true, "a");
        Assert.True(lockout.HelloSucceeded("a"));
        Assert.Null(lockout.State("a"));
        Assert.Equal(0, store.Load("a")!.Failures);
    }

    [Fact]
    public void AccountsAreSeparate()
    {
        lockout.SetPin("1111", "a");
        lockout.SetPin("2222", "b");
        Assert.NotEqual(PinCheck.Ok, lockout.Verify("1111", "b"));
        Assert.Equal(PinCheck.Ok, lockout.Verify("2222", "b"));
        Assert.False(lockout.IsProtected("c"));
        Assert.Equal(PinCheck.Ok, lockout.Verify("0000", "c"));
        lockout.RemoveLock("a");
        Assert.False(lockout.IsProtected("a"));
    }

    [Fact]
    public void VaultStoreKeepsRecordUnderLockKey()
    {
        var vault = new Vault();
        var real = new AccountLock(new VaultLockStore(vault), 1_000);
        real.SetPin("4321", "acct");
        Assert.True(vault.Items.ContainsKey("lock-acct"));
        Assert.DoesNotContain("4321", vault.Items["lock-acct"]);
        Assert.Equal(PinCheck.Ok, real.Verify("4321", "acct"));
        vault.Items["lock-acct"] = "{broken";
        Assert.False(real.IsProtected("acct"));
    }

    private sealed class Vault : ICredentialVault
    {
        public Dictionary<string, string> Items { get; } = new();
        public string? Load(string key) => Items.TryGetValue(key, out var v) ? v : null;
        public void Save(string key, string value) => Items[key] = value;
        public bool Delete(string key) => Items.Remove(key);
    }
}
