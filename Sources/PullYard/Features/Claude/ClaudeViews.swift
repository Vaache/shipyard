import AppKit
import SwiftUI

struct ClaudeTerminalHost: NSViewRepresentable {
    let tab: ClaudeTab

    func makeNSView(context: Context) -> NSView {
        let container = NSView()
        attach(to: container)
        return container
    }

    func updateNSView(_ container: NSView, context: Context) {
        if tab.terminal.superview !== container {
            container.subviews.forEach { $0.removeFromSuperview() }
            attach(to: container)
        }
    }

    private func attach(to container: NSView) {
        let term = tab.terminal
        term.removeFromSuperview()
        term.applyAppearance()
        term.frame = container.bounds
        term.autoresizingMask = [.width, .height]
        container.addSubview(term)
        DispatchQueue.main.async { term.window?.makeFirstResponder(term) }
    }
}

struct ClaudeProjectsColumn: View {
    @Environment(AppModel.self) private var app
    @State private var query = ""
    @State private var expanded: Set<String> = []
    @State private var ready = false

    private var ws: ClaudeWorkspace { app.claude }

    private var filtered: [ClaudeProject] {
        guard !query.isEmpty else { return ws.projects }
        return ws.projects.compactMap { p in
            if p.name.localizedCaseInsensitiveContains(query) { return p }
            let s = p.sessions.filter { $0.title.localizedCaseInsensitiveContains(query) || ($0.branch ?? "").localizedCaseInsensitiveContains(query) }
            return s.isEmpty ? nil : ClaudeProject(id: p.id, path: p.path, sessions: s)
        }
    }

    var body: some View {
        List {
            Section {
                switch ClaudeAccountStore.shared.status {
                case .loggedIn(let info):
                    ClaudeAccountCard(info: info, compact: true)
                        .listRowInsets(EdgeInsets(top: 4, leading: 8, bottom: 4, trailing: 8))
                case .loggedOut:
                    Label("Not signed in to Claude — sign in on the right", systemImage: "person.crop.circle.badge.exclamationmark")
                        .foregroundStyle(Color.syAmber)
                    if ClaudeProfiles.shared.profiles.count > 1 {
                        ClaudeProfileSwitcher()
                    }
                case .notInstalled:
                    Label("claude CLI not found — see the right side", systemImage: "exclamationmark.triangle")
                        .foregroundStyle(Color.syAmber)
                default:
                    EmptyView()
                }
            }
            if ClaudeAccountStore.shared.isLoggedIn {
                if ready { historyContent }
            } else if case .checking = ClaudeAccountStore.shared.status {
                HStack { Spacer(); ProgressView().controlSize(.small); Spacer() }
            } else if case .unknown = ClaudeAccountStore.shared.status {
                HStack { Spacer(); ProgressView().controlSize(.small); Spacer() }
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    Image(systemName: "lock.fill").font(.title3).foregroundStyle(Color.syDim)
                    Text("Session history and projects appear after you sign in to Claude.")
                        .font(.callout)
                        .foregroundStyle(Color.syMuted)
                }
                .padding(.vertical, 8)
            }
        }
        .listStyle(.inset)
        .surface(.list)
        .overlay { if ws.loadingProjects && ws.projects.isEmpty { ProgressView().controlSize(.small) } }
        .liveRefresh(updated: ws.projectsUpdated, refreshing: ws.loadingProjects) { await ws.reloadProjects() }
        .searchable(text: $query, placement: .toolbar, prompt: "Search sessions")
        .navigationTitle("Claude Code")
        .toolbar {
            ToolbarItemGroup {
                Button { openFolder() } label: { Label("Open folder", systemImage: "folder.badge.plus") }
                    .disabled(!ClaudeAccountStore.shared.isLoggedIn)
                    .help("New session in folder…")
            }
        }
        .onAppear {
            ws.sessionRows.ingest(cascadeIDs, mode: .cascade)
            ready = true
        }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            Task { await ClaudeAccountStore.shared.refresh() }
        }
        .task {
            await ClaudeAccountStore.shared.refresh()
            await ws.reloadProjects()
            if expanded.isEmpty, let first = ws.projects.first { expanded.insert(first.id) }
        }
        .onChange(of: app.refreshTick) { Task { await ws.reloadProjects() } }
    }

    private var cascadeIDs: [String] {
        let open = expanded.isEmpty ? Set(ws.projects.prefix(1).map(\.id)) : expanded
        return filtered.filter { open.contains($0.id) }.flatMap { $0.sessions.prefix(30).map(\.id) }
    }

    @ViewBuilder
    private var historyContent: some View {
            if !ws.tabs.isEmpty {
                Section {
                    ForEach(ws.tabs) { tab in
                        OpenTabRow(tab: tab, selected: ws.selectedTabID == tab.id)
                            .contentShape(Rectangle())
                            .onTapGesture { ws.selectedTabID = tab.id }
                            .listRowBackground(ClaudeRowHighlight(selected: ws.selectedTabID == tab.id))
                            .contextMenu {
                                if tab.running {
                                    Button("Free memory (sleep)") { tab.sleepNow() }
                                        .disabled(tab.state == .working || tab.state == .waiting)
                                } else {
                                    Button("Wake up") { ws.restart(tab) }
                                }
                                Button("Close") { ws.close(tab) }
                            }
                    }
                } header: {
                    HStack(spacing: 6) {
                        Text("Open")
                        Spacer()
                        Image(systemName: "memorychip").imageScale(.small)
                        Text(ws.totalMemory > 0 ? ProcessMemory.format(ws.totalMemory) : "0 " + "MB")
                            .monospacedDigit()
                        if ws.awakeCount > 0 {
                            Button("Sleep all") { ws.sleepAll() }
                                .buttonStyle(.link)
                                .help("Stop idle processes and free memory")
                        }
                    }
                    .font(.caption)
                    .foregroundStyle(Color.syMuted)
                    .textCase(nil)
                    .help("Memory used by running Claude processes")
                }
            }
            ForEach(filtered) { project in
                Section {
                    if expanded.contains(project.id) || !query.isEmpty {
                        ForEach(project.sessions.prefix(query.isEmpty ? 30 : 200)) { s in
                            SessionRow(session: s, open: ws.tab(forSession: s.id) != nil)
                                .rowAppear(s.id, ws.sessionRows)
                                .contentShape(Rectangle())
                                .onTapGesture { ws.resume(s) }
                                .contextMenu { sessionMenu(s) }
                        }
                    }
                } header: {
                    ProjectHeader(project: project, expanded: expanded.contains(project.id) || !query.isEmpty) {
                        if expanded.contains(project.id) { expanded.remove(project.id) } else { expanded.insert(project.id) }
                    } onNew: {
                        ws.newSession(folder: project.path)
                    }
                }
            }
            if ws.projects.isEmpty, !ws.loadingProjects {
                Text("No Claude Code sessions yet").foregroundStyle(Color.syMuted)
            }
    }

    @ViewBuilder
    private func sessionMenu(_ s: ClaudeSessionInfo) -> some View {
        Button("Resume as chat") { ws.resume(s, kind: .chat) }
        Button("Resume in terminal") { ws.resume(s, kind: .terminal) }
        Button("Fork session") { ws.resume(s, fork: true) }
        Divider()
        Button("Copy session ID") { copyToPasteboard(s.id) }
        Button("Copy resume command") { copyToPasteboard("cd \(ClaudeLauncher.shellQuote(s.cwd)) && claude --resume \(s.id)") }
        Button("Reveal transcript in Finder") { NSWorkspace.shared.activateFileViewerSelecting([s.file]) }
        if let pr = s.prURL { Button("Open PR") { if let r = GitHubURL.parse(pr) { app.section = .prs(.mine); app.open(r) } } }
    }

    private func openFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.prompt = "Open"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        ws.newSession(folder: url.path)
    }
}

