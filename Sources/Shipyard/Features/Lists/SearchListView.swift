import SwiftUI

enum StateFilter: String, CaseIterable, Identifiable {
    case open, closed, all
    var id: String { rawValue }

    var title: String {
        switch self {
        case .open: return t("Открытые", "Open")
        case .closed: return t("Закрытые", "Closed")
        case .all: return t("Все", "All")
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
    }

    private static var cache: [String: Snapshot] = [:]

    var items: [SearchItem] = []
    var total = 0
    var loading = false
    var error: String?
    private var cursor: String?
    private var hasMore = false
    private var currentQuery = ""

    func load(api: GitHubAPI?, query: String) async {
        guard let api else { return }
        if query != currentQuery {
            currentQuery = query
            error = nil
            if let snap = Self.cache[query] {
                apply(snap)
            } else {
                items = []
                total = 0
            }
        }
        loading = true
        do {
            let r = try await api.search(query)
            guard query == currentQuery else { return }
            let snap = Snapshot(items: r.nodes, total: r.issueCount, cursor: r.pageInfo?.endCursor, hasMore: r.pageInfo?.hasNextPage ?? false)
            Self.cache[query] = snap
            apply(snap)
            error = nil
            loading = false
        } catch {
            guard query == currentQuery else { return }
            loading = false
            if Task.isCancelled || (error as? URLError)?.code == .cancelled || error is CancellationError { return }
            self.error = error.localizedDescription
        }
    }

    private func apply(_ snap: Snapshot) {
        items = snap.items
        total = snap.total
        cursor = snap.cursor
        hasMore = snap.hasMore
    }

    func loadMore(api: GitHubAPI?) async {
        guard let api, hasMore, !loading, let cursor else { return }
        let query = currentQuery
        loading = true
        defer { loading = false }
        if let r = try? await api.search(query, after: cursor), query == currentQuery {
            items += r.nodes
            self.cursor = r.pageInfo?.endCursor
            hasMore = r.pageInfo?.hasNextPage ?? false
            Self.cache[query] = Snapshot(items: items, total: total, cursor: self.cursor, hasMore: hasMore)
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
    @State private var model = SearchListModel()
    @State private var stateFilter: StateFilter = .open
    @State private var text = ""

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
                if let e = model.error {
                    Text(e).foregroundStyle(.red).font(.caption)
                }
                ForEach(model.items) { item in
                    ItemRow(item: item)
                        .tag(item.route)
                        .listRowSeparator(.visible)
                        .listRowSeparatorTint(Color.syLine)
                        .onAppear {
                            if item.id == model.items.last?.id { Task { await model.loadMore(api: app.api) } }
                        }
                        .contextMenu { ItemContextMenu(item: item) }
                }
                if model.items.isEmpty, !model.loading, model.error == nil {
                    Text(t("Ничего нет", "Nothing here")).foregroundStyle(Color.syMuted)
                }
            }
            .opacity(model.loading && !model.items.isEmpty ? 0.55 : 1)
            .animation(.easeOut(duration: 0.15), value: model.loading)
            .overlay {
                if model.loading && model.items.isEmpty { ProgressView().controlSize(.small) }
            }
        }
        .searchable(text: $text, placement: .toolbar, prompt: t("Фильтр: текст, repo:org/name, label:bug", "Filter: text, repo:org/name, label:bug"))
        .navigationTitle(title)
        .navigationSubtitle(model.total > 0 ? "\(model.total)" : "")
        .task(id: "\(query)|\(app.api != nil)") {
            if !text.isEmpty { try? await Task.sleep(nanoseconds: 350_000_000) }
            guard !Task.isCancelled else { return }
            await model.load(api: app.api, query: query)
        }
        .onChange(of: app.refreshTick) { Task { await model.load(api: app.api, query: query) } }
    }
}

struct ItemContextMenu: View {
    let item: SearchItem

    var body: some View {
        Button(t("Открыть в браузере", "Open in Browser")) { openInBrowser(item.url) }
        Button(t("Копировать ссылку", "Copy Link")) { copyToPasteboard(item.url) }
        if item.isPR {
            Button(t("Копировать «gh pr checkout»", "Copy “gh pr checkout”")) { copyToPasteboard("gh pr checkout \(item.number) -R \(item.repository.nameWithOwner)") }
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
                    Label(t("в стеке на \(base)", "stacked on \(base)"), systemImage: "square.stack.3d.up")
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
        case "APPROVED": Image(systemName: "checkmark.seal.fill").foregroundStyle(Color.ghGreen).font(.caption).help(t("Одобрено", "Approved"))
        case "CHANGES_REQUESTED": Image(systemName: "exclamationmark.bubble.fill").foregroundStyle(Color.ghRed).font(.caption).help(t("Запрошены изменения", "Changes requested"))
        case "REVIEW_REQUIRED": Image(systemName: "eye").foregroundStyle(Color.syMuted).font(.caption).help(t("Нужно ревью", "Review required"))
        default: EmptyView()
        }
    }
}
