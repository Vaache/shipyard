import SwiftUI

struct RepoCodeView: View {
    @Bindable var model: RepoCodeModel
    let defaultBranch: String?
    @Environment(AppModel.self) private var app
    @State private var showQuickOpen = false
    @State private var markdownSource = false
    @State private var wrap = false

    var body: some View {
        HStack(spacing: 0) {
            sidebar.frame(width: 300)
            Divider()
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .task(id: defaultBranch) {
            if defaultBranch != nil { await model.start(defaultBranch: defaultBranch) }
        }
        .sheet(isPresented: $showQuickOpen) { QuickOpenSheet(model: model) }
        .background {
            Button("") { showQuickOpen = true }
                .keyboardShortcut("p")
                .hidden()
        }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Menu {
                    ForEach(model.branches) { b in
                        Button {
                            Task { await model.switchRef(b.name) }
                        } label: {
                            if b.name == model.ref { Label(b.name, systemImage: "checkmark") } else { Text(b.name) }
                        }
                    }
                } label: {
                    Label(model.ref.isEmpty ? "…" : model.ref, systemImage: "arrow.branch")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help(t("Ветка", "Branch"))
                Spacer()
                Button { showQuickOpen = true } label: { Image(systemName: "magnifyingglass") }
                    .buttonStyle(.borderless)
                    .help(t("Перейти к файлу (⌘P)", "Go to file (⌘P)"))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            breadcrumbs
                .padding(.horizontal, 10)
                .padding(.bottom, 6)
            Divider()
            List(selection: Binding(get: { model.selectedFile }, set: { _ in })) {
                if !model.dir.isEmpty {
                    Button {
                        Task { await model.openDir((model.dir as NSString).deletingLastPathComponent) }
                    } label: {
                        Label("..", systemImage: "arrow.turn.left.up")
                    }
                    .buttonStyle(.plain)
                }
                if let e = model.dirError { Text(e).font(.caption).foregroundStyle(.red) }
                ForEach(model.entries) { entry in
                    EntryRow(entry: entry)
                        .tag(entry.path)
                        .contentShape(Rectangle())
                        .onTapGesture { Task { await model.select(entry) } }
                        .contextMenu {
                            Button(t("Копировать путь", "Copy path")) { copyToPasteboard(entry.path) }
                            Button(t("Открыть в браузере", "Open in browser")) {
                                openInBrowser("https://github.com/\(model.repo.full)/\(entry.isDir ? "tree" : "blob")/\(model.ref)/\(entry.path)")
                            }
                        }
                }
            }
            .listStyle(.inset)
            .overlay { if model.loadingDir && model.entries.isEmpty { ProgressView().controlSize(.small) } }
        }
    }

    private var breadcrumbs: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 2) {
                ForEach(Array(model.crumbs.enumerated()), id: \.offset) { i, c in
                    if i > 0 { Text("/").foregroundStyle(.tertiary) }
                    Button(c.name) {
                        model.selectedFile = nil
                        model.file = nil
                        Task { await model.openDir(c.path) }
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(i == model.crumbs.count - 1 ? Color.primary : Color.accentColor)
                    .fontWeight(i == 0 ? .semibold : .regular)
                }
            }
            .font(.callout)
        }
    }

    @ViewBuilder
    private var content: some View {
        if let path = model.selectedFile {
            VStack(spacing: 0) {
                fileHeader(path)
                Divider()
                if let e = model.fileError {
                    ErrorStateView(message: e) { Task { await model.openFile(path) } }
                } else if let file = model.file {
                    fileBody(file, path: path)
                } else {
                    LoadingView()
                }
            }
        } else if let readme = model.readme {
            HTMLView(html: page(readme), onLink: openLink)
        } else if model.loadingDir {
            LoadingView()
        } else {
            VStack(spacing: 8) {
                Image(systemName: "doc.text.magnifyingglass").font(.system(size: 36)).foregroundStyle(.tertiary)
                Text(t("Выберите файл слева или нажмите ⌘P", "Pick a file on the left or press ⌘P")).foregroundStyle(.secondary)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func fileHeader(_ path: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "doc.text").foregroundStyle(.secondary)
            Text(path).font(.system(.callout, design: .monospaced).weight(.medium)).lineLimit(1).truncationMode(.head).textSelection(.enabled)
            if let size = model.fileSize {
                Text(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)).font(.caption).foregroundStyle(.secondary)
            }
            if case .text(let s, _) = model.file {
                Text(tp(s.reduce(into: 1) { if $1 == "\n" { $0 += 1 } }, "строка", "строки", "строк", "line", "lines")).font(.caption).foregroundStyle(.secondary)
            }
            Spacer()
            if case .markdown = model.file {
                Picker("", selection: $markdownSource) {
                    Text(t("Просмотр", "Preview")).tag(false)
                    Text(t("Код", "Source")).tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 160)
            }
            if case .text = model.file {
                Toggle(t("Перенос", "Wrap"), isOn: $wrap).toggleStyle(.checkbox).controlSize(.small)
            }
            Menu {
                Button(t("Копировать путь", "Copy path")) { copyToPasteboard(path) }
                if case .text(let s, _) = model.file { Button(t("Копировать содержимое", "Copy contents")) { copyToPasteboard(s) } }
                if case .markdown(_, let s) = model.file { Button(t("Копировать содержимое", "Copy contents")) { copyToPasteboard(s) } }
                Button(t("Копировать ссылку", "Copy link")) { copyToPasteboard(model.webURL) }
                Button(t("Открыть в браузере", "Open in browser")) { openInBrowser(model.webURL) }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 7)
    }

    @ViewBuilder
    private func fileBody(_ file: FileContent, path: String) -> some View {
        let ext = (path as NSString).pathExtension.isEmpty ? (path as NSString).lastPathComponent : (path as NSString).pathExtension
        switch file {
        case .text(let s, let truncated):
            VStack(spacing: 0) {
                if truncated {
                    Text(t("Файл слишком большой — показана только часть", "File is too large — showing only part of it"))
                        .font(.caption).foregroundStyle(.orange).padding(6)
                }
                CodeTextView(text: s, ext: ext, wrap: wrap)
            }
        case .markdown(let html, let source):
            if markdownSource {
                CodeTextView(text: source, ext: ext, wrap: true)
            } else {
                HTMLView(html: page(html), onLink: openLink)
            }
        case .image(let img):
            ScrollView([.horizontal, .vertical]) {
                Image(nsImage: img)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: min(img.size.width, 1200))
                    .padding(20)
            }
            .background(Color(nsColor: .textBackgroundColor))
        case .binary(let size):
            VStack(spacing: 8) {
                Image(systemName: "doc.zipper").font(.system(size: 36)).foregroundStyle(.tertiary)
                Text(t("Бинарный файл", "Binary file") + " · " + ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)).foregroundStyle(.secondary)
                Button(t("Открыть в браузере", "Open in browser")) { openInBrowser(model.webURL) }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func page(_ inner: String) -> String {
        """
        <!doctype html><html><head><meta charset="utf-8"><style>\(ConversationHTML.css) .md{padding:8px 4px} .tl{max-width:900px} .md a.anchor,.md .anchor,.md .octicon-link{display:none}</style></head>
        <body><div class="tl"><div class="md">\(inner)</div></div></body></html>
        """
    }

    private func openLink(_ url: URL) {
        if url.host == "github.com", url.pathComponents.count > 1, url.pathComponents.count <= 2 { LinkBrowser.open(url); return }
        if let route = GitHubURL.parse(url.absoluteString, allowShort: false), case .repo = route {
            LinkBrowser.open(url)
        } else if let route = GitHubURL.parse(url.absoluteString, allowShort: false) {
            app.open(route, push: true)
        } else if url.host == "github.com", url.path.isEmpty || url.path == "/" {
            return
        } else {
            LinkBrowser.open(url)
        }
    }
}

struct EntryRow: View {
    let entry: TreeEntry

    var body: some View {
        HStack(spacing: 7) {
            Image(systemName: icon)
                .foregroundStyle(entry.isDir ? Color.accentColor : .secondary)
                .frame(width: 16)
            Text(entry.name).lineLimit(1)
            Spacer()
            if let size = entry.object?.byteSize, !entry.isDir {
                Text(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)).font(.caption2).foregroundStyle(.tertiary)
            }
        }
    }

    private var icon: String {
        if entry.isDir { return "folder.fill" }
        if entry.isSubmodule { return "shippingbox" }
        switch (entry.name as NSString).pathExtension.lowercased() {
        case "md", "markdown", "txt": return "doc.plaintext"
        case "png", "jpg", "jpeg", "gif", "webp", "svg", "ico": return "photo"
        case "json", "yaml", "yml", "toml", "xml", "plist": return "curlybraces"
        case "swift", "dart", "js", "ts", "tsx", "jsx", "kt", "java", "go", "rs", "py", "rb", "c", "cpp", "h", "m": return "chevron.left.forwardslash.chevron.right"
        case "sh", "bash", "zsh": return "terminal"
        case "lock": return "lock"
        default: return "doc"
        }
    }
}

