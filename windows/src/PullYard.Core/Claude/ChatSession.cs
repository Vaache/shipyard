using System.Diagnostics;
using System.Text;
using System.Text.Json.Nodes;
using CommunityToolkit.Mvvm.ComponentModel;
using PullYard.Core.Util;

namespace PullYard.Core.Claude;

public sealed partial class ChatSession : ObservableObject
{
    public string Folder { get; }
    public string? ConfigDir { get; }

    [ObservableProperty] private string? sessionId;
    [ObservableProperty] private bool running;
    [ObservableProperty] private bool busy;
    [ObservableProperty] private string? status;
    [ObservableProperty] private string model = "";
    [ObservableProperty] private string modelValue = "default";
    [ObservableProperty] private string permissionMode = "default";
    [ObservableProperty] private string effort = "";
    [ObservableProperty] private List<ChatModelOption> models = new();
    [ObservableProperty] private List<ChatCommand> commands = new();
    [ObservableProperty] private double totalCost;
    [ObservableProperty] private int? contextPercent;
    [ObservableProperty] private ContextUsage? context;
    [ObservableProperty] private string? lastError;
    [ObservableProperty] private DateTimeOffset? turnStartedAt;
    [ObservableProperty] private bool sleeping;
    [ObservableProperty] private long memoryBytes;
    [ObservableProperty] private int pendingCount;

    public List<ChatItem> Items { get; } = new();
    public List<PermissionRequest> PendingPermissions { get; } = new();
    public DateTimeOffset LastActivity { get; private set; } = DateTimeOffset.Now;

    private readonly SynchronizationContext? uiContext;
    private Process? process;
    private readonly StringBuilder stderrTail = new();
    private readonly Dictionary<string, ToolItem> tools = new();
    private readonly Dictionary<string, TextItem> streamTexts = new();
    private readonly Dictionary<string, string> streamTools = new();
    private readonly Dictionary<string, List<TextItem>> messageTexts = new();
    private readonly HashSet<string> dirty = new();
    private bool resetPending = true;
    private string? queuedPrompt;
    private int requestCounter;
    private bool intentionalStop;

    public event Action<string>? OnSessionId;
    public event Action? OnTurnFinished;
    public event Action? ItemsChanged;
    public event Action? AttentionRequested;

    public ChatSession(string folder, string? configDir)
    {
        Folder = folder;
        ConfigDir = configDir;
        uiContext = SynchronizationContext.Current;
    }

    public int Pid
    {
        get
        {
            try
            {
                return process is { HasExited: false } p ? p.Id : 0;
            }
            catch
            {
                return 0;
            }
        }
    }

    public bool CanSleep => Running && !Busy && PendingCount == 0;

    private void Ui(Action a)
    {
        if (uiContext is null || SynchronizationContext.Current == uiContext) a();
        else uiContext.Post(_ => a(), null);
    }

    private void Touch(ChatItem item)
    {
        dirty.Add(item.Id);
        ItemsChanged?.Invoke();
    }

    private void Append(ChatItem item)
    {
        Items.Add(item);
        Touch(item);
    }

    public (bool Reset, List<ChatItem> Changed) TakeChanges()
    {
        if (resetPending)
        {
            resetPending = false;
            dirty.Clear();
            return (true, Items.ToList());
        }
        var changed = Items.Where(i => dirty.Contains(i.Id)).ToList();
        dirty.Clear();
        return (false, changed);
    }

    public void MarkAllDirty()
    {
        resetPending = true;
        ItemsChanged?.Invoke();
    }

    private void UpdatePending()
    {
        PendingPermissions.RemoveAll(p => p.Resolved);
        PendingCount = PendingPermissions.Count(p => !p.Resolved);
    }

    public void Sleep()
    {
        if (!CanSleep || process is null) return;
        intentionalStop = true;
        Kill();
    }

    public void Wake()
    {
        if (Running || string.IsNullOrEmpty(SessionId)) return;
        Start(SessionId, false, ModelValue is "default" or "" ? null : ModelValue, Effort.Length == 0 ? null : Effort, PermissionMode == "default" ? null : PermissionMode);
    }

