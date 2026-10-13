import SwiftUI

struct SettingsView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("pollInterval") private var pollInterval = 120.0
    @AppStorage("oauthClientID") private var clientID = ""
    @State private var browserTick = 0

    var body: some View {
        Form {
            Section(t("Язык", "Language")) {
                Picker(t("Язык интерфейса", "Interface language"), selection: Binding(get: { Loc.shared.lang }, set: { Loc.shared.set($0); app.refresh() })) {
                    ForEach(AppLanguage.allCases) { l in
                        Text(l.title).tag(l)
                    }
                }
                .pickerStyle(.segmented)
            }
            Section(t("Аккаунт", "Account")) {
                if let u = app.user {
                    HStack(spacing: 10) {
                        Avatar(url: u.avatarUrl, size: 36)
                        VStack(alignment: .leading) {
                            Text(u.name ?? u.login).font(.headline)
                            Text("@\(u.login)").foregroundStyle(.secondary)
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
                    Text(t("Вход не выполнен", "Not signed in")).foregroundStyle(.secondary)
                }
            }
            Section(t("Ссылки", "Links")) {
                Picker(t("Открывать ссылки в", "Open links in"), selection: Binding(get: { LinkBrowser.selected }, set: { LinkBrowser.selected = $0; browserTick += 1 })) {
                    Text(t("Браузер по умолчанию", "System default")).tag(LinkBrowser.systemDefault)
                    ForEach(LinkBrowser.installed) { Text($0.name).tag($0.id) }
                }
                .id(browserTick)
            }
            Section(t("Обновление", "Refresh")) {
                Picker(t("Проверять счётчики", "Check counters"), selection: $pollInterval) {
                    Text(t("Каждую минуту", "Every minute")).tag(60.0)
                    Text(t("Каждые 2 минуты", "Every 2 minutes")).tag(120.0)
                    Text(t("Каждые 5 минут", "Every 5 minutes")).tag(300.0)
                    Text(t("Каждые 15 минут", "Every 15 minutes")).tag(900.0)
                }
                .onChange(of: pollInterval) { app.startPolling() }
            }
            Section("Claude Code") {
                Picker(t("Усыплять сессии после простоя", "Sleep idle sessions after"), selection: Binding(get: { app.claude.idleMinutes }, set: { app.claude.idleMinutes = $0 })) {
                    Text(t("Никогда", "Never")).tag(0)
                    Text(t("5 минут", "5 minutes")).tag(5)
                    Text(t("15 минут", "15 minutes")).tag(15)
                    Text(t("30 минут", "30 minutes")).tag(30)
                    Text(t("1 час", "1 hour")).tag(60)
                }
                Text(t("Спящая сессия не занимает память: процесс claude останавливается, переписка остаётся, следующее сообщение продолжит её.", "A sleeping session uses no memory: the claude process stops, the conversation stays, and the next message resumes it."))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Section("OAuth Device Flow") {
                TextField("Client ID", text: $clientID)
            }
        }
        .formStyle(.grouped)
        .frame(width: 500, height: 560)
        .padding(.vertical, 10)
    }
}
