using PullYard.Core.Api;
using Xunit;

namespace PullYard.Core.Tests;

public class ListSnapshotStoreTests : IDisposable
{
    private readonly string root = Path.Combine(Path.GetTempPath(), "pullyard-lists-" + Guid.NewGuid().ToString("N"));
    private DateTimeOffset clock = new(2026, 10, 1, 12, 0, 0, TimeSpan.Zero);

    public void Dispose()
    {
        if (Directory.Exists(root)) Directory.Delete(root, true);
    }

    private ListSnapshotStore Store() => new(root, () => clock);

    private static SearchItem Item(int n, string repo = "o/r") => new()
    {
        Typename = "PullRequest",
        Number = n,
        Title = $"PR {n}",
        Url = $"https://github.com/{repo}/pull/{n}",
        PrState = "OPEN",
        IsDraft = false,
        UpdatedAt = new DateTimeOffset(2026, 9, 30, 8, 0, 0, TimeSpan.Zero),
        Author = new Actor { Login = "alice", AvatarUrl = "https://avatars/alice" },
        Repository = new RepoName { NameWithOwner = repo, DefaultBranchRef = new BranchName { Name = "main" } },
        Comments = new Count { TotalCount = n },
        Labels = new Nodes<LabelInfo> { Items = new() { new LabelInfo { Name = "bug", Color = "ff0000" } } },
        Commits = new Nodes<RollupCommitNode> { Items = new() { new RollupCommitNode { Commit = new RollupCommit { StatusCheckRollup = new RollupState { State = "SUCCESS" } } } } }
    };

    private static ListSnapshot Snapshot(int count, DateTimeOffset at) => new(Enumerable.Range(1, count).Select(i => Item(i)).ToList(), count + 5, at);

    [Fact]
    public async Task RoundTripsFirstPage()
    {
        var store = Store();
        await store.Save("Alice", "is:pr is:open", Snapshot(3, clock));
        var loaded = Store().Load("alice", "is:pr is:open");
        Assert.NotNull(loaded);
        Assert.Equal(8, loaded!.Total);
        Assert.Equal(clock, loaded.Updated);
        Assert.Equal(new[] { 1, 2, 3 }, loaded.Items.Select(i => i.Number));
        var first = loaded.Items[0];
        Assert.True(first.IsPR);
        Assert.Equal("o/r", first.Repository.NameWithOwner);
        Assert.Equal("SUCCESS", first.CiState);
        Assert.Equal("bug", first.Labels!.Items.Single().Name);
        Assert.Equal("alice", first.Author!.Login);
    }

    [Fact]
    public async Task KeepsOnlyTheFirstPage()
    {
        var store = Store();
        await store.Save("alice", "q", Snapshot(ListSnapshotStore.MaxItems + 25, clock));
        Assert.Equal(ListSnapshotStore.MaxItems, store.Load("alice", "q")!.Items.Count);
    }

    [Fact]
    public async Task SeparatesAccountsAndQueries()
    {
        var store = Store();
        await store.Save("alice", "q1", Snapshot(2, clock));
        Assert.Null(store.Load("bob", "q1"));
        Assert.Null(store.Load("alice", "q2"));
        Assert.Null(store.Load(null, "q1"));
        Assert.NotNull(store.Load("ALICE", "q1"));
    }

    [Fact]
    public async Task DropsExpiredSnapshots()
    {
        var store = Store();
        await store.Save("alice", "q", Snapshot(2, clock));
        clock += ListSnapshotStore.MaxAge + TimeSpan.FromMinutes(1);
        Assert.Null(store.Load("alice", "q"));
        Assert.Empty(Directory.EnumerateFiles(Path.Combine(root, "alice")));
    }

    [Fact]
    public async Task ClearRemovesAccountAndDropsPendingSaves()
    {
        var store = Store();
        await store.Save("alice", "q", Snapshot(2, clock));
        await store.Save("bob", "q", Snapshot(2, clock));
        var pending = store.Save("alice", "other", Snapshot(2, clock));
        store.Clear("alice");
        await pending;
        Assert.Null(store.Load("alice", "q"));
        Assert.Null(store.Load("alice", "other"));
        Assert.NotNull(store.Load("bob", "q"));
        await store.Save("alice", "q", Snapshot(1, clock));
        Assert.NotNull(store.Load("alice", "q"));
    }

    [Fact]
    public async Task CorruptFileIsIgnored()
    {
        var store = Store();
        await store.Save("alice", "q", Snapshot(1, clock));
        var file = Directory.EnumerateFiles(Path.Combine(root, "alice")).Single();
        File.WriteAllText(file, "{not json");
        Assert.Null(store.Load("alice", "q"));
        Assert.False(File.Exists(file));
    }