struct ProjectHeader: View {
    let project: ClaudeProject
    let expanded: Bool
    let onToggle: () -> Void
    let onNew: () -> Void

    var body: some View {
        let color = ProjectPalette.color(for: project.path)
        HStack(spacing: 7) {
            Image(systemName: expanded ? "chevron.down" : "chevron.right").font(.caption2.weight(.bold)).foregroundStyle(Color.syDim).frame(width: 10)
            IconTile(systemImage: project.exists ? "folder.fill" : "folder.badge.questionmark", color: project.exists ? color : .sySlate, size: 24)
            VStack(alignment: .leading, spacing: 0) {
                Text(project.name).font(.callout.weight(.semibold)).foregroundStyle(Color.syText).lineLimit(1)
                Text(project.path.replacingOccurrences(of: FileManager.default.homeDirectoryForCurrentUser.path, with: "~"))
                    .font(.caption2).foregroundStyle(Color.syDim).lineLimit(1).truncationMode(.middle)
            }
            Spacer()
            Text("\(project.sessions.count)")
                .font(.caption2.weight(.semibold).monospacedDigit())
                .foregroundStyle(color)
                .padding(.horizontal, 6)
                .padding(.vertical, 1)
                .background(Capsule().fill(color.opacity(0.14)))
            Button(action: onNew) { Image(systemName: "plus") }
                .buttonStyle(.borderless)
                .foregroundStyle(Color.syCoral)
                .help("New session")
                .disabled(!project.exists)
        }
        .contentShape(Rectangle())
        .onTapGesture(perform: onToggle)
        .textCase(nil)
        .padding(.vertical, 3)
        .opacity(project.exists ? 1 : 0.6)
    }
}

struct SessionRow: View {
    let session: ClaudeSessionInfo
    let open: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            RoundedRectangle(cornerRadius: 1.5)
                .fill(ProjectPalette.color(for: ClaudeSessionParser.projectRoot(for: session.cwd)).opacity(open ? 1 : 0.45))
                .frame(width: 3)
            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline, spacing: 6) {
                    if open {
                        Image(systemName: "bubble.left.fill").font(.caption2).foregroundStyle(Color.ghGreen).help("Open")
                    }
                    Text(session.title).lineLimit(2).fontWeight(open ? .semibold : .regular).foregroundStyle(Color.syText)
                }
                HStack(spacing: 5) {
                    if let b = session.branch { BranchTag(name: b) }
                    if let w = session.worktreeName {
                        Label(w, systemImage: "square.stack.3d.up").labelStyle(CompactLabelStyle()).font(.caption).foregroundStyle(Color.syTeal).lineLimit(1)
                    }
                    if let n = session.prNumber { PRTag(number: n) }
                    Spacer(minLength: 4)
                    if let a = session.linesAdded, let r = session.linesRemoved, a + r > 0 {
                        DiffStat(additions: a, deletions: r)
                    }
                    Text(Fmt.rel(session.modified)).font(.caption).foregroundStyle(Color.syDim).fixedSize()
                }
            }
        }
        .padding(.vertical, 3)
    }
}

