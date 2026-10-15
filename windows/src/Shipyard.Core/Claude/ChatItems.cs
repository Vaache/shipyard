using System.Text.Json.Nodes;
using Shipyard.Core.Util;

namespace Shipyard.Core.Claude;

public abstract class ChatItem
{
    public string Id { get; protected init; } = "";
    public abstract JsonObject ToJson(string folder);
}

public sealed class ChatImage
{
    public byte[] Data { get; init; } = Array.Empty<byte>();
    public string MediaType { get; init; } = "image/png";
    public string DataUrl => $"data:{MediaType};base64,{Convert.ToBase64String(Data)}";
}

public sealed class UserItem : ChatItem
{
    public string Text { get; }
    public IReadOnlyList<ChatImage> Images { get; }

    public UserItem(string id, string text, IReadOnlyList<ChatImage> images)
    {
        Id = "u" + id;
        Text = text;
        Images = images;
    }

    public override JsonObject ToJson(string folder) => new()
    {
        ["id"] = Id,
        ["kind"] = "user",
        ["text"] = Text,
        ["images"] = new JsonArray(Images.Select(i => (JsonNode)JsonValue.Create(i.DataUrl)!).ToArray())
    };
}

public sealed class TextItem : ChatItem
{
    public string Text { get; set; } = "";
    public bool Finalized { get; set; }
    public bool Thinking { get; }

    public TextItem(string key, bool thinking, string text = "")
    {
        Id = (thinking ? "k" : "t") + key;
        Thinking = thinking;
        Text = text;
    }

    public override JsonObject ToJson(string folder) => new()
    {
        ["id"] = Id,
        ["kind"] = Thinking ? "thinking" : "text",
        ["text"] = Text
    };
}

public sealed class NoticeItem : ChatItem
{
    public string Text { get; }
    public bool IsError { get; }

    public NoticeItem(string id, string text, bool isError)
    {
        Id = "n" + id;
        Text = text;
        IsError = isError;
    }

    public override JsonObject ToJson(string folder) => new()
    {
        ["id"] = Id,
        ["kind"] = "notice",
        ["text"] = Text,
        ["error"] = IsError
    };
}

public sealed record PermissionSuggestion(JsonObject Raw, string Label);

public sealed class PermissionRequest
{
    public string Id { get; init; } = "";
    public string ToolName { get; init; } = "";
    public JsonObject Input { get; init; } = new();
    public string? Description { get; init; }
    public string? BlockedPath { get; init; }
    public IReadOnlyList<PermissionSuggestion> Suggestions { get; init; } = Array.Empty<PermissionSuggestion>();
    public string? ToolUseId { get; init; }
    public bool Resolved { get; set; }

    public string Question => ToolName switch
    {
        "Bash" => "Claude wants to run a command",
        "Edit" or "MultiEdit" => "Claude wants to edit a file",
        "Write" => "Claude wants to write a file",
        "WebFetch" => "Claude wants to fetch a URL",
        _ => $"Claude wants to use {ToolName}"
    };
}

public enum ToolStatus { Running, Done, Error, Denied }

public sealed class ToolItem : ChatItem
{
    public string ToolId { get; }
    public string Name { get; }
    public JsonObject Input { get; set; } = new();
    public string PartialInput { get; set; } = "";
    public ToolStatus Status { get; set; } = ToolStatus.Running;
    public string? ResultText { get; set; }
    public JsonObject? Structured { get; set; }
    public PermissionRequest? Permission { get; set; }
    public string? ParentToolUseId { get; set; }

    public ToolItem(string id, string name)
    {
        ToolId = id;
        Id = "c" + id;
        Name = name;
    }

    public string? Str(string key) => Input.Str(key);

    public static string Relative(string path, string folder)
    {
        if (path.Length == 0) return path;
        var f = folder.TrimEnd('\\', '/');
        if (path.StartsWith(f + "\\", StringComparison.OrdinalIgnoreCase) || path.StartsWith(f + "/", StringComparison.OrdinalIgnoreCase))
            return path[(f.Length + 1)..];
        return Fmt.HomeRelative(path);
    }

