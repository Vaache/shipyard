import AppKit
import SwiftUI
import UniformTypeIdentifiers

final class ChatTextView: NSTextView {
    var onSubmit: (() -> Void)?
    var onKey: ((NSEvent) -> Bool)?
    var onPasteImage: ((Data, String) -> Void)?

    override func keyDown(with event: NSEvent) {
        if onKey?(event) == true { return }
        if event.keyCode == 36 || event.keyCode == 76 {
            if event.modifierFlags.contains(.shift) || event.modifierFlags.contains(.option) {
                insertNewlineIgnoringFieldEditor(nil)
            } else {
                onSubmit?()
            }
            return
        }
        super.keyDown(with: event)
    }

    override func paste(_ sender: Any?) {
        let pb = NSPasteboard.general
        if let png = pb.data(forType: .png) {
            onPasteImage?(png, "image/png")
            return
        }
        if pb.string(forType: .string) == nil, let tiff = pb.data(forType: .tiff), let rep = NSBitmapImageRep(data: tiff), let png = rep.representation(using: .png, properties: [:]) {
            onPasteImage?(png, "image/png")
            return
        }
        if let urls = pb.readObjects(forClasses: [NSURL.self]) as? [URL], let img = urls.first(where: { ["png", "jpg", "jpeg", "gif", "webp"].contains($0.pathExtension.lowercased()) }), let data = try? Data(contentsOf: img) {
            onPasteImage?(data, img.pathExtension.lowercased() == "png" ? "image/png" : img.pathExtension.lowercased() == "gif" ? "image/gif" : img.pathExtension.lowercased() == "webp" ? "image/webp" : "image/jpeg")
            return
        }
        pasteAsPlainText(sender)
    }
}

struct ChatInputView: NSViewRepresentable {
    @Binding var text: String
    @Binding var height: CGFloat
    @Binding var caret: Int
    var focusToken: Int
    var onSubmit: () -> Void
    var onKey: (NSEvent) -> Bool
    var onPasteImage: (Data, String) -> Void

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = true
        scroll.autohidesScrollers = true
        let tv = ChatTextView()
        tv.isRichText = false
        tv.allowsUndo = true
        tv.font = .systemFont(ofSize: 13)
        tv.drawsBackground = false
        tv.textContainerInset = NSSize(width: 4, height: 6)
        tv.isAutomaticQuoteSubstitutionEnabled = false
        tv.isAutomaticDashSubstitutionEnabled = false
        tv.isAutomaticTextReplacementEnabled = false
        tv.isVerticallyResizable = true
        tv.autoresizingMask = [.width]
        tv.textContainer?.widthTracksTextView = true
        tv.delegate = context.coordinator
        tv.onSubmit = { context.coordinator.parent.onSubmit() }
        tv.onKey = { context.coordinator.parent.onKey($0) }
        tv.onPasteImage = { context.coordinator.parent.onPasteImage($0, $1) }
        scroll.documentView = tv
        context.coordinator.textView = tv
        DispatchQueue.main.async { tv.window?.makeFirstResponder(tv) }
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.parent = self
        guard let tv = context.coordinator.textView else { return }
        if tv.string != text {
            tv.string = text
            let pos = min(caret, (text as NSString).length)
            tv.setSelectedRange(NSRange(location: pos, length: 0))
            context.coordinator.recalcHeight()
        }
        if context.coordinator.focusToken != focusToken {
            context.coordinator.focusToken = focusToken
            DispatchQueue.main.async { tv.window?.makeFirstResponder(tv) }
        }
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: ChatInputView
        weak var textView: ChatTextView?
        var focusToken = 0

        init(_ parent: ChatInputView) {
            self.parent = parent
        }

        func textDidChange(_ notification: Notification) {
            guard let tv = textView else { return }
            parent.text = tv.string
            parent.caret = tv.selectedRange().location
            recalcHeight()
        }

        func textViewDidChangeSelection(_ notification: Notification) {
            guard let tv = textView else { return }
            if parent.caret != tv.selectedRange().location { parent.caret = tv.selectedRange().location }
        }

