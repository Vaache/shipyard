import SwiftUI

enum ForgeListKind: Hashable {
    case pulls(PRFilter)
    case pullsByProject
    case issues(IssueFilter)

    var key: String {
        switch self {
        case .pulls(let f): return "pr-\(f.rawValue)"
        case .pullsByProject: return "pr-grouped"
        case .issues(let f): return "issue-\(f.rawValue)"
        }
    }

    var grouped: Bool { self == .pullsByProject }

    func title(_ provider: GitProvider) -> String {
        switch self {
        case .pulls(let f): return f.title
        case .pullsByProject: return "My \(provider.requestShort)s by project"
        case .issues(let f): return "Issues: \(f.title)"
        }
    }

    @MainActor
    func fetch(_ forge: ForgeClient, state: StateFilter) async throws -> [SearchItem] {
        switch self {
        case .pulls(let f): return try await forge.pullRequests(f, state: state)
        case .pullsByProject: return try await forge.pullRequests(.mine, state: state)
        case .issues(let f): return try await forge.issues(f, state: state)
        }
    }
}

@Observable
@MainActor
final class ForgeListModel {
    private static var cache: [String: (items: [SearchItem], updated: Date)] = [:]

    static func resetCache() {
        cache = [:]
    }

    var items: [SearchItem] = []
    var loading = false
    var refreshing = false
    var lastUpdated: Date?
    var error: String?
    private var currentKey = ""
    private var generation = 0
    let rows: RowAppearTracker

    init(rows: RowAppearTracker) {
        self.rows = rows
    }

    func load(_ forge: ForgeClient?, account: String, kind: ForgeListKind, state: StateFilter, silent: Bool = false) async {
        guard let forge else { return }
        let key = "\(account)|\(kind.key)|\(state.rawValue)"
        var switched = false
        if key != currentKey {
            switched = true
            currentKey = key
            error = nil
            if let hit = Self.cache[key] {
                rows.ingest(hit.items.map(\.id), mode: .silent)
                items = hit.items
                lastUpdated = hit.updated
            } else {
                items = []
                lastUpdated = nil
            }
        }
        generation += 1
        let gen = generation
        if !silent || switched { loading = true }
        refreshing = true
        defer {
            if gen == generation {
                loading = false
                refreshing = false
            }
        }
        do {
            let fresh = try await kind.fetch(forge, state: state)
            guard gen == generation else { return }
            let updated = Date()
            Self.cache[key] = (fresh, updated)
            rows.ingest(fresh.map(\.id), mode: switched && items.isEmpty ? .cascade : .refresh)
            items = fresh
            lastUpdated = updated
            error = nil
        } catch {
            guard gen == generation, !isCancellation(error) else { return }
            self.error = error.localizedDescription
        }
    }
}

struct ForgeListView: View {
    let kind: ForgeListKind
    @Environment(AppModel.self) private var app
    @State private var model: ForgeListModel
    @State private var stateFilter: StateFilter = .open
    @State private var text = ""
    @State private var collapsed: Set<String> = []

    init(kind: ForgeListKind) {
        self.kind = kind
        _model = State(initialValue: ForgeListModel(rows: .shared("forge|\(kind.key)")))
    }

    private func groups(_ items: [SearchItem]) -> [RepoGroup] {
        var order: [String] = []
        var byRepo: [String: [SearchItem]] = [:]
        for item in items {
            let key = item.repository.nameWithOwner
            if byRepo[key] == nil { order.append(key) }
            byRepo[key, default: []].append(item)
        }
        return order.map { RepoGroup(id: $0, items: byRepo[$0] ?? []) }
    }

    private var visible: [SearchItem] {
        let q = text.trimmingCharacters(in: .whitespaces).lowercased()
        guard !q.isEmpty else { return model.items }
        return model.items.filter {
            $0.title.lowercased().contains(q) || $0.repository.nameWithOwner.lowercased().contains(q) || ($0.author?.login.lowercased().contains(q) ?? false) || "\($0.number)" == q
        }
    }

