using System.ComponentModel;
using System.Text.Json.Nodes;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Shipyard.Controls;
using Shipyard.Core.Claude;
using Shipyard.Core.Util;
using Shipyard.Services;
using Shipyard.ViewModels;

namespace Shipyard.Views;

public sealed class TerminalView : Grid, IDisposable
{
    private readonly ClaudeTab tab;
    private readonly HtmlView web = new("terminal.html");
    private readonly Border overlay = new();
    private TerminalSession? attached;
    private Action<byte[]>? sink;
    private bool ready;
    private readonly List<byte[]> pending = new();
    private readonly object gate = new();
    private bool flushScheduled;

    public TerminalView(ClaudeTab tab)
    {
        this.tab = tab;
        Children.Add(web);
        overlay.HorizontalAlignment = HorizontalAlignment.Center;
        overlay.VerticalAlignment = VerticalAlignment.Center;
        Children.Add(overlay);
        web.MessageReceived += OnMessage;
        web.LinkClicked += url => LinkBrowser.Open(url);
        tab.PropertyChanged += OnTab;
        ThemeService.Changed += SendTheme;
        UpdateOverlay();
    }

    private void OnTab(object? sender, PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(ClaudeTab.TerminalRunning) or nameof(ClaudeTab.ExitCode) or nameof(ClaudeTab.TerminalSleeping))
        {
            UpdateOverlay();
            Attach();
        }
    }

    public void FocusTerminal()
    {
        if (ready) web.Send(new JsonObject { ["op"] = "focus" });
    }

    private void OnMessage(JsonObject msg)
    {
        switch (msg.Str("type"))
        {
            case "ready":
                ready = true;
                SendTheme();
                Attach();
                FocusTerminal();
                break;
            case "input":
                if (msg.Str("data") is { } data) tab.Terminal?.Write(data);
                break;
            case "resize":
                if (msg.Int("cols") is { } cols && msg.Int("rows") is { } rows) tab.Terminal?.Resize(cols, rows);
                break;
            case "copy":
                if (msg.Str("data") is { } text) WindowInterop.Copy(text);
                break;
            case "paste":
                _ = Paste();
                break;
        }
    }

    private async Task Paste()
    {
        if (await WindowInterop.ReadClipboardText() is { } text) web.Send(new JsonObject { ["op"] = "paste", ["data"] = text });
    }

    private void SendTheme()
    {
        if (!ready) return;
        string bg, fg, cursor, selection;
        if (ThemeService.IsBrand)
        {
            bg = Palette.CssHex("panel");
            fg = Palette.CssHex("text");
            cursor = Palette.CssHex("accent");
            selection = ThemeService.IsDark ? "rgba(91,184,236,0.30)" : "rgba(42,139,200,0.25)";
        }
        else
        {
            bg = ThemeService.IsDark ? "#1c1c1f" : "#fcfcfc";
            fg = ThemeService.IsDark ? "#e6e6e6" : "#1f1f1f";
            cursor = Palette.CssHex("accent");
            selection = ThemeService.IsDark ? "rgba(255,255,255,0.25)" : "rgba(0,0,0,0.18)";
        }
        web.Send(new JsonObject { ["op"] = "theme", ["theme"] = new JsonObject { ["background"] = bg, ["foreground"] = fg, ["cursor"] = cursor, ["selection"] = selection } });
    }

    private void Attach()
    {
        if (!ready) return;
        var session = tab.Terminal;
        if (ReferenceEquals(session, attached)) return;
        if (attached is not null && sink is not null) attached.Detach(sink);
        attached = session;
        if (session is null) return;
        web.Send(new JsonObject { ["op"] = "reset" });
        sink = chunk =>
        {
            lock (gate)
            {
                pending.Add(chunk);
                if (flushScheduled) return;
                flushScheduled = true;
            }
            DispatcherQueue.TryEnqueue(Flush);
        };
        var history = session.Attach(sink);
        if (history.Length > 0) Send(history);
        FocusTerminal();
    }

    private void Flush()
    {
        byte[] data;
        lock (gate)
        {
            flushScheduled = false;
            if (pending.Count == 0) return;
            var total = pending.Sum(p => p.Length);
            data = new byte[total];
            var offset = 0;
            foreach (var p in pending)
            {
                Buffer.BlockCopy(p, 0, data, offset, p.Length);
                offset += p.Length;
            }
            pending.Clear();
        }
        Send(data);
    }

    private void Send(byte[] data)
    {
        const int chunk = 512 * 1024;
        for (var i = 0; i < data.Length; i += chunk)
        {
            var len = Math.Min(chunk, data.Length - i);
            web.Send(new JsonObject { ["op"] = "output", ["data"] = Convert.ToBase64String(data, i, len) });
        }
    }

    private void UpdateOverlay()
    {
        if (tab.TerminalRunning)
        {
            overlay.Visibility = Visibility.Collapsed;
            return;
        }
        var ws = AppViewModel.Shared.Claude;
        var message = tab.TerminalSleeping
            ? Ui.H(6, Ui.Glyph("", 13, "indigo"), Ui.Text(L.T("Terminal_Sleeping", "Session is sleeping — memory freed"), 13, "indigo"))
            : Ui.H(6, Ui.Text(L.T("Terminal_Ended", "Session ended") + (tab.ExitCode is { } c ? $" ({c})" : ""), 13, "muted"));
        if (tab.ExitCode == -1 && !ClaudeLauncher.Available) message = Ui.H(6, Ui.Glyph("", 13, "amber"), Ui.Text(L.T("Claude_NotInstalled", "claude CLI isn't installed"), 13, "amber"));
        var resume = Ui.Button(L.T("Terminal_Resume", "Resume session"), () => ws.Restart(tab), accent: true);
        var close = Ui.Button(L.T("Terminal_CloseTab", "Close tab"), () => ws.Close(tab));
        overlay.Child = Ui.V(10, message, Ui.H(8, resume, close));
        overlay.Padding = new Thickness(16);
        overlay.CornerRadius = new CornerRadius(12);
        overlay.Background = Palette.Brush("raised");
        overlay.Visibility = Visibility.Visible;
    }

    public void Dispose()
    {
        tab.PropertyChanged -= OnTab;
        ThemeService.Changed -= SendTheme;
        if (attached is not null && sink is not null) attached.Detach(sink);
        attached = null;
    }
}