        func recalcHeight() {
            guard let tv = textView, let lm = tv.layoutManager, let tc = tv.textContainer else { return }
            lm.ensureLayout(for: tc)
            let h = min(200, max(22, lm.usedRect(for: tc).height)) + tv.textContainerInset.height * 2
            if abs(h - parent.height) > 0.5 {
                DispatchQueue.main.async { self.parent.height = h }
            }
        }
    }
}

enum CompletionKind { case slash, file }

struct CompletionItem: Identifiable, Hashable {
    let id: String
    let title: String
    let detail: String
    let insert: String
}

struct ChatComposer: View {
    let tab: ClaudeTab
    let chat: ChatSession
    @State private var text = ""
    @State private var height: CGFloat = 34
    @State private var caret = 0
    @State private var images: [ChatImage] = []
    @State private var completion: CompletionKind?
    @State private var query = ""
    @State private var selected = 0
    @State private var files: [String] = []
    @State private var focusToken = 0

    var body: some View {
        VStack(spacing: 0) {
            Rectangle().fill(Color.syLine).frame(height: 1)
            if let pending = chat.pendingPermissions.first(where: { !$0.resolved }) {
                HStack(spacing: 6) {
                    Image(systemName: "hand.raised.fill").foregroundStyle(Color.syAmber)
                    if pending.toolName == "AskUserQuestion" || pending.toolName == "ExitPlanMode" {
                        Text(pending.toolName == "AskUserQuestion" ? t("Claude задал вопрос — ответьте выше", "Claude asked a question — answer above") : t("Claude ждёт одобрения плана выше", "Claude is waiting for plan approval above")).font(.callout)
                        Spacer()
                    } else {
                        Text(t("Claude ждёт разрешения: \(pending.toolName)", "Claude needs permission: \(pending.toolName)")).font(.callout)
                        Spacer()
                        Button(t("Разрешить", "Allow")) { chat.respond(pending, allow: true) }.controlSize(.small).buttonStyle(.borderedProminent)
                        Button(t("Отклонить", "Deny")) { chat.respond(pending, allow: false) }.controlSize(.small)
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 6)
                .background(Color.syAmber.opacity(0.08))
                .overlay(alignment: .bottom) { Rectangle().fill(Color.syAmber.opacity(0.35)).frame(height: 1) }
            }
            VStack(alignment: .leading, spacing: 6) {
                if completion != nil, !completionItems.isEmpty {
                    completionList
                }
                if !images.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(images) { img in
                            ZStack(alignment: .topTrailing) {
                                if let ns = img.image {
                                    Image(nsImage: ns).resizable().scaledToFill().frame(width: 52, height: 52).clipShape(RoundedRectangle(cornerRadius: 6))
                                }
                                Button { images.removeAll { $0.id == img.id } } label: {
                                    Image(systemName: "xmark.circle.fill").foregroundStyle(.white, .black.opacity(0.6))
                                }
                                .buttonStyle(.plain)
                                .offset(x: 4, y: -4)
                            }
                        }
                    }
                }
                ZStack(alignment: .topLeading) {
                    ChatInputView(text: $text, height: $height, caret: $caret, focusToken: focusToken, onSubmit: submit, onKey: handleKey, onPasteImage: { data, type in
                        images.append(ChatImage(data: data, mediaType: type))
                    })
                    .frame(height: height)
                    if text.isEmpty {
                        Text(chat.busy ? t("Сообщение будет добавлено в очередь…", "Message will be queued…") : t("Спросите Claude или опишите задачу…", "Ask Claude or describe a task…"))
                            .foregroundStyle(Color.syDim)
                            .padding(.leading, 9)
                            .padding(.top, 6)
                            .allowsHitTesting(false)
                    }
                }
                bottomBar
            }
            .padding(10)
            .card(radius: 12)
            .padding(12)
        }
        .onChange(of: text) { updateCompletion() }
        .onChange(of: caret) { updateCompletion() }
        .onDrop(of: [.fileURL, .image], isTargeted: nil) { providers in
            for p in providers {
                _ = p.loadDataRepresentation(for: .png) { data, _ in
                    if let data { DispatchQueue.main.async { images.append(ChatImage(data: data, mediaType: "image/png")) } }
                }
            }
            return true
        }
        .background {
            Button("") { cycleMode() }.keyboardShortcut(.tab, modifiers: .shift).hidden()
            Button("") { focusToken += 1 }.keyboardShortcut("k", modifiers: [.command]).hidden()
        }
    }

    private var bottomBar: some View {
        HStack(spacing: 8) {
            Menu {
                ForEach(ClaudeMode.allCases) { m in
                    Button { chat.setPermissionMode(m.rawValue) } label: { checked(m.title, chat.permissionMode == m.rawValue) }
                }
            } label: {
                Label(modeTitle, systemImage: modeIcon)
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .help(t("Режим разрешений (⇧⇥ — переключить)", "Permission mode (⇧⇥ to cycle)"))
            Menu {
                ForEach(chat.models) { m in
                    Button { chat.setModel(m.value) } label: { checked("\(m.displayName) — \(m.description)", chat.modelValue == m.value) }
                }
            } label: {
                Label(modelTitle, systemImage: "cpu")
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .disabled(chat.models.isEmpty)
            if !effortLevels.isEmpty {
                Menu {
                    Button { chat.setEffort("") } label: { checked(t("По умолчанию", "Default"), chat.effort.isEmpty) }
                    ForEach(effortLevels, id: \.self) { e in
                        Button { chat.setEffort(e) } label: { checked(e, chat.effort == e) }
                    }
                } label: {
                    Label(chat.effort.isEmpty ? "effort" : chat.effort, systemImage: "gauge.with.dots.needle.50percent")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
            }
            Button { attachImage() } label: { Image(systemName: "photo.badge.plus") }
                .buttonStyle(.borderless)
                .foregroundStyle(Color.syMuted)
                .help(t("Прикрепить изображение", "Attach image"))
            Spacer()
            UsageRingButton(chat: chat)
            if chat.busy {
                Button { chat.interrupt() } label: { Image(systemName: "stop.fill") }
                    .buttonStyle(.borderedProminent)
                    .tint(.syRed)
                    .controlSize(.small)
                    .help(t("Остановить (Esc)", "Stop (Esc)"))
            }
            Button { submit() } label: { Image(systemName: "arrow.up") }
                .buttonStyle(.borderedProminent)
                .tint(.syCoral)
                .controlSize(.small)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && images.isEmpty)
                .help(t("Отправить (↩)", "Send (↩)"))
        }
        .font(.caption)
        .controlSize(.small)
    }

    private var modeTitle: String { (ClaudeMode(rawValue: chat.permissionMode) ?? .standard).title }

    private var modeIcon: String {
        switch chat.permissionMode {
        case "acceptEdits": return "pencil.and.outline"
        case "plan": return "list.bullet.clipboard"
        case "auto": return "bolt"
        default: return "hand.raised"
        }
    }

    private var modelTitle: String {
        chat.models.first { $0.value == chat.modelValue }?.displayName ?? (chat.model.isEmpty ? t("Модель", "Model") : chat.model.replacingOccurrences(of: "claude-", with: ""))
    }

    private var effortLevels: [String] {
        chat.models.first { $0.value == chat.modelValue }?.efforts ?? chat.models.first?.efforts ?? []
    }

    @ViewBuilder
    private func checked(_ title: String, _ on: Bool) -> some View {
        if on { Label(title, systemImage: "checkmark") } else { Text(title) }
    }

    private func cycleMode() {
        let order = ["default", "acceptEdits", "plan"]
        let i = order.firstIndex(of: chat.permissionMode) ?? 0
        chat.setPermissionMode(order[(i + 1) % order.count])
    }

    private func submit() {
        if completion != nil, let item = completionItems[safe: selected] {
            accept(item)
            return
        }
        let message = text
        guard !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !images.isEmpty else { return }
        if !tab.chatRunning { tab.ensureChatRunning() }
        chat.send(message, images: images)
        text = ""
        caret = 0
        images = []
        completion = nil
    }

    private func handleKey(_ e: NSEvent) -> Bool {
        if e.keyCode == 53 {
            if completion != nil { completion = nil; return true }
            if chat.busy { chat.interrupt(); return true }
            return false
        }
        guard completion != nil, !completionItems.isEmpty else { return false }
        switch e.keyCode {
        case 125: selected = min(selected + 1, completionItems.count - 1); return true
        case 126: selected = max(selected - 1, 0); return true
        case 48:
            if let item = completionItems[safe: selected] { accept(item) }
            return true
        default: return false
        }
    }

    private var tokenRange: NSRange? {
        let ns = text as NSString
        let pos = min(caret, ns.length)
        var start = pos
        while start > 0 {
            let c = ns.character(at: start - 1)
            if c == 32 || c == 10 || c == 9 { break }
            start -= 1
        }
        guard start < pos || (start == pos && start < ns.length) else { return nil }
        return NSRange(location: start, length: pos - start)
    }

    private func updateCompletion() {
        guard let r = tokenRange, r.length > 0 else { completion = nil; return }
        let token = (text as NSString).substring(with: r)
        if token.hasPrefix("/"), r.location == 0 {
            completion = .slash
            query = String(token.dropFirst())
        } else if token.hasPrefix("@") {
            completion = .file
            query = String(token.dropFirst())
            if files.isEmpty { loadFiles() }
        } else {
            completion = nil
        }
        selected = 0
    }

    private var completionItems: [CompletionItem] {
        switch completion {
        case .slash:
            let builtin: [ChatCommand] = [ChatCommand(name: "compact", description: t("Сжать контекст", "Compact the conversation")), ChatCommand(name: "clear", description: t("Новая сессия", "Start a new session"))]
            let all = builtin + chat.commands.filter { $0.name != "compact" && $0.name != "clear" }
            let q = query.lowercased()
            return all.filter { q.isEmpty || $0.name.lowercased().contains(q) }
                .sorted { ($0.name.lowercased().hasPrefix(q) ? 0 : 1, $0.name) < ($1.name.lowercased().hasPrefix(q) ? 0 : 1, $1.name) }
                .prefix(12)
                .map { CompletionItem(id: "/" + $0.name, title: "/" + $0.name, detail: $0.description, insert: "/" + $0.name + " ") }
        case .file:
            return RepoCodeModel.fuzzyMatch(query, in: files, limit: 12).map { CompletionItem(id: "@" + $0, title: ($0 as NSString).lastPathComponent, detail: $0, insert: "@" + $0 + " ") }
        case nil:
            return []
        }
    }

    private var completionList: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(completionItems.enumerated()), id: \.element.id) { i, item in
                HStack(spacing: 8) {
                    Text(item.title).font(.callout.weight(.medium)).foregroundStyle(Color.syText).lineLimit(1)
                    Text(item.detail).font(.caption).foregroundStyle(Color.syMuted).lineLimit(1).truncationMode(.tail)
                    Spacer()
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 4)
                .background(RoundedRectangle(cornerRadius: 6).fill(i == selected ? Color.sySky.opacity(0.16) : .clear))
                .contentShape(Rectangle())
                .onTapGesture { accept(item) }
            }
        }
        .padding(4)
        .card(radius: 8, fill: .sySunken)
    }

    private func accept(_ item: CompletionItem) {
        guard let r = tokenRange else { return }
        if item.insert == "/clear " {
            text = ""
            completion = nil
            tab.newChatInSameFolder()
            return
        }
        let ns = text as NSString
        text = ns.replacingCharacters(in: r, with: item.insert)
        caret = r.location + (item.insert as NSString).length
        completion = nil
        focusToken += 1
    }

    private func loadFiles() {
        let folder = tab.folder
        Task.detached {
            var list = Git.run(["ls-files", "--cached", "--others", "--exclude-standard"], in: folder).out.split(separator: "\n").map(String.init)
            if list.isEmpty, let e = FileManager.default.enumerator(atPath: folder) {
                while let p = e.nextObject() as? String, list.count < 5000 {
                    if p.hasPrefix(".git/") || p.contains("/node_modules/") || p.contains("/build/") { continue }
                    list.append(p)
                }
            }
            let result = Array(list.prefix(50_000))
            await MainActor.run { files = result }
        }
    }

    private func attachImage() {
        let panel = NSOpenPanel()
        panel.allowedContentTypes = [.png, .jpeg, .gif, .webP]
        panel.allowsMultipleSelection = true
        guard panel.runModal() == .OK else { return }
        for url in panel.urls {
            if let data = try? Data(contentsOf: url) {
                let ext = url.pathExtension.lowercased()
                images.append(ChatImage(data: data, mediaType: ext == "png" ? "image/png" : ext == "gif" ? "image/gif" : ext == "webp" ? "image/webp" : "image/jpeg"))
            }
        }
    }
}

extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}