    var body: some View {
        let items = visible
        VStack(spacing: 0) {
            Picker("", selection: $stateFilter) {
                ForEach(StateFilter.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            LoadingBar(active: model.loading && !model.items.isEmpty)
            Divider()
            List(selection: Binding(get: { app.forgeSelection?.id }, set: { id in app.forgeSelection = model.items.first { $0.id == id } })) {
                if let e = model.error, model.items.isEmpty {
                    Text(e).foregroundStyle(.red).font(.caption).textSelection(.enabled)
                }
                if kind.grouped {
                    ForEach(groups(items)) { group in
                        let open = !collapsed.contains(group.id) || !text.isEmpty
                        Section {
                            if open { ForEach(group.items) { row($0, showRepo: false) } }
                        } header: {
                            RepoGroupHeader(group: group, expanded: open) {
                                if collapsed.contains(group.id) { collapsed.remove(group.id) } else { collapsed.insert(group.id) }
                            }
                        }
                    }
                } else {
                    ForEach(items) { row($0, showRepo: true) }
                }
                if items.isEmpty, !model.loading, model.error == nil {
                    Text("Nothing here").foregroundStyle(Color.syMuted)
                }
            }
            .opacity(model.loading && !model.items.isEmpty ? 0.55 : 1)
            .animation(.easeOut(duration: 0.15), value: model.loading)
            .overlay {
                if model.loading && model.items.isEmpty { ProgressView().controlSize(.small) }
            }
            .liveRefresh(updated: model.lastUpdated, refreshing: model.refreshing, error: model.items.isEmpty ? nil : model.error) {
                await reload(silent: true)
            }
        }
        .searchable(text: $text, placement: .toolbar, prompt: "Filter: title, repository, author")
        .navigationTitle(kind.title(app.provider))
        .navigationSubtitle(items.isEmpty ? "" : "\(items.count)")
        .task(id: "\(stateFilter.rawValue)|\(app.account?.id ?? "")") { await reload() }
        .onChange(of: app.refreshTick) { Task { await reload() } }
        .onLiveTick { await reload(silent: true) }
    }

    private func row(_ item: SearchItem, showRepo: Bool) -> some View {
        ItemRow(item: item, showRepo: showRepo, numberPrefix: item.isPR ? app.provider.numberPrefix : "#")
            .rowAppear(item.id, model.rows)
            .tag(item.id)
            .listRowSeparator(.visible)
            .listRowSeparatorTint(Color.syLine)
            .contextMenu { ForgeItemContextMenu(item: item) }
    }

    private func reload(silent: Bool = false) async {
        await model.load(app.forge, account: app.account?.id ?? "", kind: kind, state: stateFilter, silent: silent)
    }
}

struct ForgeItemContextMenu: View {
    let item: SearchItem

    var body: some View {
        Button("Open in Browser") { openInBrowser(item.url) }
        Button("Copy Link") { copyToPasteboard(item.url) }
        if let base = item.baseRefName {
            Button("Copy Target Branch") { copyToPasteboard(base) }
        }
    }
}

struct ForgeDetailColumn: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        if let item = app.forgeSelection {
            ForgeItemDetailView(item: item, provider: app.provider).id(item.id).transition(.rise)
        } else {
            VStack(spacing: 12) {
                IconTile(systemImage: app.provider.icon, color: .syViolet, size: 56).opacity(0.9)
                Text("Select a \(app.provider.requestTitle)").foregroundStyle(Color.syMuted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .transition(.opacity)
        }
    }
}

struct ForgeItemDetailView: View {
    let item: SearchItem
    let provider: GitProvider

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                HStack(spacing: 8) {
                    StateBadge(state: item.visualState)
                    Text("\(item.repository.nameWithOwner)\(item.isPR ? provider.numberPrefix : "#")\(item.number)")
                        .font(.callout.monospaced())
                        .foregroundStyle(Color.syMuted)
                        .textSelection(.enabled)
                }
                Text(item.title)
                    .font(.title2.weight(.semibold))
                    .foregroundStyle(Color.syText)
                    .textSelection(.enabled)
                HStack(spacing: 6) {
                    Avatar(url: item.author?.avatarUrl, size: 18)
                    Text(item.author?.login ?? "").fontWeight(.medium)
                    Text("opened \(Fmt.rel(item.createdAt)) · updated \(Fmt.rel(item.updatedAt))").foregroundStyle(Color.syMuted)
                }
                .font(.callout)
                if let base = item.baseRefName {
                    Label("into \(base)", systemImage: "arrow.triangle.merge")
                        .font(.callout)
                        .foregroundStyle(Color.syMuted)
                        .textSelection(.enabled)
                }
                if let labels = item.labels?.nodes, !labels.isEmpty {
                    HStack(spacing: 4) { ForEach(labels.prefix(8), id: \.name) { LabelChip(label: $0) } }
                }
                HStack {
                    Button {
                        openInBrowser(item.url)
                    } label: {
                        Label("Open in Browser", systemImage: "safari")
                    }
                    .buttonStyle(.borderedProminent)
                    Button("Copy Link") { copyToPasteboard(item.url) }
                }
                .controlSize(.large)
                .padding(.top, 4)
                Label(item.isPR ? "Changed files, comments and review for \(provider.title) are coming in a later version. Open the \(provider.requestTitle) in the browser to review it." : "Comments for \(provider.title) issues are coming in a later version. Open the issue in the browser to reply.", systemImage: "info.circle")
                    .font(.callout)
                    .foregroundStyle(Color.syMuted)
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .card(radius: 10, fill: .sySunken)
            }
            .padding(24)
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

struct ForgeUnavailableView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        VStack(spacing: 10) {
            IconTile(systemImage: app.provider.icon, color: .syDim, size: 40)
            Text("Not available for \(app.provider.title) yet").foregroundStyle(Color.syMuted)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
