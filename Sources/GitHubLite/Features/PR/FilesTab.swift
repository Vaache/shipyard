import SwiftUI

struct FilesTab: View {
    @Bindable var model: PRDetailModel
    @State private var showList = true
    @State private var filter = ""

    var body: some View {
        Group {
            if model.filesLoaded {
                HStack(spacing: 0) {
                    if showList {
                        fileList.frame(width: 260)
                        Divider()
                    }
                    diff
                }
            } else if let e = model.filesError {
                ErrorStateView(message: e) { Task { await model.loadFiles(force: true) } }
            } else {
                LoadingView()
            }
        }
        .task { await model.loadFiles() }
    }

    private var visibleFiles: [DiffFile] {
        filter.isEmpty ? model.files : model.files.filter { $0.id.localizedCaseInsensitiveContains(filter) }
    }

    private var fileList: some View {
        VStack(spacing: 0) {
            HStack(spacing: 6) {
                TextField(t("Фильтр файлов", "Filter files"), text: $filter).textFieldStyle(.roundedBorder).controlSize(.small)
                Text("\(model.viewedPaths.intersection(model.files.map(\.id)).count)/\(model.files.count)")
                    .font(.caption.monospacedDigit()).foregroundStyle(.secondary).help(t("Просмотрено", "Viewed"))
            }
            .padding(8)
            List(visibleFiles, selection: Binding(get: { model.scrollTarget }, set: { p in if let p { model.revealFile(p) } })) { f in
                HStack(spacing: 6) {
                    Text(f.statusLetter)
                        .font(.caption2.weight(.bold).monospaced())
                        .foregroundStyle(statusColor(f.status))
                        .frame(width: 12)
                    VStack(alignment: .leading, spacing: 0) {
                        Text((f.id as NSString).lastPathComponent).lineLimit(1)
                        Text((f.id as NSString).deletingLastPathComponent).font(.caption2).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                    }
                    Spacer(minLength: 2)
                    if model.threadCount(f.id) > 0 {
                        Image(systemName: "text.bubble").font(.caption2).foregroundStyle(.secondary)
                    }
                    if model.viewedPaths.contains(f.id) {
                        Image(systemName: "checkmark.circle.fill").font(.caption).foregroundStyle(Color.ghGreen)
                    }
                }
                .tag(f.id)
            }
            .listStyle(.sidebar)
        }
    }

    private var diff: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                    ForEach(visibleFiles) { file in
                        Section {
                            if !model.collapsed.contains(file.id) {
                                ForEach(model.rows(for: file)) { row in
                                    rowView(row, file: file)
                                }
                            }
                        } header: {
                            FileHeader(model: model, file: file, showList: $showList)
                                .id(file.id)
                        }
                    }
                }
                .padding(.bottom, 40)
            }
            .onChange(of: model.scrollTarget) { _, target in
                guard let target else { return }
                withAnimation(.easeInOut(duration: 0.2)) { proxy.scrollTo(target, anchor: .top) }
            }
            .onAppear {
                if let t = model.scrollTarget {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { proxy.scrollTo(t, anchor: .top) }
                }
            }
        }
    }

    @ViewBuilder
    private func rowView(_ row: DiffRow, file: DiffFile) -> some View {
        switch row {
        case .line(let line):
            DiffLineView(line: line) {
                guard let n = line.anchorLine else { return }
                model.composer = DiffAnchor(path: file.id, line: n, side: line.anchorSide)
            }
        case .thread(let t):
            InlineThreadView(model: model, thread: t)
        case .pending(let p):
            PendingCommentView(model: model, comment: p)
        case .composer(let anchor):
            InlineComposer(model: model, anchor: anchor)
        case .note(let text):
            Text(text).font(.caption).foregroundStyle(.secondary).padding(.horizontal, 12).padding(.vertical, 8)
        }
    }

    private func statusColor(_ s: String) -> Color {
        switch s {
        case "added": return .ghGreen
        case "removed": return .ghRed
        case "renamed": return .ghPurple
        default: return .ghYellow
        }
    }
}

struct FileHeader: View {
    @Bindable var model: PRDetailModel
    let file: DiffFile
    @Binding var showList: Bool

