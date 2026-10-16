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
            sidebar.frame(width: 300).surface(.list)
            Rectangle().fill(Color.syLine).frame(width: 1)
            content.frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .task(id: defaultBranch) {
            if defaultBranch != nil { await model.start(defaultBranch: defaultBranch) }
        }
        .sheet(isPresented: $showQuickOpen) { QuickOpenSheet(model: model) }
        .focusedSceneValue(\.blame, model.selectedFile != nil && model.file != nil ? blameBinding : nil)
        .onChange(of: model.blameOn) { _, on in
            if on, case .markdown = model.file { markdownSource = true }
        }
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
                    Label(model.ref.isEmpty ? "…" : model.refLabel, systemImage: "arrow.branch")
                        .font(.callout.monospaced().weight(.medium))
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help("Branch")
                Spacer()
                Button { showQuickOpen = true } label: { Image(systemName: "magnifyingglass").foregroundStyle(Color.syMuted) }
                    .buttonStyle(.borderless)
                    .help("Go to file (⌘P)")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            breadcrumbs
                .padding(.horizontal, 10)
                .padding(.bottom, 6)
            Rectangle().fill(Color.syLine).frame(height: 1)
            List(selection: Binding(get: { model.selectedFile }, set: { _ in })) {
                if !model.dir.isEmpty {
                    Button {
                        Task { await model.openDir((model.dir as NSString).deletingLastPathComponent) }
                    } label: {
                        Label("..", systemImage: "arrow.turn.left.up").foregroundStyle(Color.syMuted)
                    }
                    .buttonStyle(.plain)
                }
                if let e = model.dirError { Text(e).font(.caption).foregroundStyle(Color.syRed) }
                ForEach(model.entries) { entry in
                    EntryRow(entry: entry)
                        .tag(entry.path)
                        .contentShape(Rectangle())
                        .onTapGesture { Task { await model.select(entry) } }
                        .contextMenu {
                            Button("Copy path") { copyToPasteboard(entry.path) }
                            Button("Open in browser") {
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
                    if i > 0 { Text("/").foregroundStyle(Color.syDim) }
                    Button(c.name) {
                        model.selectedFile = nil
                        model.file = nil
                        Task { await model.openDir(c.path) }
                    }
                    .buttonStyle(.plain)
                    .foregroundStyle(i == model.crumbs.count - 1 ? Color.syText : Color.syAccent)
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
                .clipShape(RoundedRectangle(cornerRadius: 10))
                .card(radius: 10, fill: .syPanel)
                .padding(10)
        } else if model.loadingDir {
            LoadingView()
        } else {
            VStack(spacing: 10) {
                IconTile(systemImage: "doc.text.magnifyingglass", color: .syTeal, size: 48)
                Text("Pick a file on the left or press ⌘P").foregroundStyle(Color.syMuted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
    }

    private func fileHeader(_ path: String) -> some View {
        HStack(spacing: 10) {
            IconTile(systemImage: "doc.text.fill", color: .sySky, size: 20)
            Text(path).font(.system(.callout, design: .monospaced).weight(.medium)).foregroundStyle(Color.syText).lineLimit(1).truncationMode(.head).textSelection(.enabled)
            if let size = model.fileSize {
                ChipView(text: ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file))
            }
            if case .text(let s, _) = model.file {
                ChipView(text: plural(s.reduce(into: 1) { if $1 == "\n" { $0 += 1 } }, "line", "lines"))
            }
            Spacer()
            if case .markdown = model.file {
                Picker("", selection: $markdownSource) {
                    Text("Preview").tag(false)
                    Text("Source").tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 160)
            }
            if case .text = model.file {
                Toggle("Wrap", isOn: $wrap).toggleStyle(.checkbox).controlSize(.small)
            }
            blameControls
            Menu {
                Button("Copy path") { copyToPasteboard(path) }
                if case .text(let s, _) = model.file { Button("Copy contents") { copyToPasteboard(s) } }
                if case .markdown(_, let s) = model.file { Button("Copy contents") { copyToPasteboard(s) } }
                Button("Copy link") { copyToPasteboard(model.webURL) }
                Button("Open in browser") { openInBrowser(model.webURL) }
            } label: {
                Image(systemName: "ellipsis.circle").foregroundStyle(Color.syMuted)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
    }

    private var blameBinding: Binding<Bool> {
        Binding(get: { model.blameOn }, set: { model.setBlame($0) })
    }

    @ViewBuilder
    private var blameControls: some View {
        if model.file != nil {
            if model.blameOn {
                if model.blameLoading {
                    ProgressView().controlSize(.mini)
                } else if let e = model.blameError {
                    Label(e, systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(Color.syAmber)
                        .lineLimit(1)
                    if model.canRetryBlame {
                        Button("Retry") { Task { await model.loadBlame() } }
                            .buttonStyle(.link)
                            .font(.caption)
                    }
                }
            }
            Toggle("Blame", isOn: blameBinding)
                .toggleStyle(.checkbox)
                .controlSize(.small)
                .help("Show who last changed each line (⌥⌘B)")
        }
    }

    private var visibleBlame: BlameMap? {
        model.blameOn ? model.blame : nil
    }

    private func openCommit(_ commit: BlameCommit) {
        app.open(.commit(model.repo, commit.oid), push: true)
    }

    @ViewBuilder
    private func fileBody(_ file: FileContent, path: String) -> some View {
        let ext = (path as NSString).pathExtension.isEmpty ? (path as NSString).lastPathComponent : (path as NSString).pathExtension
        switch file {
        case .text(let s, let truncated):
            VStack(spacing: 0) {
                if truncated {
                    Label("File is too large — showing only part of it", systemImage: "exclamationmark.triangle.fill")
                        .font(.caption).foregroundStyle(Color.syAmber).padding(6)
                }
                CodeTextView(text: s, ext: ext, wrap: wrap, blame: visibleBlame, onOpenCommit: openCommit)
                    .clipShape(RoundedRectangle(cornerRadius: 10))
                    .card(radius: 10, fill: .sySunken)
            }
            .padding([.horizontal, .bottom], 10)
        case .markdown(let html, let source):
            Group {
                if markdownSource {
                    CodeTextView(text: source, ext: ext, wrap: true, blame: visibleBlame, onOpenCommit: openCommit)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                        .card(radius: 10, fill: .sySunken)
                } else {
                    HTMLView(html: page(html), onLink: openLink)
                        .clipShape(RoundedRectangle(cornerRadius: 10))
                        .card(radius: 10, fill: .syPanel)
                }
            }
            .padding([.horizontal, .bottom], 10)
        case .image(let img):
            ScrollView([.horizontal, .vertical]) {
                Image(nsImage: img)
                    .resizable()
                    .aspectRatio(contentMode: .fit)
                    .frame(maxWidth: min(img.size.width, 1200))
                    .padding(20)
            }
            .clipShape(RoundedRectangle(cornerRadius: 10))
            .card(radius: 10, fill: .sySunken)
            .padding([.horizontal, .bottom], 10)
        case .binary(let size):
            VStack(spacing: 8) {
                IconTile(systemImage: "doc.zipper", color: .sySlate, size: 48)
                Text("Binary file" + " · " + ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)).foregroundStyle(Color.syMuted)
                Button("Open in browser") { openInBrowser(model.webURL) }
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
                .foregroundStyle(iconColor)
                .frame(width: 16)
            Text(entry.name).fontWeight(entry.isDir ? .medium : .regular).foregroundStyle(Color.syText).lineLimit(1)
            Spacer()
            if let size = entry.object?.byteSize, !entry.isDir {
                Text(ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)).font(.caption2).foregroundStyle(Color.syDim)
            }
        }
    }

    private var iconColor: Color {
        if entry.isDir { return .sySky }
        if entry.isSubmodule { return .syIndigo }
        return .syMuted
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
            TextField("Go to file…", text: $query)
                .textFieldStyle(.plain)
                .font(.title3)
                .padding(14)
                .onSubmit { open(selection ?? results.first) }
            Rectangle().fill(Color.syLine).frame(height: 1)
            if model.allPaths == nil {
                LoadingView().frame(height: 300)
            } else {
                List(results, id: \.self, selection: $selection) { p in
                    VStack(alignment: .leading, spacing: 1) {
                        Text((p as NSString).lastPathComponent).foregroundStyle(Color.syText)
                        Text(p).font(.caption.monospaced()).foregroundStyle(Color.syMuted).lineLimit(1).truncationMode(.head)
                    }
                    .tag(p)
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) { open(p) }
                }
                .frame(height: 380)
                if model.pathsTruncated {
                    Text("Large repository — file list is incomplete")
                        .font(.caption).foregroundStyle(Color.syAmber).padding(6)
                }
            }
            HStack {
                Text("↩ open · Esc close").font(.caption).foregroundStyle(Color.syDim)
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Open") { open(selection ?? results.first) }.keyboardShortcut(.defaultAction)
            }
            .padding(10)
            .background(Theme.isBrand ? Color.syBar : Color.clear)
            .overlay(alignment: .top) { Rectangle().fill(Color.syLine).frame(height: 1) }
        }
        .frame(width: 620)
        .surface(.panel)
        .task { await model.loadAllPaths() }
    }

    private func open(_ path: String?) {
        guard let path else { return }
        dismiss()
        Task { await model.reveal(path) }
    }
}
