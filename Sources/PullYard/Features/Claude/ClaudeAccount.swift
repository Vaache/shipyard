import AppKit
import SwiftUI

struct ClaudeAccountInfo {
    let email: String?
    let orgName: String?
    let subscription: String?
    let method: String?
    let provider: String?
}

enum ClaudeLoginMethod: String, CaseIterable, Identifiable {
    case claudeai, console, sso
    var id: String { rawValue }

    var title: String {
        switch self {
        case .claudeai: return "Claude subscription (Pro, Max, Team)"
        case .console: return "Anthropic Console (API billing)"
        case .sso: return "SSO"
        }
    }

    var flag: String { "--" + rawValue }
}

@Observable
@MainActor
final class ClaudeAccountStore {
    static let shared = ClaudeAccountStore()

    enum Status { case unknown, checking, notInstalled, loggedOut, loggedIn(ClaudeAccountInfo) }

    var status: Status = .unknown
    var loggingIn = false
    var fallbackURL: URL?
    var loginError: String?
    var lastChecked: Date?

    @ObservationIgnored private var loginProcess: Process?
    @ObservationIgnored private var loginInput: FileHandle?
    @ObservationIgnored private var output = ""

    var isLoggedIn: Bool {
        if case .loggedIn = status { return true }
        return false
    }

    var info: ClaudeAccountInfo? {
        if case .loggedIn(let i) = status { return i }
        return nil
    }

    private static func environment(configDir: String?? = .none) -> [String: String] {
        var env = ProcessInfo.processInfo.environment
        ClaudeProfiles.apply(configDir ?? ClaudeProfiles.shared.activeOverride, to: &env)
        for key in env.keys where key == "PULLYARD_TOKEN" || key == "CLAUDECODE" || key.hasPrefix("CLAUDE_CODE_") { env.removeValue(forKey: key) }
        if let bin = ClaudeLauncher.claudeBinary {
            env["PATH"] = [(bin as NSString).deletingLastPathComponent, "/opt/homebrew/bin", "/usr/local/bin", env["PATH"] ?? "/usr/bin:/bin"].joined(separator: ":")
        }
        if let browser = LinkBrowser.helperScript() { env["BROWSER"] = browser }
        return env
    }

    func refresh() async {
        guard let bin = ClaudeLauncher.claudeBinary else {
            status = .notInstalled
            return
        }
        if case .unknown = status { status = .checking }
        let env = Self.environment()
        let result = await Task.detached(priority: .userInitiated) { () -> [String: Any]? in
            let p = Process()
            p.executableURL = URL(fileURLWithPath: bin)
            p.arguments = ["auth", "status"]
            p.environment = env
            let out = Pipe()
            p.standardOutput = out
            p.standardError = FileHandle.nullDevice
            guard (try? p.run()) != nil else { return nil }
            let data = out.fileHandleForReading.readDataToEndOfFile()
            p.waitUntilExit()
            return try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        }.value
        lastChecked = Date()
        guard let json = result else {
            status = .loggedOut
            return
        }
        if json["loggedIn"] as? Bool == true {
            if !Demo.isOn { ClaudeProfiles.shared.updateCachedInfo(email: json["email"] as? String, plan: (json["subscriptionType"] as? String)?.capitalized) }
            status = .loggedIn(ClaudeAccountInfo(
                email: Demo.isOn ? Demo.email : json["email"] as? String,
                orgName: Demo.isOn ? nil : json["orgName"] as? String,
                subscription: (json["subscriptionType"] as? String)?.capitalized,
                method: json["authMethod"] as? String,
                provider: json["apiProvider"] as? String
            ))
        } else {
            status = .loggedOut
        }
    }

