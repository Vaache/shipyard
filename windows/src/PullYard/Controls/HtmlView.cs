using System.Text.Json.Nodes;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Web.WebView2.Core;
using PullYard.Core.Api;
using PullYard.Core.Html;
using PullYard.Services;
using PullYard.ViewModels;

namespace PullYard.Controls;

public sealed class HtmlView : Grid
{
    private readonly WebView2 web = new();
    private bool ready;
    private bool initializing;
    private readonly List<string> queue = new();
    private JsonObject? lastRender;
    private string page;

    public event Action<string>? LinkClicked;
    public event Action<string, string>? ActionInvoked;
    public event Action<JsonObject>? MessageReceived;

    public HtmlView(string page = "shell.html")
    {
        this.page = page;
        web.DefaultBackgroundColor = Microsoft.UI.Colors.Transparent;
        Children.Add(web);
        Loaded += (_, _) => EnsureInitialized().Forget();
        ThemeService.Changed += OnTheme;
        Unloaded += (_, _) => ThemeService.Changed -= OnTheme;
        Loaded += (_, _) =>
        {
            ThemeService.Changed -= OnTheme;
            ThemeService.Changed += OnTheme;
        };
    }

    public WebView2 WebView => web;

    private void OnTheme()
    {
        WebViewEnvironment.ApplyScheme(web);
        if (lastRender is not null)
        {
            var again = (JsonObject)lastRender.DeepClone();
            again["css"] = CssFor(again["kind"]?.GetValue<string>() ?? "");
            again["scroll"] = null;
            Send(again.ToJsonString());
        }
    }

    public async Task EnsureInitialized()
    {
        if (ready || initializing) return;
        initializing = true;
        try
        {
            await WebViewEnvironment.Initialize(web);
            web.CoreWebView2.WebMessageReceived += OnMessage;
            web.Source = new Uri(WebViewEnvironment.Origin + page);
        }
        catch (Exception e)
        {
            initializing = false;
            Children.Clear();
            Children.Add(Ui.Error(L.F("Html_WebViewMissing", "Couldn't start the HTML view: {0}. Install the Microsoft Edge WebView2 Runtime.", e.Message)));
        }
    }

    private void OnMessage(CoreWebView2 sender, CoreWebView2WebMessageReceivedEventArgs args)
    {
        JsonObject? msg;
        try
        {
            msg = JsonNode.Parse(args.WebMessageAsJson) as JsonObject;
        }
        catch
        {
            return;
        }
        if (msg is null) return;
        var type = msg["type"]?.GetValue<string>() ?? "";
        var id = msg["id"] is JsonValue v && v.TryGetValue<string>(out var s) ? s : "";
        switch (type)
        {
            case "ready":
                ready = true;
                WebViewEnvironment.ApplyScheme(web);
                foreach (var q in queue) sender.PostWebMessageAsJson(q);
                queue.Clear();
                break;
            case "link":
                LinkClicked?.Invoke(id);
                break;
            case "refresh":
                AppViewModel.Shared.Refresh();
                break;
            case "key":
                Shortcuts.Dispatch(this, id);
                break;
            default:
                ActionInvoked?.Invoke(type, id);
                break;
        }
        MessageReceived?.Invoke(msg);
    }

    public void Send(string json)
    {
        if (ready && web.CoreWebView2 is { } core)
        {
            try
            {
                core.PostWebMessageAsJson(json);
            }
            catch
            {
            }
        }
        else
        {
            queue.Add(json);
            EnsureInitialized().Forget();
        }
    }

    public void Send(JsonObject msg) => Send(msg.ToJsonString());

    public static string CssFor(string kind) => kind switch
    {
        "diff" => ConversationHtml.Css + DiffHtml.Css,
        "code" => ConversationHtml.Css + CodeHtml.Css,
        _ => ConversationHtml.Css
    };

    public void Render(string html, string kind = "conversation", string? scroll = null, string bodyClass = "")
    {
        var msg = new JsonObject
        {
            ["op"] = "render",
            ["kind"] = kind,
            ["html"] = html,
            ["css"] = CssFor(kind),
            ["scroll"] = scroll,
            ["bodyClass"] = bodyClass
        };
        lastRender = (JsonObject)msg.DeepClone();
        lastRender["scroll"] = null;
        if (!ready) queue.RemoveAll(q => q.Contains("\"op\":\"render\""));
        Send(msg);
    }

    public void Post(string op, string key, string value)
    {
        Send(new JsonObject { ["op"] = op, [key] = value });
    }

    public void ScrollToBottom() => Send(new JsonObject { ["op"] = "scrollBottom" });

    public static void OpenLink(string url, bool push = true)
    {
        if (!Uri.TryCreate(url, UriKind.Absolute, out var uri)) return;
        if (uri.Host == "github.com" && (uri.AbsolutePath == "/" || uri.AbsolutePath.Length == 0)) return;
        if (GitHubUrl.Parse(url, allowShort: false) is { } route) AppViewModel.Shared.Open(route, push);
        else LinkBrowser.Open(url);
    }
}
