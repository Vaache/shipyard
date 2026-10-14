import SwiftUI

enum StateFilter: String, CaseIterable, Identifiable {
    case open, closed, all
    var id: String { rawValue }

    var title: String {
        switch self {
        case .open: return "Open"
        case .closed: return "Closed"
        case .all: return "All"
        }
    }

    var query: String {
        switch self {
        case .open: return "is:open"
        case .closed: return "is:closed"
        case .all: return ""
        }
    }
}

@Observable
@MainActor
final class SearchListModel {
    private struct Snapshot {
        let items: [SearchItem]
        let total: Int
        let cursor: String?
        let hasMore: Bool
        let updated: Date
    }

    private static var cache: [String: Snapshot] = [:]

    var items: [SearchItem] = []
    var total = 0
    var loading = false
    var refreshing = false
    var lastUpdated: Date?
    var error: String?
    private var cursor: String?
    private var hasMore = false
    private var loadingMore = false
    private var currentQuery = ""
    private var generation = 0
    let rows: RowAppearTracker

    init(rows: RowAppearTracker) {
        self.rows = rows
    }

    func load(api: GitHubAPI?, query: String, silent: Bool = false, typing: Bool = false) async {
        guard let api else { return }
        var switched = false
        var cached = false
        if query != currentQuery {
            switched = true
            currentQuery = query
            error = nil
            if let snap = Self.cache[query] {
                cached = true
                apply(snap, mode: typing ? .silent : .cascade)
            } else {
                items = []
                total = 0
                lastUpdated = nil
            }
        }
        generation += 1
        let gen = generation
        if !silent || switched { loading = true }
        refreshing = true
        do {
            let r = try await api.search(query)
            guard gen == generation else { return }
            var merged = r.nodes
            var nextCursor = r.pageInfo?.endCursor
            var more = r.pageInfo?.hasNextPage ?? false
            if !switched, items.count > r.nodes.count {
                let fresh = Set(r.nodes.map(\.id))
                merged += items.dropFirst(r.nodes.count).filter { !fresh.contains($0.id) }
                nextCursor = cursor
                more = hasMore
            }
            let snap = Snapshot(items: merged, total: r.issueCount, cursor: nextCursor, hasMore: more, updated: Date())
            Self.cache[query] = snap
            apply(snap, mode: typing ? .silent : (switched && !cached ? .cascade : .refresh))
            error = nil
            loading = false
            refreshing = false
        } catch {
            guard gen == generation else { return }
            loading = false
            refreshing = false
            if isCancellation(error) { return }
            self.error = error.localizedDescription
        }
    }

    private func apply(_ snap: Snapshot, mode: RowAppearMode) {
        rows.ingest(snap.items.map(\.id), mode: mode)
        items = snap.items
        total = snap.total
        cursor = snap.cursor
        hasMore = snap.hasMore
        lastUpdated = snap.updated
    }

    func loadMore(api: GitHubAPI?) async {
        guard let api, hasMore, !loadingMore, let cursor else { return }
        let query = currentQuery
        loadingMore = true
        defer { loadingMore = false }
        if let r = try? await api.search(query, after: cursor), query == currentQuery {
            let known = Set(items.map(\.id))
            let added = r.nodes.filter { !known.contains($0.id) }
            rows.ingest(added.map(\.id), mode: .append)
            items += added
            self.cursor = r.pageInfo?.endCursor
            hasMore = r.pageInfo?.hasNextPage ?? false
            Self.cache[query] = Snapshot(items: items, total: total, cursor: self.cursor, hasMore: hasMore, updated: lastUpdated ?? Date())
        }
    }
}

struct LoadingBar: View {
    let active: Bool

    var body: some View {
        ZStack {
            if active {
                ProgressView()
                    .progressViewStyle(.linear)
                    .controlSize(.small)
                    .labelsHidden()
                    .transition(.opacity)
            }
        }
        .frame(height: 4)
        .clipped()
    }
}

struct SearchListView: View {
    let title: String
    let kind: String
    let baseQuery: String
    @Environment(AppModel.self) private var app
    @State private var model: SearchListModel
    @State private var stateFilter: StateFilter = .open
    @State private var text = ""
    @State private var loadedText = ""

    init(title: String, kind: String, baseQuery: String) {
        self.title = title
        self.kind = kind
        self.baseQuery = baseQuery
        _model = State(initialValue: SearchListModel(rows: .shared("search|\(kind)|\(baseQuery)")))
    }

    private var query: String {
        [baseQuery, "is:\(kind)", stateFilter.query, "archived:false", "sort:updated-desc", text]
            .filter { !$0.isEmpty }.joined(separator: " ")
    }