    public string Title
    {
        get
        {
            if (!Name.StartsWith("mcp__")) return Name;
            var parts = Name.Split('_', StringSplitOptions.RemoveEmptyEntries);
            return parts.Length > 0 ? parts[^1] : Name;
        }
    }

    public string Summary(string folder)
    {
        string S(string k) => Input.Str(k) ?? "";
        switch (Name)
        {
            case "Read" or "Edit" or "MultiEdit" or "Write" or "NotebookEdit":
                return Relative(Input.Str("file_path") ?? Input.Str("notebook_path") ?? "", folder);
            case "Bash":
                return Input.Str("description") ?? S("command");
            case "Grep":
                return $"\"{S("pattern")}\"" + (Input.Str("path") is { } p ? " · " + Relative(p, folder) : "");
            case "Glob":
                return S("pattern");
            case "WebFetch":
                return S("url");
            case "WebSearch":
                return S("query");
            case "Task" or "Agent":
                return S("description");
            case "Skill":
                return S("skill");
            default:
                foreach (var (_, v) in Input)
                    if (v is JsonValue jv && jv.TryGetValue<string>(out var s)) return s.Length > 120 ? s[..120] : s;
                return PartialInput.Length == 0 ? "" : "…";
        }
    }

    public string Icon => Name switch
    {
        "Read" => "📄",
        "Edit" or "MultiEdit" => "✏️",
        "Write" => "📝",
        "Bash" => "⌨️",
        "Grep" => "🔎",
        "Glob" => "🗂️",
        "WebFetch" => "🌐",
        "WebSearch" => "🔍",
        "Task" or "Agent" => "👥",
        "TodoWrite" => "☑️",
        "AskUserQuestion" => "❓",
        "ExitPlanMode" => "📋",
        "NotebookEdit" => "📓",
        "Skill" => "✨",
        _ => Name.StartsWith("mcp__") ? "🧩" : "🔧"
    };

    private static string Truncate(string? s, int max = 20_000) => s is null ? "" : s.Length > max ? s[..max] : s;

    public override JsonObject ToJson(string folder)
    {
        var o = new JsonObject
        {
            ["id"] = Id,
            ["kind"] = "tool",
            ["name"] = Name,
            ["title"] = Title,
            ["icon"] = Icon,
            ["summary"] = Summary(folder),
            ["status"] = Status.ToString().ToLowerInvariant(),
            ["input"] = Input.DeepClone(),
            ["result"] = Truncate(ResultText)
        };
        if (Input.Str("file_path") is { } fp) o["path"] = fp;
        if (Structured is { } s)
        {
            var st = new JsonObject();
            if (s["structuredPatch"] is JsonArray patch) st["structuredPatch"] = patch.DeepClone();
            if (s.Str("stdout") is { } so) st["stdout"] = Truncate(so);
            if (s.Str("stderr") is { } se) st["stderr"] = Truncate(se);
            if (s.Obj("file").Str("content") is { } content) st["fileContent"] = Truncate(content);
            o["structured"] = st;
        }
        if (Permission is { } p)
        {
            o["permission"] = new JsonObject
            {
                ["id"] = p.Id,
                ["toolName"] = p.ToolName,
                ["question"] = p.Question,
                ["description"] = p.Description,
                ["blockedPath"] = p.BlockedPath is null ? null : Relative(p.BlockedPath, folder),
                ["resolved"] = p.Resolved,
                ["suggestions"] = new JsonArray(p.Suggestions.Select(x => (JsonNode)JsonValue.Create(x.Label)!).ToArray()),
                ["input"] = p.Input.DeepClone()
            };
        }
        return o;
    }
}

public sealed record ChatModelOption(string Value, string DisplayName, string Description, IReadOnlyList<string> Efforts);

public sealed record ContextCategory(string Name, int Tokens, string Color, string Kind);

public sealed record ContextUsage(int Total, int Max, int Percentage, int? AutoCompactThreshold, bool AutoCompactEnabled, IReadOnlyList<ContextCategory> Categories);

public sealed record ChatCommand(string Name, string Description);
