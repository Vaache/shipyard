using System.ComponentModel;
using System.Text.Json.Nodes;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using PullYard.Controls;
using PullYard.Core.Claude;
using PullYard.Core.Git;
using PullYard.Core.Util;
using PullYard.Services;
using PullYard.ViewModels;
using System.Runtime.InteropServices.WindowsRuntime;
using Windows.ApplicationModel.DataTransfer;
using Windows.Graphics.Imaging;
using Windows.Storage;
using Windows.Storage.Streams;
using Windows.System;

namespace PullYard.Views;

public sealed class ChatView : Grid
{
    private readonly ClaudeTab tab;
    private readonly ChatSession chat;
    private readonly HtmlView web = new("chat.html");
    private readonly Border sleepBanner = new();
    private readonly Border permissionStrip = new();
    private readonly TextBox input = new()
    {
        AcceptsReturn = true,
        TextWrapping = TextWrapping.Wrap,
        MinHeight = 36,
        MaxHeight = 200,
        BorderThickness = new Thickness(0),
        Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent)
    };
    private readonly StackPanel images = Ui.H(6);
    private readonly ListView completions = new() { SelectionMode = ListViewSelectionMode.Single, MaxHeight = 260, IsItemClickEnabled = true };
    private readonly Grid bottomBar = new();
    private readonly List<ChatImage> attached = new();
    private Microsoft.UI.Dispatching.DispatcherQueueTimer? flushTimer;
    private bool ready;
    private readonly HashSet<string> shownUserItems = new();
    private List<string> files = new();
    private bool filesLoading;
    private string? completionKind;

    private sealed record Completion(string Title, string Detail, string Insert) : IRowContent
    {
        public UIElement BuildRow()
        {
            var g = Ui.Columns((Ui.Text(Title, 13, "text", FontWeights.Medium), Ui.Auto), (Ui.Text(Detail, 12, "muted"), Ui.Star));
            g.Padding = new Thickness(0, 3, 0, 3);
            return g;
        }
    }

    public ChatView(ClaudeTab tab)
    {
        this.tab = tab;
        chat = tab.Chat!;
        RowDefinitions.Add(new RowDefinition { Height = new GridLength(1, GridUnitType.Star) });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        RowDefinitions.Add(new RowDefinition { Height = GridLength.Auto });
        Children.Add(web);
        SetRow(sleepBanner, 1);
        Children.Add(sleepBanner);
        SetRow(permissionStrip, 2);
        Children.Add(permissionStrip);
        var composer = BuildComposer();
        SetRow(composer, 3);
        Children.Add(composer);

        web.MessageReceived += OnWebMessage;
        web.LinkClicked += url => HtmlView.OpenLink(url);
        chat.ItemsChanged += ScheduleFlush;
        chat.PropertyChanged += OnChat;
        ClaudeUsageStore.Shared.PropertyChanged += OnUsage;
        ThemeService.Changed += SendVars;
        Unloaded += (_, _) =>
        {
            ThemeService.Changed -= SendVars;
        };
        UpdateChrome();
    }

    private void OnUsage(object? sender, PropertyChangedEventArgs e) => DispatcherQueue.TryEnqueue(RenderBottomBar);

    private void SendVars() => web.Send(new JsonObject { ["op"] = "vars", ["css"] = ThemeService.WebVars() });

    private void OnWebMessage(JsonObject msg)
    {
        var type = msg.Str("type");
        switch (type)
        {
            case "ready":
                ready = true;
                SendVars();
                chat.MarkAllDirty();
                Flush();
                break;
            case "perm":
                HandlePermission(msg);
                break;
            case "send":
                if (msg.Str("text") is { } text) Send(text, new List<ChatImage>());
                break;
            case "stop":
                chat.Interrupt();
                break;
            case "restart":
                AppViewModel.Shared.Claude.Restart(tab);
                break;
            case "checkAuth":
                ClaudeAccountStore.Shared.Refresh().Forget();
                break;
            case "open":
                if (msg.Str("path") is { } path) PathGuard.OpenWithinWorkspace(tab.Folder, path);
                break;
        }
    }

    private void HandlePermission(JsonObject msg)
    {
        if (msg.Str("id") is not { } id || chat.FindPermission(id) is not { } req) return;
        switch (msg.Str("action"))
        {
            case "allow":
                chat.Respond(req, true);
                if (msg.Str("mode") is { } mode) chat.SetPermissionMode(mode);
                break;
            case "always":
                var index = msg.Int("index") ?? -1;
                chat.Respond(req, true, index >= 0 && index < req.Suggestions.Count ? req.Suggestions[index] : null);
                break;
            case "deny":
                chat.Respond(req, false, message: msg.Str("message"));
                break;
            case "answer":
                var updated = (JsonObject)req.Input.DeepClone();
                var answers = new JsonObject();
                if (msg.Obj("answers") is { } a)
                    foreach (var (k, v) in a)
                        answers[k] = v?.DeepClone();
                updated["answers"] = answers;
                chat.Respond(req, true, updatedInput: updated);
                break;
        }
        UpdateChrome();
    }

    private void ScheduleFlush()
    {
        if (flushTimer is not null) return;
        flushTimer = DispatcherQueue.CreateTimer();
        flushTimer.Interval = TimeSpan.FromMilliseconds(60);
        flushTimer.IsRepeating = false;
        flushTimer.Tick += (_, _) =>
        {
            flushTimer = null;
            Flush();
        };
        flushTimer.Start();
    }

    private JsonObject Meta() => new()
    {
        ["busy"] = chat.Busy,
        ["running"] = chat.Running,
        ["sleeping"] = chat.Sleeping,
        ["status"] = chat.Status,
        ["pendingCount"] = chat.PendingCount,
        ["lastError"] = chat.LastError,
        ["folder"] = chat.Folder,
        ["turnStartedAt"] = chat.TurnStartedAt?.ToUnixTimeMilliseconds()
    };

    private void Flush()
    {
        if (!ready) return;
        var (reset, changed) = chat.TakeChanges();
        if (!reset && changed.Count == 0) return;
        var arr = new JsonArray(changed.Select(i => (JsonNode)i.ToJson(chat.Folder)).ToArray());
        web.Send(new JsonObject { ["op"] = reset ? "reset" : "upsert", ["items"] = arr, ["meta"] = Meta() });
        if (reset) shownUserItems.Clear();
        var ownMessage = false;
        foreach (var item in changed.OfType<UserItem>())
            ownMessage |= shownUserItems.Add(item.Id) && !reset;
        if (ownMessage) web.Send(new JsonObject { ["op"] = "bottom" });
    }

    private void OnChat(object? sender, PropertyChangedEventArgs e)
    {
        switch (e.PropertyName)
        {
            case nameof(ChatSession.Busy):
            case nameof(ChatSession.Status):
            case nameof(ChatSession.PendingCount):
            case nameof(ChatSession.LastError):
            case nameof(ChatSession.Sleeping):
            case nameof(ChatSession.Running):
            case nameof(ChatSession.TurnStartedAt):
                if (ready) web.Send(new JsonObject { ["op"] = "meta", ["meta"] = Meta() });
                UpdateChrome();
                break;
            case nameof(ChatSession.Models):
            case nameof(ChatSession.ModelValue):
            case nameof(ChatSession.Model):
            case nameof(ChatSession.PermissionMode):
            case nameof(ChatSession.Effort):
            case nameof(ChatSession.ContextPercent):
            case nameof(ChatSession.Context):
            case nameof(ChatSession.TotalCost):
                RenderBottomBar();
                break;
        }
    }

    private void UpdateChrome()
    {
        if (chat.Sleeping && !chat.Running)
        {
            var row = Ui.Columns((Ui.H(8, Ui.Glyph("", 13, "indigo"), Ui.Text(L.T("Chat_Sleeping", "Session is sleeping — process stopped, memory freed. A new message resumes it."), 13, "muted", wrap: true)), Ui.Star),
                (Ui.Button(L.T("Claude_WakeUp", "Wake up"), chat.Wake), Ui.Auto));
            sleepBanner.Child = row;
            sleepBanner.Padding = new Thickness(14, 6, 14, 6);
            sleepBanner.Background = Palette.Brush("indigo", 0.08);
            sleepBanner.Visibility = Visibility.Visible;
        }
        else
        {
            sleepBanner.Visibility = Visibility.Collapsed;
        }
        if (chat.PendingPermissions.FirstOrDefault(p => !p.Resolved) is { } pending)
        {
            UIElement content;
            if (pending.ToolName is "AskUserQuestion" or "ExitPlanMode")
            {
                content = Ui.H(6, Ui.Glyph("", 13, "amber"), Ui.Text(pending.ToolName == "AskUserQuestion" ? L.T("Chat_AskedQuestion", "Claude asked a question — answer above") : L.T("Chat_WaitingPlan", "Claude is waiting for plan approval above"), 13));
            }
            else
            {
                content = Ui.Columns((Ui.H(6, Ui.Glyph("", 13, "amber"), Ui.Text(L.F("Chat_NeedsPermission", "Claude needs permission: {0}", pending.ToolName), 13)), Ui.Star),
                    (Ui.H(6, Ui.Button(L.T("Chat_Allow", "Allow"), () => { chat.Respond(pending, true); UpdateChrome(); }, accent: true), Ui.Button(L.T("Chat_Deny", "Deny"), () => { chat.Respond(pending, false); UpdateChrome(); })), Ui.Auto));
            }
            permissionStrip.Child = content;
            permissionStrip.Padding = new Thickness(14, 6, 14, 6);
            permissionStrip.Background = Palette.Brush("amber", 0.08);
            permissionStrip.Visibility = Visibility.Visible;
        }
        else
        {
            permissionStrip.Visibility = Visibility.Collapsed;
        }
        input.PlaceholderText = chat.Busy ? L.T("Chat_Queued", "Message will be queued…") : L.T("Chat_Placeholder", "Ask Claude or describe a task…");
        RenderBottomBar();
    }

    private FrameworkElement BuildComposer()
    {
        completions.ItemTemplate = ListSupport.Template("GenericRowTemplate");
        completions.ItemContainerStyle = ListSupport.StretchItem;
        completions.Visibility = Visibility.Collapsed;
        completions.ItemClick += (_, e) =>
        {
            if (e.ClickedItem is Completion c) Accept(c);
        };
        var completionHost = new Border { Child = completions, Background = Palette.Brush("sunken"), CornerRadius = new CornerRadius(8), Padding = new Thickness(4) };
        completionHost.Visibility = Visibility.Collapsed;
        completions.RegisterPropertyChangedCallback(VisibilityProperty, (_, _) => completionHost.Visibility = completions.Visibility);

        input.TextChanged += (_, _) => UpdateCompletion();
        input.SelectionChanged += (_, _) => UpdateCompletion();
        input.PreviewKeyDown += OnKey;
        input.Paste += OnPaste;
        input.AllowDrop = true;
        input.DragOver += (_, e) => e.AcceptedOperation = DataPackageOperation.Copy;
        input.Drop += async (_, e) => await DropItems(e.DataView);

        var panel = Ui.V(6, completionHost, images, input, bottomBar);
        var card = new Border { Child = panel, Background = Palette.Brush("raised"), CornerRadius = new CornerRadius(12), Padding = new Thickness(10), Margin = new Thickness(12) };
        card.AllowDrop = true;
        card.DragOver += (_, e) => e.AcceptedOperation = DataPackageOperation.Copy;
        card.Drop += async (_, e) => await DropItems(e.DataView);

        var shiftTab = new KeyboardAccelerator { Key = VirtualKey.Tab, Modifiers = VirtualKeyModifiers.Shift };
        shiftTab.Invoked += (_, e) =>
        {
            e.Handled = true;
            CycleMode();
        };
        var focus = new KeyboardAccelerator { Key = VirtualKey.K, Modifiers = VirtualKeyModifiers.Control };
        focus.Invoked += (_, e) =>
        {
            e.Handled = true;
            input.Focus(FocusState.Programmatic);
        };
        card.KeyboardAccelerators.Add(shiftTab);
        KeyboardAccelerators.Add(focus);
        var wrapper = new Grid();
        var line = Ui.Line();
        line.VerticalAlignment = VerticalAlignment.Top;
        wrapper.Children.Add(line);
        wrapper.Children.Add(card);
        RenderImages();
        RenderBottomBar();
        return wrapper;
    }

    private void RenderBottomBar()
    {
        bottomBar.Children.Clear();
        bottomBar.ColumnDefinitions.Clear();
        bottomBar.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(1, GridUnitType.Star) });
        bottomBar.ColumnDefinitions.Add(new ColumnDefinition { Width = GridLength.Auto });
        var left = Ui.H(4);
        var mode = ClaudeEnums.ParseMode(chat.PermissionMode);
        var modeGlyph = chat.PermissionMode switch
        {
            "acceptEdits" => "\uE70F",
            "plan" => "\uE9D5",
            "auto" => "\uE945",
            _ => "\uE7C1"
        };
        var modeMenu = new MenuFlyout();
        foreach (var m in Enum.GetValues<ClaudeMode>())
        {
            var raw = m.Raw();
            modeMenu.Items.Add(Ui.CheckItem(m.Title(), chat.PermissionMode == raw, () => chat.SetPermissionMode(raw)));
        }
        left.Children.Add(Small(modeGlyph, mode.Title(), modeMenu, L.T("Chat_ModeHelp", "Permission mode (Shift+Tab to cycle)")));
        var modelMenu = new MenuFlyout();
        foreach (var m in chat.Models)
        {
            var value = m.Value;
            modelMenu.Items.Add(Ui.CheckItem($"{m.DisplayName} — {m.Description}", chat.ModelValue == value, () => chat.SetModel(value)));
        }
        var modelTitle = chat.Models.FirstOrDefault(m => m.Value == chat.ModelValue)?.DisplayName ?? (chat.Model.Length == 0 ? L.T("Claude_Model", "Model") : chat.Model.Replace("claude-", ""));
        var modelButton = Small("\uE950", modelTitle, modelMenu, L.T("Claude_Model", "Model"));
        modelButton.IsEnabled = chat.Models.Count > 0;
        left.Children.Add(modelButton);
        var efforts = chat.Models.FirstOrDefault(m => m.Value == chat.ModelValue)?.Efforts ?? chat.Models.FirstOrDefault()?.Efforts ?? new List<string>();
        if (efforts.Count > 0)
        {
            var effortMenu = new MenuFlyout();
            effortMenu.Items.Add(Ui.CheckItem(L.T("Claude_Default", "Default"), chat.Effort.Length == 0, () => chat.SetEffort("")));
            foreach (var e in efforts)
            {
                var level = e;
                effortMenu.Items.Add(Ui.CheckItem(level, chat.Effort == level, () => chat.SetEffort(level)));
            }
            left.Children.Add(Small("\uEC4A", chat.Effort.Length == 0 ? "effort" : chat.Effort, effortMenu, L.T("Claude_Effort", "Effort")));
        }
        left.Children.Add(Ui.IconButton("\uEB9F", L.T("Chat_AttachImage", "Attach image"), () => PickImages().Forget()));
        var right = Ui.H(6, UsageRing());
        if (chat.Busy) right.Children.Add(Ui.IconButton("\uE71A", L.T("Chat_Stop", "Stop (Esc)"), chat.Interrupt, "red"));
        var send = new Button { Content = Ui.Glyph("\uE724", 13, "text"), Style = (Style)Application.Current.Resources["AccentButtonStyle"], Padding = new Thickness(10, 5, 10, 5) };
        Ui.Tip(send, L.T("Chat_Send", "Send (Enter)"));
        send.Click += (_, _) => Submit();
        right.Children.Add(send);
        bottomBar.Children.Add(left);
        SetColumn(right, 1);
        bottomBar.Children.Add(right);
    }

    private static DropDownButton Small(string glyph, string text, MenuFlyout menu, string help)
    {
        var b = new DropDownButton
        {
            Content = Ui.H(4, Ui.Glyph(glyph, 11, "muted"), Ui.Text(text, 12, "muted")),
            Flyout = menu,
            Padding = new Thickness(6, 2, 6, 2),
            Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent),
            BorderThickness = new Thickness(0)
        };
        Ui.Tip(b, help);
        return b;
    }

    private FrameworkElement UsageRing()
    {
        var pct = chat.Context?.Percentage ?? chat.ContextPercent ?? 0;
        var color = pct >= 85 ? "red" : pct >= 65 ? "amber" : "sky";
        var ring = new ProgressRing { IsIndeterminate = false, Value = Math.Clamp(pct, 1, 100), Maximum = 100, Width = 18, Height = 18, Foreground = Palette.Brush(color) };
        var b = new Button { Content = ring, Background = new SolidColorBrush(Microsoft.UI.Colors.Transparent), BorderThickness = new Thickness(0), Padding = new Thickness(4) };
        Ui.Tip(b, L.F("Chat_ContextHelp", "Context {0}% — click for usage and limits", pct));
        var flyout = new Flyout();
        b.Click += (_, _) =>
        {
            chat.RefreshContext();
            flyout.Content = UsageOverlay();
            flyout.ShowAt(b);
        };
        return b;
    }

    private FrameworkElement UsageOverlay()
    {
        var usage = ClaudeUsageStore.Shared;
        var panel = Ui.V(10);
        panel.Width = 360;
        var ctxPanel = Ui.V(8);
        if (chat.Context is { } ctx)
        {
            ctxPanel.Children.Add(Ui.Columns((Ui.Text(L.T("Usage_Context", "Context window"), 13, "text", FontWeights.Medium), Ui.Star), (Ui.Text($"{Fmt.Tokens(ctx.Total)} / {Fmt.Tokens(ctx.Max)} ({ctx.Percentage}%)", 12.5, "muted"), Ui.Auto)));
            ctxPanel.Children.Add(ContextBar(ctx));
            var note = ctx.AutoCompactEnabled && ctx.AutoCompactThreshold is { } t && t > ctx.Total
                ? L.F("Usage_UntilCompact", "{0} until auto-compact", Fmt.Tokens(t - ctx.Total))
                : !ctx.AutoCompactEnabled ? L.T("Usage_CompactOff", "Auto-compact is off") : "";
            var compact = Ui.Button(L.T("Usage_Compact", "Compact session"), chat.Compact);
            compact.IsEnabled = chat.Running && !chat.Busy;
            ctxPanel.Children.Add(Ui.Columns((Ui.Text(note, 12.5, "muted"), Ui.Star), (compact, Ui.Auto)));
        }
        else
        {
            ctxPanel.Children.Add(Ui.Columns((Ui.Text(L.T("Usage_Context", "Context window"), 13, "muted"), Ui.Star), (Ui.Text(chat.Running ? L.T("Common_Loading", "Loading…") : L.T("Usage_Sleeping", "session is sleeping"), 12.5, "dim"), Ui.Auto)));
        }
        panel.Children.Add(Ui.Card(ctxPanel, 10, "raised", 12));
        var limits = Ui.V(8, Ui.Columns((Ui.Text(L.T("Usage_Limits", "Your usage limits") + (usage.Subscription is { } s ? $" · {s}" : ""), 13, "text", FontWeights.Medium), Ui.Star),
            (Ui.IconButton("", L.T("Claude_UsagePage", "Usage page"), () => LinkBrowser.Open("https://claude.ai/settings/usage")), Ui.Auto)));
        if (usage.Windows.Count == 0) limits.Children.Add(Ui.Text(L.T("Usage_AfterFirstReply", "Shows up after Claude's first reply"), 12, "dim"));
        foreach (var w in usage.Windows)
        {
            var color = w.Utilization >= 0.9 ? "red" : w.Utilization >= 0.7 ? "amber" : "sky";
            limits.Children.Add(Ui.Columns((Ui.Text(ClaudeUsageStore.WindowTitle(w.Key), 13), Ui.Star), (Ui.Caption(ClaudeUsageStore.Resets(w.ResetsAt)), Ui.Auto), (Ui.Text($"{Math.Round(w.Utilization * 100)}%", 13), Ui.Auto)));
            limits.Children.Add(new ProgressBar { Value = w.Utilization, Maximum = 1, Foreground = Palette.Brush(color) });
        }
        if (usage.OverageNote is { } over) limits.Children.Add(Ui.H(6, Ui.Glyph("", 12, "amber"), Ui.Text(over, 12, "amber")));
        panel.Children.Add(Ui.Card(limits, 10, "raised", 12));
        if (chat.TotalCost > 0) panel.Children.Add(Ui.Text(L.F("Usage_SessionCost", "Session: ${0:0.00}", chat.TotalCost), 12, "muted"));
        if (chat.Context is { Categories.Count: > 0 } c)
        {
            var details = Ui.V(4);
            foreach (var cat in c.Categories)
            {
                var swatch = new Border { Width = 10, Height = 10, CornerRadius = new CornerRadius(2), Background = cat.Kind == "free" ? Palette.Brush("text", 0.15) : Palette.Brush(CategoryColor(cat.Color, cat.Kind)) };
                details.Children.Add(Ui.Columns((swatch, Ui.Auto), (Ui.Text(cat.Name + (cat.Kind == "deferred" ? " (deferred)" : ""), 12, cat.Kind == "used" ? "text" : "muted"), Ui.Star), (Ui.Caption(Fmt.Tokens(cat.Tokens)), Ui.Auto)));
            }
            panel.Children.Add(new Expander { Header = L.T("Usage_Breakdown", "Detailed breakdown"), Content = details, HorizontalAlignment = HorizontalAlignment.Stretch, HorizontalContentAlignment = HorizontalAlignment.Stretch });
        }
        return panel;
    }

    private static string CategoryColor(string name, string kind)
    {
        if (kind is "buffer" or "deferred") return "dim";
        return name switch
        {
            "claude" => "coral",
            "warning" => "amber",
            "error" => "red",
            "inactive" => "muted",
            "promptBorder" => "slate",
            _ when name.StartsWith("green") => "green",
            _ when name.StartsWith("purple") => "violet",
            _ when name.StartsWith("blue") => "accent",
            _ when name.StartsWith("red") => "red",
            _ when name.StartsWith("orange") => "amber",
            _ when name.StartsWith("pink") => "coral",
            _ when name.StartsWith("cyan") => "sky",
            _ => "teal"
        };
    }

    private static FrameworkElement ContextBar(ContextUsage ctx)
    {
        var grid = new Grid { Height = 6, CornerRadius = new CornerRadius(3), Background = Palette.Brush("text", 0.15) };
        var used = ctx.Categories.Where(c => c.Kind is "used" or "buffer").ToList();
        double total = Math.Max(ctx.Max, 1);
        foreach (var c in used) grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(c.Tokens / total, GridUnitType.Star) });
        grid.ColumnDefinitions.Add(new ColumnDefinition { Width = new GridLength(Math.Max(0, 1 - used.Sum(c => c.Tokens) / total), GridUnitType.Star) });
        for (var i = 0; i < used.Count; i++)
        {
            var r = new Border { Background = Palette.Brush(CategoryColor(used[i].Color, used[i].Kind)), Margin = new Thickness(0, 0, 1.5, 0) };
            SetColumn(r, i);
            grid.Children.Add(r);
        }
        return grid;
    }

    private void CycleMode()
    {
        var order = new[] { "default", "acceptEdits", "plan" };
        var i = Array.IndexOf(order, chat.PermissionMode);
        chat.SetPermissionMode(order[(Math.Max(i, 0) + 1) % order.Length]);
    }

    private void Submit()
    {
        if (completionKind is not null && completions.SelectedItem is Completion c)
        {
            Accept(c);
            return;
        }
        var message = input.Text;
        if (message.Trim().Length == 0 && attached.Count == 0) return;
        Send(message, attached.ToList());
        input.Text = "";
        attached.Clear();
        RenderImages();
        HideCompletion();
    }

    private void Send(string text, List<ChatImage> imgs)
    {
        if (!chat.Running) AppViewModel.Shared.Claude.Restart(tab);
        chat.Send(text, imgs);
    }

    private void OnKey(object sender, KeyRoutedEventArgs e)
    {
        var shift = Microsoft.UI.Input.InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Shift).HasFlag(Windows.UI.Core.CoreVirtualKeyStates.Down);
        switch (e.Key)
        {
            case VirtualKey.Enter when !shift:
                e.Handled = true;
                Submit();
                return;
            case VirtualKey.Escape:
                if (completionKind is not null)
                {
                    HideCompletion();
                    e.Handled = true;
                }
                else if (chat.Busy)
                {
                    chat.Interrupt();
                    e.Handled = true;
                }
                return;
        }
        if (completionKind is null || completions.Items.Count == 0) return;
        switch (e.Key)
        {
            case VirtualKey.Down:
                completions.SelectedIndex = Math.Min(completions.SelectedIndex + 1, completions.Items.Count - 1);
                completions.ScrollIntoView(completions.SelectedItem);
                e.Handled = true;
                break;
            case VirtualKey.Up:
                completions.SelectedIndex = Math.Max(completions.SelectedIndex - 1, 0);
                completions.ScrollIntoView(completions.SelectedItem);
                e.Handled = true;
                break;
            case VirtualKey.Tab:
                if (completions.SelectedItem is Completion c) Accept(c);
                e.Handled = true;
                break;
        }
    }

    private (int Start, int Length)? TokenRange()
    {
        var text = input.Text;
        var pos = Math.Min(input.SelectionStart, text.Length);
        var start = pos;
        while (start > 0 && !char.IsWhiteSpace(text[start - 1])) start--;
        return pos > start ? (start, pos - start) : null;
    }

    private void UpdateCompletion()
    {
        if (TokenRange() is not { } r)
        {
            HideCompletion();
            return;
        }
        var token = input.Text.Substring(r.Start, r.Length);
        List<Completion> items;
        if (token.StartsWith('/') && r.Start == 0)
        {
            completionKind = "slash";
            var q = token[1..].ToLowerInvariant();
            var builtin = new List<ChatCommand> { new("compact", "Compact the conversation"), new("clear", "Start a new session") };
            items = builtin.Concat(chat.Commands.Where(c => c.Name is not ("compact" or "clear")))
                .Where(c => q.Length == 0 || c.Name.Contains(q, StringComparison.OrdinalIgnoreCase))
                .OrderBy(c => c.Name.StartsWith(q, StringComparison.OrdinalIgnoreCase) ? 0 : 1).ThenBy(c => c.Name)
                .Take(12)
                .Select(c => new Completion("/" + c.Name, c.Description, "/" + c.Name + " "))
                .ToList();
        }
        else if (token.StartsWith('@'))
        {
            completionKind = "file";
            if (files.Count == 0 && !filesLoading) LoadFiles().Forget();
            items = RepoCodeViewModel.FuzzyMatch(token[1..], files, 12).Select(p => new Completion(p.Contains('/') ? p[(p.LastIndexOf('/') + 1)..] : p, p, "@" + p + " ")).ToList();
        }
        else
        {
            HideCompletion();
            return;
        }
        completions.ItemsSource = items;
        completions.Visibility = items.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
        if (items.Count > 0) completions.SelectedIndex = 0;
    }

    private void HideCompletion()
    {
        completionKind = null;
        completions.Visibility = Visibility.Collapsed;
    }

    private void Accept(Completion item)
    {
        if (TokenRange() is not { } r) return;
        if (item.Insert == "/clear ")
        {
            input.Text = "";
            HideCompletion();
            AppViewModel.Shared.Claude.NewSession(tab.Folder, kind: ClaudeTabKind.Chat);
            return;
        }
        var text = input.Text;
        input.Text = text[..r.Start] + item.Insert + text[(r.Start + r.Length)..];
        input.SelectionStart = r.Start + item.Insert.Length;
        HideCompletion();
        input.Focus(FocusState.Programmatic);
    }

    private async Task LoadFiles()
    {
        filesLoading = true;
        var folder = tab.Folder;
        var list = await Task.Run(async () =>
        {
            var r = await GitCli.Run(new[] { "ls-files", "--cached", "--others", "--exclude-standard" }, folder);
            var result = r.Out.Split('\n', StringSplitOptions.RemoveEmptyEntries).Select(s => s.TrimEnd('\r')).ToList();
            if (result.Count == 0 && Directory.Exists(folder))
            {
                foreach (var f in Directory.EnumerateFiles(folder, "*", new EnumerationOptions { RecurseSubdirectories = true, IgnoreInaccessible = true }))
                {
                    var rel = Path.GetRelativePath(folder, f).Replace('\\', '/');
                    if (rel.StartsWith(".git/") || rel.Contains("/node_modules/") || rel.Contains("/bin/") || rel.Contains("/obj/")) continue;
                    result.Add(rel);
                    if (result.Count >= 5000) break;
                }
            }
            return result.Take(50_000).ToList();
        });
        files = list;
        filesLoading = false;
        UpdateCompletion();
    }

    private void RenderImages()
    {
        images.Children.Clear();
        foreach (var img in attached)
        {
            var bmp = new BitmapImage();
            SetImage(bmp, img.Data).Forget();
            var picture = new Image { Source = bmp, Width = 52, Height = 52, Stretch = Stretch.UniformToFill };
            var remove = Ui.IconButton("", L.T("Chat_RemoveImage", "Remove"), () =>
            {
                attached.Remove(img);
                RenderImages();
            });
            remove.HorizontalAlignment = HorizontalAlignment.Right;
            remove.VerticalAlignment = VerticalAlignment.Top;
            remove.Padding = new Thickness(2);
            var grid = new Grid { Width = 52, Height = 52, CornerRadius = new CornerRadius(6) };
            grid.Children.Add(picture);
            grid.Children.Add(remove);
            images.Children.Add(grid);
        }
        images.Visibility = attached.Count > 0 ? Visibility.Visible : Visibility.Collapsed;
    }

    private static async Task SetImage(BitmapImage bmp, byte[] data)
    {
        try
        {
            using var stream = new InMemoryRandomAccessStream();
            await stream.WriteAsync(data.AsBuffer());
            stream.Seek(0);
            await bmp.SetSourceAsync(stream);
        }
        catch
        {
        }
    }

    private async void OnPaste(object sender, TextControlPasteEventArgs e)
    {
        try
        {
            var content = Clipboard.GetContent();
            if (content.Contains(StandardDataFormats.Bitmap) && !content.Contains(StandardDataFormats.Text))
            {
                e.Handled = true;
                var reference = await content.GetBitmapAsync();
                using var stream = await reference.OpenReadAsync();
                if (await ToPng(stream) is { } png)
                {
                    attached.Add(new ChatImage { Data = png, MediaType = "image/png" });
                    RenderImages();
                }
                return;
            }
            if (content.Contains(StandardDataFormats.StorageItems))
            {
                var items = await content.GetStorageItemsAsync();
                if (items.OfType<StorageFile>().Any(IsImage))
                {
                    e.Handled = true;
                    await AddFiles(items.OfType<StorageFile>());
                }
            }
        }
        catch
        {
        }
    }

    private static bool IsImage(StorageFile f) => f.FileType.ToLowerInvariant() is ".png" or ".jpg" or ".jpeg" or ".gif" or ".webp";

    private static string MediaType(string ext) => ext.ToLowerInvariant() switch
    {
        ".png" => "image/png",
        ".gif" => "image/gif",
        ".webp" => "image/webp",
        _ => "image/jpeg"
    };

    private async Task AddFiles(IEnumerable<StorageFile> filesToAdd)
    {
        foreach (var f in filesToAdd.Where(IsImage))
        {
            var buffer = await FileIO.ReadBufferAsync(f);
            attached.Add(new ChatImage { Data = buffer.ToArray(), MediaType = MediaType(f.FileType) });
        }
        RenderImages();
    }

    private async Task DropItems(DataPackageView view)
    {
        try
        {
            if (view.Contains(StandardDataFormats.StorageItems))
            {
                var items = await view.GetStorageItemsAsync();
                await AddFiles(items.OfType<StorageFile>());
            }
            else if (view.Contains(StandardDataFormats.Bitmap))
            {
                var reference = await view.GetBitmapAsync();
                using var stream = await reference.OpenReadAsync();
                if (await ToPng(stream) is { } png)
                {
                    attached.Add(new ChatImage { Data = png, MediaType = "image/png" });
                    RenderImages();
                }
            }
        }
        catch
        {
        }
    }

    private static async Task<byte[]?> ToPng(IRandomAccessStream source)
    {
        try
        {
            var decoder = await BitmapDecoder.CreateAsync(source);
            using var bitmap = await decoder.GetSoftwareBitmapAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied);
            using var output = new InMemoryRandomAccessStream();
            var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, output);
            encoder.SetSoftwareBitmap(bitmap);
            await encoder.FlushAsync();
            var bytes = new byte[output.Size];
            output.Seek(0);
            await output.ReadAsync(bytes.AsBuffer(), (uint)bytes.Length, InputStreamOptions.None);
            return bytes;
        }
        catch
        {
            return null;
        }
    }

    private async Task PickImages()
    {
        if (App.MainWindow is null) return;
        foreach (var path in await WindowInterop.PickImages(App.MainWindow))
        {
            try
            {
                attached.Add(new ChatImage { Data = await File.ReadAllBytesAsync(path), MediaType = MediaType(Path.GetExtension(path)) });
            }
            catch
            {
            }
        }
        RenderImages();
    }
}
