import SwiftUI

struct ReposView: View {
    @Environment(AppModel.self) private var app
    @State private var repos: [RepoSummary] = []
    @State private var cursor: String?
    @State private var hasMore = false
    @State private var loading = false
    @State private var error: String?
    @State private var text = ""
    @State private var hideArchived = true

    private var visible: [RepoSummary] {
        repos.filter { (!hideArchived || !$0.isArchived) && (text.isEmpty || $0.nameWithOwner.localizedCaseInsensitiveContains(text) || ($0.description ?? "").localizedCaseInsensitiveContains(text)) }
    }

    var body: some View {
        List(selection: Binding(get: { app.detailRoot }, set: { app.select($0) })) {
            if let error { Text(error).foregroundStyle(.red).font(.caption) }
            ForEach(visible) { r in
                RepoRow(repo: r)
                    .tag(Route.repo(r.ref ?? RepoRef(owner: "", name: "")))
                    .contextMenu {
                        Button(t("Открыть в браузере", "Open in browser")) { openInBrowser(r.url) }
                        Button(t("Копировать ссылку", "Copy link")) { copyToPasteboard(r.url) }
                        Button(t("Копировать «gh repo clone»", "Copy “gh repo clone”")) { copyToPasteboard("gh repo clone \(r.nameWithOwner)") }
                    }
                    .onAppear { if r.id == repos.last?.id { Task { await loadMore() } } }
            }
        }
        .overlay { if loading && repos.isEmpty { ProgressView().controlSize(.small) } }
        .searchable(text: $text, placement: .toolbar, prompt: t("Фильтр репозиториев", "Filter repositories"))
        .navigationTitle(t("Репозитории", "Repositories"))
        .toolbar {
            ToolbarItem {
                Toggle(isOn: $hideArchived) { Label(t("Скрыть архивные", "Hide archived"), systemImage: "archivebox") }
                    .help(t("Скрыть архивные", "Hide archived"))
            }
        }
        .task(id: app.api != nil) { if repos.isEmpty { await load() } }
        .onChange(of: app.refreshTick) { Task { await load() } }
        .onChange(of: text) { _, t in
            if !t.isEmpty, hasMore { Task { await loadAll() } }
        }
    }

    private func load() async {
        guard let api = app.api else { return }
        loading = true
        defer { loading = false }
        do {
            let r = try await api.repos(after: nil)
            repos = r.nodes
            cursor = r.pageInfo.endCursor
            hasMore = r.pageInfo.hasNextPage
            error = nil
        } catch {
            if isCancellation(error) { return }
            self.error = error.localizedDescription
        }
    }

    private func loadMore() async {
        guard let api = app.api, hasMore, !loading else { return }
        loading = true
        defer { loading = false }
        if let r = try? await api.repos(after: cursor) {
            repos += r.nodes
            cursor = r.pageInfo.endCursor
            hasMore = r.pageInfo.hasNextPage
        }
    }

    private func loadAll() async {
        var guardCount = 0
        while hasMore, guardCount < 20 {
            await loadMore()
            guardCount += 1
        }
    }
}

struct RepoRow: View {
    let repo: RepoSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 5) {
                Image(systemName: repo.isPrivate ? "lock" : "book.closed").font(.caption).foregroundStyle(.secondary)
                Text(repo.nameWithOwner).fontWeight(.medium).lineLimit(1)
                if repo.isArchived { Text(t("архив", "archived")).font(.caption2).foregroundStyle(.secondary) }
                if repo.isFork { Image(systemName: "tuningfork").font(.caption2).foregroundStyle(.secondary) }
            }
            if let d = repo.description, !d.isEmpty {
                Text(d).font(.caption).foregroundStyle(.secondary).lineLimit(2)
            }
            HStack(spacing: 10) {
                if let lang = repo.primaryLanguage {
                    HStack(spacing: 3) {
                        Circle().fill(Color(hex: lang.color ?? "#888")).frame(width: 8, height: 8)
                        Text(lang.name)
                    }
                }
                if repo.pullRequests.totalCount > 0 { Label("\(repo.pullRequests.totalCount)", systemImage: "arrow.triangle.pull") }
                if repo.issues.totalCount > 0 { Label("\(repo.issues.totalCount)", systemImage: "smallcircle.filled.circle") }
                if repo.stargazerCount > 0 { Label("\(repo.stargazerCount)", systemImage: "star") }
                Spacer()
                Text(Fmt.rel(repo.pushedAt))
            }
            .font(.caption)
            .foregroundStyle(.secondary)
        }
        .padding(.vertical, 3)
    }
}