    public void Start(string? resume, bool fork, string? model, string? effort, string? mode, string? initialPrompt = null)
    {
        if (process is not null) return;
        if (ClaudeLauncher.Command is not { } cmd)
        {
            LastError = "claude CLI not found";
            return;
        }
        var args = new List<string>
        {
            "-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
            "--include-partial-messages", "--permission-prompts", "host", "--permission-prompt-tool", "stdio"
        };
        if (!string.IsNullOrEmpty(model))
        {
            args.AddRange(new[] { "--model", model });
            ModelValue = model;
        }
        if (!string.IsNullOrEmpty(effort))
        {
            args.AddRange(new[] { "--effort", effort });
            Effort = effort;
        }
        if (!string.IsNullOrEmpty(mode) && mode != "default")
        {
            args.AddRange(new[] { "--permission-mode", mode });
            PermissionMode = mode;
        }
        if (resume is not null)
        {
            args.AddRange(new[] { "--resume", resume });
            if (fork) args.Add("--fork-session");
            else SessionId = resume;
        }
        var psi = new ProcessStartInfo(cmd.File)
        {
            UseShellExecute = false,
            RedirectStandardInput = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            CreateNoWindow = true,
            WorkingDirectory = Folder,
            StandardOutputEncoding = new UTF8Encoding(false),
            StandardErrorEncoding = new UTF8Encoding(false),
            StandardInputEncoding = new UTF8Encoding(false)
        };
        foreach (var a in cmd.PrefixArgs.Concat(args)) psi.ArgumentList.Add(a);
        foreach (var (k, v) in ClaudeLauncher.ProcessEnvironment(ConfigDir))
        {
            if (v is null) psi.Environment.Remove(k);
            else psi.Environment[k] = v;
        }
        var p = new Process { StartInfo = psi, EnableRaisingEvents = true };
        p.OutputDataReceived += (_, e) =>
        {
            if (e.Data is null) return;
            var line = e.Data;
            Ui(() => Consume(line));
        };
        p.ErrorDataReceived += (_, e) =>
        {
            if (e.Data is null) return;
            var line = e.Data;
            Ui(() =>
            {
                stderrTail.AppendLine(line);
                if (stderrTail.Length > 2000) stderrTail.Remove(0, stderrTail.Length - 2000);
            });
        };
        p.Exited += (_, _) =>
        {
            int code;
            try
            {
                code = p.ExitCode;
            }
            catch
            {
                code = -1;
            }
            Ui(() => Terminated(p, code));
        };
        try
        {
            p.Start();
            p.BeginOutputReadLine();
            p.BeginErrorReadLine();
        }
        catch (Exception e)
        {
            LastError = e.Message;
            return;
        }
        process = p;
        Running = true;
        Sleeping = false;
        intentionalStop = false;
        LastActivity = DateTimeOffset.Now;
        LastError = null;
        Control("initialize", new JsonObject());
        if (!string.IsNullOrEmpty(initialPrompt)) Send(initialPrompt);
    }

    public void Stop()
    {
        intentionalStop = true;
        Kill();
    }

    private void Kill()
    {
        try
        {
            process?.Kill(true);
        }
        catch
        {
        }
    }

    private void Terminated(Process p, int code)
    {
        if (!ReferenceEquals(p, process)) return;
        var wasIntentional = intentionalStop;
        intentionalStop = false;
        Running = false;
        Busy = false;
        process = null;
        MemoryBytes = 0;
        p.Dispose();
        if (wasIntentional)
        {
            Sleeping = SessionId is not null;
            PendingPermissions.Clear();
            UpdatePending();
            return;
        }
        foreach (var r in PendingPermissions) r.Resolved = true;
        PendingPermissions.Clear();
        UpdatePending();
        if (code != 0 && code != 15 && code != -1)
        {
            var tail = stderrTail.ToString().Trim();
            LastError = $"Claude exited with code {code}" + (tail.Length == 0 ? "" : "\n" + (tail.Length > 600 ? tail[^600..] : tail));
        }
    }

    private void Write(JsonObject obj)
    {
        if (process is null) return;
        try
        {
            process.StandardInput.Write(obj.ToJsonString() + "\n");
            process.StandardInput.Flush();
        }
        catch
        {
        }
    }

