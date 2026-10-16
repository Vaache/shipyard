import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct ChatView: View {
    @Environment(AppModel.self) private var app
    let tab: ClaudeTab
    let chat: ChatSession
    @State private var follow = ChatFollow()

    var body: some View {
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 14) {
                        if chat.items.isEmpty {
                            ChatWelcome(chat: chat, folder: tab.folder)
                        }
                        ForEach(chat.items) { item in
                            ChatItemView(item: item, chat: chat, folder: tab.folder)
                                .id(item.id)
                                .transition(.rise)
                        }
                        if chat.busy {
                            WorkingIndicator(chat: chat)
                                .transition(.opacity)
                        }
                        if let e = chat.lastError {
                            ErrorBanner(text: e) { app.claude.restart(tab) }
                                .transition(.pop)
                        }
                        Color.clear.frame(height: 1).id("bottom")
                    }
                    .padding(.horizontal, 20)
                    .padding(.vertical, 16)
                    .animation(Motion.smooth, value: chat.items.count)
                    .animation(Motion.fade, value: chat.busy)
                    .animation(Motion.spring, value: chat.lastError)
                    .frame(maxWidth: 920)
                    .frame(maxWidth: .infinity)
                    .background {
                        GeometryReader { g in
                            Color.clear.preference(key: ChatContentFrameKey.self, value: g.frame(in: .named(ChatContentFrameKey.space)))
                        }
                    }
                }
                .coordinateSpace(name: ChatContentFrameKey.space)
                .background {
                    GeometryReader { g in
                        Color.clear.preference(key: ChatViewportKey.self, value: g.size.height)
                    }
                }
                .modifier(InitialBottomAnchor())
                .onPreferenceChange(ChatViewportKey.self) { follow.viewportHeight = $0 }
                .onPreferenceChange(ChatContentFrameKey.self) { if follow.track($0) { scrollDown(proxy) } }
                .onChange(of: chat.items.count) {
                    if case .user? = chat.items.last { follow.stick = true }
                    scrollDown(proxy)
                }
                .onChange(of: lastTextLength) { scrollDown(proxy) }
                .onChange(of: chat.busy) { scrollDown(proxy) }
                .onChange(of: chat.lastError) { scrollDown(proxy) }
                .onChange(of: chat.pendingCount) {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { scrollDown(proxy) }
                }
                .onAppear { jumpToBottom(proxy) }
                .onChange(of: tab.id) { jumpToBottom(proxy) }
            }
            if chat.sleeping && !chat.running {
                HStack(spacing: 8) {
                    Image(systemName: "moon.zzz.fill").foregroundStyle(Color.syIndigo)
                    Text("Session is sleeping — process stopped, memory freed. A new message resumes it.")
                        .font(.callout)
                        .foregroundStyle(Color.syMuted)
                    Spacer()
                    Button("Wake up") { chat.wake() }.controlSize(.small)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 6)
                .background(Color.syIndigo.opacity(0.08))
                .overlay(alignment: .top) { Rectangle().fill(Color.syLine).frame(height: 1) }
            }
            ChatComposer(tab: tab, chat: chat)
        }
        .background(Theme.isBrand ? Color.syPanel : Color(nsColor: .textBackgroundColor).opacity(0.35))
    }

    private var lastTextLength: Int {
        if case .text(let t)? = chat.items.last { return t.text.count }
        return 0
    }

    private func jumpToBottom(_ proxy: ScrollViewProxy) {
        follow.stick = true
        proxy.scrollTo("bottom", anchor: .bottom)
        for delay in [0.05, 0.2, 0.5, 1.0] {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { proxy.scrollTo("bottom", anchor: .bottom) }
        }
    }

    private func scrollDown(_ proxy: ScrollViewProxy) {
        guard follow.stick else { return }
        withAnimation(.easeOut(duration: 0.15)) { proxy.scrollTo("bottom", anchor: .bottom) }
    }
}

private final class ChatFollow {
    var stick = true
    var viewportHeight: CGFloat = 0
    private var contentFrame: CGRect = .zero

    func track(_ frame: CGRect) -> Bool {
        let grew = frame.height > contentFrame.height + 1
        if frame.maxY - viewportHeight <= 40 {
            stick = true
        } else if frame.minY > contentFrame.minY + 1 {
            stick = false
        }
        contentFrame = frame
        return grew && stick
    }
}

