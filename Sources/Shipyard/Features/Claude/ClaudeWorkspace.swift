import AppKit
import SwiftTerm
import SwiftUI

final class ClaudeTerminalView: LocalProcessTerminalView {
    var onTerminated: ((Int32?) -> Void)?
    var onTitle: ((String) -> Void)?
    var lastOutput = Date()

    override func dataReceived(slice: ArraySlice<UInt8>) {
        lastOutput = Date()
        super.dataReceived(slice: slice)
    }

    override func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
        if let url = URL(string: link) { LinkBrowser.open(url) }
    }

    override func processTerminated(_ source: LocalProcess, exitCode: Int32?) {
        super.processTerminated(source, exitCode: exitCode)
        DispatchQueue.main.async { self.onTerminated?(exitCode) }
    }

    func applyAppearance() {
        if Theme.isBrand {
            var bg = NSColor.syPanel, fg = NSColor.syText, caret = NSColor.syAccent
            effectiveAppearance.performAsCurrentDrawingAppearance {
                bg = NSColor.syPanel.usingColorSpace(.sRGB) ?? bg
                fg = NSColor.syText.usingColorSpace(.sRGB) ?? fg
                caret = NSColor.syAccent.usingColorSpace(.sRGB) ?? caret
            }
            nativeBackgroundColor = bg
            nativeForegroundColor = fg
            caretColor = caret
            return
        }
        let dark = effectiveAppearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
        nativeBackgroundColor = dark ? NSColor(calibratedRed: 0.11, green: 0.11, blue: 0.12, alpha: 1) : NSColor(calibratedRed: 0.99, green: 0.99, blue: 0.99, alpha: 1)
        nativeForegroundColor = dark ? NSColor(calibratedWhite: 0.9, alpha: 1) : NSColor(calibratedWhite: 0.12, alpha: 1)
        caretColor = .controlAccentColor
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyAppearance()
    }
}

enum ClaudeModelOption: String, CaseIterable, Identifiable {
    case standard = "", opus, sonnet, haiku, fable
    var id: String { rawValue }

    var title: String {
        switch self {
        case .standard: return "Default"
        case .opus: return "Opus"
        case .sonnet: return "Sonnet"
        case .haiku: return "Haiku"
        case .fable: return "Fable"
        }
    }
}

enum ClaudeEffort: String, CaseIterable, Identifiable {
    case standard = "", low, medium, high, xhigh, max
    var id: String { rawValue }
    var title: String { self == .standard ? "Default" : rawValue }
}

enum ClaudeMode: String, CaseIterable, Identifiable {
    case standard = "default", acceptEdits, plan, auto
    var id: String { rawValue }

    var title: String {
        switch self {
        case .standard: return "Ask"
        case .acceptEdits: return "Accept edits"
        case .plan: return "Plan"
        case .auto: return "Auto"
        }
    }
}

enum ClaudeTabKind: String { case chat, terminal }

@Observable
@MainActor
final class ClaudeTab: Identifiable {
    let id = UUID()
    let folder: String
    let kind: ClaudeTabKind
    var title: String
    var sessionId: String
    var terminalRunning = false
    var terminalSleeping = false
    var terminalMemory = 0
    var exitCode: Int32?
    var startedAt = Date()
    let git: GitWorkingCopy
    let chat: ChatSession?
    let configDir: String?
    @ObservationIgnored private var _terminal: ClaudeTerminalView?
    @ObservationIgnored var restartChat: (() -> Void)?
    @ObservationIgnored var openNewChat: (() -> Void)?

    init(folder: String, kind: ClaudeTabKind, title: String, sessionId: String, git: GitWorkingCopy) {
        self.folder = folder
        self.kind = kind
        self.title = title
        self.sessionId = sessionId
        self.git = git
        configDir = ClaudeProfiles.shared.activeOverride
        chat = kind == .chat ? ChatSession(folder: folder, configDir: ClaudeProfiles.shared.activeOverride) : nil
    }

    var terminal: ClaudeTerminalView {
        if let t = _terminal { return t }
        let t = ClaudeTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
        t.font = NSFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
        t.optionAsMetaKey = false
        t.applyAppearance()
        _terminal = t
        return t
    }

    var running: Bool { kind == .chat ? (chat?.running ?? false) : terminalRunning }