    var body: some View {
        let collapsed = model.collapsed.contains(file.id)
        HStack(spacing: 8) {
            Button {
                if collapsed { model.collapsed.remove(file.id) } else { model.collapsed.insert(file.id) }
            } label: {
                Image(systemName: collapsed ? "chevron.right" : "chevron.down").frame(width: 12)
            }
            .buttonStyle(.plain)
            Group {
                if let prev = file.previousFilename {
                    Text("\(prev) → \(file.id)")
                } else {
                    Text(file.id)
                }
            }
            .font(.system(.callout, design: .monospaced).weight(.medium))
            .lineLimit(1)
            .truncationMode(.head)
            .textSelection(.enabled)
            DiffStat(additions: file.additions, deletions: file.deletions)
            Spacer()
            Toggle(t("Просмотрено", "Viewed"), isOn: Binding(get: { model.viewedPaths.contains(file.id) }, set: { _ in Task { await model.toggleViewed(file.id) } }))
                .toggleStyle(.checkbox)
                .controlSize(.small)
            Menu {
                Button(t("Копировать путь", "Copy path")) { copyToPasteboard(file.id) }
                Button(showList ? t("Скрыть список файлов", "Hide file list") : t("Показать список файлов", "Show file list")) { showList.toggle() }
                Button(t("Свернуть все", "Collapse all")) { model.collapsed = Set(model.files.map(\.id)) }
                Button(t("Развернуть все", "Expand all")) { model.collapsed = [] }
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(.bar)
        .overlay(alignment: .bottom) { Divider() }
        .overlay(alignment: .top) { Divider() }
    }
}

struct DiffLineView: View {
    let line: DiffLine
    var onComment: (() -> Void)?

    private static let font = Font.system(size: 12, design: .monospaced)

    var body: some View {
        HStack(alignment: .top, spacing: 0) {
            if line.kind == .hunk {
                Text(line.text)
                    .foregroundStyle(.secondary)
                    .padding(.leading, 104)
                    .padding(.vertical, 3)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.accentColor.opacity(0.08))
            } else {
                gutter
                Text(sign + line.text)
                    .padding(.leading, 6)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .fixedSize(horizontal: false, vertical: true)
                    .foregroundStyle(line.kind == .meta ? .secondary : .primary)
            }
        }
        .font(Self.font)
        .background(background)
        .textSelection(.enabled)
    }

    private var gutter: some View {
        HStack(spacing: 0) {
            Text(line.oldLine.map(String.init) ?? "").frame(width: 46, alignment: .trailing)
            Text(line.newLine.map(String.init) ?? "").frame(width: 46, alignment: .trailing)
            Group {
                if line.commentable, onComment != nil {
                    Image(systemName: "plus.bubble").font(.system(size: 10)).foregroundStyle(Color.accentColor.opacity(0.7))
                } else {
                    Color.clear
                }
            }
            .frame(width: 16)
        }
        .foregroundStyle(.tertiary)
        .contentShape(Rectangle())
        .onTapGesture { if line.commentable { onComment?() } }
        .help(line.commentable ? t("Комментировать строку", "Comment on line") : "")
    }

    private var sign: String {
        switch line.kind {
        case .add: return "+"
        case .del: return "-"
        case .context: return " "
        default: return ""
        }
    }

    private var background: Color {
        switch line.kind {
        case .add: return Color.green.opacity(0.14)
        case .del: return Color.red.opacity(0.14)
        default: return .clear
        }
    }
}

struct InlineThreadView: View {
    let model: PRDetailModel
    let thread: ReviewThread
    @State private var expanded: Bool?
    @State private var reply = ""
    @State private var replying = false