    private void Control(string subtype, JsonObject request)
    {
        requestCounter++;
        request["subtype"] = subtype;
        Write(new JsonObject
        {
            ["type"] = "control_request",
            ["request_id"] = $"gl-{requestCounter}-{subtype}",
            ["request"] = request
        });
    }

    public void Send(string text, IReadOnlyList<ChatImage>? images = null)
    {
        images ??= Array.Empty<ChatImage>();
        var trimmed = text.Trim();
        if (trimmed.Length == 0 && images.Count == 0) return;
        if (!Running)
        {
            queuedPrompt = text;
            return;
        }
        LastActivity = DateTimeOffset.Now;
        Append(new UserItem(Guid.NewGuid().ToString(), trimmed, images));
        var content = new JsonArray();
        foreach (var img in images)
            content.Add(new JsonObject
            {
                ["type"] = "image",
                ["source"] = new JsonObject { ["type"] = "base64", ["media_type"] = img.MediaType, ["data"] = Convert.ToBase64String(img.Data) }
            });
        if (trimmed.Length > 0) content.Add(new JsonObject { ["type"] = "text", ["text"] = trimmed });
        Write(new JsonObject { ["type"] = "user", ["message"] = new JsonObject { ["role"] = "user", ["content"] = content } });
        Busy = true;
        Status = null;
        TurnStartedAt = DateTimeOffset.Now;
    }

    public void Interrupt() => Control("interrupt", new JsonObject());

    public void SetModel(string value)
    {
        ModelValue = value;
        Control("set_model", new JsonObject { ["model"] = value });
    }

    public void SetPermissionMode(string mode)
    {
        PermissionMode = mode;
        Control("set_permission_mode", new JsonObject { ["mode"] = mode });
    }

    public void SetEffort(string level)
    {
        Effort = level;
        Control("apply_flag_settings", new JsonObject { ["settings"] = new JsonObject { ["effortLevel"] = level.Length == 0 ? null : level } });
    }

    public PermissionRequest? FindPermission(string id) => PendingPermissions.FirstOrDefault(p => p.Id == id) ?? tools.Values.Select(t => t.Permission).FirstOrDefault(p => p?.Id == id);

    public void Respond(PermissionRequest req, bool allow, PermissionSuggestion? always = null, string? message = null, JsonObject? updatedInput = null)
    {
        if (req.Resolved) return;
        req.Resolved = true;
        LastActivity = DateTimeOffset.Now;
        JsonObject response;
        if (allow)
        {
            response = new JsonObject { ["behavior"] = "allow", ["updatedInput"] = (updatedInput ?? req.Input).DeepClone() };
            if (always is not null) response["updatedPermissions"] = new JsonArray(always.Raw.DeepClone());
        }
        else
        {
            response = new JsonObject { ["behavior"] = "deny", ["message"] = string.IsNullOrEmpty(message) ? "The user declined this action" : message };
        }
        Write(new JsonObject
        {
            ["type"] = "control_response",
            ["response"] = new JsonObject { ["subtype"] = "success", ["request_id"] = req.Id, ["response"] = response }
        });
        UpdatePending();
        if (req.ToolUseId is { } id && tools.TryGetValue(id, out var tool))
        {
            if (!allow) tool.Status = ToolStatus.Denied;
            Touch(tool);
        }
        else
        {
            var owner = tools.Values.FirstOrDefault(t => ReferenceEquals(t.Permission, req));
            if (owner is not null) Touch(owner);
        }
    }

    internal void Consume(string line)
    {
        if (string.IsNullOrWhiteSpace(line)) return;
        JsonObject? obj;
        try
        {
            obj = JsonNode.Parse(line) as JsonObject;
        }
        catch
        {
            return;
        }
        if (obj is null) return;
        try
        {
            Handle(obj);
        }
        catch (Exception e)
        {
            ErrorLog.Write(e, $"chat message handling failed for line: {(line.Length > 2000 ? line[..2000] : line)}");
            Busy = false;
            LastError = e.Message;
        }
    }