    var state: ClaudeTabState {
        if let chat {
            if chat.pendingCount > 0 { return .waiting }
            if chat.busy { return .working }
            if chat.running { return .active }
            return chat.sleeping ? .sleeping : .stopped
        }
        if terminalRunning { return Date().timeIntervalSince(terminal.lastOutput) < 3 ? .working : .active }
        return terminalSleeping ? .sleeping : .stopped
    }

    var memoryBytes: Int { chat?.memoryBytes ?? terminalMemory }

    var pid: pid_t {
        if let chat { return chat.pid }
        return terminalRunning ? terminal.process?.shellPid ?? 0 : 0
    }

    func sleepNow() {
        if let chat {
            chat.sleep()
        } else if terminalRunning {
            terminalSleeping = true
            terminal.terminate()
        }
    }
    var busy: Bool { chat?.busy ?? false }
    var needsAttention: Bool { (chat?.pendingCount ?? 0) > 0 }
    var chatRunning: Bool { chat?.running ?? false }

    func ensureChatRunning() { restartChat?() }
    func newChatInSameFolder() { openNewChat?() }

    var projectName: String { (folder as NSString).lastPathComponent }
}

@Observable
@MainActor
final class ClaudeWorkspace {
    var tabs: [ClaudeTab] = []
    var selectedTabID: UUID?
    var projects: [ClaudeProject] = []
    var loadingProjects = false
    var showInspector = true