private struct ChatContentFrameKey: PreferenceKey {
    static let space = "chatScroll"
    static let defaultValue: CGRect = .zero
    static func reduce(value: inout CGRect, nextValue: () -> CGRect) {
        value = nextValue()
    }
}

private struct ChatViewportKey: PreferenceKey {
    static let defaultValue: CGFloat = 0
    static func reduce(value: inout CGFloat, nextValue: () -> CGFloat) {
        value = nextValue()
    }
}

private struct InitialBottomAnchor: ViewModifier {
    func body(content: Content) -> some View {
        if #available(macOS 15, *) {
            content.defaultScrollAnchor(.bottom, for: .initialOffset)
        } else {
            content
        }
    }
}

struct ChatWelcome: View {
    let chat: ChatSession
    let folder: String

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                IconTile(systemImage: "sparkles", color: .syCoral, size: 24)
                Text("Claude Code").font(.title3.weight(.semibold)).foregroundStyle(Color.syText)
            }
            Text("Describe a task — Claude will read the code, propose changes and ask before acting.")
                .foregroundStyle(Color.syMuted)
            HStack(spacing: 8) {
                ForEach(suggestions, id: \.self) { s in
                    Button(s) { chat.send(s) }
                        .buttonStyle(.bordered)
                        .controlSize(.small)
                }
            }
            Text("/ commands · @ files · ⇧↩ new line · paste a screenshot ⌘V")
                .font(.caption)
                .foregroundStyle(Color.syDim)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .card(radius: 12, fill: Color.syCoral.opacity(0.06))
    }

    private var suggestions: [String] {
        ["Explain the project structure",
         "Find bugs in the latest changes",
         "Write tests for the changed code"]
    }
}

struct WorkingIndicator: View {
    let chat: ChatSession

    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { ctx in
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(chat.status ?? (chat.pendingCount > 0 ? "Waiting for you" : "Working…"))
                    .foregroundStyle(Color.syMuted)
                if let start = chat.turnStartedAt {
                    Text("\(Int(ctx.date.timeIntervalSince(start)))s").font(.caption.monospacedDigit()).foregroundStyle(Color.syDim)
                }
                Spacer()
                Button("Stop") { chat.interrupt() }
                    .controlSize(.small)
                    .keyboardShortcut(.escape, modifiers: [])
            }
            .font(.callout)
        }
    }
}

struct ErrorBanner: View {
    let text: String
    let retry: () -> Void

    private var looksLikeAuth: Bool {
        let s = text.lowercased()
        return ["login", "log in", "logged in", "auth", "api key", "credential", "401", "oauth", "token"].contains { s.contains($0) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(text, systemImage: "exclamationmark.triangle.fill").foregroundStyle(Color.syAmber).textSelection(.enabled)
            HStack {
                Button("Restart session", action: retry).controlSize(.small)
                if looksLikeAuth {
                    Button("Check Claude sign-in") {
                        Task { await ClaudeAccountStore.shared.refresh() }
                    }
                    .controlSize(.small)
                    .buttonStyle(.borderedProminent)
                    .tint(.syCoral)
                }
            }
        }
        .padding(12)
        .padding(.leading, 3)
        .background(Color.syRaised)
        .overlay(alignment: .leading) { Rectangle().fill(Color.syAmber).frame(width: 3) }
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }
}

struct ChatItemView: View {
    let item: ChatItem
    let chat: ChatSession
    let folder: String

    var body: some View {
        switch item {
        case .user(_, let text, let images):
            UserBubble(text: text, images: images)
        case .text(let t):
            AssistantText(text: t)
        case .thinking(let t):
            ThinkingView(text: t)
        case .tool(let call):
            ToolCallView(call: call, chat: chat, folder: folder)
        case .notice(_, let text, let isError):
            HStack(spacing: 6) {
                Rectangle().fill(Color.syLine).frame(height: 1)
                Text(text).font(.caption).foregroundStyle(isError ? Color.syRed : Color.syDim).fixedSize()
                Rectangle().fill(Color.syLine).frame(height: 1)
            }
        }
    }
}

