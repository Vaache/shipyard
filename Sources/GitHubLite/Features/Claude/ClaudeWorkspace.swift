import AppKit
import SwiftTerm
import SwiftUI

final class ClaudeTerminalView: LocalProcessTerminalView {
    var onTerminated: ((Int32?) -> Void)?
    var onTitle: ((String) -> Void)?

    override func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
        if let url = URL(string: link) { LinkBrowser.open(url) }
    }

    override func processTerminated(_ source: LocalProcess, exitCode: Int32?) {
        super.processTerminated(source, exitCode: exitCode)
        DispatchQueue.main.async { self.onTerminated?(exitCode) }
    }

    func applyAppearance() {
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
        case .standard: return t("По умолчанию", "Default")
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
    var title: String { self == .standard ? t("По умолчанию", "Default") : rawValue }
}

enum ClaudeMode: String, CaseIterable, Identifiable {
    case standard = "default", acceptEdits, plan, auto
    var id: String { rawValue }

    var title: String {
        switch self {
        case .standard: return t("Спрашивать", "Ask")
        case .acceptEdits: return t("Принимать правки", "Accept edits")
        case .plan: return t("План", "Plan")
        case .auto: return "Auto"
        }
    }
}

@Observable
@MainActor
final class ClaudeTab: Identifiable {
    let id = UUID()
    let folder: String
    var title: String
    var sessionId: String
    var running = false
    var exitCode: Int32?
    var startedAt = Date()
    let terminal: ClaudeTerminalView
    let git: GitWorkingCopy

    init(folder: String, title: String, sessionId: String, git: GitWorkingCopy) {
        self.folder = folder
        self.title = title
        self.sessionId = sessionId
        self.git = git
        terminal = ClaudeTerminalView(frame: NSRect(x: 0, y: 0, width: 800, height: 500))
        terminal.font = NSFont.monospacedSystemFont(ofSize: 12.5, weight: .regular)
        terminal.optionAsMetaKey = false
        terminal.applyAppearance()
    }

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
    private var modelTick = 0

    @ObservationIgnored private var gits: [String: GitWorkingCopy] = [:]

    var selectedTab: ClaudeTab? { tabs.first { $0.id == selectedTabID } }

    var cliAvailable: Bool { ClaudeLauncher.claudeBinary != nil }

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
        projects = await Task.detached(priority: .userInitiated) { ClaudeSessionParser.loadAll() }.value
    }

    func tab(forSession id: String) -> ClaudeTab? {
        tabs.first { $0.sessionId == id && $0.running }
    }

    @discardableResult
    func newSession(folder: String, prompt: String? = nil, name: String? = nil, mode: ClaudeMode? = nil) -> ClaudeTab {
        let sessionId = UUID().uuidString.lowercased()
        var args = ["--session-id", sessionId]
        if let name, !name.isEmpty { args += ["-n", name] }
        if let prompt, !prompt.isEmpty { args.append(prompt) }
        return start(folder: folder, title: name ?? t("Новая сессия", "New session"), sessionId: sessionId, extra: args, mode: mode)
    }

    @discardableResult
    func resume(_ s: ClaudeSessionInfo, fork: Bool = false) -> ClaudeTab {
        if !fork, let open = tab(forSession: s.id) {
            selectedTabID = open.id
            return open
        }
        var args = ["--resume", s.id]
        if fork { args.append("--fork-session") }
        return start(folder: s.cwd, title: s.title, sessionId: s.id, extra: args)
    }

    func restart(_ tab: ClaudeTab) {
        guard !tab.running else { return }
        launch(tab, extra: ["--resume", tab.sessionId])
    }

    func close(_ tab: ClaudeTab) {
        if tab.running { tab.terminal.terminate() }
        tabs.removeAll { $0.id == tab.id }
        if selectedTabID == tab.id { selectedTabID = tabs.last?.id }
    }

    func applyModelToCurrent() {
        guard let tab = selectedTab, tab.running else { return }
        if model != .standard { tab.terminal.send(txt: "/model \(model.rawValue)\r") }
    }

    func send(_ text: String, to tab: ClaudeTab? = nil, submit: Bool = false) {
        guard let tab = tab ?? selectedTab, tab.running else { return }
        tab.terminal.send(txt: "\u{1b}[200~" + text + "\u{1b}[201~")
        if submit { tab.terminal.send(txt: "\r") }
        tab.terminal.window?.makeFirstResponder(tab.terminal)
    }

    private func start(folder: String, title: String, sessionId: String, extra: [String], mode: ClaudeMode? = nil) -> ClaudeTab {
        let tab = ClaudeTab(folder: folder, title: title, sessionId: sessionId, git: git(for: folder))
        tabs.append(tab)
        selectedTabID = tab.id
        launch(tab, extra: extra, mode: mode)
        Task { [weak self] in
            try? await Task.sleep(nanoseconds: 4_000_000_000)
            await self?.reloadProjects()
        }
        return tab
    }

    private func launch(_ tab: ClaudeTab, extra: [String], mode modeOverride: ClaudeMode? = nil) {
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
        for key in env.keys where key == "GHLITE_TOKEN" || key == "CLAUDECODE" || key.hasPrefix("CLAUDE_CODE_") {
            env.removeValue(forKey: key)
        }

        tab.running = true
        tab.exitCode = nil
        tab.startedAt = Date()
        tab.terminal.onTerminated = { [weak tab] code in
            tab?.running = false
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