    func login(method: ClaudeLoginMethod, email: String?) {
        guard !loggingIn, let bin = ClaudeLauncher.claudeBinary else { return }
        loginError = nil
        fallbackURL = nil
        output = ""
        var args = ["auth", "login", method.flag]
        if let email, !email.trimmingCharacters(in: .whitespaces).isEmpty { args += ["--email", email.trimmingCharacters(in: .whitespaces)] }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: bin)
        p.arguments = args
        p.environment = Self.environment()
        p.currentDirectoryURL = FileManager.default.homeDirectoryForCurrentUser
        let inPipe = Pipe(), outPipe = Pipe()
        p.standardInput = inPipe
        p.standardOutput = outPipe
        p.standardError = outPipe
        outPipe.fileHandleForReading.readabilityHandler = { [weak self] h in
            let data = h.availableData
            guard !data.isEmpty else { h.readabilityHandler = nil; return }
            let s = String(decoding: data, as: UTF8.self)
            DispatchQueue.main.async { self?.consume(s) }
        }
        p.terminationHandler = { [weak self] proc in
            DispatchQueue.main.async { self?.loginFinished(code: proc.terminationStatus) }
        }
        do {
            try p.run()
            loginProcess = p
            loginInput = inPipe.fileHandleForWriting
            loggingIn = true
        } catch {
            loginError = error.localizedDescription
        }
    }

    private func consume(_ s: String) {
        output += s
        if fallbackURL == nil, let range = output.range(of: "https://[^\\s]+", options: .regularExpression) {
            fallbackURL = URL(string: String(output[range]))
        }
    }

    func submitCode(_ code: String) {
        let trimmed = code.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let loginInput else { return }
        try? loginInput.write(contentsOf: Data((trimmed + "\n").utf8))
    }

    func cancelLogin() {
        loginProcess?.terminate()
    }

    private func loginFinished(code: Int32) {
        loggingIn = false
        loginProcess = nil
        loginInput = nil
        if code != 0 && code != 15 {
            let tail = output.components(separatedBy: "\n").filter { !$0.contains("http") && !$0.trimmingCharacters(in: .whitespaces).isEmpty }.suffix(3).joined(separator: "\n")
            loginError = tail.isEmpty ? "Sign-in failed (code \(code))" : tail
        }
        Task { await refresh() }
    }

    func logout(configDir: String? = nil) async {
        guard let bin = ClaudeLauncher.claudeBinary else { return }
        let env = configDir.map { Self.environment(configDir: .some($0)) } ?? Self.environment()
        await Task.detached {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: bin)
            p.arguments = ["auth", "logout"]
            p.environment = env
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
            try? p.run()
            p.waitUntilExit()
        }.value
        await refresh()
    }
}

struct ClaudeAuthView: View {
    @State private var method: ClaudeLoginMethod = .claudeai
    @State private var email = ""
    @State private var code = ""
    private var store: ClaudeAccountStore { ClaudeAccountStore.shared }
    static let installCommand = "curl -fsSL https://claude.ai/install.sh | bash"

    var body: some View {
        ScrollView {
            VStack(spacing: 18) {
                IconTile(systemImage: "sparkles", color: .syCoral, size: 56)
                Text("Claude Code").font(.title.weight(.bold)).foregroundStyle(Color.syText)
                ClaudeProfileSwitcher()
                switch store.status {
                case .notInstalled: installCard
                case .unknown, .checking: ProgressView().controlSize(.small)
                case .loggedOut: loginCard
                case .loggedIn(let info): ClaudeAccountCard(info: info)
                }
            }
            .padding(36)
            .frame(maxWidth: 520)
            .frame(maxWidth: .infinity)
        }
        .surface(.panel)
        .task { await store.refresh() }
    }

    private var installCard: some View {
        card {
            Text("Claude Code isn't installed").font(.headline).foregroundStyle(Color.syText)
            Text("Install the CLI from Terminal, then press “Check again”.")
                .font(.callout).foregroundStyle(Color.syMuted)
            Text(Self.installCommand)
                .font(.system(.callout, design: .monospaced))
                .textSelection(.enabled)
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .card(radius: 8, fill: .sySunken)
            HStack {
                Button("Copy command") { copyToPasteboard(Self.installCommand) }
                Button("Docs") { openInBrowser("https://code.claude.com/docs/en/setup") }
                Spacer()
                Button("Check again") { Task { await store.refresh() } }.buttonStyle(.borderedProminent).tint(.syCoral)
            }
        }
    }