struct UserBubble: View {
    let text: String
    let images: [ChatImage]
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            if !images.isEmpty {
                HStack(spacing: 6) {
                    ForEach(images) { img in
                        if let ns = img.image {
                            Image(nsImage: ns).resizable().scaledToFill().frame(width: 64, height: 64).clipShape(RoundedRectangle(cornerRadius: 6))
                        }
                    }
                }
            }
            if !text.isEmpty {
                Text(text)
                    .foregroundStyle(Color.syText)
                    .textSelection(.enabled)
                    .lineLimit(expanded ? nil : 12)
                    .fixedSize(horizontal: false, vertical: true)
                if text.split(separator: "\n").count > 12 {
                    Button(expanded ? "Collapse" : "Show all") { expanded.toggle() }
                        .buttonStyle(.link)
                        .font(.caption)
                }
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .card(radius: 10)
    }
}

struct AssistantText: View {
    let text: ChatText

    var body: some View {
        if !text.text.isEmpty {
            MarkdownView(text: text.text)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct ThinkingView: View {
    let text: ChatText
    @State private var open = false

    var body: some View {
        if !text.text.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                Button { withAnimation(Motion.quick) { open.toggle() } } label: {
                    HStack(spacing: 5) {
                        Image(systemName: "chevron.right").font(.caption2).rotationEffect(.degrees(open ? 90 : 0))
                        Image(systemName: "brain").foregroundStyle(Color.syViolet)
                        Text("Thinking")
                    }
                    .font(.caption)
                    .foregroundStyle(Color.syMuted)
                }
                .buttonStyle(.plain)
                if open {
                    Text(text.text).font(.callout).foregroundStyle(Color.syMuted).textSelection(.enabled).padding(.leading, 18)
                        .transition(.opacity)
                }
            }
        }
    }
}

enum ToolPresentation {
    static func icon(_ name: String) -> String {
        switch name {
        case "Read": return "doc.text"
        case "Edit", "MultiEdit": return "pencil"
        case "Write": return "square.and.pencil"
        case "Bash": return "terminal"
        case "Grep": return "magnifyingglass"
        case "Glob": return "folder.badge.questionmark"
        case "WebFetch": return "globe"
        case "WebSearch": return "magnifyingglass.circle"
        case "Task", "Agent": return "person.2"
        case "TodoWrite": return "checklist"
        case "AskUserQuestion": return "questionmark.bubble"
        case "ExitPlanMode": return "list.bullet.clipboard"
        case "NotebookEdit": return "book"
        case "Skill": return "wand.and.stars"
        default: return name.hasPrefix("mcp__") ? "puzzlepiece.extension" : "wrench.and.screwdriver"
        }
    }

    static func title(_ call: ChatToolCall) -> String {
        if call.name.hasPrefix("mcp__") {
            let parts = call.name.split(separator: "_").filter { !$0.isEmpty }
            return parts.last.map(String.init) ?? call.name
        }
        return call.name
    }

    static func relative(_ path: String, _ folder: String) -> String {
        path.hasPrefix(folder + "/") ? String(path.dropFirst(folder.count + 1)) : path.replacingOccurrences(of: FileManager.default.homeDirectoryForCurrentUser.path, with: "~")
    }

    static func summary(_ call: ChatToolCall, folder: String) -> String {
        let i = call.input
        switch call.name {
        case "Read", "Edit", "MultiEdit", "Write", "NotebookEdit":
            return relative(i["file_path"] as? String ?? i["notebook_path"] as? String ?? "", folder)
        case "Bash": return i["description"] as? String ?? i["command"] as? String ?? ""
        case "Grep": return "\"\(i["pattern"] as? String ?? "")\"" + ((i["path"] as? String).map { " · " + relative($0, folder) } ?? "")
        case "Glob": return i["pattern"] as? String ?? ""
        case "WebFetch": return i["url"] as? String ?? ""
        case "WebSearch": return i["query"] as? String ?? ""
        case "Task", "Agent": return i["description"] as? String ?? ""
        case "Skill": return i["skill"] as? String ?? ""
        default:
            if let s = i.values.first(where: { $0 is String }) as? String { return String(s.prefix(120)) }
            return call.partialInput.isEmpty ? "" : "…"
        }
    }
}

struct ToolCallView: View {
    let call: ChatToolCall
    let chat: ChatSession
    let folder: String
    @State private var open: Bool?

    private var defaultOpen: Bool {
        if call.permission?.resolved == false { return true }
        switch call.name {
        case "Edit", "MultiEdit", "Write", "ExitPlanMode", "TodoWrite": return true
        default: return false
        }
    }

