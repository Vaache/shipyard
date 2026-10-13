import AppKit
import Foundation

@Observable
final class ChatText: Identifiable {
    let id: String
    var text: String
    var finalized = false

    init(id: String, text: String) {
        self.id = id
        self.text = text
    }
}

struct PermissionSuggestion {
    let raw: [String: Any]
    let label: String
}

@Observable
final class PermissionRequest: Identifiable {
    let id: String
    let toolName: String
    let input: [String: Any]
    let description: String?
    let blockedPath: String?
    let suggestions: [PermissionSuggestion]
    let toolUseId: String?
    var resolved = false

    init(id: String, toolName: String, input: [String: Any], description: String?, blockedPath: String?, suggestions: [PermissionSuggestion], toolUseId: String?) {
        self.id = id
        self.toolName = toolName
        self.input = input
        self.description = description
        self.blockedPath = blockedPath
        self.suggestions = suggestions
        self.toolUseId = toolUseId
    }
}

@Observable
final class ChatToolCall: Identifiable {
    enum Status { case running, done, error, denied }

    let id: String
    let name: String
    var input: [String: Any] = [:]
    var partialInput = ""
    var status: Status = .running
    var resultText: String?
    var structured: [String: Any]?
    var permission: PermissionRequest?
    var parentToolUseId: String?

    init(id: String, name: String) {
        self.id = id
        self.name = name
    }

    func string(_ key: String) -> String? { input[key] as? String }
}

struct ChatImage: Identifiable {
    let id = UUID()
    let data: Data
    let mediaType: String
    var image: NSImage? { NSImage(data: data) }
}

enum ChatItem: Identifiable {
    case user(id: String, text: String, images: [ChatImage])
    case text(ChatText)
    case thinking(ChatText)
    case tool(ChatToolCall)
    case notice(id: String, text: String, isError: Bool)

    var id: String {
        switch self {
        case .user(let id, _, _): return "u" + id
        case .text(let t): return "t" + t.id
        case .thinking(let t): return "k" + t.id
        case .tool(let c): return "c" + c.id
        case .notice(let id, _, _): return "n" + id
        }
    }
}

struct ChatModelOption: Identifiable, Hashable {
    let value: String
    let displayName: String
    let description: String
    let efforts: [String]
    var id: String { value }
}

struct ChatCommand: Identifiable, Hashable {
    let name: String
    let description: String
    var id: String { name }
}

@Observable
@MainActor
final class ChatSession {
    let folder: String
    var sessionId: String?
    var items: [ChatItem] = []
    var running = false
    var busy = false
    var status: String?
    var model: String = ""
    var modelValue: String = "default"
    var permissionMode: String = "default"
    var effort: String = ""
    var models: [ChatModelOption] = []
    var commands: [ChatCommand] = []
    var pendingPermissions: [PermissionRequest] = []
    var totalCost: Double = 0
    var contextPercent: Int?
    var lastError: String?
    var turnStartedAt: Date?

    @ObservationIgnored private var process: Process?
    @ObservationIgnored private var stdin: FileHandle?
    @ObservationIgnored private var buffer = Data()
    @ObservationIgnored private var stderrTail = ""
    @ObservationIgnored private var tools: [String: ChatToolCall] = [:]
    @ObservationIgnored private var streamTexts: [String: ChatText] = [:]
    @ObservationIgnored private var messageTexts: [String: [ChatText]] = [:]
    @ObservationIgnored private var queuedPrompt: String?
    @ObservationIgnored private var requestCounter = 0
    @ObservationIgnored var onSessionId: ((String) -> Void)?
    @ObservationIgnored var onTurnFinished: (() -> Void)?

    init(folder: String) {
        self.folder = folder
    }

    var pendingCount: Int { pendingPermissions.filter { !$0.resolved }.count }

