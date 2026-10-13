import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("pollInterval") private var pollInterval = 120.0
    @AppStorage("oauthClientID") private var clientID = ""
    @AppStorage(Theme.styleKey) private var themeStyle = ThemeStyle.shipyard.rawValue
    @AppStorage(Theme.appearanceKey) private var appearance = AppearanceMode.auto.rawValue
    @State private var browserTick = 0
    @State private var confirmClaudeLogout = false
    @State private var loggingOut = false
    @State private var showAddProfile = false
    @AppStorage(Updater.autoCheckKey) private var autoCheckUpdates = true
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Form {
            Section {
                Picker(t("Язык интерфейса", "Interface language"), selection: Binding(get: { Loc.shared.lang }, set: { Loc.shared.set($0); app.refresh() })) {
                    ForEach(AppLanguage.allCases) { l in
                        Text(l.title).tag(l)
                    }
                }
                .pickerStyle(.segmented)
            } header: {
                SettingsHeader(title: t("Язык", "Language"), icon: "globe", color: .sySky)
            }
            Section {
                Picker(t("Тема", "Theme"), selection: $themeStyle) {
                    ForEach(ThemeStyle.allCases) { Text($0.title).tag($0.rawValue) }
                }
                .pickerStyle(.segmented)
                Picker(t("Режим", "Mode"), selection: $appearance) {
                    ForEach(AppearanceMode.allCases) { Text($0.title).tag($0.rawValue) }
                }
                .pickerStyle(.segmented)
            } header: {
                SettingsHeader(title: t("Оформление", "Appearance"), icon: "paintpalette.fill", color: .syViolet)
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
                    LabeledContent(t("Права токена", "Token scopes"), value: app.scopes.isEmpty ? t("fine-grained / неизвестно", "fine-grained / unknown") : app.scopes.joined(separator: ", "))
                    HStack {
                        if GHCLI.binary != nil {
                            Button(t("Обновить токен из gh", "Refresh token from gh")) {
                                Task { if let t = try? await GHCLI.token() { try? await app.login(token: t) } }
                            }
                        }
                        Button(t("Выйти", "Sign out"), role: .destructive) { app.logout() }
                    }
                } else {
                    Text(t("Вход не выполнен", "Not signed in")).foregroundStyle(Color.syMuted)
                }
            } header: {
                SettingsHeader(title: t("Аккаунт", "Account"), icon: "person.fill", color: .syTeal)
            }
            Section {
                Picker(t("Открывать ссылки в", "Open links in"), selection: Binding(get: { LinkBrowser.selected }, set: { LinkBrowser.selected = $0; browserTick += 1 })) {
                    Text(t("Браузер по умолчанию", "System default")).tag(LinkBrowser.systemDefault)
                    ForEach(LinkBrowser.installed) { Text($0.name).tag($0.id) }
                }
                .id(browserTick)
            } header: {
                SettingsHeader(title: t("Ссылки", "Links"), icon: "link", color: .syIndigo)
            }
            Section {
                Picker(t("Проверять счётчики", "Check counters"), selection: $pollInterval) {
                    Text(t("Каждую минуту", "Every minute")).tag(60.0)
                    Text(t("Каждые 2 минуты", "Every 2 minutes")).tag(120.0)
                    Text(t("Каждые 5 минут", "Every 5 minutes")).tag(300.0)
                    Text(t("Каждые 15 минут", "Every 15 minutes")).tag(900.0)
                }
                .onChange(of: pollInterval) { app.startPolling() }
            } header: {
                SettingsHeader(title: t("Обновление", "Refresh"), icon: "arrow.clockwise", color: .syGreen)
            }
            Section {
                HStack {
                    Picker(t("Аккаунт", "Account"), selection: Binding(get: { ClaudeProfiles.shared.activeID }, set: { app.switchClaudeProfile(to: $0) })) {
                        ForEach(ClaudeProfiles.shared.profiles) { p in
                            Text(p.label + (p.email.map { " — \($0)" } ?? "")).tag(p.id)
                        }
                    }
                    Button(t("Добавить…", "Add…")) { showAddProfile = true }
                }
                switch ClaudeAccountStore.shared.status {
                case .loggedIn(let info):
                    ClaudeAccountCard(info: info, compact: true)
                    HStack {
                        Button(t("Выйти из Claude Code", "Sign out of Claude Code"), role: .destructive) { confirmClaudeLogout = true }
                        Spacer()
                        if loggingOut { ProgressView().controlSize(.small) }
                    }
                    .disabled(loggingOut)
                case .notInstalled:
                    Text(t("claude CLI не установлен", "claude CLI isn't installed")).foregroundStyle(Color.syMuted)
                default:
                    HStack {
                        Text(t("Вход в Claude не выполнен", "Not signed in to Claude")).foregroundStyle(Color.syMuted)
                        Spacer()
                        Button(t("Войти…", "Sign in…")) {
                            app.section = .claude
                            NSApp.activate(ignoringOtherApps: true)
                        }
                    }
                }
                Picker(t("Усыплять сессии после простоя", "Sleep idle sessions after"), selection: Binding(get: { app.claude.idleMinutes }, set: { app.claude.idleMinutes = $0 })) {
                    Text(t("Никогда", "Never")).tag(0)
                    Text(t("5 минут", "5 minutes")).tag(5)
                    Text(t("15 минут", "15 minutes")).tag(15)
                    Text(t("30 минут", "30 minutes")).tag(30)
                    Text(t("1 час", "1 hour")).tag(60)
                }
                Text(t("Спящая сессия не занимает память: процесс claude останавливается, переписка остаётся, следующее сообщение продолжит её.", "A sleeping session uses no memory: the claude process stops, the conversation stays, and the next message resumes it."))
                    .font(.caption)
                    .foregroundStyle(Color.syMuted)
            } header: {
                SettingsHeader(title: "Claude Code", icon: "sparkles", color: .syCoral)
            }
            Section {
                Toggle(t("Проверять обновления автоматически", "Check for updates automatically"), isOn: $autoCheckUpdates)
                HStack {
                    Text(t("Текущая версия \(Updater.shared.currentVersion)", "Current version \(Updater.shared.currentVersion)"))
                        .foregroundStyle(Color.syMuted)
                    Spacer()
                    Button(t("Проверить сейчас", "Check Now")) {
                        openWindow(id: "update")
                        Task { await Updater.shared.check(userInitiated: true) }
                    }
                    .disabled(Updater.shared.isBusy)
                }
            } header: {
                SettingsHeader(title: t("Обновления", "Updates"), icon: "arrow.down.circle", color: .syGreen)
            }
            Section {
                TextField("Client ID", text: $clientID)
            } header: {
                SettingsHeader(title: "OAuth Device Flow", icon: "key.fill", color: .syAmber)
            }
        }
        .formStyle(.grouped)
        .surface(.panel)
        .task { await ClaudeAccountStore.shared.refresh() }
        .sheet(isPresented: $showAddProfile) { AddClaudeProfileSheet().environment(app) }
        .confirmationDialog(t("Выйти из Claude Code?", "Sign out of Claude Code?"), isPresented: $confirmClaudeLogout) {
            Button(t("Выйти", "Sign out"), role: .destructive) {
                loggingOut = true
                Task {
                    app.claude.sleepAll()
                    await ClaudeAccountStore.shared.logout()
                    loggingOut = false
                }
            }
            Button(t("Отмена", "Cancel"), role: .cancel) {}
        } message: {
            Text(t("Вход сбросится и для Claude Code в терминале. Открытые сессии уснут. Войти снова (в том числе под другим аккаунтом) можно здесь же или в разделе Claude Code.", "This also signs out Claude Code in Terminal. Open sessions will go to sleep. You can sign in again (including with another account) here or in the Claude Code section."))
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