    var body: some View {
        let isOpen = open ?? defaultOpen
        VStack(alignment: .leading, spacing: 0) {
            Button { withAnimation(Motion.quick) { open = !isOpen } } label: {
                HStack(spacing: 8) {
                    statusIcon
                    Image(systemName: ToolPresentation.icon(call.name)).foregroundStyle(Color.syMuted).frame(width: 16)
                    Text(ToolPresentation.title(call)).fontWeight(.semibold).foregroundStyle(Color.syText)
                    Text(ToolPresentation.summary(call, folder: folder))
                        .foregroundStyle(Color.syMuted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 4)
                    if let stat = diffStat { stat }
                    Image(systemName: "chevron.down").font(.caption2).foregroundStyle(Color.syDim).rotationEffect(.degrees(isOpen ? 180 : 0))
                }
                .font(.callout)
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if isOpen {
                Rectangle().fill(Color.syLine).frame(height: 1)
                ToolDetails(call: call, folder: folder)
                    .padding(10)
                    .transition(.opacity)
            }
            if let p = call.permission, !p.resolved {
                PermissionCard(request: p, chat: chat, folder: folder)
                    .padding(10)
                    .transition(.pop)
            }
        }
        .clipped()
        .animation(Motion.spring, value: call.permission?.resolved)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.syRaised))
        .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(borderColor, lineWidth: call.permission?.resolved == false ? 1.5 : 1))
    }

    private var borderColor: Color {
        if call.permission?.resolved == false { return .clear }
        switch call.status {
        case .error: return Color.syRed.opacity(0.5)
        case .denied: return Color.syAmber.opacity(0.5)
        default: return .clear
        }
    }

    @ViewBuilder
    private var statusIcon: some View {
        switch call.status {
        case .running:
            if call.permission?.resolved == false {
                Image(systemName: "hand.raised.fill").foregroundStyle(Color.syAmber)
            } else {
                ProgressView().controlSize(.mini).frame(width: 14)
            }
        case .done: Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.syGreen)
        case .error: Image(systemName: "xmark.circle.fill").foregroundStyle(Color.syRed)
        case .denied: Image(systemName: "nosign").foregroundStyle(Color.syAmber)
        }
    }

    private var diffStat: DiffStat? {
        if let patch = call.structured?["structuredPatch"] as? [[String: Any]], !patch.isEmpty {
            let lines = patch.flatMap { $0["lines"] as? [String] ?? [] }
            return DiffStat(additions: lines.filter { $0.hasPrefix("+") }.count, deletions: lines.filter { $0.hasPrefix("-") }.count)
        }
        if call.name == "Write", let content = call.string("content") {
            return DiffStat(additions: content.split(separator: "\n", omittingEmptySubsequences: false).count, deletions: 0)
        }
        return nil
    }
}