    func start(resume: String? = nil, fork: Bool = false, model: String?, effort: String?, mode: String?, initialPrompt: String? = nil) {
        guard process == nil, let bin = ClaudeLauncher.claudeBinary else {
            if ClaudeLauncher.claudeBinary == nil { lastError = t("claude CLI не найден", "claude CLI not found") }
            return
        }
        var args = ["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose",
                    "--include-partial-messages", "--permission-prompts", "host", "--permission-prompt-tool", "stdio"]
        if let model, !model.isEmpty { args += ["--model", model]; modelValue = model }
        if let effort, !effort.isEmpty { args += ["--effort", effort]; self.effort = effort }
        if let mode, !mode.isEmpty, mode != "default" { args += ["--permission-mode", mode]; permissionMode = mode }
        if let resume {
            args += ["--resume", resume]
            if fork { args.append("--fork-session") } else { sessionId = resume }
        }

        let p = Process()
        p.executableURL = URL(fileURLWithPath: bin)
        p.arguments = args
        p.currentDirectoryURL = URL(fileURLWithPath: folder)
        var env = ProcessInfo.processInfo.environment
        for key in env.keys where key == "GHLITE_TOKEN" || key == "CLAUDECODE" || key.hasPrefix("CLAUDE_CODE_") { env.removeValue(forKey: key) }
        env["PATH"] = [(bin as NSString).deletingLastPathComponent, "/opt/homebrew/bin", "/usr/local/bin", env["PATH"] ?? "/usr/bin:/bin"].joined(separator: ":")
        if let browser = LinkBrowser.helperScript() { env["BROWSER"] = browser }
        p.environment = env

        let inPipe = Pipe(), outPipe = Pipe(), errPipe = Pipe()
        p.standardInput = inPipe
        p.standardOutput = outPipe
        p.standardError = errPipe
        outPipe.fileHandleForReading.readabilityHandler = { [weak self] h in
            let data = h.availableData
            guard !data.isEmpty else { return }
            DispatchQueue.main.async { self?.consume(data) }
        }
        errPipe.fileHandleForReading.readabilityHandler = { [weak self] h in
            let data = h.availableData
            guard !data.isEmpty else { return }
            let s = String(decoding: data, as: UTF8.self)
            DispatchQueue.main.async {
                guard let self else { return }
                self.stderrTail = String((self.stderrTail + s).suffix(2000))
            }
        }
        p.terminationHandler = { [weak self] proc in
            DispatchQueue.main.async { self?.terminated(code: proc.terminationStatus) }
        }
        do {
            try p.run()
        } catch {
            lastError = error.localizedDescription
            return
        }
        process = p
        stdin = inPipe.fileHandleForWriting
        running = true
        lastError = nil
        control("initialize", [:])
        if let initialPrompt, !initialPrompt.isEmpty { send(initialPrompt) }
    }

    func stop() {
        process?.terminate()
    }

    private func terminated(code: Int32) {
        running = false
        busy = false
        process = nil
        stdin = nil
        for p in pendingPermissions where !p.resolved { p.resolved = true }
        pendingPermissions.removeAll()
        if code != 0 && code != 15 {
            let tail = stderrTail.trimmingCharacters(in: .whitespacesAndNewlines)
            lastError = t("Claude завершился с кодом \(code)", "Claude exited with code \(code)") + (tail.isEmpty ? "" : "\n" + String(tail.suffix(600)))
        }
    }

    private func write(_ obj: [String: Any]) {
        guard let stdin, let data = try? JSONSerialization.data(withJSONObject: obj) else { return }
        var line = data
        line.append(0x0A)
        try? stdin.write(contentsOf: line)
    }

    private func control(_ subtype: String, _ extra: [String: Any]) {
        requestCounter += 1
        var req = extra
        req["subtype"] = subtype
        write(["type": "control_request", "request_id": "gl-\(requestCounter)-\(subtype)", "request": req])
    }