struct OpenTabRow: View {
    let tab: ClaudeTab
    let selected: Bool

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            RoundedRectangle(cornerRadius: 1.5).fill(ProjectPalette.color(for: tab.folder)).frame(width: 3)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Image(systemName: tab.kind == .chat ? "bubble.left.and.text.bubble.right" : "terminal").font(.caption2).foregroundStyle(Color.syMuted)
                    Text(tab.title).lineLimit(1).fontWeight(selected ? .semibold : .regular).foregroundStyle(Color.syText)
                }
                HStack(spacing: 6) {
                    TabStateBadge(state: tab.state, memory: tab.memoryBytes)
                    Spacer(minLength: 4)
                    Text(tab.projectName).font(.caption).foregroundStyle(ProjectPalette.color(for: tab.folder)).lineLimit(1)
                }
            }
        }
        .padding(.vertical, 2)
    }
}

struct ClaudeRowHighlight: View {
    let selected: Bool
    var color: Color = .sySky

    var body: some View {
        if selected {
            ZStack(alignment: .leading) {
                color.opacity(0.14)
                Rectangle().fill(color).frame(width: 3)
            }
        } else {
            Color.clear
        }
    }
}

struct ClaudeHairline: View {
    var body: some View {
        Rectangle().fill(Color.syLine).frame(height: 1)
    }
}

struct ClaudeBarBackground: ViewModifier {
    func body(content: Content) -> some View {
        if Theme.isBrand {
            content
                .background(Color.syBar)
                .overlay(alignment: .bottom) { ClaudeHairline() }
        } else {
            content.background(.bar)
        }
    }
}

struct CompactLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(spacing: 3) {
            configuration.icon.imageScale(.small)
            configuration.title
        }
    }
}

struct ClaudeWorkspaceView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        let ws = app.claude
        let account = ClaudeAccountStore.shared
        Group {
            if case .loggedOut = account.status {
                ClaudeAuthView()
            } else if case .notInstalled = account.status {
                ClaudeAuthView()
            } else if let tab = ws.selectedTab {
                HSplitView {
                    VStack(spacing: 0) {
                        ClaudeTabBar()
                        ClaudeControlBar(tab: tab)
                        if !Theme.isBrand { Divider() }
                        Group {
                            if let chat = tab.chat {
                                ChatView(tab: tab, chat: chat)
                            } else {
                                ZStack {
                                    ClaudeTerminalHost(tab: tab)
                                    if !tab.running {
                                        VStack(spacing: 10) {
                                            if tab.terminalSleeping {
                                                Label("Session is sleeping — memory freed", systemImage: "moon.zzz.fill").foregroundStyle(Color.syIndigo)
                                            } else {
                                                Text("Session ended" + (tab.exitCode.map { " (\($0))" } ?? "")).foregroundStyle(Color.syMuted)
                                            }
                                            HStack {
                                                Button("Resume session") { ws.restart(tab) }.buttonStyle(.borderedProminent).tint(.syCoral)
                                                Button("Close tab") { ws.close(tab) }
                                            }
                                        }
                                        .padding(16)
                                        .card(radius: 12)
                                        .transition(.pop)
                                    }
                                }
                                .animation(Motion.spring, value: tab.running)
                            }
                        }
                        .id(tab.id)
                        .transition(.opacity)
                    }
                    .animation(Motion.fade, value: tab.id)
                    .frame(minWidth: 520, idealWidth: 900)
                    .surface(.panel)
                    .layoutPriority(1)
                    if ws.showInspector {
                        ClaudeInspector(tab: tab)
                            .frame(minWidth: 300, idealWidth: 380, maxWidth: 640)
                    }
                }
            } else {
                ClaudeEmptyState()
            }
        }
        .navigationTitle(ws.selectedTab?.title ?? "Claude Code")
        .navigationSubtitle(ws.selectedTab?.folder.replacingOccurrences(of: FileManager.default.homeDirectoryForCurrentUser.path, with: "~") ?? "")
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                if ws.selectedTab != nil {
                    Button { ws.showInspector.toggle() } label: { Label("Inspector", systemImage: "sidebar.right") }
                        .help("Toggle changes panel")
                }
            }
        }
    }
}

struct ClaudeEmptyState: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        let ws = app.claude
        VStack(spacing: 16) {
            IconTile(systemImage: "sparkles", color: .syCoral, size: 56)
            Text("Claude Code").font(.title2.weight(.semibold)).foregroundStyle(Color.syText)
            Text("Start a new session in a project or resume one from the history on the left.")
                .foregroundStyle(Color.syMuted)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 420)
            VStack(alignment: .leading, spacing: 6) {
                ForEach(ws.projects.filter(\.exists).prefix(6)) { p in
                    Button {
                        ws.newSession(folder: p.path)
                    } label: {
                        HStack(spacing: 10) {
                            IconTile(systemImage: "folder.fill", color: ProjectPalette.color(for: p.path), size: 22)
                            Text(p.name).fontWeight(.medium).foregroundStyle(Color.syText)
                            Spacer()
                            Text(Fmt.rel(p.lastActivity)).font(.caption).foregroundStyle(Color.syDim)
                            Image(systemName: "plus.circle.fill").foregroundStyle(Color.syCoral)
                        }
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .contentShape(Rectangle())
                        .card(radius: 10)
                    }
                    .buttonStyle(.plain)
                }
            }
            .frame(width: 380)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .surface(.panel)
    }
}