struct ToolDetails: View {
    let call: ChatToolCall
    let folder: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            switch call.name {
            case "Edit", "MultiEdit", "Write":
                if let path = call.string("file_path") {
                    HStack {
                        Text(ToolPresentation.relative(path, folder)).font(.caption.monospaced()).foregroundStyle(Color.syMuted)
                        Spacer()
                        Button("Open") { NSWorkspace.shared.open(URL(fileURLWithPath: path)) }
                            .buttonStyle(.link).font(.caption)
                    }
                }
                InlineDiff(lines: diffLines)
            case "Bash":
                if let cmd = call.string("command") {
                    Text("$ " + cmd).font(.system(size: 12, design: .monospaced)).foregroundStyle(Color.syText).textSelection(.enabled)
                        .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                        .card(radius: 8, fill: .sySunken)
                }
                outputView(bashOutput)
            case "TodoWrite":
                TodoListView(todos: call.input["todos"] as? [[String: Any]] ?? [])
            case "ExitPlanMode":
                MarkdownView(text: call.string("plan") ?? "")
            case "AskUserQuestion":
                ForEach(Array((call.input["questions"] as? [[String: Any]] ?? []).enumerated()), id: \.offset) { _, q in
                    Text(q["question"] as? String ?? "").font(.callout.weight(.medium))
                }
                if let r = call.resultText { Text(r).font(.callout).foregroundStyle(Color.syMuted).textSelection(.enabled) }
            case "Read":
                if let content = (call.structured?["file"] as? [String: Any])?["content"] as? String {
                    outputView(content)
                } else {
                    outputView(call.resultText)
                }
            default:
                if !call.input.isEmpty, let data = try? JSONSerialization.data(withJSONObject: call.input, options: [.prettyPrinted, .sortedKeys]) {
                    outputView(String(decoding: data, as: UTF8.self), title: "Input")
                }
                outputView(call.resultText, title: "Result")
            }
        }
    }

    private var bashOutput: String? {
        if let s = call.structured {
            let out = (s["stdout"] as? String ?? "") + ((s["stderr"] as? String).flatMap { $0.isEmpty ? nil : "\n" + $0 } ?? "")
            return out.isEmpty ? call.resultText : out
        }
        return call.resultText
    }

    @ViewBuilder
    private func outputView(_ text: String?, title: String? = nil) -> some View {
        if let text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                if let title { Text(title).font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted) }
                ScrollView([.vertical, .horizontal]) {
                    Text(String(text.prefix(20_000)))
                        .font(.system(size: 11.5, design: .monospaced))
                        .textSelection(.enabled)
                        .fixedSize()
                        .padding(8)
                }
                .frame(maxHeight: 220)
                .card(radius: 8, fill: .sySunken)
            }
        }
    }

    private var diffLines: [DiffLine] {
        if let patch = call.structured?["structuredPatch"] as? [[String: Any]], !patch.isEmpty {
            var out: [DiffLine] = []
            var idx = 0
            for hunk in patch {
                var old = hunk["oldStart"] as? Int ?? 1, new = hunk["newStart"] as? Int ?? 1
                for l in hunk["lines"] as? [String] ?? [] {
                    let body = String(l.dropFirst())
                    switch l.first {
                    case "+": out.append(DiffLine(id: idx, kind: .add, text: body, oldLine: nil, newLine: new)); new += 1
                    case "-": out.append(DiffLine(id: idx, kind: .del, text: body, oldLine: old, newLine: nil)); old += 1
                    default: out.append(DiffLine(id: idx, kind: .context, text: body, oldLine: old, newLine: new)); old += 1; new += 1
                    }
                    idx += 1
                }
            }
            return out
        }
        var out: [DiffLine] = []
        var idx = 0
        if call.name == "Write" {
            for (n, l) in (call.string("content") ?? "").components(separatedBy: "\n").enumerated() {
                out.append(DiffLine(id: idx, kind: .add, text: l, oldLine: nil, newLine: n + 1)); idx += 1
            }
            return out
        }
        let edits: [[String: Any]] = call.name == "MultiEdit" ? (call.input["edits"] as? [[String: Any]] ?? []) : [call.input]
        for e in edits {
            for l in (e["old_string"] as? String ?? "").components(separatedBy: "\n") {
                out.append(DiffLine(id: idx, kind: .del, text: l, oldLine: nil, newLine: nil)); idx += 1
            }
            for l in (e["new_string"] as? String ?? "").components(separatedBy: "\n") {
                out.append(DiffLine(id: idx, kind: .add, text: l, oldLine: nil, newLine: nil)); idx += 1
            }
        }
        return out
    }
}

struct InlineDiff: View {
    let lines: [DiffLine]
    @State private var showAll = false

    var body: some View {
        let shown = showAll ? lines : Array(lines.prefix(60))
        VStack(alignment: .leading, spacing: 0) {
            ForEach(shown) { DiffLineView(line: $0).equatable() }
            if lines.count > 60 && !showAll {
                Button("Show \(lines.count - 60) more lines") { showAll = true }
                    .buttonStyle(.link).font(.caption).padding(6)
            }
        }
        .background(Color.sySunken)
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }
}

struct TodoListView: View {
    let todos: [[String: Any]]

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            ForEach(Array(todos.enumerated()), id: \.offset) { _, todo in
                let status = todo["status"] as? String ?? "pending"
                HStack(spacing: 8) {
                    Image(systemName: status == "completed" ? "checkmark.circle.fill" : status == "in_progress" ? "circle.dotted.circle" : "circle")
                        .foregroundStyle(status == "completed" ? Color.syGreen : status == "in_progress" ? Color.sySky : Color.syDim)
                    Text(todo["content"] as? String ?? todo["subject"] as? String ?? "")
                        .strikethrough(status == "completed")
                        .foregroundStyle(status == "completed" ? Color.syMuted : Color.syText)
                }
                .font(.callout)
            }
        }
    }
}

