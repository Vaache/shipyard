import SwiftUI

@main
enum Main {
    static func main() {
        if CommandLine.arguments.contains("--selftest") {
            SelfTest.run()
        } else {
            LegacyMigration.run()
            UserDefaults.standard.register(defaults: ["pollInterval": 120.0])
            ShipyardApp.main()
        }
    }
}

struct ShipyardApp: App {
    @State private var app = AppModel()

    var body: some Scene {
        Window("Shipyard", id: "main") {
            ThemedRoot {
                RootView()
                    .environment(app)
                    .frame(minWidth: 960, minHeight: 600)
                    .onOpenURL { app.handle(url: $0) }
            }
        }
        .defaultSize(width: 1380, height: 860)
        .commands {
            CommandGroup(after: .appInfo) {
                CheckForUpdatesButton()
            }
            CommandGroup(after: .toolbar) {
                Button(t("Обновить", "Refresh")) { app.refresh() }
                    .keyboardShortcut("r")
                Button(t("Открыть по ссылке…", "Open by Link…")) { app.showOpenLink = true }
                    .keyboardShortcut("l")
                Divider()
                Button(t("Ждут моего ревью", "Review Requests")) { app.section = .prs(.reviewRequested) }.keyboardShortcut("1")
                Button(t("Мои PR", "My Pull Requests")) { app.section = .prs(.mine) }.keyboardShortcut("2")
                Button(t("Уведомления", "Notifications")) { app.section = .notifications }.keyboardShortcut("3")
                Button(t("Репозитории", "Repositories")) { app.section = .repos }.keyboardShortcut("4")
                Button(t("Проекты", "Projects")) { app.section = .projects }.keyboardShortcut("5")
                Button("Claude Code") { app.section = .claude }.keyboardShortcut("6")
                Divider()
                Button(t("Назад", "Back")) { if !app.detailPath.isEmpty { app.detailPath.removeLast() } }
                    .keyboardShortcut("[")
            }
        }

        MenuBarExtra {
            ThemedRoot { MenuBarView().environment(app) }
        } label: {
            MenuBarLabel(app: app)
        }
        .menuBarExtraStyle(.window)

        Window(t("Обновление ПО", "Software Update"), id: "update") {
            ThemedRoot { UpdateView() }
        }
        .windowResizability(.contentSize)
        .defaultPosition(.center)

        Settings {
            ThemedRoot { SettingsView().environment(app) }
        }
    }
}