    private void Handle(JsonObject o)
    {
        LastActivity = DateTimeOffset.Now;
        switch (o.Str("type"))
        {
            case "system": HandleSystem(o); break;
            case "stream_event": HandleStream(o); break;
            case "assistant": HandleAssistant(o); break;
            case "user": HandleUser(o); break;
            case "result": HandleResult(o); break;
            case "control_request": HandleControlRequest(o); break;
            case "rate_limit_event": HandleRateLimit(o); break;
            case "control_response": HandleControlResponse(o); break;
        }
    }

    private void HandleSystem(JsonObject o)
    {
        switch (o.Str("subtype"))
        {
            case "init":
                if (o.Str("session_id") is { } sid && sid != SessionId)
                {
                    SessionId = sid;
                    OnSessionId?.Invoke(sid);
                }
                if (o.Str("model") is { } m) Model = m;
                if (o.Str("permissionMode") is { } pm) PermissionMode = pm;
                if (queuedPrompt is { } q)
                {
                    queuedPrompt = null;
                    Send(q);
                }
                break;
            case "status":
                var s = o.Str("status");
                Status = s == "compacting" ? "Compacting context…" : s == "requesting" ? null : s;
                break;
            case "compact_boundary":
                Append(new NoticeItem(Guid.NewGuid().ToString(), "Context compacted", false));
                break;
            case "permission_denied":
                if (o.Str("tool_use_id") is { } id && tools.TryGetValue(id, out var tool))
                {
                    tool.Status = ToolStatus.Denied;
                    tool.ResultText = o.Str("message");
                    Touch(tool);
                }
                break;
        }
    }

    private static string StreamKey(JsonObject o, int index) => $"{o.Str("api_message_id") ?? "m"}:{index}";

    private void HandleStream(JsonObject o)
    {
        if (o.Obj("event") is not { } ev || ev.Str("type") is not { } type) return;
        var parent = o.Str("parent_tool_use_id");
        switch (type)
        {
            case "content_block_start":
            {
                if (ev.Int("index") is not { } index || ev.Obj("content_block") is not { } block) return;
                var key = StreamKey(o, index);
                switch (block.Str("type"))
                {
                    case "text":
                    {
                        if (parent is not null) return;
                        var t = new TextItem(key, false);
                        streamTexts[key] = t;
                        var mid = o.Str("api_message_id") ?? "m";
                        if (!messageTexts.TryGetValue(mid, out var list)) messageTexts[mid] = list = new();
                        list.Add(t);
                        Append(t);
                        break;
                    }
                    case "thinking":
                    {
                        if (parent is not null) return;
                        var t = new TextItem(key, true);
                        streamTexts[key] = t;
                        Append(t);
                        break;
                    }
                    case "tool_use":
                    {
                        if (block.Str("id") is not { } id || block.Str("name") is not { } name || tools.ContainsKey(id)) return;
                        var call = new ToolItem(id, name) { ParentToolUseId = parent };
                        tools[id] = call;
                        streamTools[key] = id;
                        if (parent is null) Append(call);
                        break;
                    }
                }
                break;
            }
            case "content_block_delta":
            {
                if (ev.Int("index") is not { } index || ev.Obj("delta") is not { } delta) return;
                var key = StreamKey(o, index);
                switch (delta.Str("type"))
                {
                    case "text_delta":
                        if (streamTexts.TryGetValue(key, out var t))
                        {
                            t.Text += delta.Str("text") ?? "";
                            Touch(t);
                        }
                        break;
                    case "thinking_delta":
                        if (streamTexts.TryGetValue(key, out var k))
                        {
                            k.Text += delta.Str("thinking") ?? "";
                            Touch(k);
                        }
                        break;
                    case "input_json_delta":
                        if (streamTools.TryGetValue(key, out var toolId) && tools.TryGetValue(toolId, out var tool))
                        {
                            tool.PartialInput += delta.Str("partial_json") ?? "";
                            if (tool.ParentToolUseId is null) Touch(tool);
                        }
                        break;
                }
                break;
            }
        }
    }