struct PermissionCard: View {
    let request: PermissionRequest
    let chat: ChatSession
    let folder: String
    @State private var feedback = ""
    @State private var answers: [String: String] = [:]

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            switch request.toolName {
            case "AskUserQuestion":
                questionView
            case "ExitPlanMode":
                Text("Claude has a plan. Start implementing?").font(.callout.weight(.semibold))
                HStack {
                    Button("Yes, auto-accept edits") {
                        chat.respond(request, allow: true)
                        chat.setPermissionMode("acceptEdits")
                    }
                    .buttonStyle(.borderedProminent)
                    Button("Yes, ask before edits") {
                        chat.respond(request, allow: true)
                        chat.setPermissionMode("default")
                    }
                    Spacer()
                }
                denyRow(placeholder: "What to change in the plan…")
            default:
                Text(question).font(.caption).foregroundStyle(Color.syAmber)
                if let d = request.description, !d.isEmpty { Text(d).font(.callout).foregroundStyle(Color.syMuted) }
                if let path = request.blockedPath {
                    Text(ToolPresentation.relative(path, folder)).font(.caption.monospaced()).foregroundStyle(Color.syMuted)
                }
                VStack(alignment: .leading, spacing: 6) {
                    HStack {
                        Button("Allow") { chat.respond(request, allow: true) }
                            .buttonStyle(.borderedProminent)
                            .keyboardShortcut(.return, modifiers: [.command])
                        Spacer()
                        Text("⌘↩").font(.caption).foregroundStyle(Color.syDim)
                    }
                    ForEach(Array(request.suggestions.enumerated()), id: \.offset) { _, s in
                        Button { chat.respond(request, allow: true, always: s) } label: {
                            Text("Allow, and don't ask again for " + s.label)
                                .lineLimit(1)
                        }
                        .buttonStyle(.link)
                    }
                }
                denyRow(placeholder: "No — tell Claude what to do instead…")
            }
        }
        .padding(12)
        .padding(.leading, 3)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.syRaised)
        .overlay(alignment: .leading) { Rectangle().fill(Color.syAmber).frame(width: 3) }
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }

    private var question: String {
        switch request.toolName {
        case "Bash": return "Claude wants to run a command"
        case "Edit", "MultiEdit": return "Claude wants to edit a file"
        case "Write": return "Claude wants to write a file"
        case "WebFetch": return "Claude wants to fetch a URL"
        default: return "Claude wants to use \(request.toolName)"
        }
    }

    private func denyRow(placeholder: String) -> some View {
        HStack {
            TextField(placeholder, text: $feedback)
                .textFieldStyle(.roundedBorder)
                .onSubmit { chat.respond(request, allow: false, message: feedback) }
            Button("Deny") { chat.respond(request, allow: false, message: feedback) }
        }
    }

    @ViewBuilder
    private var questionView: some View {
        let questions = request.input["questions"] as? [[String: Any]] ?? []
        ForEach(Array(questions.enumerated()), id: \.offset) { _, q in
            let text = q["question"] as? String ?? ""
            let options = q["options"] as? [[String: Any]] ?? []
            VStack(alignment: .leading, spacing: 6) {
                Text(text).font(.callout.weight(.semibold))
                ForEach(Array(options.enumerated()), id: \.offset) { _, o in
                    let label = o["label"] as? String ?? ""
                    Button {
                        answers[text] = label
                    } label: {
                        HStack(alignment: .top) {
                            Image(systemName: answers[text] == label ? "largecircle.fill.circle" : "circle")
                                .foregroundStyle(answers[text] == label ? Color.syAccent : Color.syDim)
                            VStack(alignment: .leading, spacing: 1) {
                                Text(label)
                                if let d = o["description"] as? String { Text(d).font(.caption).foregroundStyle(Color.syMuted) }
                            }
                            Spacer()
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                }
                TextField("Other…", text: Binding(get: { options.contains { $0["label"] as? String == answers[text] } ? "" : (answers[text] ?? "") }, set: { answers[text] = $0 }))
                    .textFieldStyle(.roundedBorder)
            }
        }
        HStack {
            Button("Submit") {
                var input = request.input
                input["answers"] = answers
                chat.respond(request, allow: true, updatedInput: input)
            }
            .buttonStyle(.borderedProminent)
            .disabled(answers.count < questions.count)
            Button("Skip") { chat.respond(request, allow: false, message: "The user skipped the question") }
        }
    }
}
