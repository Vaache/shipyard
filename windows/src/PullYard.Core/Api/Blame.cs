namespace PullYard.Core.Api;

public sealed record BlameRange(int Start, int End, int Age, string Sha, string ShortSha, string Message, DateTimeOffset? Date, string Url, string Author, string? Login, string? AvatarUrl)
{
    public int Lines => End - Start + 1;

    public int Heat => 10 - Math.Clamp(Age, 1, 10);
}

public static class Blame
{
    public static List<BlameRange> Map(IEnumerable<BlameRangeNode>? nodes, int lineCount = int.MaxValue, string unknown = "unknown")
    {
        var result = new List<BlameRange>();
        if (nodes is null || lineCount <= 0) return result;
        var next = 1;
        foreach (var n in nodes.Where(n => n.Commit is not null).OrderBy(n => n.StartingLine))
        {
            var start = Math.Max(n.StartingLine, next);
            var end = Math.Min(n.EndingLine, lineCount);
            if (end < start) continue;
            var c = n.Commit!;
            var login = string.IsNullOrEmpty(c.Author?.User?.Login) ? null : c.Author!.User!.Login;
            var name = string.IsNullOrWhiteSpace(c.Author?.Name) ? null : c.Author!.Name!.Trim();
            var sha = c.Oid;
            var shortSha = c.AbbreviatedOid.Length > 0 ? c.AbbreviatedOid : sha.Length > 7 ? sha[..7] : sha;
            result.Add(new BlameRange(start, end, n.Age, sha, shortSha, c.MessageHeadline, c.CommittedDate, c.Url, login ?? name ?? unknown, login, c.Author?.User?.AvatarUrl));
            next = end + 1;
        }
        return result;
    }

    public static int LineCount(string text)
    {
        if (text.Length == 0) return 1;
        var count = 1;
        foreach (var ch in text)
            if (ch == '\n') count++;
        return text[^1] == '\n' ? count - 1 : count;
    }
}

public sealed class BlameCache
{
    public static BlameCache Shared { get; } = new();

    private readonly object gate = new();
    private readonly Dictionary<string, (List<BlameRange> Ranges, DateTimeOffset Stored)> entries = new();
    private readonly LinkedList<string> order = new();
    private readonly int capacity;
    private readonly TimeSpan branchTtl;

    public BlameCache(int capacity = 60, TimeSpan? branchTtl = null)
    {
        this.capacity = capacity;
        this.branchTtl = branchTtl ?? TimeSpan.FromMinutes(10);
    }

    public static string Key(string? account, RepoRef repo, string @ref, string path) => $"{account?.ToLowerInvariant()}|{repo.Full.ToLowerInvariant()}|{@ref}|{path}";

    public static bool IsCommitId(string @ref) => @ref.Length == 40 && @ref.All(Uri.IsHexDigit);

    public List<BlameRange>? Get(string key, string @ref, DateTimeOffset? now = null)
    {
        lock (gate)
        {
            if (!entries.TryGetValue(key, out var entry)) return null;
            if (!IsCommitId(@ref) && (now ?? DateTimeOffset.Now) - entry.Stored > branchTtl)
            {
                Remove(key);
                return null;
            }
            order.Remove(key);
            order.AddFirst(key);
            return entry.Ranges;
        }
    }

    public void Set(string key, List<BlameRange> ranges, DateTimeOffset? now = null)
    {
        lock (gate)
        {
            if (entries.ContainsKey(key)) order.Remove(key);
            entries[key] = (ranges, now ?? DateTimeOffset.Now);
            order.AddFirst(key);
            while (order.Count > capacity)
            {
                entries.Remove(order.Last!.Value);
                order.RemoveLast();
            }
        }
    }

    public void Invalidate(string key)
    {
        lock (gate) Remove(key);
    }

    public void Clear()
    {
        lock (gate)
        {
            entries.Clear();
            order.Clear();
        }
    }

    public int Count
    {
        get
        {
            lock (gate) return entries.Count;
        }
    }

    private void Remove(string key)
    {
        if (entries.Remove(key)) order.Remove(key);
    }
}

public static class ShortLink
{
    public static string Of(ItemRef item) => $"{item.Repo.Full}#{item.Number}";

    public static string Of(string nameWithOwner, int number) => $"{nameWithOwner}#{number}";

    public static string? Of(Route? route) => route is { Kind: RouteKind.PullRequest or RouteKind.Issue, Item: { } item } ? Of(item) : null;

    public static string? FromUrl(string? url) => Of(GitHubUrl.Parse(url, allowShort: false));
}
