using PullYard.Core.Util;

namespace PullYard.Core.Api;

internal static class RecordedHttp
{
    public static async Task<(HttpResponseMessage Response, byte[] Data)> Send(HttpClient client, HttpRequestMessage req, CancellationToken ct, string? graphQLQuery = null, string caller = "", bool keepRedirectBody = true, bool retry = false)
    {
        var probe = RequestProbe.Start();
        HttpResponseMessage? response = null;
        try
        {
            response = await client.SendAsync(req, ct).ConfigureAwait(false);
            var code = (int)response.StatusCode;
            var data = !keepRedirectBody && code is >= 300 and < 400
                ? Array.Empty<byte>()
                : await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
            if (probe.Active)
            {
                var ms = probe.ElapsedMs;
                var (error, note) = graphQLQuery is not null && code is >= 200 and < 300 ? Diagnostics.GraphQLOutcome(data) : (null, null);
                probe.Finish(Kind(graphQLQuery), Name(req, graphQLQuery, caller), Diagnostics.RawPath(req.RequestUri), response, data.Length, null, note ?? (retry ? "retry" : null), ms, error);
            }
            return (response, data);
        }
        catch (Exception e)
        {
            if (probe.Active) probe.Finish(Kind(graphQLQuery), Name(req, graphQLQuery, caller), Diagnostics.RawPath(req.RequestUri), response, 0, e, retry ? "retry" : null);
            response?.Dispose();
            throw;
        }
    }

    public static async Task<(HttpResponseMessage Response, byte[] Data, bool Truncated)> SendTail(HttpClient client, HttpRequestMessage req, int maxBytes, CancellationToken ct)
    {
        var probe = RequestProbe.Start();
        HttpResponseMessage? response = null;
        try
        {
            response = await client.SendAsync(req, HttpCompletionOption.ResponseHeadersRead, ct).ConfigureAwait(false);
            await using var stream = await response.Content.ReadAsStreamAsync(ct).ConfigureAwait(false);
            var (data, total, truncated) = await TailReader.Read(stream, maxBytes, ct).ConfigureAwait(false);
            if (probe.Active) probe.Finish(Diagnostics.KindRest, Diagnostics.RestName(req.Method, req.RequestUri), Diagnostics.RawPath(req.RequestUri), response, total, null, truncated ? "tail" : null, probe.ElapsedMs);
            return (response, data, truncated);
        }
        catch (Exception e)
        {
            if (probe.Active) probe.Finish(Diagnostics.KindRest, Diagnostics.RestName(req.Method, req.RequestUri), Diagnostics.RawPath(req.RequestUri), response, 0, e);
            response?.Dispose();
            throw;
        }
    }

    private static string Kind(string? query) => query is null ? Diagnostics.KindRest : Diagnostics.KindGraphQL;

    private static string Name(HttpRequestMessage req, string? query, string caller) =>
        query is null ? Diagnostics.RestName(req.Method, req.RequestUri) : Diagnostics.GraphQLName(query, caller);
}