    private void HandleAssistant(JsonObject o)
    {
        if (o.Obj("message") is not { } msg || msg.Arr("content") is not { } content) return;
        var parent = o.Str("parent_tool_use_id");
        var mid = msg.Str("id") ?? "m";
        if (msg.Str("model") is { } m && parent is null) Model = m;
        foreach (var block in content.Objects())
        {
            switch (block.Str("type"))
            {
                case "text":
                {
                    if (parent is not null) continue;
                    var full = block.Str("text") ?? "";
                    var existing = messageTexts.TryGetValue(mid, out var list) ? list.FirstOrDefault(t => !t.Finalized) : null;
                    if (existing is not null)
                    {
                        existing.Text = full;
                        existing.Finalized = true;
                        Touch(existing);
                    }
                    else if (full.Length > 0)
                    {
                        var t = new TextItem(mid + ":" + Guid.NewGuid(), false, full) { Finalized = true };
                        if (!messageTexts.TryGetValue(mid, out var l2)) messageTexts[mid] = l2 = new();
                        l2.Add(t);
                        Append(t);
                    }
                    break;
                }
                case "tool_use":
                {
                    if (block.Str("id") is not { } id || block.Str("name") is not { } name) continue;
                    if (!tools.TryGetValue(id, out var call))
                    {
                        call = new ToolItem(id, name) { ParentToolUseId = parent };
                        tools[id] = call;
                        if (parent is null) Append(call);
                    }
                    call.Input = block.Obj("input")?.DeepClone() as JsonObject ?? new JsonObject();
                    call.PartialInput = "";
                    if (call.ParentToolUseId is null) Touch(call);
                    break;
                }
            }
        }
    }

    private void HandleUser(JsonObject o)
    {
        if (o.Obj("message") is not { } msg || msg.Arr("content") is not { } content) return;
        foreach (var block in content.Objects().Where(b => b.Str("type") == "tool_result"))
        {
            if (block.Str("tool_use_id") is not { } id || !tools.TryGetValue(id, out var tool)) continue;
            if (tool.Status != ToolStatus.Denied) tool.Status = block.Bool("is_error") == true ? ToolStatus.Error : ToolStatus.Done;
            tool.ResultText = ResultText(block["content"]);
            tool.Structured = o.Obj("tool_use_result")?.DeepClone() as JsonObject;
            if (tool.ParentToolUseId is null) Touch(tool);
        }
    }

    public static string? ResultText(JsonNode? content)
    {
        if (content is JsonValue v && v.TryGetValue<string>(out var s)) return s;
        if (content is JsonArray arr) return string.Join("\n", arr.Objects().Where(b => b.Str("type") == "text").Select(b => b.Str("text")));
        return null;
    }

    private void HandleResult(JsonObject o)
    {
        Busy = false;
        Status = null;
        if (o.Dbl("total_cost_usd") is { } c) TotalCost += c;
        foreach (var t in tools.Values.Where(t => t.Status == ToolStatus.Running && t.ParentToolUseId is null))
        {
            t.Status = ToolStatus.Done;
            Touch(t);
        }
        if (o.Bool("is_error") == true)
        {
            var sub = o.Str("subtype") ?? "";
            var text = sub == "error_during_execution" ? "Stopped" : o.Str("result") ?? sub;
            Append(new NoticeItem(Guid.NewGuid().ToString(), text, sub != "error_during_execution"));
        }
        Control("get_context_usage", new JsonObject());
        OnTurnFinished?.Invoke();
    }