struct ClaudeTabBar: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        let ws = app.claude
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 4) {
                ForEach(ws.tabs) { tab in
                    let selected = tab.id == ws.selectedTabID
                    HStack(spacing: 6) {
                        RoundedRectangle(cornerRadius: 2).fill(ProjectPalette.color(for: tab.folder)).frame(width: 3, height: 14)
                        TabStateBadge(state: tab.state, compact: true)
                        Image(systemName: tab.kind == .chat ? "bubble.left.and.text.bubble.right" : "terminal").font(.caption2).foregroundStyle(Color.syMuted)
                        Text(tab.title).lineLimit(1).frame(maxWidth: 180, alignment: .leading).foregroundStyle(selected ? Color.syText : Color.syMuted)
                        Text(tab.projectName).font(.caption).foregroundStyle(Color.syDim).lineLimit(1)
                        Button { ws.close(tab) } label: { Image(systemName: "xmark").font(.caption2.weight(.bold)) }
                            .buttonStyle(.borderless)
                            .foregroundStyle(Color.syDim)
                            .help("Close")
                    }
                    .font(.callout)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 5)
                    .background(RoundedRectangle(cornerRadius: 8).fill(selected ? Color.syAccent.opacity(0.14) : Color.syText.opacity(0.04)))
                    .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(selected ? Color.syAccent.opacity(0.45) : Color.syLine))
                    .contentShape(Rectangle())
                    .onTapGesture { ws.selectedTabID = tab.id }
                }
                Menu {
                    if let current = ws.selectedTab {
                        Button("Chat in \(current.projectName)") { ws.newSession(folder: current.folder, kind: .chat) }
                        Button("Terminal in \(current.projectName)") { ws.newSession(folder: current.folder, kind: .terminal) }
                        Divider()
                    }
                    ForEach(ws.projects.filter(\.exists).prefix(12)) { p in
                        Button(p.name) { ws.newSession(folder: p.path) }
                    }
                } label: {
                    Image(systemName: "plus")
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("New session")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
        }
        .modifier(ClaudeBarBackground())
    }
}

struct ClaudeControlBar: View {
    @Environment(AppModel.self) private var app
    let tab: ClaudeTab

