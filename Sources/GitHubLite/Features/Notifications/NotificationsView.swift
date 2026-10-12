import SwiftUI

struct NotificationsView: View {
    @Environment(AppModel.self) private var app
    @State private var items: [NotificationThread] = []
    @State private var loading = false
    @State private var error: String?
    @State private var showAll = false
    @State private var selection: String?

    var body: some View {
        VStack(spacing: 0) {
            Picker("", selection: $showAll) {
                Text(t("Непрочитанные", "Unread")).tag(false)
                Text(t("Все", "All")).tag(true)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            Divider()
            List(selection: $selection) {
                if let error { Text(error).foregroundStyle(.red).font(.caption) }
                ForEach(items) { n in
                    NotificationRow(item: n)
                        .tag(n.id)
                        .contextMenu {
                            if n.unread { Button(t("Отметить прочитанным", "Mark as read")) { Task { await markRead(n) } } }
                            Button(t("Готово (убрать из входящих)", "Done (remove from inbox)")) { Task { await markDone(n) } }
                            Button(t("Открыть в браузере", "Open in browser")) { openInBrowser(n.webURL) }
                        }
                }
                if items.isEmpty, !loading, error == nil {
                    VStack(spacing: 6) {
                        Image(systemName: "tray").font(.title).foregroundStyle(.secondary)
                        Text(showAll ? t("Уведомлений нет", "No notifications") : t("Всё прочитано", "All caught up")).foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.top, 30)
                }
            }
            .overlay { if loading && items.isEmpty { ProgressView().controlSize(.small) } }
        }
        .navigationTitle(t("Уведомления", "Notifications"))
        .toolbar {
            ToolbarItem {
                Button { Task { await markAll() } } label: { Label(t("Прочитать все", "Mark all read"), systemImage: "checkmark.circle") }
                    .help(t("Отметить все прочитанными", "Mark all as read"))
                    .disabled(!items.contains { $0.unread })
            }
        }
        .task(id: showAll) { await load() }
        .onChange(of: app.refreshTick) { Task { await load() } }
        .onChange(of: selection) { _, id in
            guard let id, let n = items.first(where: { $0.id == id }) else { return }
            if let route = n.route { app.select(route) } else { openInBrowser(n.webURL) }
            if n.unread { Task { await markRead(n) } }
        }
    }

    private func load() async {
        guard let api = app.api else { return }
        loading = true
        defer { loading = false }
        do {
            items = try await api.notifications(all: showAll)
            if !showAll { app.unreadCount = items.count }
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func markRead(_ n: NotificationThread) async {
        guard let api = app.api else { return }
        try? await api.markNotificationRead(n.id)
        if let i = items.firstIndex(where: { $0.id == n.id }) { items[i].unread = false }
        app.unreadCount = max(0, app.unreadCount - 1)
    }

    private func markDone(_ n: NotificationThread) async {
        guard let api = app.api else { return }
        try? await api.markNotificationDone(n.id)
        items.removeAll { $0.id == n.id }
        if n.unread { app.unreadCount = max(0, app.unreadCount - 1) }
    }

    private func markAll() async {
        guard let api = app.api else { return }
        try? await api.markAllNotificationsRead()
        for i in items.indices { items[i].unread = false }
        app.unreadCount = 0
    }
}

struct NotificationRow: View {
    let item: NotificationThread

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Circle().fill(item.unread ? Color.accentColor : .clear).frame(width: 7, height: 7).padding(.top, 6)
            Image(systemName: icon).foregroundStyle(.secondary).frame(width: 16).padding(.top, 2)
            VStack(alignment: .leading, spacing: 3) {
                Text(item.subject.title).lineLimit(2).fontWeight(item.unread ? .semibold : .regular)
                HStack(spacing: 5) {
                    Avatar(url: item.repository.owner?.avatarUrl, size: 13)
                    Text(item.repository.fullName).lineLimit(1)
                    Text("·")
                    Text(reason).lineLimit(1)
                    Text("·")
                    Text(Fmt.rel(item.updatedAt)).lineLimit(1)
                }
                .font(.caption)
                .foregroundStyle(.secondary)
            }
        }
        .padding(.vertical, 3)
    }

    private var icon: String {
        switch item.subject.type {
        case "PullRequest": return "arrow.triangle.pull"
        case "Issue": return "smallcircle.filled.circle"
        case "Release": return "tag"
        case "Discussion": return "bubble.left.and.bubble.right"
        case "CheckSuite": return "xmark.octagon"
        case "Commit": return "point.3.connected.trianglepath.dotted"
        default: return "bell"
        }
    }

    private var reason: String {
        switch item.reason {
        case "review_requested": return t("запрошено ревью", "review requested")
        case "mention": return t("упоминание", "mention")
        case "team_mention": return t("упоминание команды", "team mention")
        case "author": return t("вы автор", "author")
        case "comment": return t("комментарий", "comment")
        case "assign": return t("назначено", "assigned")
        case "state_change": return t("смена статуса", "state change")
        case "subscribed": return t("подписка", "subscribed")
        case "ci_activity": return "CI"
        case "manual": return t("подписка", "subscribed")
        case "security_alert": return t("безопасность", "security alert")
        default: return item.reason
        }
    }
}