    private void HandleControlRequest(JsonObject o)
    {
        if (o.Str("request_id") is not { } id || o.Obj("request") is not { } req) return;
        if (req.Str("subtype") != "can_use_tool")
        {
            Write(new JsonObject
            {
                ["type"] = "control_response",
                ["response"] = new JsonObject { ["subtype"] = "error", ["request_id"] = id, ["error"] = "Unsupported" }
            });
            return;
        }
        var suggestions = new List<PermissionSuggestion>();
        foreach (var s in req.Arr("permission_suggestions").Objects())
        {
            switch (s.Str("type"))
            {
                case "addRules":
                    var rules = s.Arr("rules").Objects().Select(r =>
                    {
                        var tool = r.Str("toolName") ?? "";
                        return r.Str("ruleContent") is { } rc ? $"{tool}({rc})" : tool;
                    });
                    suggestions.Add(new PermissionSuggestion((JsonObject)s.DeepClone(), string.Join(", ", rules)));
                    break;
                case "addDirectories":
                    var dirs = s.Arr("directories").Strings().Select(d => Path.GetFileName(d.TrimEnd('\\', '/')));
                    suggestions.Add(new PermissionSuggestion((JsonObject)s.DeepClone(), "access to " + string.Join(", ", dirs)));
                    break;
                case "setMode":
                    suggestions.Add(new PermissionSuggestion((JsonObject)s.DeepClone(), "mode " + (s.Str("mode") ?? "")));
                    break;
            }
        }
        var p = new PermissionRequest
        {
            Id = id,
            ToolName = req.Str("tool_name") ?? "?",
            Input = req.Obj("input")?.DeepClone() as JsonObject ?? new JsonObject(),
            Description = req.Str("description"),
            BlockedPath = req.Str("blocked_path"),
            Suggestions = suggestions,
            ToolUseId = req.Str("tool_use_id")
        };
        if (p.ToolUseId is { } tid && tools.TryGetValue(tid, out var existing))
        {
            existing.Permission = p;
            if (existing.Input.Count == 0) existing.Input = (JsonObject)p.Input.DeepClone();
            Touch(existing);
        }
        else
        {
            var call = new ToolItem(p.ToolUseId ?? id, p.ToolName) { Input = (JsonObject)p.Input.DeepClone(), Permission = p };
            tools[call.ToolId] = call;
            Append(call);
        }
        PendingPermissions.Add(p);
        UpdatePending();
        AttentionRequested?.Invoke();
    }

    private static void HandleRateLimit(JsonObject o)
    {
        if (o.Obj("rate_limit_info") is not { } info) return;
        var store = ClaudeUsageStore.Shared;
        if (info.Obj("unifiedWindows") is { } windows)
        {
            var order = new Dictionary<string, int> { ["five_hour"] = 0, ["seven_day"] = 1 };
            store.Windows = windows
                .Where(kv => kv.Value is JsonObject)
                .Select(kv =>
                {
                    var w = (JsonObject)kv.Value!;
                    DateTimeOffset? resets = w.Dbl("resetsAt") is { } r ? DateTimeOffset.FromUnixTimeSeconds((long)r) : null;
                    return new RateWindow(kv.Key, w.Dbl("utilization") ?? 0, resets);
                })
                .OrderBy(w => order.TryGetValue(w.Key, out var i) ? i : 9)
                .ThenBy(w => w.Key)
                .ToList();
        }
        var st = info.Str("status");
        store.OverageNote = st == "rejected" ? "Limit reached" : st == "allowed_warning" ? "Close to the limit" : null;
        store.UpdatedAt = DateTimeOffset.Now;
    }

    public void RefreshContext()
    {
        if (Running) Control("get_context_usage", new JsonObject());
    }

    public void Compact() => Send("/compact");

    private void HandleControlResponse(JsonObject o)
    {
        if (o.Obj("response") is not { } r || r.Str("request_id") is not { } rid || r.Obj("response") is not { } body) return;
        if (rid.EndsWith("-initialize"))
        {
            Models = body.Arr("models").Objects()
                .Where(m => m.Str("value") is not null)
                .Select(m => new ChatModelOption(m.Str("value")!, m.Str("displayName") ?? m.Str("value")!, m.Str("description") ?? "", m.Arr("supportedEffortLevels").Strings().ToList()))
                .ToList();
            if (body.Obj("account").Str("subscriptionType") is { } sub)
                ClaudeUsageStore.Shared.Subscription = sub.Replace("Claude ", "");
            Commands = body.Arr("commands").Objects()
                .Where(c => c.Str("name") is not null)
                .Select(c => new ChatCommand(c.Str("name")!, c.Str("description") ?? ""))
                .ToList();
        }
        else if (rid.EndsWith("-get_context_usage"))
        {
            var total = body.Int("totalTokens") ?? 0;
            var max = body.Int("maxTokens") ?? 0;
            var pct = body.Int("percentage") ?? (max > 0 ? total * 100 / max : 0);
            ContextPercent = pct;
            var cats = body.Arr("categories").Objects()
                .Where(c => c.Str("name") is not null)
                .Select(c => new ContextCategory(c.Str("name")!, c.Int("tokens") ?? 0, c.Str("color") ?? "", c.Str("kind") ?? "used"))
                .ToList();
            Context = new ContextUsage(total, max, pct, body.Int("autoCompactThreshold"), body.Bool("isAutoCompactEnabled") ?? true, cats);
        }
    }

