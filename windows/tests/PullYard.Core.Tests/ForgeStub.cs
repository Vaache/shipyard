using System.Net;
using System.Text;

namespace PullYard.Core.Tests;

public sealed class ForgeStub : HttpMessageHandler
{
    public Dictionary<string, (HttpStatusCode Status, string Body)> Routes { get; } = new();
    public List<(string Contains, HttpStatusCode Status, string Body)> QueryRoutes { get; } = new();
    public List<HttpRequestMessage> Requests { get; } = new();

    public HttpClient Client() => new(this);

    public List<HttpRequestMessage> Containing(string text)
    {
        lock (Requests) return Requests.Where(r => r.RequestUri!.AbsoluteUri.Contains(text, StringComparison.Ordinal)).ToList();
    }

    public string LastQuery => Uri.UnescapeDataString(Requests[^1].RequestUri!.Query);

    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
    {
        lock (Requests) Requests.Add(request);
        var full = Uri.UnescapeDataString(request.RequestUri!.AbsoluteUri);
        var path = request.RequestUri.AbsolutePath;
        (HttpStatusCode Status, string Body) reply;
        if (QueryRoutes.FirstOrDefault(r => full.Contains(r.Contains, StringComparison.Ordinal)) is { Contains: not null } q) reply = (q.Status, q.Body);
        else if (Routes.FirstOrDefault(r => path.EndsWith(r.Key, StringComparison.Ordinal)) is { Key: not null } hit) reply = hit.Value;
        else reply = (HttpStatusCode.NotFound, "{\"message\":\"not found\"}");
        return Task.FromResult(new HttpResponseMessage(reply.Status) { Content = new StringContent(reply.Body, Encoding.UTF8, "application/json") });
    }
}