    var body: some View {
        @Bindable var app = app
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
            List(selection: Binding(get: { app.detailRoot }, set: { app.select($0) })) {
                if let e = model.error, model.items.isEmpty {
                    Text(e).foregroundStyle(.red).font(.caption)
                }
                ForEach(model.items) { item in
                    ItemRow(item: item)
                        .rowAppear(item.id, model.rows)
                        .tag(item.route)
                        .listRowSeparator(.visible)
                        .listRowSeparatorTint(Color.syLine)
                        .onAppear {
                            if item.id == model.items.last?.id { Task { await model.loadMore(api: app.api) } }
                        }
                        .contextMenu { ItemContextMenu(item: item) }
                }
                if model.items.isEmpty, !model.loading, model.error == nil {
                    Text("Nothing here").foregroundStyle(Color.syMuted)
                }
            }
            .opacity(model.loading && !model.items.isEmpty ? 0.55 : 1)
            .animation(.easeOut(duration: 0.15), value: model.loading)
            .overlay {
                if model.loading && model.items.isEmpty { ProgressView().controlSize(.small) }
            }
            .liveRefresh(updated: model.lastUpdated, refreshing: model.refreshing, error: model.items.isEmpty ? nil : model.error) {
                await model.load(api: app.api, query: query, silent: true)
            }
        }
        .searchable(text: $text, placement: .toolbar, prompt: "Filter: text, repo:org/name, label:bug")
        .navigationTitle(title)
        .navigationSubtitle(model.total > 0 ? "\(model.total)" : "")
        .task(id: "\(query)|\(app.api != nil)") {
            if !text.isEmpty { try? await Task.sleep(nanoseconds: 350_000_000) }
            guard !Task.isCancelled else { return }
            let typing = text != loadedText
            loadedText = text
            await model.load(api: app.api, query: query, typing: typing)
        }
        .onChange(of: app.refreshTick) { Task { await model.load(api: app.api, query: query) } }
        .onChange(of: app.liveTick) { Task { await model.load(api: app.api, query: query, silent: true) } }
    }
}

struct ItemContextMenu: View {
    let item: SearchItem

    var body: some View {
        Button("Open in Browser") { openInBrowser(item.url) }
        Button("Copy Link") { copyToPasteboard(item.url) }
        if item.isPR {
            Button("Copy “gh pr checkout”") { copyToPasteboard("gh pr checkout \(item.number) -R \(item.repository.nameWithOwner)") }
        }
    }
}

struct ItemRow: View {
    let item: SearchItem

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            StateBadge(state: item.visualState, compact: true)
            VStack(alignment: .leading, spacing: 3) {
                Text(item.title).lineLimit(2).fontWeight(.semibold)
                HStack(spacing: 5) {
                    Text("\(item.repository.nameWithOwner)#\(item.number)").lineLimit(1)
                    Text("·")
                    Text(Fmt.rel(item.updatedAt)).lineLimit(1)
                }
                .font(.caption)
                .foregroundStyle(Color.syMuted)
                HStack(spacing: 6) {
                    Avatar(url: item.author?.avatarUrl, size: 14)
                    Text(item.author?.login ?? "").font(.caption).foregroundStyle(Color.syMuted)
                    if item.isPR {
                        CIIcon(state: item.ciState).font(.caption)
                        reviewBadge
                        if let a = item.additions, let d = item.deletions { DiffStat(additions: a, deletions: d) }
                    }
                    if item.comments.totalCount > 0 {
                        Label("\(item.comments.totalCount)", systemImage: "bubble.left").font(.caption).foregroundStyle(Color.syMuted).labelStyle(.titleAndIcon)
                    }
                }
                if let base = item.stackedOn {
                    Label("stacked on \(base)", systemImage: "square.stack.3d.up")
                        .font(.caption)
                        .foregroundStyle(Color.syIndigo)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(base)
                }
                if let labels = item.labels?.nodes, !labels.isEmpty {
                    HStack(spacing: 4) { ForEach(labels.prefix(4), id: \.name) { LabelChip(label: $0) } }
                }
            }
        }
        .padding(.vertical, 5)
    }

    @ViewBuilder
    private var reviewBadge: some View {
        switch item.reviewDecision {
        case "APPROVED": Image(systemName: "checkmark.seal.fill").foregroundStyle(Color.ghGreen).font(.caption).help("Approved")
        case "CHANGES_REQUESTED": Image(systemName: "exclamationmark.bubble.fill").foregroundStyle(Color.ghRed).font(.caption).help("Changes requested")
        case "REVIEW_REQUIRED": Image(systemName: "eye").foregroundStyle(Color.syMuted).font(.caption).help("Review required")
        default: EmptyView()
        }
    }
}
