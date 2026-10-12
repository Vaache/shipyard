import SwiftUI

struct MenuBarLabel: View {
    let app: AppModel

    var body: some View {
        let count = app.reviewRequestCount
        if count > 0 {
            HStack(spacing: 2) {
                Image(systemName: "arrow.triangle.pull")
                Text("\(count)")
            }
        } else {
            Image(systemName: "arrow.triangle.pull")
        }
    }
}

struct MenuBarView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("GitHub Lite").font(.headline)
                if let u = app.user { Text("@\(u.login)").foregroundStyle(.secondary) }
                Spacer()
                Button { Task { await app.refreshBadges() } } label: { Image(systemName: "arrow.clockwise") }
                    .buttonStyle(.borderless)
                    .help(t("Обновить", "Refresh"))
            }
            .padding(12)
            Divider()
            if app.phase != .loggedIn {
                Button(t("Войти…", "Sign in…")) { show(nil) }.padding(12)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        section(t("Ждут моего ревью", "Review requests"), count: app.reviewRequestCount, items: app.reviewRequests, filter: .reviewRequested)
                        section(t("Мои открытые PR", "My open PRs"), count: app.myPRCount, items: app.myPRs, filter: .mine)
                        Button {
                            app.section = .notifications
                            show(nil)
                        } label: {
                            HStack {
                                Image(systemName: "bell")
                                Text(t("Уведомления", "Notifications"))
                                Spacer()
                                if app.unreadCount > 0 { Text(app.unreadCount >= 50 ? "50+" : "\(app.unreadCount)").foregroundStyle(.secondary) }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(MenuRowStyle())
                    }
                    .padding(6)
                }
                .frame(maxHeight: 460)
            }
            Divider()
            HStack {
                Button(t("Открыть окно", "Open window")) { show(nil) }
                Spacer()
                Button(t("Выйти", "Quit")) { NSApp.terminate(nil) }
            }
            .buttonStyle(.borderless)
            .padding(12)
        }
        .frame(width: 380)
    }

    @ViewBuilder
    private func section(_ title: String, count: Int, items: [SearchItem], filter: PRFilter) -> some View {
        Button {
            app.section = .prs(filter)
            show(nil)
        } label: {
            HStack {
                Text(title).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                Spacer()
                Text("\(count)").font(.caption).foregroundStyle(.secondary)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(MenuRowStyle())
        if items.isEmpty {
            Text(t("Пусто", "Nothing here")).font(.callout).foregroundStyle(.tertiary).padding(.horizontal, 8).padding(.bottom, 6)
        }
        ForEach(items) { item in
            Button { show(item.route) } label: {
                HStack(alignment: .top, spacing: 8) {
                    StateBadge(state: item.visualState, compact: true)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(item.title).lineLimit(1)
                        Text("\(item.repository.nameWithOwner)#\(item.number) · \(item.author?.login ?? "") · \(Fmt.rel(item.updatedAt))")
                            .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer(minLength: 4)
                    CIIcon(state: item.ciState).font(.caption)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(MenuRowStyle())
        }
        Spacer().frame(height: 6)
    }

    private func show(_ route: Route?) {
        openWindow(id: "main")
        if let route { app.select(route) }
        NSApp.activate(ignoringOtherApps: true)
    }
}

struct MenuRowStyle: ButtonStyle {
    @State private var hover = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: 6).fill(hover || configuration.isPressed ? Color.accentColor.opacity(0.15) : .clear))
            .onHover { hover = $0 }
    }
}
