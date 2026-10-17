import SwiftUI

struct ForgeInboxView: View {
    @Environment(AppModel.self) private var app
    @State private var items: [ForgeNotification] = []
    @State private var loading = false
    @State private var refreshing = false
    @State private var error: String?
    @State private var updated: Date?

    var body: some View {
        List {
            if let error, items.isEmpty {
                Text(error).foregroundStyle(.red).font(.caption).textSelection(.enabled)
            }
            ForEach(items) { n in
                ForgeNotificationRow(notification: n) {
                    Task { await markDone(n) }
                }
                .contentShape(Rectangle())
                .onTapGesture { openInBrowser(n.url) }
                .contextMenu {
                    Button("Open in Browser") { openInBrowser(n.url) }
                    Button("Copy Link") { copyToPasteboard(n.url) }
                    Button("Mark as Done") { Task { await markDone(n) } }
                }
                .listRowSeparator(.visible)
                .listRowSeparatorTint(Color.syLine)
            }
            if items.isEmpty, !loading, error == nil {
                Text("All done").foregroundStyle(Color.syMuted)
            }
        }
        .overlay {
            if loading && items.isEmpty { ProgressView().controlSize(.small) }
        }
        .liveRefresh(updated: updated, refreshing: refreshing, error: items.isEmpty ? nil : error) {
            await load()
        }
        .navigationTitle(app.forgeCaps.inboxTitle ?? "Inbox")
        .navigationSubtitle(items.isEmpty ? "" : "\(items.count)")
        .task(id: app.account?.id) {
            loading = true
            await load()
            loading = false
        }
        .onChange(of: app.refreshTick) { Task { await load() } }
        .onLiveTick { await load() }
    }

    private func load() async {
        guard let forge = app.forge else { return }
        refreshing = true
        defer { refreshing = false }
        do {
            let fresh = try await forge.notifications()
            guard app.forge === forge else { return }
            items = fresh
            app.unreadCount = fresh.filter(\.unread).count
            updated = Date()
            error = nil
        } catch {
            if !isCancellation(error) { self.error = error.localizedDescription }
        }
    }

    private func markDone(_ n: ForgeNotification) async {
        guard let forge = app.forge else { return }
        do {
            try await forge.markDone(n)
            withAnimation(Motion.quick) { items.removeAll { $0.id == n.id } }
            app.unreadCount = items.filter(\.unread).count
        } catch {
            app.showToast(error.localizedDescription)
        }
    }
}

struct ForgeNotificationRow: View {
    let notification: ForgeNotification
    let onDone: () -> Void
    @State private var hovering = false

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Avatar(url: notification.author?.avatarUrl, size: 22)
            VStack(alignment: .leading, spacing: 3) {
                Text(notification.title).fontWeight(.semibold).lineLimit(2).foregroundStyle(Color.syText)
                HStack(spacing: 5) {
                    Text(notification.reason).foregroundStyle(Color.syAccent)
                    if !notification.repo.isEmpty {
                        Text("·")
                        Text(notification.repo).lineLimit(1)
                    }
                    Text("·")
                    Text(Fmt.rel(notification.updatedAt))
                }
                .font(.caption)
                .foregroundStyle(Color.syMuted)
                if let author = notification.author {
                    Text("\(notification.kind) · @\(author.login)").font(.caption).foregroundStyle(Color.syDim)
                }
            }
            Spacer(minLength: 4)
            Button(action: onDone) {
                Image(systemName: "checkmark.circle")
            }
            .buttonStyle(.borderless)
            .help("Mark as done")
            .opacity(hovering ? 1 : 0.45)
        }
        .padding(.vertical, 5)
        .onHover { hovering = $0 }
    }
}

struct ForgeReposView: View {
    @Environment(AppModel.self) private var app
    @State private var repos: [ForgeRepo] = []
    @State private var loading = false
    @State private var error: String?
    @State private var text = ""
    @State private var updated: Date?

    private var visible: [ForgeRepo] {
        let q = text.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return repos }
        return repos.filter { $0.fullName.lowercased().contains(q) || ($0.description?.lowercased().contains(q) ?? false) }
    }

    var body: some View {
        List {
            if let error, repos.isEmpty {
                Text(error).foregroundStyle(.red).font(.caption).textSelection(.enabled)
            }
            ForEach(visible) { repo in
                ForgeRepoRow(repo: repo)
                    .contentShape(Rectangle())
                    .onTapGesture { openInBrowser(repo.url) }
                    .contextMenu {
                        Button("Open in Browser") { openInBrowser(repo.url) }
                        Button("Copy Link") { copyToPasteboard(repo.url) }
                        Button("Copy Name") { copyToPasteboard(repo.fullName) }
                    }
                    .listRowSeparator(.visible)
                    .listRowSeparatorTint(Color.syLine)
            }
            if visible.isEmpty, !loading, error == nil {
                Text("Nothing here").foregroundStyle(Color.syMuted)
            }
        }
        .overlay {
            if loading && repos.isEmpty { ProgressView().controlSize(.small) }
        }
        .liveRefresh(updated: updated, refreshing: loading, error: repos.isEmpty ? nil : error) {
            await load()
        }
        .searchable(text: $text, placement: .toolbar, prompt: "Filter \(app.provider.repositoriesTitle.lowercased())")
        .navigationTitle(app.provider.repositoriesTitle)
        .navigationSubtitle(repos.isEmpty ? "" : "\(repos.count)")
        .task(id: app.account?.id) { await load() }
        .onChange(of: app.refreshTick) { Task { await load() } }
    }

    private func load() async {
        guard let forge = app.forge else { return }
        loading = true
        defer { loading = false }
        do {
            let fresh = try await forge.repositories()
            guard app.forge === forge else { return }
            repos = fresh
            updated = Date()
            error = nil
        } catch {
            if !isCancellation(error) { self.error = error.localizedDescription }
        }
    }
}

struct ForgeRepoRow: View {
    let repo: ForgeRepo

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            IconTile(systemImage: repo.isPrivate ? "lock.fill" : "book.closed.fill", color: ProjectPalette.color(for: repo.fullName), size: 24)
            VStack(alignment: .leading, spacing: 3) {
                Text(repo.fullName).fontWeight(.semibold).lineLimit(1).foregroundStyle(Color.syText)
                if let d = repo.description, !d.isEmpty {
                    Text(d).font(.caption).foregroundStyle(Color.syMuted).lineLimit(2)
                }
                HStack(spacing: 8) {
                    if let branch = repo.defaultBranch {
                        Label(branch, systemImage: "arrow.triangle.branch")
                    }
                    if let stars = repo.stars, stars > 0 {
                        Label("\(stars)", systemImage: "star")
                    }
                    if let updated = repo.updatedAt {
                        Text("updated \(Fmt.rel(updated))")
                    }
                }
                .font(.caption)
                .foregroundStyle(Color.syDim)
            }
        }
        .padding(.vertical, 5)
    }
}
