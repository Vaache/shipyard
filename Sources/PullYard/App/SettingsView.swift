import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("pollInterval") private var pollInterval = 30.0
    @AppStorage("oauthClientID") private var clientID = ""
    @AppStorage(Theme.styleKey) private var themeStyle = ThemeStyle.pullyard.rawValue
    @AppStorage(Theme.appearanceKey) private var appearance = AppearanceMode.auto.rawValue
    @State private var browserTick = 0
    @State private var confirmClaudeLogout = false
    @State private var loggingOut = false
    @State private var showAddProfile = false
    @AppStorage(Updater.autoCheckKey) private var autoCheckUpdates = true
    @AppStorage(Diagnostics.enabledKey) private var diagnosticsEnabled = false
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Form {
            Section {
                Picker("Theme", selection: $themeStyle) {
                    ForEach(ThemeStyle.allCases) { Text($0.title).tag($0.rawValue) }
                }
                .pickerStyle(.segmented)
                Picker("Mode", selection: $appearance) {
                    ForEach(AppearanceMode.allCases) { Text($0.title).tag($0.rawValue) }
                }
                .pickerStyle(.segmented)
            } header: {
                SettingsHeader(title: "Appearance", icon: "paintpalette.fill", color: .syViolet)
            }
            Section {
                if let u = app.user {
                    HStack(spacing: 10) {
                        Avatar(url: u.avatarUrl, size: 36)
                        VStack(alignment: .leading) {
                            Text(u.name ?? u.login).font(.headline).foregroundStyle(Color.syText)
                            Text("@\(u.login)").foregroundStyle(Color.syMuted)
                        }
                    }
                    LabeledContent("Token scopes", value: app.scopes.isEmpty ? "fine-grained / unknown" : app.scopes.joined(separator: ", "))
                    HStack {
                        if GHCLI.binary != nil {
                            Button("Refresh token from gh") {
                                Task { if let t = try? await GHCLI.token() { try? await app.login(token: t) } }
                            }
                        }
                        Button("Sign out", role: .destructive) { app.logout() }
                    }
                } else {
                    Text("Not signed in").foregroundStyle(Color.syMuted)
                }
            } header: {
                SettingsHeader(title: "Account", icon: "person.fill", color: .syTeal)
            }
            Section {
                Picker("Open links in", selection: Binding(get: { LinkBrowser.selected }, set: { LinkBrowser.selected = $0; browserTick += 1 })) {
                    Text("System default").tag(LinkBrowser.systemDefault)
                    ForEach(LinkBrowser.installed) { Text($0.name).tag($0.id) }
                }
                .id(browserTick)
            } header: {
                SettingsHeader(title: "Links", icon: "link", color: .syIndigo)
            }
            Section {
                Picker("Auto-refresh pull requests", selection: $pollInterval) {
                    Text("Every 15 seconds").tag(15.0)
                    Text("Every 30 seconds").tag(30.0)
                    Text("Every minute").tag(60.0)
                    Text("Every 2 minutes").tag(120.0)
                    Text("Every 5 minutes").tag(300.0)
                    Text("Every 15 minutes").tag(900.0)
                }
                .onChange(of: pollInterval) { app.startPolling() }
            } header: {
                SettingsHeader(title: "Refresh", icon: "arrow.clockwise", color: .syGreen)
            }
            Section {
                HStack {
                    Picker("Account", selection: Binding(get: { ClaudeProfiles.shared.activeID }, set: { app.switchClaudeProfile(to: $0) })) {
                        ForEach(ClaudeProfiles.shared.profiles) { p in
                            Text(p.label + (p.email.map { " — \($0)" } ?? "")).tag(p.id)
                        }
                    }
                    Button("Add…") { showAddProfile = true }
                }
                switch ClaudeAccountStore.shared.status {
                case .loggedIn(let info):
                    ClaudeAccountCard(info: info, compact: true)
                    HStack {
                        Button("Sign out of Claude Code", role: .destructive) { confirmClaudeLogout = true }
                        Spacer()
                        if loggingOut { ProgressView().controlSize(.small) }
                    }
                    .disabled(loggingOut)
                case .notInstalled:
                    Text("claude CLI isn't installed").foregroundStyle(Color.syMuted)
                default:
                    HStack {
                        Text("Not signed in to Claude").foregroundStyle(Color.syMuted)
                        Spacer()
                        Button("Sign in…") {
                            app.section = .claude
                            NSApp.activate(ignoringOtherApps: true)
                        }
                    }
                }
                Picker("Sleep idle sessions after", selection: Binding(get: { app.claude.idleMinutes }, set: { app.claude.idleMinutes = $0 })) {
                    Text("Never").tag(0)
                    Text("5 minutes").tag(5)
                    Text("15 minutes").tag(15)
                    Text("30 minutes").tag(30)
                    Text("1 hour").tag(60)
                }
                Text("A sleeping session uses no memory: the claude process stops, the conversation stays, and the next message resumes it.")
                    .font(.caption)
                    .foregroundStyle(Color.syMuted)
            } header: {
                SettingsHeader(title: "Claude Code", icon: "sparkles", color: .syCoral)
            }
            Section {
                Toggle("Check for updates automatically", isOn: $autoCheckUpdates)
                HStack {
                    Text("Current version \(Updater.shared.currentVersion)")
                        .foregroundStyle(Color.syMuted)
                    Spacer()
                    Button("Check Now") {
                        openWindow(id: "update")
                        Task { await Updater.shared.check(userInitiated: true) }
                    }
                    .disabled(Updater.shared.isBusy)
                }
            } header: {
                SettingsHeader(title: "Updates", icon: "arrow.down.circle", color: .syGreen)
            }
            Section {
                Toggle("Record network diagnostics", isOn: $diagnosticsEnabled)
                    .onChange(of: diagnosticsEnabled) { Diagnostics.shared.setEnabled(diagnosticsEnabled) }
                HStack {
                    Text("Keeps the last 1000 GitHub requests in memory: timing, size, status and rate limit. No tokens or request bodies.")
                        .font(.caption)
                        .foregroundStyle(Color.syMuted)
                    Spacer()
                    Button("Open Diagnostics") { openWindow(id: "diagnostics") }
                }
            } header: {
                SettingsHeader(title: "Diagnostics", icon: "gauge.with.dots.needle.33percent", color: .sySky)
            }
            Section {
                TextField("Client ID", text: $clientID)
            } header: {
                SettingsHeader(title: "OAuth Device Flow", icon: "key.fill", color: .syAmber)
            }
            Section {
                HStack {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Buy me a pizza 🍕").foregroundStyle(Color.syText)
                        Text("Pay what you want, from $3. PullYard stays free and open source.")
                            .font(.caption)
                            .foregroundStyle(Color.syMuted)
                    }
                    Spacer()
                    Button("Support…") { openInBrowser(Support.pizzaURL) }
                }
            } header: {
                SettingsHeader(title: "Support PullYard", icon: "heart.fill", color: .syCoral)
            }
        }
        .formStyle(.grouped)
        .surface(.panel)
        .task { await ClaudeAccountStore.shared.refresh() }
        .sheet(isPresented: $showAddProfile) { AddClaudeProfileSheet().environment(app) }
        .confirmationDialog("Sign out of Claude Code?", isPresented: $confirmClaudeLogout) {
            Button("Sign out", role: .destructive) {
                loggingOut = true
                Task {
                    app.claude.sleepAll()
                    await ClaudeAccountStore.shared.logout()
                    loggingOut = false
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This also signs out Claude Code in Terminal. Open sessions will go to sleep. You can sign in again (including with another account) here or in the Claude Code section.")
        }
        .frame(width: 500, height: 560)
        .padding(.vertical, 10)
    }
}

struct SettingsHeader: View {
    let title: String
    let icon: String
    let color: Color

    var body: some View {
        HStack(spacing: 6) {
            IconTile(systemImage: icon, color: color, size: 16)
            Text(title).foregroundStyle(Color.syText)
        }
    }
}