enum RepoTab: String, CaseIterable, Identifiable {
    case code, pr, issue
    var id: String { rawValue }
}

struct RepoDetailView: View {
    let repo: RepoRef
    @Environment(AppModel.self) private var app
    @State private var info: RepoInfoResponse.R?
    @State private var tab: RepoTab = .code
    @State private var stateFilter: StateFilter = .open
    @State private var model = SearchListModel()
    @State private var code: RepoCodeModel?

    private var query: String {
        ["repo:\(repo.full)", "is:\(tab == .issue ? "issue" : "pr")", stateFilter.query, "sort:updated-desc"].filter { !$0.isEmpty }.joined(separator: " ")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            Divider()
            switch tab {
            case .code:
                if let code {
                    RepoCodeView(model: code, defaultBranch: info?.defaultBranchRef?.name ?? (info == nil ? nil : "HEAD"))
                } else {
                    LoadingView()
                }
            case .pr, .issue:
                itemsList
            }
        }
        .navigationTitle(repo.name)
        .navigationSubtitle(repo.owner)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button { copyToPasteboard("gh repo clone \(repo.full)") } label: { Label(t("Копировать clone", "Copy clone"), systemImage: "terminal") }
                    .help(t("Копировать «gh repo clone»", "Copy “gh repo clone”"))
                Button { openInBrowser(code?.webURL ?? "https://github.com/\(repo.full)") } label: { Label(t("В браузере", "Open in browser"), systemImage: "safari") }
            }
        }
        .task(id: repo) {
            if code == nil, let api = app.api { code = RepoCodeModel(repo: repo, api: api) }
            info = try? await app.api?.repoInfo(repo)
            if info == nil { info = nil }
        }
        .task(id: query) {
            if tab != .code { await model.load(api: app.api, query: query) }
        }
        .onChange(of: app.refreshTick) {
            Task {
                if tab != .code { await model.load(api: app.api, query: query) }
                else if let code { await code.openDir(code.dir) }
            }
        }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 8) {
                Image(systemName: info?.isPrivate == true ? "lock" : "book.closed").foregroundStyle(.secondary)
                Text(repo.full).font(.title2.weight(.semibold)).textSelection(.enabled)
                if let info {
                    HStack(spacing: 10) {
                        Label("\(info.stargazerCount)", systemImage: "star")
                        Label("\(info.forkCount)", systemImage: "tuningfork")
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .padding(.leading, 6)
                }
            }
            if let d = info?.description, !d.isEmpty { Text(d).foregroundStyle(.secondary).lineLimit(2) }
            HStack {
                TabStrip(items: [
                    TabItem(id: RepoTab.code, title: t("Код", "Code"), icon: "chevron.left.forwardslash.chevron.right"),
                    TabItem(id: RepoTab.pr, title: "Pull requests", icon: "arrow.triangle.pull", badge: info.map { "\($0.pullRequests.totalCount)" }),
                    TabItem(id: RepoTab.issue, title: "Issues", icon: "smallcircle.filled.circle", badge: info.map { "\($0.issues.totalCount)" })
                ], selection: $tab)
                .padding(.leading, -16)
                if tab != .code {
                    Picker("", selection: $stateFilter) {
                        ForEach(StateFilter.allCases) { Text($0.title).tag($0) }
                    }
                    .labelsHidden()
                    .frame(width: 130)
                }
            }
            .padding(.top, 2)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private var itemsList: some View {
        List {
            if let e = model.error { Text(e).foregroundStyle(.red) }
            ForEach(model.items) { item in
                NavigationLink(value: item.route) { ItemRow(item: item) }
                    .contextMenu { ItemContextMenu(item: item) }
                    .onAppear { if item.id == model.items.last?.id { Task { await model.loadMore(api: app.api) } } }
            }
            if model.items.isEmpty, !model.loading { Text(t("Ничего нет", "Nothing here")).foregroundStyle(.secondary) }
        }
        .overlay { if model.loading && model.items.isEmpty { ProgressView().controlSize(.small) } }
    }
}
