import SwiftUI

@main
enum Main {
    static func main() {
        signal(SIGPIPE, SIG_IGN)
        if CommandLine.arguments.contains("--selftest") {
            SelfTest.run()
        } else {
            LegacyMigration.run()
            DispatchQueue.global(qos: .utility).async { Updater.clearQuarantine() }
            UserDefaults.standard.register(defaults: ["pollInterval": 30.0])
            PullYardApp.main()
        }
    }
}

struct PullYardApp: App {
    @State private var app = AppModel()

    var body: some Scene {
        Window("PullYard", id: "main") {
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
                Button("Buy Me a Pizza…") { openInBrowser(Support.pizzaURL) }
            }
            CommandGroup(after: .toolbar) {
                Button("Refresh") { app.refresh() }
                    .keyboardShortcut("r")
                Button("Open by Link…") { app.showOpenLink = true }
                    .keyboardShortcut("l")
                Divider()
                Button("Review Requests") { app.section = .prs(.reviewRequested) }.keyboardShortcut("1")
                Button("My Pull Requests") { app.section = .prs(.mine) }.keyboardShortcut("2")
                Button("Notifications") { app.section = .notifications }.keyboardShortcut("3")
                Button("Repositories") { app.section = .repos }.keyboardShortcut("4")
                Button("Projects") { app.section = .projects }.keyboardShortcut("5")
                Button("Claude Code") { app.section = .claude }.keyboardShortcut("6")
                Divider()
                Button("Copy Short Link") { if let ref = app.frontItem { app.copyShortLink(ref) } }
                    .keyboardShortcut("c", modifiers: [.command, .shift])
                    .disabled(app.frontItem == nil)
                BlameCommand()
                Divider()
                Button("Back") { if !app.detailPath.isEmpty { app.detailPath.removeLast() } }
                    .keyboardShortcut("[")
            }
        }

        MenuBarExtra {
            ThemedRoot { MenuBarView().environment(app) }
        } label: {
            MenuBarLabel(app: app)
        }
        .menuBarExtraStyle(.window)

        Window("Software Update", id: "update") {
            ThemedRoot { UpdateView() }
        }
        .windowResizability(.contentSize)
        .defaultPosition(.center)

        Window("Diagnostics", id: "diagnostics") {
            ThemedRoot { DiagnosticsView() }
        }
        .defaultSize(width: 980, height: 640)

        Settings {
            ThemedRoot { SettingsView().environment(app) }
        }
    }
}

struct BlameFocusKey: FocusedValueKey {
    typealias Value = Binding<Bool>
}

extension FocusedValues {
    var blame: Binding<Bool>? {
        get { self[BlameFocusKey.self] }
        set { self[BlameFocusKey.self] = newValue }
    }
}

struct BlameCommand: View {
    @FocusedBinding(\.blame) private var blame

    var body: some View {
        Toggle("Blame", isOn: Binding(get: { blame ?? false }, set: { blame = $0 }))
            .keyboardShortcut("b", modifiers: [.command, .option])
            .disabled(blame == nil)
    }
}