    private var loginCard: some View {
        card {
            Text("Sign in to Claude").font(.headline).foregroundStyle(Color.syText)
            Text("Sign-in opens in your browser. The account is shared with Claude Code in Terminal.")
                .font(.callout).foregroundStyle(Color.syMuted)
            if store.loggingIn {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 8) {
                        ProgressView().controlSize(.small)
                        Text("Confirm the sign-in in your browser…")
                    }
                    if let url = store.fallbackURL {
                        Text("Browser didn't open, or the page shows a code?").font(.caption).foregroundStyle(Color.syMuted)
                        HStack {
                            Button("Open sign-in link") { LinkBrowser.open(url) }
                            Button("Copy link") { copyToPasteboard(url.absoluteString) }
                        }
                        .controlSize(.small)
                        HStack {
                            SecureField("Paste the code from the page", text: $code)
                                .textFieldStyle(.roundedBorder)
                                .onSubmit { store.submitCode(code); code = "" }
                            Button("Submit") { store.submitCode(code); code = "" }
                                .buttonStyle(.borderedProminent)
                                .tint(.syCoral)
                                .disabled(code.isEmpty)
                        }
                    }
                    Button("Cancel", role: .cancel) { store.cancelLogin() }
                }
            } else {
                Picker("Sign-in method", selection: $method) {
                    ForEach(ClaudeLoginMethod.allCases) { Text($0.title).tag($0) }
                }
                .pickerStyle(.radioGroup)
                TextField("Email (optional)", text: $email)
                    .textFieldStyle(.roundedBorder)
                HStack {
                    Spacer()
                    Button {
                        store.login(method: method, email: email)
                    } label: {
                        Label("Sign in with browser", systemImage: "person.badge.key")
                    }
                    .buttonStyle(.borderedProminent)
                    .tint(.syCoral)
                    .controlSize(.large)
                    .keyboardShortcut(.defaultAction)
                }
            }
            if let e = store.loginError {
                Text(e).font(.caption).foregroundStyle(Color.syRed).textSelection(.enabled)
            }
        }
    }

    private func card<C: View>(@ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 10, content: content)
            .padding(18)
            .frame(maxWidth: .infinity, alignment: .leading)
            .card(radius: 12)
    }
}

struct ClaudeAccountCard: View {
    let info: ClaudeAccountInfo
    var compact = false
    @State private var confirmLogout = false
    @State private var showAdd = false
    @State private var removeTarget: ClaudeProfile?
    private var store: ClaudeAccountStore { ClaudeAccountStore.shared }

    var body: some View {
        let profile = ClaudeProfiles.shared.active
        HStack(spacing: 10) {
            ProfileBadge(profile: profile, size: compact ? 26 : 34)
            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    Text(info.email ?? "Signed in").font(compact ? .callout.weight(.medium) : .headline).foregroundStyle(Color.syText).lineLimit(1)
                    if ClaudeProfiles.shared.profiles.count > 1 {
                        Text(profile.label).font(.caption2.weight(.semibold)).foregroundStyle(Color.syMuted)
                            .padding(.horizontal, 5).background(Capsule().fill(Color.syMuted.opacity(0.14)))
                    }
                }
                HStack(spacing: 5) {
                    if let org = info.orgName { Text(org).lineLimit(1) }
                    if let plan = info.subscription {
                        Text(plan)
                            .font(.caption2.weight(.semibold))
                            .foregroundStyle(Color.syCoral)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(Capsule().fill(Color.syCoral.opacity(0.12)))
                    } else if info.method == "console" || info.provider != "firstParty" {
                        Text(info.provider == "firstParty" ? "API" : (info.provider ?? "API")).font(.caption2)
                    }
                }
                .font(.caption)
                .foregroundStyle(Color.syMuted)
            }
            Spacer(minLength: 4)
            Menu {
                ClaudeProfileMenuItems(showAdd: $showAdd, removeTarget: $removeTarget)
                Divider()
                Button("Sign out of this account…", role: .destructive) { confirmLogout = true }
                Divider()
                Button("Usage page") { openInBrowser("https://claude.ai/settings/usage") }
                Button("Refresh status") { Task { await store.refresh() } }
            } label: {
                Image(systemName: "ellipsis.circle")
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
        }
        .padding(compact ? 8 : 14)
        .card(radius: 10)
        .modifier(ProfileSheets(showAdd: $showAdd, removeTarget: $removeTarget))
        .confirmationDialog("Sign out of Claude Code?", isPresented: $confirmLogout) {
            Button("Sign out", role: .destructive) { Task { await store.logout() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(ClaudeProfiles.shared.active.isDefault ? "This also signs out Claude Code in Terminal. You can sign in again afterwards, including with another account." : "Signs out only “\(ClaudeProfiles.shared.active.label)”. Other accounts are not affected.")
        }
    }
}
