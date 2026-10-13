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
                    Label(t("Вход в Claude не выполнен — войдите справа", "Not signed in to Claude — sign in on the right"), systemImage: "person.crop.circle.badge.exclamationmark")
                        .foregroundStyle(Color.syAmber)
                    if ClaudeProfiles.shared.profiles.count > 1 {
                        ClaudeProfileSwitcher()
                    }
                case .notInstalled:
                    Label(t("claude CLI не найден — инструкция справа", "claude CLI not found — see the right side"), systemImage: "exclamationmark.triangle")
                        .foregroundStyle(Color.syAmber)
                default:
                    EmptyView()
                }
            }
            if ClaudeAccountStore.shared.isLoggedIn {
                historyContent
            } else if case .checking = ClaudeAccountStore.shared.status {
                HStack { Spacer(); ProgressView().controlSize(.small); Spacer() }
            } else if case .unknown = ClaudeAccountStore.shared.status {
                HStack { Spacer(); ProgressView().controlSize(.small); Spacer() }
            } else {
                VStack(alignment: .leading, spacing: 6) {
                    Image(systemName: "lock.fill").font(.title3).foregroundStyle(Color.syDim)
                    Text(t("История сессий и проекты появятся после входа в Claude.", "Session history and projects appear after you sign in to Claude."))
                        .font(.callout)
                        .foregroundStyle(Color.syMuted)
                }
                .padding(.vertical, 8)
            }
        }
        .listStyle(.inset)
        .surface(.list)
        .overlay { if ws.loadingProjects && ws.projects.isEmpty { ProgressView().controlSize(.small) } }
        .searchable(text: $query, placement: .toolbar, prompt: t("Поиск по сессиям", "Search sessions"))
        .navigationTitle("Claude Code")
        .toolbar {
            ToolbarItemGroup {
                Button { openFolder() } label: { Label(t("Открыть папку", "Open folder"), systemImage: "folder.badge.plus") }
                    .disabled(!ClaudeAccountStore.shared.isLoggedIn)
                    .help(t("Новая сессия в папке…", "New session in folder…"))
                Button { Task { await ws.reloadProjects() } } label: { Label(t("Обновить", "Refresh"), systemImage: "arrow.clockwise") }
            }
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
                                    Button(t("Освободить память (усыпить)", "Free memory (sleep)")) { tab.sleepNow() }
                                        .disabled(tab.state == .working || tab.state == .waiting)
                                } else {
                                    Button(t("Разбудить", "Wake up")) { ws.restart(tab) }
                                }
                                Button(t("Закрыть", "Close")) { ws.close(tab) }
                            }
                    }
                } header: {
                    HStack(spacing: 6) {
                        Text(t("Открытые", "Open"))
                        Spacer()
                        Image(systemName: "memorychip").imageScale(.small)
                        Text(ws.totalMemory > 0 ? ProcessMemory.format(ws.totalMemory) : "0 " + t("МБ", "MB"))
                            .monospacedDigit()
                        if ws.awakeCount > 0 {
                            Button(t("Усыпить все", "Sleep all")) { ws.sleepAll() }
                                .buttonStyle(.link)
                                .help(t("Остановить простаивающие процессы и освободить память", "Stop idle processes and free memory"))
                        }
                    }
                    .font(.caption)
                    .foregroundStyle(Color.syMuted)
                    .textCase(nil)
                    .help(t("Память, которую сейчас занимают процессы Claude", "Memory used by running Claude processes"))
                }
            }
            ForEach(filtered) { project in
                Section {
                    if expanded.contains(project.id) || !query.isEmpty {
                        ForEach(project.sessions.prefix(query.isEmpty ? 30 : 200)) { s in
                            SessionRow(session: s, open: ws.tab(forSession: s.id) != nil)
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
                Text(t("Сессий Claude Code пока нет", "No Claude Code sessions yet")).foregroundStyle(Color.syMuted)
            }
    }

    @ViewBuilder
    private func sessionMenu(_ s: ClaudeSessionInfo) -> some View {
        Button(t("Продолжить как чат", "Resume as chat")) { ws.resume(s, kind: .chat) }
        Button(t("Продолжить в терминале", "Resume in terminal")) { ws.resume(s, kind: .terminal) }
        Button(t("Ответвить сессию (fork)", "Fork session")) { ws.resume(s, fork: true) }
        Divider()
        Button(t("Копировать ID сессии", "Copy session ID")) { copyToPasteboard(s.id) }
        Button(t("Копировать команду resume", "Copy resume command")) { copyToPasteboard("cd \(ClaudeLauncher.shellQuote(s.cwd)) && claude --resume \(s.id)") }
        Button(t("Показать транскрипт в Finder", "Reveal transcript in Finder")) { NSWorkspace.shared.activateFileViewerSelecting([s.file]) }
        if let pr = s.prURL { Button(t("Открыть PR", "Open PR")) { if let r = GitHubURL.parse(pr) { app.section = .prs(.mine); app.open(r) } } }
    }

    private func openFolder() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.prompt = t("Открыть", "Open")
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
                .overlay(Capsule().strokeBorder(color.opacity(0.28)))
            Button(action: onNew) { Image(systemName: "plus") }
                .buttonStyle(.borderless)
                .foregroundStyle(Color.syCoral)
                .help(t("Новая сессия", "New session"))
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
                        Image(systemName: "bubble.left.fill").font(.caption2).foregroundStyle(Color.ghGreen).help(t("Открыта", "Open"))
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
                                                Label(t("Сессия спит — память освобождена", "Session is sleeping — memory freed"), systemImage: "moon.zzz.fill").foregroundStyle(Color.syIndigo)
                                            } else {
                                                Text(t("Сессия завершена", "Session ended") + (tab.exitCode.map { " (\($0))" } ?? "")).foregroundStyle(Color.syMuted)
                                            }
                                            HStack {
                                                Button(t("Продолжить сессию", "Resume session")) { ws.restart(tab) }.buttonStyle(.borderedProminent).tint(.syCoral)
                                                Button(t("Закрыть вкладку", "Close tab")) { ws.close(tab) }
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
                    Button { ws.showInspector.toggle() } label: { Label(t("Инспектор", "Inspector"), systemImage: "sidebar.right") }
                        .help(t("Показать/скрыть панель изменений", "Toggle changes panel"))
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
            Text(t("Начните новую сессию в проекте или продолжите сессию из истории слева.", "Start a new session in a project or resume one from the history on the left."))
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
                            .help(t("Закрыть", "Close"))
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
                        Button(t("Чат в \(current.projectName)", "Chat in \(current.projectName)")) { ws.newSession(folder: current.folder, kind: .chat) }
                        Button(t("Терминал в \(current.projectName)", "Terminal in \(current.projectName)")) { ws.newSession(folder: current.folder, kind: .terminal) }
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
                .help(t("Новая сессия", "New session"))
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
            ControlMenu(icon: "cpu", title: ws.model.title, help: t("Модель", "Model")) {
                ForEach(ClaudeModelOption.allCases) { m in
                    Button { ws.model = m } label: { checkLabel(m.title, ws.model == m) }
                }
                Divider()
                Button(t("Применить к текущей сессии (/model)", "Apply to current session (/model)")) { ws.applyModelToCurrent() }
                    .disabled(!tab.running || ws.model == .standard)
            }
            ControlMenu(icon: "gauge.with.dots.needle.50percent", title: ws.effort == .standard ? "Effort" : ws.effort.rawValue, help: "Effort") {
                ForEach(ClaudeEffort.allCases) { e in
                    Button { ws.effort = e } label: { checkLabel(e.title, ws.effort == e) }
                }
            }
            ControlMenu(icon: "hand.raised", title: ws.mode.title, help: t("Режим разрешений", "Permission mode")) {
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
            .help(t("Чат или терминал (та же сессия)", "Chat or terminal (same session)"))
            Menu {
                Button(t("Перезапустить с новыми настройками", "Restart with new settings")) {
                    if tab.kind == .chat { tab.chat?.stop() } else if tab.running { tab.terminal.terminate() }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { ws.restart(tab) }
                }
                Button(t("Освободить память сейчас", "Free memory now")) { tab.sleepNow() }
                    .disabled(!tab.running || tab.state == .working || tab.state == .waiting)
                Menu(t("Усыплять после простоя", "Sleep after idle")) {
                    ForEach([0, 5, 15, 30, 60], id: \.self) { m in
                        Button { ws.idleMinutes = m } label: { checkLabel(m == 0 ? t("Никогда", "Never") : t("\(m) мин", "\(m) min"), ws.idleMinutes == m) }
                    }
                }
                Menu(t("Новые сессии открывать как", "Open new sessions as")) {
                    Button { ws.preferredKind = .chat } label: { checkLabel(t("Чат", "Chat"), ws.preferredKind == .chat) }
                    Button { ws.preferredKind = .terminal } label: { checkLabel(t("Терминал", "Terminal"), ws.preferredKind == .terminal) }
                }
                Divider()
                Button(t("Продолжить в Terminal", "Continue in Terminal")) {
                    try? ClaudeLauncher.launch(prompt: "", folder: tab.folder, target: .terminal, mode: .standard, sessionName: tab.title, resumeId: tab.sessionId, configDir: tab.configDir)
                }
                Button(t("Показать в Finder", "Reveal in Finder")) { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: tab.folder)]) }
                Button(t("Копировать ID сессии", "Copy session ID")) { copyToPasteboard(tab.sessionId) }
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
        .help(help + " — " + t("применяется к новым и перезапущенным сессиям", "applies to new and restarted sessions"))
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
                TabItem(id: InspectorTab.changes, title: t("Изменения", "Changes"), icon: "plusminus", badge: tab.git.changes.isEmpty ? nil : "\(tab.git.changes.count)", badgeColor: .syAmber),
                TabItem(id: InspectorTab.files, title: t("Файлы", "Files"), icon: "folder"),
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
                Text(git.changes.isEmpty ? t("Нет незакоммиченных изменений", "No uncommitted changes") : tp(git.changes.count, "файл изменён", "файла изменено", "файлов изменено", "file changed", "files changed"))
                    .font(.caption).foregroundStyle(Color.syMuted)
                if !git.changes.isEmpty {
                    DiffStat(additions: git.changes.reduce(0) { $0 + $1.additions }, deletions: git.changes.reduce(0) { $0 + $1.deletions })
                }
                Spacer()
                Button { Task { await git.refresh() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless)
                    .help(t("Обновить", "Refresh"))
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            if !git.unpushed.isEmpty {
                VStack(alignment: .leading, spacing: 3) {
                    Text(t("Неотправленные коммиты", "Unpushed commits") + " · \(git.unpushed.count)").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
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
                    Text(t("Изменения, которые сделает Claude, появятся здесь автоматически", "Changes Claude makes will show up here automatically"))
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
                    Text(t("Бинарный или большой файл", "Binary or large file")).foregroundStyle(Color.syMuted).frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                    Text(t("Выберите файл", "Pick a file")).foregroundStyle(Color.syDim).frame(maxWidth: .infinity, maxHeight: .infinity)
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
        case .open: return t("Открытые", "Open")
        case .merged: return t("Слитые", "Merged")
        case .mine: return t("Мои", "Mine")
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
    @State private var loading = false
    @State private var error: String?
    @State private var selected: ItemRef?

    private var repo: RepoRef? { tab.git.repo }

    var body: some View {
        Group {
            if let selected {
                InspectorPRDetail(tab: tab, ref: selected) { self.selected = nil }
            } else if repo == nil {
                VStack(spacing: 8) {
                    Image(systemName: "link.badge.plus").font(.title2).foregroundStyle(Color.syDim)
                    Text(t("Папка не связана с репозиторием GitHub", "Folder isn't linked to a GitHub repository")).foregroundStyle(Color.syMuted)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                listView
            }
        }
        .task(id: "\(repo?.full ?? "")|\(tab.git.branch ?? "")|\(scope.rawValue)") { await load() }
    }

    private var listView: some View {
        List {
            Section {
                if branchPRs.isEmpty && linked == nil {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(t("Для ветки \(tab.git.branch ?? "?") PR нет", "No PR for branch \(tab.git.branch ?? "?")"))
                            .font(.callout).foregroundStyle(Color.syMuted)
                        if let repo, let b = tab.git.branch, b != "HEAD" {
                            Button(t("Создать PR в браузере", "Create PR in browser")) { openInBrowser("https://github.com/\(repo.full)/pull/new/\(b)") }
                                .controlSize(.small)
                        }
                    }
                    .padding(.vertical, 4)
                }
                ForEach(branchPRs) { item in row(item) }
                if let linked, !branchPRs.contains(where: { $0.number == linked.number }) {
                    row(linked, note: t("из этой сессии", "from this session"))
                }
            } header: {
                HStack(spacing: 6) {
                    Text(t("Эта ветка", "This branch"))
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
                if list.isEmpty, !loading {
                    Text(t("Ничего нет", "Nothing here")).font(.callout).foregroundStyle(Color.syDim)
                }
                ForEach(list) { item in row(item) }
            } header: {
                HStack {
                    Text(repo?.full ?? "")
                    Spacer()
                    if loading { ProgressView().controlSize(.mini) }
                    Button { Task { await load() } } label: { Image(systemName: "arrow.clockwise") }
                        .buttonStyle(.borderless)
                }
                .textCase(nil)
            }
        }
        .listStyle(.inset)
        .surface(.panel)
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
        .contentShape(Rectangle())
        .onTapGesture { selected = item.ref }
        .contextMenu {
            Button(t("Открыть в разделе PR", "Open in PR section")) { app.section = .prs(.mine); app.open(item.route) }
            Button(t("Открыть в браузере", "Open in browser")) { openInBrowser(item.url) }
            Button(t("Копировать ссылку", "Copy link")) { copyToPasteboard(item.url) }
        }
    }

    private func load() async {
        guard let api = app.api, let repo else { return }
        loading = true
        defer { loading = false }
        do {
            if let b = tab.git.branch, b != "HEAD" {
                branchPRs = try await api.repoPullRequests(repo, headRefName: b, states: ["OPEN", "MERGED", "CLOSED"], first: 5)
            } else {
                branchPRs = []
            }
            if let number = app.claude.projects.flatMap(\.sessions).first(where: { $0.id == tab.sessionId })?.prNumber,
               !branchPRs.contains(where: { $0.number == number }) {
                linked = try? await api.search("repo:\(repo.full) is:pr \(number)").nodes.first { $0.number == number }
            } else {
                linked = nil
            }
            switch scope {
            case .open: list = try await api.repoPullRequests(repo, states: ["OPEN"])
            case .merged: list = try await api.repoPullRequests(repo, states: ["MERGED"])
            case .mine: list = try await api.search("repo:\(repo.full) is:pr author:@me sort:updated-desc").nodes
            }
            error = nil
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
                Button(action: onBack) { Label(t("Назад", "Back"), systemImage: "chevron.left") }
                    .buttonStyle(.borderless)
                Spacer()
                Button { openInBrowser("https://github.com/\(ref.repo.full)/pull/\(ref.number)") } label: { Image(systemName: "safari") }
                    .buttonStyle(.borderless)
                    .help(t("В браузере", "Open in browser"))
                Button(t("Открыть полностью", "Open full")) { app.section = .prs(.mine); app.open(.pr(ref)) }
                    .controlSize(.small)
            }
            .padding(10)
            ClaudeHairline()
            if let pr = model?.pr {
                ScrollView { content(pr).padding(12) }
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
            }
            HStack(spacing: 14) {
                let checks = pr.checks
                Label("\(checks.filter { $0.outcome == .success }.count)/\(checks.count)", systemImage: "checklist")
                    .foregroundStyle(checks.contains { $0.outcome == .failure } ? Color.syRed : Color.syMuted)
                Label("\(unresolved.count)", systemImage: "text.bubble")
                    .foregroundStyle(unresolved.isEmpty ? Color.syMuted : Color.syAmber)
                    .help(t("Нерешённые комментарии", "Unresolved comments"))
                if !model!.reviewers.isEmpty {
                    HStack(spacing: 3) {
                        ForEach(model!.reviewers, id: \.login) { ReviewerBadge(login: $0.login, avatar: $0.avatar, state: $0.state) }
                    }
                }
            }
            .font(.caption)
            if tab.chat != nil || tab.running {
                VStack(alignment: .leading, spacing: 6) {
                    Text(t("Отправить в эту сессию", "Send to this session")).font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                    if let fix {
                        Button {
                            app.claude.send(FixPromptExtractor.wrap(fix, pr: pr, options: FixOptions()), to: tab)
                        } label: {
                            Label(t("Fix prompt от @\(fix.author)", "Fix prompt by @\(fix.author)"), systemImage: "wand.and.stars")
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                    Button {
                        app.claude.send(FixPrompt.build(pr: pr, repo: ref.repo, threads: unresolved, general: FixPrompt.generalFeedback(pr), options: FixOptions()), to: tab)
                    } label: {
                        Label(t("Нерешённые комментарии (\(unresolved.count))", "Unresolved comments (\(unresolved.count))"), systemImage: "text.bubble")
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .disabled(unresolved.isEmpty)
                    Button {
                        app.claude.send(t("Посмотри PR #\(pr.number) (\(pr.url)) и кратко расскажи, что в нём сделано и есть ли проблемы.", "Review PR #\(pr.number) (\(pr.url)) and briefly summarize what it does and any problems."), to: tab)
                    } label: {
                        Label(t("Попросить Claude разобрать PR", "Ask Claude to review this PR"), systemImage: "sparkles")
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    .tint(.syCoral)
                }
                .controlSize(.small)
            }
            if !unresolved.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text(t("Нерешённые комментарии", "Unresolved comments")).font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
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
                Text(t("Изменённые файлы · \(pr.changedFiles)", "Files changed · \(pr.changedFiles)")).font(.caption.weight(.semibold))
            }
            .onChange(of: showFiles) { _, open in if open { Task { await model?.loadFiles() } } }
        }
    }
}