struct QuickOpenSheet: View {
    let model: RepoCodeModel
    @Environment(\.dismiss) private var dismiss
    @State private var query = ""
    @State private var selection: String?

    var body: some View {
        let results = RepoCodeModel.fuzzyMatch(query, in: model.allPaths ?? [])
        VStack(spacing: 0) {
            TextField(t("Перейти к файлу…", "Go to file…"), text: $query)
                .textFieldStyle(.plain)
                .font(.title3)
                .padding(14)
                .onSubmit { open(selection ?? results.first) }
            Divider()
            if model.allPaths == nil {
                LoadingView().frame(height: 300)
            } else {
                List(results, id: \.self, selection: $selection) { p in
                    VStack(alignment: .leading, spacing: 1) {
                        Text((p as NSString).lastPathComponent)
                        Text(p).font(.caption).foregroundStyle(.secondary).lineLimit(1).truncationMode(.head)
                    }
                    .tag(p)
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) { open(p) }
                }
                .frame(height: 380)
                if model.pathsTruncated {
                    Text(t("Репозиторий большой — список файлов неполный", "Large repository — file list is incomplete"))
                        .font(.caption).foregroundStyle(.secondary).padding(6)
                }
            }
            HStack {
                Text(t("↩ открыть · Esc закрыть", "↩ open · Esc close")).font(.caption).foregroundStyle(.secondary)
                Spacer()
                Button(t("Отмена", "Cancel")) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(t("Открыть", "Open")) { open(selection ?? results.first) }.keyboardShortcut(.defaultAction)
            }
            .padding(10)
        }
        .frame(width: 620)
        .task { await model.loadAllPaths() }
    }

    private func open(_ path: String?) {
        guard let path else { return }
        dismiss()
        Task { await model.reveal(path) }
    }
}
