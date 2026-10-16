namespace PullYard.Core.Util;

public static class TailReader
{
    public static async Task<(byte[] Data, long Total, bool Truncated)> Read(Stream stream, int maxBytes, CancellationToken ct)
    {
        var chunks = new LinkedList<byte[]>();
        long kept = 0;
        long total = 0;
        var buffer = new byte[81920];
        while (true)
        {
            var n = await stream.ReadAsync(buffer, ct).ConfigureAwait(false);
            if (n == 0) break;
            total += n;
            chunks.AddLast(buffer[..n]);
            kept += n;
            while (chunks.First is { } first && kept - first.Value.Length >= maxBytes)
            {
                kept -= first.Value.Length;
                chunks.RemoveFirst();
            }
        }
        var all = new byte[kept];
        var offset = 0;
        foreach (var c in chunks)
        {
            c.CopyTo(all, offset);
            offset += c.Length;
        }
        if (kept <= maxBytes && total == kept) return (all, total, false);
        var start = (int)Math.Max(0, kept - maxBytes);
        var newline = Array.IndexOf(all, (byte)'\n', start);
        start = newline >= 0 ? newline + 1 : start;
        return (all[start..], total, true);
    }
}