    public void LoadHistory(string file, int limit = 600)
    {
        string[] lines;
        try
        {
            using var fs = new FileStream(file, FileMode.Open, FileAccess.Read, FileShare.ReadWrite | FileShare.Delete);
            using var reader = new StreamReader(fs, Encoding.UTF8);
            lines = reader.ReadToEnd().Split('\n');
        }
        catch
        {
            return;
        }
        var loaded = new List<ChatItem>();
        var map = new Dictionary<string, ToolItem>();
        foreach (var raw in lines)
        {
            if (string.IsNullOrWhiteSpace(raw)) continue;
            JsonObject? o;
            try
            {
                o = JsonNode.Parse(raw) as JsonObject;
            }
            catch
            {
                continue;
            }
            if (o is null || o.Bool("isSidechain") == true || o.Bool("isMeta") == true) continue;
            var type = o.Str("type");
            if (o.Obj("message") is not { } msg) continue;
            if (type == "user")
            {
                if (msg["content"] is JsonValue sv && sv.TryGetValue<string>(out var s))
                {
                    if (o.Bool("isCompactSummary") == true) continue;
                    var clean = s.Trim();
                    if (clean.StartsWith("<command-name>") || clean.StartsWith("<local-command") || clean.StartsWith("Caveat:") || clean.Length == 0) continue;
                    loaded.Add(new UserItem(o.Str("uuid") ?? Guid.NewGuid().ToString(), clean, Array.Empty<ChatImage>()));
                }
                else if (msg.Arr("content") is { } blocks)
                {
                    var texts = new List<string>();
                    foreach (var b in blocks.Objects())
                    {
                        if (b.Str("type") == "tool_result" && b.Str("tool_use_id") is { } id && map.TryGetValue(id, out var tool))
                        {
                            tool.Status = b.Bool("is_error") == true ? ToolStatus.Error : ToolStatus.Done;
                            tool.ResultText = ResultText(b["content"]);
                            tool.Structured = o.Obj("toolUseResult")?.DeepClone() as JsonObject;
                        }
                        else if (b.Str("type") == "text" && b.Str("text") is { } t && !t.StartsWith('<'))
                        {
                            texts.Add(t);
                        }
                    }
                    if (texts.Count > 0) loaded.Add(new UserItem(o.Str("uuid") ?? Guid.NewGuid().ToString(), string.Join("\n", texts), Array.Empty<ChatImage>()));
                }
            }
            else if (type == "assistant" && msg.Arr("content") is { } blocks)
            {
                var uuid = o.Str("uuid") ?? Guid.NewGuid().ToString();
                var i = 0;
                foreach (var b in blocks.Objects())
                {
                    switch (b.Str("type"))
                    {
                        case "text":
                            if (b.Str("text") is { Length: > 0 } s)
                                loaded.Add(new TextItem($"h{uuid}:{i}", false, s) { Finalized = true });
                            break;
                        case "tool_use":
                            if (b.Str("id") is { } id && b.Str("name") is { } name)
                            {
                                var c = new ToolItem(id, name) { Input = b.Obj("input")?.DeepClone() as JsonObject ?? new JsonObject(), Status = ToolStatus.Done };
                                map[id] = c;
                                loaded.Add(c);
                            }
                            break;
                    }
                    i++;
                }
            }
        }
        var tail = loaded.Skip(Math.Max(0, loaded.Count - limit)).ToList();
        foreach (var c in tail.OfType<ToolItem>()) tools[c.ToolId] = c;
        Items.Clear();
        if (loaded.Count > limit) Items.Add(new NoticeItem("older", $"Showing the last {limit} messages", false));
        Items.AddRange(tail);
        if (Items.Count > 0) Items.Add(new NoticeItem("resumed", "Session resumed", false));
        MarkAllDirty();
    }
}