    [Fact]
    public async Task NeverWritesTheToken()
    {
        var store = Store();
        const string token = "ghp_secretsecretsecretsecret1234567890";
        store.RememberAccount(token, "alice");
        await store.Save("alice", "q", Snapshot(2, clock));
        foreach (var file in Directory.EnumerateFiles(root, "*", SearchOption.AllDirectories))
            Assert.DoesNotContain("secret", File.ReadAllText(file));
    }

    [Fact]
    public void RemembersAccountPerToken()
    {
        var store = Store();
        store.RememberAccount("token-a", "alice");
        Assert.Equal("alice", Store().RememberedAccount("token-a"));
        Assert.Null(store.RememberedAccount("token-b"));
        store.ForgetAccount();
        Assert.Null(store.RememberedAccount("token-a"));
    }

    [Fact]
    public async Task PruneRemovesOldFiles()
    {
        var store = Store();
        await store.Save("alice", "q", Snapshot(1, clock));
        var file = Directory.EnumerateFiles(Path.Combine(root, "alice")).Single();
        File.SetLastWriteTimeUtc(file, clock.UtcDateTime - ListSnapshotStore.MaxAge - TimeSpan.FromDays(1));
        store.Prune();
        Assert.False(Directory.Exists(Path.Combine(root, "alice")));
    }

    [Fact]
    public async Task CapsEntriesPerAccount()
    {
        var store = Store();
        for (var i = 0; i < ListSnapshotStore.MaxEntries + 5; i++)
        {
            await store.Save("alice", $"q{i}", Snapshot(1, clock));
            File.SetLastWriteTimeUtc(Directory.EnumerateFiles(Path.Combine(root, "alice")).OrderByDescending(File.GetLastWriteTimeUtc).First(), clock.UtcDateTime.AddMinutes(i));
        }
        Assert.Equal(ListSnapshotStore.MaxEntries, Directory.EnumerateFiles(Path.Combine(root, "alice")).Count());
        Assert.NotNull(store.Load("alice", $"q{ListSnapshotStore.MaxEntries + 4}"));
    }

    [Fact]
    public async Task SkipsRewritingUnchangedContent()
    {
        var store = Store();
        await store.Save("alice", "q", Snapshot(2, clock));
        var file = Directory.EnumerateFiles(Path.Combine(root, "alice")).Single();
        var stamp = new DateTime(2026, 1, 1, 0, 0, 0, DateTimeKind.Utc);
        File.SetLastWriteTimeUtc(file, stamp);
        await store.Save("alice", "q", Snapshot(2, clock + TimeSpan.FromMinutes(1)));
        Assert.Equal(stamp, File.GetLastWriteTimeUtc(file));
        Assert.Equal(clock, store.Load("alice", "q")!.Updated);
        await store.Save("alice", "q", Snapshot(3, clock + TimeSpan.FromMinutes(2)));
        Assert.Equal(3, store.Load("alice", "q")!.Items.Count);
        await store.Save("alice", "q", Snapshot(3, clock + ListSnapshotStore.RewriteAfter + TimeSpan.FromMinutes(3)));
        Assert.Equal(clock + ListSnapshotStore.RewriteAfter + TimeSpan.FromMinutes(3), store.Load("alice", "q")!.Updated);
    }

    [Fact]
    public async Task LockedFileIsKept()
    {
        var store = Store();
        await store.Save("alice", "q", Snapshot(1, clock));
        var file = Directory.EnumerateFiles(Path.Combine(root, "alice")).Single();
        using (new FileStream(file, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            Assert.Null(store.Load("alice", "q"));
        Assert.True(File.Exists(file));
        Assert.NotNull(store.Load("alice", "q"));
    }

    [Fact]
    public void RememberQueuedBeforeForgetIsDropped()
    {
        var store = Store();
        var generation = store.AccountGeneration;
        store.ForgetAccount();
        store.RememberAccount("token-a", "alice", generation);
        Assert.Null(store.RememberedAccount("token-a"));
        store.RememberAccount("token-a", "alice", store.AccountGeneration);
        Assert.Equal("alice", store.RememberedAccount("token-a"));
    }

    [Fact]
    public void AccountKeyIsSanitized()
    {
        Assert.Equal("alice-b", ListSnapshotStore.AccountKey(" Alice-B "));
        Assert.Equal("evil", ListSnapshotStore.AccountKey("../e/v\\il"));
        Assert.Null(ListSnapshotStore.AccountKey("../"));
        Assert.Null(ListSnapshotStore.AccountKey(""));
    }
}