    var body: some View {
        let ws = app.claude
        HStack(spacing: 6) {
            HStack(spacing: 6) {
                IconTile(systemImage: "folder.fill", color: ProjectPalette.color(for: tab.folder), size: 18)
                Text(tab.projectName).fontWeight(.semibold).foregroundStyle(Color.syText).lineLimit(1)
                if let b = tab.git.branch, b != "HEAD" { BranchTag(name: b) }
                TabStateBadge(state: tab.state, memory: tab.memoryBytes)
                    .padding(.leading, 4)
            }
            .fixedSize(horizontal: false, vertical: true)
            .layoutPriority(1)
            Spacer(minLength: 8)
            if tab.kind == .terminal {
            ControlMenu(icon: "cpu", title: ws.model.title, help: "Model") {
                ForEach(ClaudeModelOption.allCases) { m in
                    Button { ws.model = m } label: { checkLabel(m.title, ws.model == m) }
                }
                Divider()
                Button("Apply to current session (/model)") { ws.applyModelToCurrent() }
                    .disabled(!tab.running || ws.model == .standard)
            }
            ControlMenu(icon: "gauge.with.dots.needle.50percent", title: ws.effort == .standard ? "Effort" : ws.effort.rawValue, help: "Effort") {
                ForEach(ClaudeEffort.allCases) { e in
                    Button { ws.effort = e } label: { checkLabel(e.title, ws.effort == e) }
                }
            }
            ControlMenu(icon: "hand.raised", title: ws.mode.title, help: "Permission mode") {
                ForEach(ClaudeMode.allCases) { m in
                    Button { ws.mode = m } label: { checkLabel(m.title, ws.mode == m) }
                }
            }
            } else if let chat = tab.chat, chat.totalCost > 0 || chat.sessionId != nil {
                Text(chat.model.replacingOccurrences(of: "claude-", with: "")).font(.caption).foregroundStyle(Color.syMuted)
            }
            Picker("", selection: Binding(get: { tab.kind }, set: { k in
                if k == .terminal && tab.kind == .chat { ws.openInTerminal(tab) }
                if k == .chat && tab.kind == .terminal { ws.openInChat(tab) }
            })) {
                Image(systemName: "bubble.left.and.text.bubble.right").tag(ClaudeTabKind.chat)
                Image(systemName: "terminal").tag(ClaudeTabKind.terminal)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .fixedSize()
            .disabled(tab.sessionId.isEmpty)
            .help("Chat or terminal (same session)")
            Menu {
                Button("Restart with new settings") {
                    if tab.kind == .chat { tab.chat?.stop() } else if tab.running { tab.terminal.terminate() }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { ws.restart(tab) }
                }
                Button("Free memory now") { tab.sleepNow() }
                    .disabled(!tab.running || tab.state == .working || tab.state == .waiting)
                Menu("Sleep after idle") {
                    ForEach([0, 5, 15, 30, 60], id: \.self) { m in
                        Button { ws.idleMinutes = m } label: { checkLabel(m == 0 ? "Never" : "\(m) min", ws.idleMinutes == m) }
                    }
                }
                Menu("Open new sessions as") {
                    Button { ws.preferredKind = .chat } label: { checkLabel("Chat", ws.preferredKind == .chat) }
                    Button { ws.preferredKind = .terminal } label: { checkLabel("Terminal", ws.preferredKind == .terminal) }
                }
                Divider()
                Button("Continue in Terminal") {
                    try? ClaudeLauncher.launch(prompt: "", folder: tab.folder, target: .terminal, mode: .standard, sessionName: tab.title, resumeId: tab.sessionId, configDir: tab.configDir)
                }
                Button("Reveal in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: tab.folder)]) }
                Button("Copy session ID") { copyToPasteboard(tab.sessionId) }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .font(.callout)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .modifier(ClaudeBarBackground())
    }

    @ViewBuilder
    private func checkLabel(_ title: String, _ on: Bool) -> some View {
        if on { Label(title, systemImage: "checkmark") } else { Text(title) }
    }
}

struct ControlMenu<Content: View>: View {
    let icon: String
    let title: String
    let help: String
    @ViewBuilder let content: Content

    var body: some View {
        Menu {
            content
        } label: {
            HStack(spacing: 4) {
                Image(systemName: icon)
                Text(title).lineLimit(1)
            }
        }
        .menuStyle(.button)
        .buttonStyle(.bordered)
        .controlSize(.small)
        .fixedSize()
        .help(help + " — " + "applies to new and restarted sessions")
    }
}

enum InspectorTab: String, CaseIterable, Identifiable {
    case changes, files, pr
    var id: String { rawValue }
}

struct ClaudeInspector: View {
    let tab: ClaudeTab
    @State private var selection: InspectorTab = .changes

    var body: some View {
        VStack(spacing: 0) {
            TabStrip(items: [
                TabItem(id: InspectorTab.changes, title: "Changes", icon: "plusminus", badge: tab.git.changes.isEmpty ? nil : "\(tab.git.changes.count)", badgeColor: .syAmber),
                TabItem(id: InspectorTab.files, title: "Files", icon: "folder"),
                TabItem(id: InspectorTab.pr, title: "PR", icon: "arrow.triangle.pull")
            ], selection: $selection)
            .padding(.top, 4)
            ClaudeHairline()
            switch selection {
            case .changes: ChangesPanel(tab: tab)
            case .files: LocalFilesPanel(root: tab.folder)
            case .pr: BranchPRPanel(tab: tab)
            }
        }
        .background(Theme.isBrand ? Color.syPanel : Color(nsColor: .windowBackgroundColor))
        .overlay(alignment: .leading) { if Theme.isBrand { Rectangle().fill(Color.syLine).frame(width: 1) } }
    }
}

struct ChangesPanel: View {
    let tab: ClaudeTab
    @State private var collapsed: Set<String> = []
    @State private var target: String?

    var body: some View {
        let git = tab.git
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                if git.loading { ProgressView().controlSize(.mini) }
                Text(git.changes.isEmpty ? "No uncommitted changes" : plural(git.changes.count, "file changed", "files changed"))
                    .font(.caption).foregroundStyle(Color.syMuted)
                if !git.changes.isEmpty {
                    DiffStat(additions: git.changes.reduce(0) { $0 + $1.additions }, deletions: git.changes.reduce(0) { $0 + $1.deletions })
                }
                Spacer()
                Button { Task { await git.refresh() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless)
                    .help("Refresh")
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            if !git.unpushed.isEmpty {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Unpushed commits" + " · \(git.unpushed.count)").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                    ForEach(git.unpushed.prefix(6)) { c in
                        HStack(spacing: 6) {
                            Text(c.short).font(.system(.caption, design: .monospaced)).foregroundStyle(Color.syMuted)
                            Text(c.subject).font(.caption).lineLimit(1)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 10)
                .padding(.bottom, 6)
            }
            ClaudeHairline()
            if git.changes.isEmpty {
                VStack(spacing: 8) {
                    Image(systemName: "checkmark.circle").font(.largeTitle).foregroundStyle(Color.syGreen.opacity(0.6))
                    Text("Changes Claude makes will show up here automatically")
                        .font(.callout).foregroundStyle(Color.syMuted).multilineTextAlignment(.center)
                }
                .padding(24)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                PlainDiffList(files: git.changes, collapsed: $collapsed, scrollTarget: target) { path in
                    NSWorkspace.shared.open(URL(fileURLWithPath: tab.folder + "/" + path))
                }
            }
        }
        .task(id: tab.id) { await git.refresh() }
    }
}

struct LocalFilesPanel: View {
    let root: String
    @State private var dir = ""
    @State private var entries: [TreeEntry] = []
    @State private var file: String?
    @State private var content: String?

    var body: some View {
        VSplitView {
            VStack(spacing: 0) {
                HStack(spacing: 4) {
                    Button { if !dir.isEmpty { dir = (dir as NSString).deletingLastPathComponent } } label: { Image(systemName: "chevron.left") }
                        .buttonStyle(.borderless)
                        .disabled(dir.isEmpty)
                    Text(dir.isEmpty ? (root as NSString).lastPathComponent : dir).font(.caption.monospaced()).foregroundStyle(Color.syMuted).lineLimit(1).truncationMode(.head)
                    Spacer()
                }
                .padding(.horizontal, 8)
                .padding(.vertical, 5)
                ClaudeHairline()
                List(entries) { e in
                    EntryRow(entry: e)
                        .contentShape(Rectangle())
                        .onTapGesture {
                            if e.isDir { dir = e.path } else { file = e.path }
                        }
                        .listRowBackground(ClaudeRowHighlight(selected: file == e.path))
                }
                .listStyle(.plain)
                .surface(.panel)
            }
            .frame(minHeight: 140, idealHeight: 220)
            Group {
                if let file, let content {
                    CodeTextView(text: content, ext: (file as NSString).pathExtension.isEmpty ? (file as NSString).lastPathComponent : (file as NSString).pathExtension)
                } else if file != nil {
                    Text("Binary or large file").foregroundStyle(Color.syMuted).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Text("Pick a file").foregroundStyle(Color.syDim).frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(minHeight: 160)
        }
        .task(id: dir) { load() }
        .task(id: file) {
            guard let file else { content = nil; return }
            let full = root + "/" + file
            content = await Task.detached {
                guard let attrs = try? FileManager.default.attributesOfItem(atPath: full), (attrs[.size] as? Int ?? 0) < 2_000_000 else { return nil }
                return try? String(contentsOfFile: full, encoding: .utf8)
            }.value
        }
    }

    private func load() {
        let fm = FileManager.default
        let base = dir.isEmpty ? root : root + "/" + dir
        let names = (try? fm.contentsOfDirectory(atPath: base)) ?? []
        entries = names.filter { $0 != ".git" && $0 != ".DS_Store" }.compactMap { name in
            var isDir: ObjCBool = false
            let full = base + "/" + name
            guard fm.fileExists(atPath: full, isDirectory: &isDir) else { return nil }
            let size = (try? fm.attributesOfItem(atPath: full)[.size] as? Int) ?? nil
            return TreeEntry(name: name, path: dir.isEmpty ? name : dir + "/" + name, type: isDir.boolValue ? "tree" : "blob", object: BlobMeta(byteSize: isDir.boolValue ? nil : size, isBinary: nil))
        }
        .sorted { a, b in a.isDir != b.isDir ? a.isDir : a.name.localizedStandardCompare(b.name) == .orderedAscending }
    }
}

enum PRListScope: String, CaseIterable, Identifiable {
    case open, merged, mine
    var id: String { rawValue }

    var title: String {
        switch self {
        case .open: return "Open"
        case .merged: return "Merged"
        case .mine: return "Mine"
        }
    }
}

struct BranchPRPanel: View {
    @Environment(AppModel.self) private var app
    let tab: ClaudeTab
    @State private var branchPRs: [SearchItem] = []
    @State private var linked: SearchItem?
    @State private var list: [SearchItem] = []
    @State private var scope: PRListScope = .open
    @State private var branchLoading = false
    @State private var listLoading = false
    @State private var branchError: String?
    @State private var error: String?
    @State private var lastUpdated: Date?
    @State private var selected: ItemRef?
    @State private var rows = RowAppearTracker()
    @State private var listScope: PRListScope?

    private var repo: RepoRef? { tab.git.repo }

    var body: some View {
        Group {
            if let selected {
                InspectorPRDetail(tab: tab, ref: selected) { self.selected = nil }
            } else if repo == nil {
                VStack(spacing: 8) {
                    Image(systemName: "link.badge.plus").font(.title2).foregroundStyle(Color.syDim)
                    Text("Folder isn't linked to a GitHub repository").foregroundStyle(Color.syMuted)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                listView
            }
        }
        .task(id: "\(repo?.full ?? "")|\(tab.git.branch ?? "")") { await loadBranch() }
        .task(id: "\(repo?.full ?? "")|\(scope.rawValue)") { await loadList() }
        .onChange(of: app.liveTick) { if selected == nil { Diagnostics.liveTask { await load() } } }
        .onChange(of: app.refreshTick) { if selected == nil { Task { await load() } } }
        .onChange(of: tab.busy) { _, busy in if !busy { Task { await loadBranch() } } }
        .onChange(of: tab.git.ahead) { Task { await loadBranch() } }
    }

    private var listView: some View {
        List {
            Section {
                if let branchError, branchPRs.isEmpty {
                    Text(branchError).font(.caption).foregroundStyle(Color.syRed)
                } else if branchPRs.isEmpty && linked == nil && !branchLoading {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("No PR for branch \(tab.git.branch ?? "?")")
                            .font(.callout).foregroundStyle(Color.syMuted)
                        if let repo, let b = tab.git.branch, b != "HEAD" {
                            Button("Create PR in browser") { openInBrowser("https://github.com/\(repo.full)/pull/new/\(b)") }
                                .controlSize(.small)
                        }
                    }
                    .padding(.vertical, 4)
                }
                ForEach(branchPRs) { item in row(item) }
                if let linked, !branchPRs.contains(where: { $0.number == linked.number }) {
                    row(linked, note: "from this session")
                }
            } header: {
                HStack(spacing: 6) {
                    Text("This branch")
                    if let b = tab.git.branch { BranchTag(name: b) }
                }
                .textCase(nil)
            }
            Section {
                Picker("", selection: $scope) {
                    ForEach(PRListScope.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .listRowSeparator(.hidden)
                if let error { Text(error).font(.caption).foregroundStyle(Color.syRed) }
                if list.isEmpty, !listLoading, error == nil {
                    Text("Nothing here").font(.callout).foregroundStyle(Color.syDim)
                }
                ForEach(list) { item in row(item) }
            } header: {
                HStack {
                    Text(repo?.full ?? "")
                    Spacer()
                    RefreshStatus(updated: lastUpdated, refreshing: branchLoading || listLoading, error: branchError ?? error, compact: true) {
                        Task { await load() }
                    }
                }
                .textCase(nil)
            }
        }
        .listStyle(.inset)
        .surface(.panel)
        .liveRefresh(updated: lastUpdated, refreshing: branchLoading || listLoading, footer: false) { await load() }
    }

    private func row(_ item: SearchItem, note: String? = nil) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .top, spacing: 6) {
                StateBadge(state: item.visualState, compact: true)
                Text(item.title).lineLimit(2).fontWeight(.medium).foregroundStyle(Color.syText)
            }
            HStack(spacing: 6) {
                Text("#\(item.number)").foregroundStyle(Color.syMuted)
                Text(item.author?.login ?? "").foregroundStyle(Color.syMuted)
                CIIcon(state: item.ciState)
                if let a = item.additions, let d = item.deletions { DiffStat(additions: a, deletions: d) }
                if let note { Text(note).foregroundStyle(Color.syViolet) }
                Spacer()
                Text(Fmt.rel(item.updatedAt)).foregroundStyle(Color.syDim)
            }
            .font(.caption)
        }
        .padding(.vertical, 2)
        .rowAppear(item.id, rows)
        .contentShape(Rectangle())
        .onTapGesture { selected = item.ref }
        .contextMenu {
            Button("Open in PR section") { app.section = .prs(.mine); app.open(item.route) }
            Button("Open in browser") { openInBrowser(item.url) }
            Button("Copy link") { copyToPasteboard(item.url) }
        }
    }

    private func load() async {
        async let b: Void = loadBranch()
        async let l: Void = loadList()
        _ = await (b, l)
    }

    private func loadBranch() async {
        guard let api = app.api, let repo else { return }
        let branch = tab.git.branch
        branchLoading = true
        defer { branchLoading = false }
        do {
            var found: [SearchItem] = []
            if let branch, branch != "HEAD" {
                found = try await api.repoPullRequests(repo, headRefName: branch, states: ["OPEN", "MERGED", "CLOSED"], first: 5)
            }
            var linkedItem: SearchItem?
            if let number = app.claude.projects.flatMap(\.sessions).first(where: { $0.id == tab.sessionId })?.prNumber,
               !found.contains(where: { $0.number == number }) {
                linkedItem = try? await api.pullRequestItem(ItemRef(repo: repo, number: number))
            }
            guard branch == tab.git.branch, repo == self.repo else { return }
            rows.ingest((found + [linkedItem].compactMap { $0 }).map(\.id), mode: .refresh)
            branchPRs = found
            linked = linkedItem
            branchError = nil
            lastUpdated = Date()
        } catch {
            if isCancellation(error) { return }
            branchError = error.localizedDescription
        }
    }

    private func loadList() async {
        guard let api = app.api, let repo else { return }
        let scope = scope
        listLoading = true
        defer { listLoading = false }
        do {
            let items: [SearchItem]
            switch scope {
            case .open: items = try await api.repoPullRequests(repo, states: ["OPEN"])
            case .merged: items = try await api.repoPullRequests(repo, states: ["MERGED"])
            case .mine: items = try await api.search("repo:\(repo.full) is:pr author:@me sort:updated-desc").nodes
            }
            guard scope == self.scope, repo == self.repo else { return }
            rows.ingest(items.map(\.id), mode: listScope == scope ? .refresh : .cascade)
            listScope = scope
            list = items
            error = nil
            lastUpdated = Date()
        } catch {
            if isCancellation(error) { return }
            self.error = error.localizedDescription
        }
    }
}

struct InspectorPRDetail: View {
    @Environment(AppModel.self) private var app
    let tab: ClaudeTab
    let ref: ItemRef
    let onBack: () -> Void
    @State private var model: PRDetailModel?
    @State private var showFiles = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Button(action: onBack) { Label("Back", systemImage: "chevron.left") }
                    .buttonStyle(.borderless)
                Spacer()
                if let model {
                    RefreshStatus(updated: model.lastUpdated, refreshing: model.loading, error: model.refreshError, compact: true) {
                        Task { await model.load(full: true) }
                    }
                }
                Button { openInBrowser("https://github.com/\(ref.repo.full)/pull/\(ref.number)") } label: { Image(systemName: "safari") }
                    .buttonStyle(.borderless)
                    .help("Open in browser")
                Button("Open full") { app.section = .prs(.mine); app.open(.pr(ref)) }
                    .controlSize(.small)
            }
            .padding(10)
            ClaudeHairline()
            if let pr = model?.pr {
                ScrollView { content(pr).padding(12) }
                    .liveRefresh(updated: model?.lastUpdated, refreshing: model?.loading ?? false, footer: false) { await model?.load(full: true) }
            } else if let e = model?.error {
                ErrorStateView(message: e) { Task { await model?.load() } }
            } else {
                LoadingView()
            }
        }
        .task(id: ref) {
            let m = app.prModel(ref)
            model = m
            await m?.load()
        }
        .onChange(of: app.liveTick) { Diagnostics.liveTask { await model?.load() } }
        .onChange(of: app.refreshTick) { Task { await model?.load(full: true) } }
        .onChange(of: tab.busy) { _, busy in if !busy { Task { await model?.load() } } }
        .alert("Error", isPresented: Binding(get: { model?.actionError != nil }, set: { if !$0 { model?.actionError = nil } })) {
            Button("OK") {}
        } message: {
            Text(model?.actionError ?? "")
        }
    }

    @ViewBuilder
    private func content(_ pr: PullRequestDetail) -> some View {
        let unresolved = pr.reviewThreads.nodes.filter { !$0.isResolved }
        let fix = FixPromptExtractor.latest(in: pr)
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 6) {
                Text(pr.title).font(.headline).textSelection(.enabled)
                HStack(spacing: 6) {
                    StateBadge(state: pr.visualState)
                    Text("#\(pr.number)").foregroundStyle(Color.syMuted)
                    DiffStat(additions: pr.additions, deletions: pr.deletions)
                }
                HStack(spacing: 4) {
                    BranchTag(name: pr.baseRefName)
                    Image(systemName: "arrow.left").font(.caption2).foregroundStyle(Color.syMuted)
                    BranchTag(name: pr.headRefName)
                }
            }
            if let m = model, pr.isOpen {
                MergeStatusBar(model: m, pr: pr, onMerge: { app.section = .prs(.mine); app.open(.pr(ref)) })
                ReReviewBanner(model: m, pr: pr, inset: false) { app.section = .prs(.mine); app.open(.pr(ref)) }
            }
            HStack(spacing: 14) {
                let checks = pr.checks
                Label("\(checks.filter { $0.outcome == .success }.count)/\(checks.count)", systemImage: "checklist")
                    .foregroundStyle(checks.contains { $0.outcome == .failure } ? Color.syRed : Color.syMuted)
                Label("\(unresolved.count)", systemImage: "text.bubble")
                    .foregroundStyle(unresolved.isEmpty ? Color.syMuted : Color.syAmber)
                    .help("Unresolved comments")
                if !model!.reviewers.isEmpty {
                    HStack(spacing: 3) {
                        ForEach(model!.reviewers, id: \.login) { ReviewerBadge(login: $0.login, avatar: $0.avatar, state: $0.state) }
                    }
                }
            }
            .font(.caption)
            if tab.chat != nil || tab.running {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Send to this session").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                    if let fix {
                        Button {
                            app.claude.send(FixPromptExtractor.wrap(fix, pr: pr, options: FixOptions()), to: tab)
                        } label: {
                            Label("Fix prompt by @\(fix.author)", systemImage: "wand.and.stars")
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    Button {
                        app.claude.send(FixPrompt.build(pr: pr, repo: ref.repo, threads: unresolved, general: FixPrompt.generalFeedback(pr), options: FixOptions()), to: tab)
                    } label: {
                        Label("Unresolved comments (\(unresolved.count))", systemImage: "text.bubble")
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .disabled(unresolved.isEmpty)
                    Button {
                        app.claude.send("Review PR #\(pr.number) (\(pr.url)) and briefly summarize what it does and any problems.", to: tab)
                    } label: {
                        Label("Ask Claude to review this PR", systemImage: "sparkles")
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .tint(.syCoral)
                }
                .controlSize(.small)
            }
            if !unresolved.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Unresolved comments").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                    ForEach(unresolved) { th in
                        VStack(alignment: .leading, spacing: 3) {
                            Text("\(th.path)\((th.line ?? th.originalLine).map { ":\($0)" } ?? "")")
                                .font(.caption.monospaced()).foregroundStyle(Color.syMuted).lineLimit(1).truncationMode(.head)
                            if let c = th.comments.nodes.first {
                                HStack(alignment: .top, spacing: 5) {
                                    Avatar(url: c.author?.avatarUrl, size: 14)
                                    MarkdownText(text: c.body).font(.callout).lineLimit(4)
                                }
                            }
                        }
                        .padding(8)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .card(radius: 8, fill: .sySunken)
                    }
                }
            }
            DisclosureGroup(isExpanded: $showFiles) {
                VStack(alignment: .leading, spacing: 3) {
                    if let m = model, m.filesLoaded {
                        ForEach(m.files) { f in
                            HStack(spacing: 6) {
                                Text(f.statusLetter).font(.caption2.weight(.bold).monospaced()).foregroundStyle(Color.syMuted)
                                Text(f.id).font(.caption.monospaced()).lineLimit(1).truncationMode(.head)
                                Spacer()
                                DiffStat(additions: f.additions, deletions: f.deletions)
                            }
                        }
                    } else {
                        ProgressView().controlSize(.small)
                    }
                }
                .padding(.top, 4)
            } label: {
                Text("Files changed · \(pr.changedFiles)").font(.caption.weight(.semibold))
            }
            .onChange(of: showFiles) { _, open in if open { Task { await model?.loadFiles() } } }
        }
    }
}