    var body: some View {
        let open = expanded ?? !thread.isResolved
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Button { expanded = !open } label: {
                    Image(systemName: open ? "chevron.down" : "chevron.right").frame(width: 10)
                }
                .buttonStyle(.plain)
                Image(systemName: "text.bubble")
                Text(tp(thread.comments.nodes.count, "комментарий", "комментария", "комментариев", "comment", "comments")).font(.caption)
                if thread.isOutdated { Text(t("устарело", "outdated")).font(.caption2).foregroundStyle(.secondary) }
                if thread.isResolved { Text(t("решено", "resolved")).font(.caption2).foregroundStyle(Color.ghGreen) }
                Spacer()
                if thread.isResolved, thread.viewerCanUnresolve {
                    Button(t("Переоткрыть", "Unresolve")) { Task { await model.setResolved(thread.id, false) } }.controlSize(.small)
                } else if !thread.isResolved, thread.viewerCanResolve {
                    Button(t("Решено", "Resolve")) { Task { await model.setResolved(thread.id, true) } }.controlSize(.small)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            if open {
                Divider()
                ForEach(thread.comments.nodes) { c in
                    HStack(alignment: .top, spacing: 8) {
                        Avatar(url: c.author?.avatarUrl, size: 20)
                        VStack(alignment: .leading, spacing: 2) {
                            HStack(spacing: 4) {
                                Text(c.author?.login ?? "ghost").fontWeight(.semibold)
                                Text("· \(Fmt.rel(c.createdAt))").foregroundStyle(.secondary)
                            }
                            .font(.caption)
                            MarkdownText(text: c.body).font(.callout)
                        }
                        Spacer(minLength: 0)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                }
                if thread.viewerCanReply {
                    Divider()
                    if replying {
                        VStack(alignment: .trailing, spacing: 6) {
                            ComposerBox(text: $reply, placeholder: t("Ответ", "Reply"), minHeight: 50).frame(maxHeight: 120)
                            HStack {
                                Button(t("Отмена", "Cancel")) { replying = false; reply = "" }
                                Button(t("Ответить", "Reply")) {
                                    let body = reply
                                    Task { if await model.reply(thread.id, body: body) { reply = ""; replying = false } }
                                }
                                .buttonStyle(.borderedProminent)
                                .keyboardShortcut(.return, modifiers: .command)
                                .disabled(reply.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy)
                            }
                            .controlSize(.small)
                        }
                        .padding(8)
                    } else {
                        Button(t("Ответить…", "Reply…")) { replying = true }
                            .buttonStyle(.borderless)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 6)
                    }
                }
            }
        }
        .font(.callout)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: .windowBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color(nsColor: .separatorColor)))
        .padding(.horizontal, 12)
        .padding(.leading, 96)
        .padding(.vertical, 6)
        .frame(maxWidth: 900, alignment: .leading)
    }
}

struct PendingCommentView: View {
    let model: PRDetailModel
    let comment: PendingComment

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Text(t("черновик", "pending")).font(.caption2.weight(.semibold)).padding(.horizontal, 6).padding(.vertical, 1)
                .background(Capsule().fill(Color.ghYellow.opacity(0.25)))
            MarkdownText(text: comment.body).font(.callout)
            Spacer()
            Button {
                model.pending.removeAll { $0.id == comment.id }
            } label: {
                Image(systemName: "trash")
            }
            .buttonStyle(.borderless)
            .help(t("Удалить черновой комментарий", "Delete pending comment"))
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.ghYellow.opacity(0.08)))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.ghYellow.opacity(0.4)))
        .padding(.horizontal, 12)
        .padding(.leading, 96)
        .padding(.vertical, 6)
        .frame(maxWidth: 900, alignment: .leading)
    }
}

struct InlineComposer: View {
    let model: PRDetailModel
    let anchor: DiffAnchor
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        VStack(alignment: .trailing, spacing: 6) {
            ComposerBox(text: $text, placeholder: t("Комментарий к строке \(anchor.line) (Markdown)", "Comment on line \(anchor.line) (Markdown)"), minHeight: 70)
                .frame(maxHeight: 160)
                .focused($focused)
            HStack {
                Button(t("Отмена", "Cancel")) { model.composer = nil }.keyboardShortcut(.cancelAction)
                Spacer()
                Button(t("Добавить в ревью", "Add to review")) { submit(immediately: false) }
                    .keyboardShortcut(.return, modifiers: .command)
                    .disabled(empty)
                    .help(t("Сохранить как черновик и отправить вместе с ревью", "Save as pending and submit with the review"))
                Button(t("Отправить сразу", "Add single comment")) { submit(immediately: true) }
                    .buttonStyle(.borderedProminent)
                    .disabled(empty || model.busy)
            }
            .controlSize(.small)
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: .windowBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.accentColor.opacity(0.5)))
        .padding(.horizontal, 12)
        .padding(.leading, 96)
        .padding(.vertical, 6)
        .frame(maxWidth: 900, alignment: .leading)
        .onAppear { focused = true }
    }

    private var empty: Bool { text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    private func submit(immediately: Bool) {
        let body = text
        Task { _ = await model.addInline(anchor, body: body, immediately: immediately) }
    }
}
