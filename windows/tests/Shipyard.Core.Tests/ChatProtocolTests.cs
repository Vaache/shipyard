using Shipyard.Core.Claude;
using Xunit;

namespace Shipyard.Core.Tests;

public class ChatProtocolTests
{
    [Fact]
    public void ParsesStreamJsonTurn()
    {
        var chat = new ChatSession("C:\\code\\proj", null);
        string? sid = null;
        chat.OnSessionId += s => sid = s;
        chat.Consume("""{"type":"system","subtype":"init","session_id":"abc","model":"claude-test","permissionMode":"default"}""");
        chat.Consume("""{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}""");
        chat.Consume("""{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hel"}}}""");
        chat.Consume("""{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"lo"}}}""");
        var text = Assert.IsType<TextItem>(chat.Items[0]);
        Assert.Equal("Hello", text.Text);
        chat.Consume("""{"type":"assistant","message":{"id":"m1","model":"claude-test","content":[{"type":"text","text":"Hello world"},{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"ls","description":"List files"}}]}}""");
        Assert.Equal("Hello world", text.Text);
        Assert.True(text.Finalized);
        var tool = Assert.IsType<ToolItem>(chat.Items[1]);
        Assert.Equal("List files", tool.Summary(chat.Folder));
        chat.Consume("""{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","tool_use_id":"t1","input":{"command":"ls"},"permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"ls:*"}]}]}}""");
        Assert.Equal(1, chat.PendingCount);
        Assert.Equal("Bash(ls:*)", tool.Permission!.Suggestions[0].Label);
        chat.Respond(tool.Permission, true);
        Assert.Equal(0, chat.PendingCount);
        chat.Consume("""{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"a.txt","is_error":false}]},"tool_use_result":{"stdout":"a.txt","stderr":""}}""");
        Assert.Equal(ToolStatus.Done, tool.Status);
        Assert.Equal("a.txt", tool.ResultText);
        chat.Consume("""{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.25}""");
        Assert.False(chat.Busy);
        Assert.Equal(0.25, chat.TotalCost, 3);
        Assert.Equal("abc", sid);
        var (reset, changed) = chat.TakeChanges();
        Assert.True(reset);
        Assert.Equal(2, changed.Count);
        Assert.Equal("tool", changed[1].ToJson(chat.Folder)["kind"]!.GetValue<string>());
    }

    [Fact]
    public void LoadsHistory()
    {
        var file = Path.GetTempFileName();
        File.WriteAllLines(file, new[]
        {
            """{"type":"user","uuid":"u1","message":{"role":"user","content":"Fix it"}}""",
            """{"type":"assistant","uuid":"a1","message":{"content":[{"type":"text","text":"Done"},{"type":"tool_use","id":"t1","name":"Edit","input":{"file_path":"C:\\code\\proj\\a.cs","old_string":"x","new_string":"y"}}]}}""",
            """{"type":"user","uuid":"u2","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}}"""
        });
        var chat = new ChatSession("C:\\code\\proj", null);
        chat.LoadHistory(file);
        File.Delete(file);
        Assert.Equal(4, chat.Items.Count);
        var tool = Assert.IsType<ToolItem>(chat.Items[2]);
        Assert.Equal("a.cs", tool.Summary("C:\\code\\proj"));
        Assert.Equal("ok", tool.ResultText);
    }
}
