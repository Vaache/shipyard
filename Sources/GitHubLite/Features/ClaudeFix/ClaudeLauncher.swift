import AppKit
import Foundation

enum ClaudeTarget: String, CaseIterable, Identifiable {
    case app, terminal, iterm, desktop, copy

    var id: String { rawValue }

    var title: String {
        switch self {
        case .app: return t("В приложении", "In this app")
        case .terminal: return "Terminal"
        case .iterm: return "iTerm"
        case .desktop: return "Claude Desktop"
        case .copy: return t("Только скопировать", "Copy only")
        }
    }

    var available: Bool {
        switch self {
        case .iterm: return NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.googlecode.iterm2") != nil
        case .app: return ClaudeLauncher.claudeBinary != nil
        case .desktop: return NSWorkspace.shared.urlForApplication(withBundleIdentifier: "com.anthropic.claudefordesktop") != nil && ClaudeLauncher.claudeBinary != nil
        default: return true
        }
    }
}

enum ClaudePermissionMode: String, CaseIterable, Identifiable {
    case standard = "default", acceptEdits, plan

    var id: String { rawValue }

    var title: String {
        switch self {
        case .standard: return t("Обычный (спрашивать)", "Default (ask)")
        case .acceptEdits: return t("Принимать правки", "Accept edits")
        case .plan: return t("Сначала план", "Plan first")
        }
    }
}

enum ClaudeLauncher {
    static func shellQuote(_ s: String) -> String {
        "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
    }

    static var claudeBinary: String? {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        return [home + "/.local/bin/claude", home + "/.claude/local/claude", "/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
            .first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    static func launch(prompt: String, folder: String, target: ClaudeTarget, mode: ClaudePermissionMode, sessionName: String, resumeId: String? = nil, configDir: String? = nil) throws {
        if target == .copy {
            copyToPasteboard(prompt)
            return
        }
        if target == .desktop {
            guard let bin = claudeBinary else { throw APIError(message: t("claude CLI не найден", "claude CLI not found")) }
            copyToPasteboard(prompt)
            let p = Process()
            p.executableURL = URL(fileURLWithPath: bin)
            p.arguments = ["--desktop"]
            p.currentDirectoryURL = URL(fileURLWithPath: folder)
            var env = ProcessInfo.processInfo.environment
            env["PATH"] = (bin as NSString).deletingLastPathComponent + ":/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
            ClaudeProfiles.apply(configDir, to: &env)
            p.environment = env
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
            try p.run()
            return
        }
        let dir = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask)[0].appendingPathComponent("GitHubLite/claude", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let id = UUID().uuidString.prefix(8)
        let promptURL = dir.appendingPathComponent("prompt-\(id).md")
        let scriptURL = dir.appendingPathComponent("fix-\(id).command")
        try prompt.write(to: promptURL, atomically: true, encoding: .utf8)

        let configExport = configDir.map { "export CLAUDE_CONFIG_DIR=\(shellQuote($0))" } ?? ""
        var args = ""
        if mode != .standard { args += " --permission-mode \(mode.rawValue)" }
        if let resumeId { args += " --resume \(shellQuote(resumeId))" } else { args += " -n \(shellQuote(sessionName))" }
        let script = """
        #!/bin/bash
        export PATH="$HOME/.local/bin:$HOME/.claude/local:/opt/homebrew/bin:/usr/local/bin:$PATH"
        unset CLAUDECODE $(env | grep -o '^CLAUDE_CODE_[A-Z_]*')
        \(configExport)
        cd \(shellQuote(folder)) || { echo "Folder not found: \(folder.replacingOccurrences(of: "\"", with: ""))"; exit 1; }
        PROMPT="$(cat \(shellQuote(promptURL.path)))"
        rm -f \(shellQuote(promptURL.path)) "$0"
        clear
        if ! command -v claude >/dev/null 2>&1; then echo "claude CLI not found in PATH"; exit 1; fi
        if [ -n "$PROMPT" ]; then exec claude\(args) "$PROMPT"; else exec claude\(args); fi

        """
        try script.write(to: scriptURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: scriptURL.path)

        let bundleID = target == .iterm ? "com.googlecode.iterm2" : "com.apple.Terminal"
        guard let appURL = NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleID) else {
            throw APIError(message: t("Приложение терминала не найдено", "Terminal app not found"))
        }
        let config = NSWorkspace.OpenConfiguration()
        config.activates = true
        NSWorkspace.shared.open([scriptURL], withApplicationAt: appURL, configuration: config)
    }
}