    func send(_ text: String, images: [ChatImage] = []) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty || !images.isEmpty else { return }
        guard running else {
            queuedPrompt = text
            return
        }
        items.append(.user(id: UUID().uuidString, text: trimmed, images: images))
        var content: [[String: Any]] = images.map {
            ["type": "image", "source": ["type": "base64", "media_type": $0.mediaType, "data": $0.data.base64EncodedString()]]
        }
        if !trimmed.isEmpty { content.append(["type": "text", "text": trimmed]) }
        write(["type": "user", "message": ["role": "user", "content": content]])
        busy = true
        status = nil
        turnStartedAt = Date()
    }

    func interrupt() {
        control("interrupt", [:])
    }

    func setModel(_ value: String) {
        modelValue = value
        control("set_model", ["model": value])
    }

    func setPermissionMode(_ mode: String) {
        permissionMode = mode
        control("set_permission_mode", ["mode": mode])
    }

    func setEffort(_ level: String) {
        effort = level
        control("apply_flag_settings", ["settings": ["effortLevel": level.isEmpty ? NSNull() as Any : level as Any]])
    }

    func respond(_ req: PermissionRequest, allow: Bool, always: PermissionSuggestion? = nil, message: String? = nil, updatedInput: [String: Any]? = nil) {
        guard !req.resolved else { return }
        req.resolved = true
        var response: [String: Any]
        if allow {
            response = ["behavior": "allow", "updatedInput": updatedInput ?? req.input]
            if let always { response["updatedPermissions"] = [always.raw] }
        } else {
            response = ["behavior": "deny", "message": message?.isEmpty == false ? message! : t("Пользователь отклонил", "The user declined this action")]
        }
        write(["type": "control_response", "response": ["subtype": "success", "request_id": req.id, "response": response]])
        pendingPermissions.removeAll { $0.resolved }
        if !allow, let id = req.toolUseId, let tool = tools[id] { tool.status = .denied }
    }

    private func consume(_ data: Data) {
        buffer.append(data)
        while let nl = buffer.firstIndex(of: 0x0A) {
            let line = buffer[buffer.startIndex..<nl]
            buffer.removeSubrange(buffer.startIndex...nl)
            guard !line.isEmpty, let obj = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any] else { continue }
            handle(obj)
        }
    }

    private func handle(_ o: [String: Any]) {
        switch o["type"] as? String {
        case "system": handleSystem(o)
        case "stream_event": handleStream(o)
        case "assistant": handleAssistant(o)
        case "user": handleUser(o)
        case "result": handleResult(o)
        case "control_request": handleControlRequest(o)
        case "control_response": handleControlResponse(o)
        default: break
        }
    }

    private func handleSystem(_ o: [String: Any]) {
        switch o["subtype"] as? String {
        case "init":
            if let sid = o["session_id"] as? String, sid != sessionId {
                sessionId = sid
                onSessionId?(sid)
            }
            if let m = o["model"] as? String { model = m }
            if let pm = o["permissionMode"] as? String { permissionMode = pm }
            if let q = queuedPrompt {
                queuedPrompt = nil
                send(q)
            }
        case "status":
            let s = o["status"] as? String
            status = s == "compacting" ? t("Сжатие контекста…", "Compacting context…") : (s == "requesting" ? nil : s)
        case "compact_boundary":
            items.append(.notice(id: UUID().uuidString, text: t("Контекст сжат", "Context compacted"), isError: false))
        case "permission_denied":
            if let id = o["tool_use_id"] as? String, let tool = tools[id] {
                tool.status = .denied
                tool.resultText = o["message"] as? String
            }
        default:
            break
        }
    }

    private func streamKey(_ o: [String: Any], index: Int) -> String {
        let mid = (o["api_message_id"] as? String) ?? "m"
        return "\(mid):\(index)"
    }

    private func handleStream(_ o: [String: Any]) {
        guard let ev = o["event"] as? [String: Any], let type = ev["type"] as? String else { return }
        let parent = o["parent_tool_use_id"] as? String
        switch type {
        case "content_block_start":
            guard let index = ev["index"] as? Int, let block = ev["content_block"] as? [String: Any] else { return }
            let key = streamKey(o, index: index)
            switch block["type"] as? String {
            case "text":
                guard parent == nil else { return }
                let t = ChatText(id: key, text: "")
                streamTexts[key] = t
                messageTexts[(o["api_message_id"] as? String) ?? "m", default: []].append(t)
                items.append(.text(t))
            case "thinking":
                guard parent == nil else { return }
                let t = ChatText(id: key, text: "")
                streamTexts[key] = t
                items.append(.thinking(t))
            case "tool_use":
                guard let id = block["id"] as? String, let name = block["name"] as? String, tools[id] == nil else { return }
                let call = ChatToolCall(id: id, name: name)
                call.parentToolUseId = parent
                tools[id] = call
                streamTexts[key] = ChatText(id: "tool:" + id, text: "")
                if parent == nil { items.append(.tool(call)) }
            default:
                break
            }
        case "content_block_delta":
            guard let index = ev["index"] as? Int, let delta = ev["delta"] as? [String: Any] else { return }
            let key = streamKey(o, index: index)
            switch delta["type"] as? String {
            case "text_delta": streamTexts[key]?.text += delta["text"] as? String ?? ""
            case "thinking_delta": streamTexts[key]?.text += delta["thinking"] as? String ?? ""
            case "input_json_delta":
                if let holder = streamTexts[key], holder.id.hasPrefix("tool:") {
                    let id = String(holder.id.dropFirst(5))
                    tools[id]?.partialInput += delta["partial_json"] as? String ?? ""
                }
            default: break
            }
        default:
            break
        }
    }

    private func handleAssistant(_ o: [String: Any]) {
        guard let msg = o["message"] as? [String: Any], let content = msg["content"] as? [[String: Any]] else { return }
        let parent = o["parent_tool_use_id"] as? String
        let mid = (msg["id"] as? String) ?? "m"
        if let m = msg["model"] as? String, parent == nil { model = m }
        for block in content {
            switch block["type"] as? String {
            case "text":
                guard parent == nil else { continue }
                let full = block["text"] as? String ?? ""
                if let existing = messageTexts[mid]?.first(where: { !$0.finalized }) {
                    existing.text = full
                    existing.finalized = true
                } else if !full.isEmpty {
                    let t = ChatText(id: mid + ":" + UUID().uuidString, text: full)
                    t.finalized = true
                    messageTexts[mid, default: []].append(t)
                    items.append(.text(t))
                }
            case "tool_use":
                guard let id = block["id"] as? String, let name = block["name"] as? String else { continue }
                let call = tools[id] ?? {
                    let c = ChatToolCall(id: id, name: name)
                    c.parentToolUseId = parent
                    tools[id] = c
                    if parent == nil { items.append(.tool(c)) }
                    return c
                }()
                call.input = block["input"] as? [String: Any] ?? [:]
                call.partialInput = ""
            default:
                break
            }
        }
    }

    private func handleUser(_ o: [String: Any]) {
        guard let msg = o["message"] as? [String: Any], let content = msg["content"] as? [[String: Any]] else { return }
        for block in content where block["type"] as? String == "tool_result" {
            guard let id = block["tool_use_id"] as? String, let tool = tools[id] else { continue }
            if tool.status != .denied { tool.status = (block["is_error"] as? Bool == true) ? .error : .done }
            tool.resultText = ChatSession.resultText(block["content"])
            tool.structured = o["tool_use_result"] as? [String: Any]
        }
    }

    static func resultText(_ content: Any?) -> String? {
        if let s = content as? String { return s }
        if let arr = content as? [[String: Any]] {
            return arr.compactMap { $0["type"] as? String == "text" ? $0["text"] as? String : nil }.joined(separator: "\n")
        }
        return nil
    }

    private func handleResult(_ o: [String: Any]) {
        busy = false
        status = nil
        if let c = o["total_cost_usd"] as? Double { totalCost += c }
        for t in tools.values where t.status == .running && t.parentToolUseId == nil { t.status = .done }
        if o["is_error"] as? Bool == true {
            let sub = o["subtype"] as? String ?? ""
            let text = sub == "error_during_execution" ? t("Остановлено", "Stopped") : (o["result"] as? String ?? sub)
            items.append(.notice(id: UUID().uuidString, text: text, isError: sub != "error_during_execution"))
        }
        control("get_context_usage", [:])
        onTurnFinished?()
    }

    private func handleControlRequest(_ o: [String: Any]) {
        guard let id = o["request_id"] as? String, let req = o["request"] as? [String: Any] else { return }
        guard req["subtype"] as? String == "can_use_tool" else {
            write(["type": "control_response", "response": ["subtype": "error", "request_id": id, "error": "Unsupported"]])
            return
        }
        let suggestions: [PermissionSuggestion] = (req["permission_suggestions"] as? [[String: Any]] ?? []).compactMap { s in
            switch s["type"] as? String {
            case "addRules":
                let rules = (s["rules"] as? [[String: Any]] ?? []).map { r -> String in
                    let tool = r["toolName"] as? String ?? ""
                    if let c = r["ruleContent"] as? String { return "\(tool)(\(c))" }
                    return tool
                }
                return PermissionSuggestion(raw: s, label: rules.joined(separator: ", "))
            case "addDirectories":
                let dirs = (s["directories"] as? [String] ?? []).map { ($0 as NSString).lastPathComponent }
                return PermissionSuggestion(raw: s, label: t("доступ к папке ", "access to ") + dirs.joined(separator: ", "))
            case "setMode":
                return PermissionSuggestion(raw: s, label: t("режим ", "mode ") + (s["mode"] as? String ?? ""))
            default:
                return nil
            }
        }
        let p = PermissionRequest(
            id: id,
            toolName: req["tool_name"] as? String ?? "?",
            input: req["input"] as? [String: Any] ?? [:],
            description: req["description"] as? String,
            blockedPath: req["blocked_path"] as? String,
            suggestions: suggestions,
            toolUseId: req["tool_use_id"] as? String
        )
        if let tid = p.toolUseId, let tool = tools[tid] {
            tool.permission = p
            if tool.input.isEmpty { tool.input = p.input }
        } else {
            let call = ChatToolCall(id: p.toolUseId ?? id, name: p.toolName)
            call.input = p.input
            call.permission = p
            tools[call.id] = call
            items.append(.tool(call))
        }
        pendingPermissions.append(p)
        NSApp.requestUserAttention(.informationalRequest)
    }

    private func handleControlResponse(_ o: [String: Any]) {
        guard let r = o["response"] as? [String: Any], let rid = r["request_id"] as? String, let body = r["response"] as? [String: Any] else { return }
        if rid.hasSuffix("-initialize") {
            models = (body["models"] as? [[String: Any]] ?? []).compactMap { m in
                guard let v = m["value"] as? String else { return nil }
                return ChatModelOption(value: v, displayName: m["displayName"] as? String ?? v, description: m["description"] as? String ?? "", efforts: m["supportedEffortLevels"] as? [String] ?? [])
            }
            commands = (body["commands"] as? [[String: Any]] ?? []).compactMap { c in
                guard let n = c["name"] as? String else { return nil }
                return ChatCommand(name: n, description: c["description"] as? String ?? "")
            }
        } else if rid.hasSuffix("-get_context_usage") {
            if let pct = body["percentage"] as? Double {
                contextPercent = Int(pct.rounded())
            } else if let total = body["totalTokens"] as? Double, let max = body["maxTokens"] as? Double, max > 0 {
                contextPercent = Int((total / max * 100).rounded())
            }
        }
    }

    func loadHistory(from file: URL, limit: Int = 600) {
        guard let data = try? Data(contentsOf: file) else { return }
        var loaded: [ChatItem] = []
        var map: [String: ChatToolCall] = [:]
        for line in data.split(separator: 0x0A) {
            guard let o = try? JSONSerialization.jsonObject(with: Data(line)) as? [String: Any] else { continue }
            guard o["isSidechain"] as? Bool != true, o["isMeta"] as? Bool != true else { continue }
            let type = o["type"] as? String
            guard let msg = o["message"] as? [String: Any] else { continue }
            if type == "user" {
                if let s = msg["content"] as? String {
                    if o["isCompactSummary"] as? Bool == true { continue }
                    let clean = s.trimmingCharacters(in: .whitespacesAndNewlines)
                    if clean.hasPrefix("<command-name>") || clean.hasPrefix("<local-command") || clean.hasPrefix("Caveat:") || clean.isEmpty { continue }
                    loaded.append(.user(id: (o["uuid"] as? String) ?? UUID().uuidString, text: clean, images: []))
                } else if let blocks = msg["content"] as? [[String: Any]] {
                    var texts: [String] = []
                    for b in blocks {
                        if b["type"] as? String == "tool_result", let id = b["tool_use_id"] as? String, let tool = map[id] {
                            tool.status = (b["is_error"] as? Bool == true) ? .error : .done
                            tool.resultText = ChatSession.resultText(b["content"])
                            tool.structured = o["toolUseResult"] as? [String: Any]
                        } else if b["type"] as? String == "text", let s = b["text"] as? String, !s.hasPrefix("<") {
                            texts.append(s)
                        }
                    }
                    if !texts.isEmpty { loaded.append(.user(id: (o["uuid"] as? String) ?? UUID().uuidString, text: texts.joined(separator: "\n"), images: [])) }
                }
            } else if type == "assistant", let blocks = msg["content"] as? [[String: Any]] {
                let uuid = (o["uuid"] as? String) ?? UUID().uuidString
                for (i, b) in blocks.enumerated() {
                    switch b["type"] as? String {
                    case "text":
                        if let s = b["text"] as? String, !s.isEmpty {
                            let t = ChatText(id: "h\(uuid):\(i)", text: s)
                            t.finalized = true
                            loaded.append(.text(t))
                        }
                    case "tool_use":
                        guard let id = b["id"] as? String, let name = b["name"] as? String else { continue }
                        let c = ChatToolCall(id: id, name: name)
                        c.input = b["input"] as? [String: Any] ?? [:]
                        c.status = .done
                        map[id] = c
                        loaded.append(.tool(c))
                    default:
                        break
                    }
                }
            }
        }
        let tail = Array(loaded.suffix(limit))
        for case .tool(let c) in tail { tools[c.id] = c }
        if loaded.count > limit {
            items = [.notice(id: "older", text: t("Показаны последние \(limit) сообщений", "Showing the last \(limit) messages"), isError: false)] + tail
        } else {
            items = tail
        }
        if !items.isEmpty {
            items.append(.notice(id: "resumed", text: t("Сессия продолжена", "Session resumed"), isError: false))
        }
    }
}