    var model: ClaudeModelOption {
        get { _ = modelTick; return ClaudeModelOption(rawValue: UserDefaults.standard.string(forKey: "claude.model") ?? "") ?? .standard }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "claude.model"); modelTick += 1 }
    }
    var effort: ClaudeEffort {
        get { _ = modelTick; return ClaudeEffort(rawValue: UserDefaults.standard.string(forKey: "claude.effort") ?? "") ?? .standard }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "claude.effort"); modelTick += 1 }
    }
    var mode: ClaudeMode {
        get { _ = modelTick; return ClaudeMode(rawValue: UserDefaults.standard.string(forKey: "claude.mode") ?? "default") ?? .standard }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "claude.mode"); modelTick += 1 }
    }
    var idleMinutes: Int {
        get { _ = modelTick; return UserDefaults.standard.object(forKey: "claude.idleSleepMinutes") as? Int ?? 15 }
        set { UserDefaults.standard.set(newValue, forKey: "claude.idleSleepMinutes"); modelTick += 1 }
    }
    var preferredKind: ClaudeTabKind {
        get { _ = modelTick; return ClaudeTabKind(rawValue: UserDefaults.standard.string(forKey: "claude.ui") ?? "chat") ?? .chat }
        set { UserDefaults.standard.set(newValue.rawValue, forKey: "claude.ui"); modelTick += 1 }
    }
    private var modelTick = 0

    @ObservationIgnored private var gits: [String: GitWorkingCopy] = [:]
    @ObservationIgnored private var monitor: Task<Void, Never>?

    var totalMemory: Int { tabs.reduce(0) { $0 + $1.memoryBytes } }
    var awakeCount: Int { tabs.filter(\.running).count }

    func startMonitoring() {
        guard monitor == nil else { return }
        monitor = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 15_000_000_000)
                await self?.tick()
            }
        }
    }

    func tick() async {
        let live = tabs.filter(\.running)
        if !live.isEmpty {
            let snap = await Task.detached(priority: .utility) { ProcessMemory.snapshot() }.value
            for tab in live {
                let bytes = ProcessMemory.treeBytes(tab.pid, in: snap)
                if let chat = tab.chat { chat.memoryBytes = bytes } else { tab.terminalMemory = bytes }
            }
        }
        let limit = idleMinutes
        guard limit > 0 else { return }
        let threshold = TimeInterval(limit * 60)
        for tab in tabs {
            if let chat = tab.chat {
                if chat.canSleep, Date().timeIntervalSince(chat.lastActivity) > threshold { chat.sleep() }
            } else if tab.terminalRunning, Date().timeIntervalSince(tab.terminal.lastOutput) > threshold {
                tab.sleepNow()
            }
        }
    }

    func closeAllForProfileSwitch() {
        for tab in tabs {
            tab.chat?.stop()
            if tab.terminalRunning { tab.terminal.terminate() }
        }
        tabs.removeAll()
        selectedTabID = nil
        projects = []
    }

    func sleepAll() {
        for tab in tabs where tab.running && tab.state != .working && tab.state != .waiting { tab.sleepNow() }
    }

    var selectedTab: ClaudeTab? { tabs.first { $0.id == selectedTabID } }

    var cliAvailable: Bool { ClaudeLauncher.claudeBinary != nil }

    var attentionCount: Int { tabs.filter(\.needsAttention).count }

    func git(for folder: String) -> GitWorkingCopy {
        if let g = gits[folder] { return g }
        let g = GitWorkingCopy(path: folder)
        gits[folder] = g
        g.startWatching()
        Task { await g.refresh() }
        return g
    }

    func reloadProjects() async {
        loadingProjects = true
        defer { loadingProjects = false }
        let root = URL(fileURLWithPath: ClaudeProfiles.shared.configDir()).appendingPathComponent("projects", isDirectory: true)
        let profileID = ClaudeProfiles.shared.activeID
        let loaded = await Task.detached(priority: .userInitiated) { ClaudeSessionParser.loadAll(root: root) }.value
        guard profileID == ClaudeProfiles.shared.activeID else { return }
        projects = Demo.folder.map { f in loaded.filter { $0.path.hasPrefix(f) } } ?? loaded
    }

    func tab(forSession id: String) -> ClaudeTab? {
        tabs.first { $0.sessionId == id }
    }

    @discardableResult
    func newSession(folder: String, prompt: String? = nil, name: String? = nil, mode: ClaudeMode? = nil, kind: ClaudeTabKind? = nil) -> ClaudeTab {
        let kind = kind ?? preferredKind
        let title = name ?? prompt.map { String($0.prefix(48)) } ?? "New session"
        if kind == .chat {
            let tab = makeTab(folder: folder, kind: .chat, title: title, sessionId: "")
            startChat(tab, resume: nil, prompt: prompt, mode: mode)
            return tab
        }
        let sessionId = UUID().uuidString.lowercased()
        var args = ["--session-id", sessionId]
        if let name, !name.isEmpty { args += ["-n", name] }
        if let prompt, !prompt.isEmpty { args.append(prompt) }
        let tab = makeTab(folder: folder, kind: .terminal, title: title, sessionId: sessionId)
        launchTerminal(tab, extra: args, mode: mode)
        return tab
    }

    @discardableResult
    func resume(_ s: ClaudeSessionInfo, fork: Bool = false, kind: ClaudeTabKind? = nil) -> ClaudeTab {
        if !fork, let open = tab(forSession: s.id) {
            selectedTabID = open.id
            return open
        }
        let kind = kind ?? preferredKind
        let tab = makeTab(folder: s.cwd, kind: kind, title: s.title, sessionId: fork ? "" : s.id)
        if kind == .chat {
            tab.chat?.loadHistory(from: s.file)
            startChat(tab, resume: s.id, fork: fork, prompt: nil, mode: nil)
        } else {
            var args = ["--resume", s.id]
            if fork { args.append("--fork-session") }
            launchTerminal(tab, extra: args)
        }
        return tab
    }

    func openInTerminal(_ tab: ClaudeTab) {
        guard !tab.sessionId.isEmpty else { return }
        tab.chat?.stop()
        let t = makeTab(folder: tab.folder, kind: .terminal, title: tab.title, sessionId: tab.sessionId)
        launchTerminal(t, extra: ["--resume", tab.sessionId])
        tabs.removeAll { $0.id == tab.id }
        selectedTabID = t.id
    }

    func openInChat(_ tab: ClaudeTab) {
        guard !tab.sessionId.isEmpty else { return }
        if tab.terminalRunning { tab.terminal.terminate() }
        let c = makeTab(folder: tab.folder, kind: .chat, title: tab.title, sessionId: tab.sessionId)
        if let file = sessionFile(tab.sessionId) { c.chat?.loadHistory(from: file) }
        startChat(c, resume: tab.sessionId, prompt: nil, mode: nil)
        tabs.removeAll { $0.id == tab.id }
        selectedTabID = c.id
    }

    private func sessionFile(_ id: String) -> URL? {
        projects.flatMap(\.sessions).first { $0.id == id }?.file
    }

    func restart(_ tab: ClaudeTab) {
        guard !tab.running else { return }
        if tab.kind == .chat {
            if let chat = tab.chat, chat.sessionId?.isEmpty == false {
                chat.wake()
            } else {
                startChat(tab, resume: tab.sessionId.isEmpty ? nil : tab.sessionId, prompt: nil, mode: nil)
            }
        } else {
            tab.terminalSleeping = false
            launchTerminal(tab, extra: ["--resume", tab.sessionId])
        }
    }

    func close(_ tab: ClaudeTab) {
        tab.chat?.stop()
        if tab.terminalRunning { tab.terminal.terminate() }
        tabs.removeAll { $0.id == tab.id }
        if selectedTabID == tab.id { selectedTabID = tabs.last?.id }
    }

    func applyModelToCurrent() {
        guard let tab = selectedTab, tab.running else { return }
        if let chat = tab.chat {
            if model != .standard { chat.setModel(model.rawValue) }
        } else if model != .standard {
            tab.terminal.send(txt: "/model \(model.rawValue)\r")
        }
    }

    func send(_ text: String, to tab: ClaudeTab? = nil, submit: Bool = false) {
        guard let tab = tab ?? selectedTab else { return }
        if let chat = tab.chat {
            if !chat.running { restart(tab) }
            chat.send(text)
            return
        }
        guard tab.running else { return }
        tab.terminal.send(txt: "\u{1b}[200~" + text + "\u{1b}[201~")
        if submit { tab.terminal.send(txt: "\r") }
        tab.terminal.window?.makeFirstResponder(tab.terminal)
    }

    private func makeTab(folder: String, kind: ClaudeTabKind, title: String, sessionId: String) -> ClaudeTab {
        let tab = ClaudeTab(folder: folder, kind: kind, title: title, sessionId: sessionId, git: git(for: folder))
        tab.restartChat = { [weak self, weak tab] in
            guard let self, let tab else { return }
            self.restart(tab)
        }
        tab.openNewChat = { [weak self, weak tab] in
            guard let self, let tab else { return }
            self.newSession(folder: tab.folder, kind: .chat)
        }
        tabs.append(tab)
        selectedTabID = tab.id
        startMonitoring()
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: 4_000_000_000)
            await self?.reloadProjects()
        }
        return tab
    }

    private func startChat(_ tab: ClaudeTab, resume: String?, fork: Bool = false, prompt: String?, mode modeOverride: ClaudeMode?) {
        guard let chat = tab.chat else { return }
        chat.onSessionId = { [weak tab] sid in tab?.sessionId = sid }
        chat.onTurnFinished = { [weak tab] in
            guard let tab else { return }
            Task { await tab.git.refresh() }
        }
        let m = modeOverride ?? mode
        chat.start(
            resume: resume,
            fork: fork,
            model: model == .standard ? nil : model.rawValue,
            effort: effort == .standard ? nil : effort.rawValue,
            mode: m == .standard ? nil : m.rawValue,
            initialPrompt: prompt
        )
    }

    private func launchTerminal(_ tab: ClaudeTab, extra: [String], mode modeOverride: ClaudeMode? = nil) {
        let mode = modeOverride ?? self.mode
        var args: [String] = []
        if model != .standard { args += ["--model", model.rawValue] }
        if effort != .standard { args += ["--effort", effort.rawValue] }
        if mode != .standard { args += ["--permission-mode", mode.rawValue] }
        args += extra
        let command = "exec claude " + args.map(ClaudeLauncher.shellQuote).joined(separator: " ")
        let shell = ProcessInfo.processInfo.environment["SHELL"].flatMap { FileManager.default.isExecutableFile(atPath: $0) ? $0 : nil } ?? "/bin/zsh"

        var env = ProcessInfo.processInfo.environment
        env["TERM"] = "xterm-256color"
        env["COLORTERM"] = "truecolor"
        env["LANG"] = env["LANG"] ?? "en_US.UTF-8"
        env["PATH"] = [FileManager.default.homeDirectoryForCurrentUser.path + "/.local/bin", "/opt/homebrew/bin", "/usr/local/bin", env["PATH"] ?? "/usr/bin:/bin"].joined(separator: ":")
        if let browser = LinkBrowser.helperScript() { env["BROWSER"] = browser }
        for key in env.keys where key == "SHIPYARD_TOKEN" || key == "CLAUDECODE" || key.hasPrefix("CLAUDE_CODE_") {
            env.removeValue(forKey: key)
        }
        ClaudeProfiles.apply(tab.configDir, to: &env)

        tab.terminalRunning = true
        tab.exitCode = nil
        tab.startedAt = Date()
        tab.terminal.onTerminated = { [weak tab] code in
            tab?.terminalRunning = false
            tab?.exitCode = code
        }
        tab.terminal.startProcess(
            executable: shell,
            args: ["-l", "-c", command],
            environment: env.map { "\($0.key)=\($0.value)" },
            execName: "-" + (shell as NSString).lastPathComponent,
            currentDirectory: tab.folder
        )
    }
}
