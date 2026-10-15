import SwiftUI

struct ProjectsListView: View {
    @Environment(AppModel.self) private var app
    @State private var projects: [ProjectSummary] = []
    @State private var loading = false
    @State private var error: String?
    @State private var showClosed = false
    @State private var lastUpdated: Date?
    @State private var rows = RowAppearTracker.shared("projects")

    var body: some View {
        List(selection: Binding(get: { app.detailRoot }, set: { app.select($0) })) {
            if !app.hasProjectScope || (error?.contains("scope") ?? false) {
                ScopeHint()
            } else if let error {
                Text(error).foregroundStyle(Color.syRed).font(.caption)
            }
            ForEach(projects.filter { showClosed || !$0.closed }) { p in
                HStack(alignment: .top, spacing: 9) {
                    IconTile(systemImage: "rectangle.split.3x1.fill", color: p.closed ? .sySlate : .syIndigo, size: 22)
                        .padding(.top, 1)
                    VStack(alignment: .leading, spacing: 3) {
                        HStack(spacing: 5) {
                            Text(p.title).fontWeight(.semibold).foregroundStyle(Color.syText).lineLimit(1)
                            if p.closed { ChipView(text: "closed", color: .sySlate) }
                        }
                        if let d = p.shortDescription, !d.isEmpty { Text(d).font(.caption).foregroundStyle(Color.syMuted).lineLimit(2) }
                        HStack(spacing: 6) {
                            Text("\(p.owner?.login ?? "") · #\(p.number)").foregroundStyle(Color.syMuted)
                            Text("·").foregroundStyle(Color.syDim)
                            Text(plural(p.items.totalCount, "item", "items")).foregroundStyle(Color.syIndigo)
                            Spacer()
                            Text(Fmt.rel(p.updatedAt)).foregroundStyle(Color.syDim)
                        }
                        .font(.caption)
                    }
                }
                .padding(.vertical, 4)
                .rowAppear(p.id, rows)
                .tag(Route.project(p.ref))
                .contextMenu { Button("Open in Browser") { openInBrowser(p.url) } }
            }
            if projects.isEmpty, !loading, error == nil {
                Text("No projects").foregroundStyle(Color.syMuted)
            }
        }
        .overlay { if loading && projects.isEmpty { ProgressView().controlSize(.small) } }
        .liveRefresh(updated: lastUpdated, refreshing: loading, error: projects.isEmpty ? nil : error) { await load() }
        .navigationTitle("Projects")
        .toolbar {
            ToolbarItem {
                Toggle(isOn: $showClosed) { Label("Closed", systemImage: "archivebox") }.help("Show closed")
            }
        }
        .task { await load(first: true) }
        .onChange(of: app.refreshTick) { Task { await load() } }
        .onChange(of: app.liveTick) { Task { await load() } }
        .onChange(of: showClosed) { rows.ingest(projects.filter { showClosed || !$0.closed }.map(\.id), mode: .cascade) }
    }

    private func load(first: Bool = false) async {
        guard let api = app.api else { return }
        loading = true
        defer { loading = false }
        do {
            let fetched = try await api.projects()
            rows.ingest(fetched.filter { showClosed || !$0.closed }.map(\.id), mode: first ? .cascade : .refresh)
            projects = fetched
            lastUpdated = Date()
            error = nil
        } catch {
            if isCancellation(error) { return }
            self.error = error.localizedDescription
        }
    }
}

struct ScopeHint: View {
    @Environment(AppModel.self) private var app
    @State private var busy = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                IconTile(systemImage: "key.fill", color: .syAmber, size: 22)
                Text("Token is missing Projects scopes").font(.callout.weight(.semibold)).foregroundStyle(Color.syText)
            }
            Text("Run this in Terminal, then click “Refresh Token”:").font(.caption).foregroundStyle(Color.syMuted)
            Text("gh auth refresh -s read:project,project")
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(Color.syText)
                .textSelection(.enabled)
                .padding(.horizontal, 8)
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .card(radius: 6, fill: .sySunken)
            HStack {
                Button("Copy Command") { copyToPasteboard("gh auth refresh -s read:project,project") }
                Button("Refresh Token from gh") {
                    busy = true
                    Task {
                        if let t = try? await GHCLI.token() { try? await app.login(token: t) }
                        busy = false
                        app.refresh()
                    }
                }
                .disabled(busy)
            }
            .controlSize(.small)
        }
        .padding(10)
        .card(radius: 10, fill: Color.syAmber.opacity(0.08))
        .padding(.vertical, 4)
    }
}

@Observable
@MainActor
final class ProjectBoardModel {
    let ref: ProjectRef
    let api: GitHubAPI
    var detail: ProjectDetail?
    var items: [ProjectItem] = []
    var fieldId: String?
    var loading = false
    var lastUpdated: Date?
    var error: String?
    var actionError: String?
    let rows: RowAppearTracker
    @ObservationIgnored private var loaded = false

    init(ref: ProjectRef, api: GitHubAPI) {
        self.ref = ref
        self.api = api
        rows = .shared("project|\(ref.id)")
    }

    var groupFields: [ProjectField] {
        (detail?.fields.nodes.compactMap(\.field) ?? []).filter { $0.options != nil }
    }

    var field: ProjectField? {
        groupFields.first { $0.id == fieldId }
    }

    func load() async {
        loading = true
        defer { loading = false }
        do {
            let (d, items) = try await api.project(ref.id)
            detail = d
            if fieldId == nil || !groupFields.contains(where: { $0.id == fieldId }) {
                fieldId = (groupFields.first { $0.name.lowercased() == "status" } ?? groupFields.first)?.id
            }
            let visible = items.filter { !$0.isArchived }
            rows.ingest(cascadeOrder(visible), mode: loaded ? .refresh : .cascade)
            loaded = true
            self.items = visible
            lastUpdated = Date()
            error = nil
        } catch {
            if isCancellation(error) { return }
            self.error = error.localizedDescription
        }
    }

    private func cascadeOrder(_ items: [ProjectItem]) -> [String] {
        guard let fieldId else { return items.map(\.id) }
        var groups: [String: [String]] = [:]
        var order: [String] = []
        for item in items {
            let key = item.optionId(for: fieldId) ?? ""
            if groups[key] == nil { order.append(key) }
            groups[key, default: []].append(item.id)
        }
        var out: [String] = []
        var depth = 0
        while out.count < items.count {
            for key in order where depth < groups[key]!.count { out.append(groups[key]![depth]) }
            depth += 1
        }
        return out
    }

    func move(_ itemId: String, to optionId: String?) async {
        guard let fieldId, let i = items.firstIndex(where: { $0.id == itemId }) else { return }
        guard items[i].optionId(for: fieldId) != optionId else { return }
        let backup = items[i]
        var nodes = items[i].fieldValues.nodes.filter { $0.field?.id != fieldId }
        if let optionId {
            nodes.append(ProjectFieldValue(typename: "ProjectV2ItemFieldSingleSelectValue", optionId: optionId, field: FieldRef(id: fieldId)))
        }
        items[i].fieldValues = Nodes(nodes: nodes)
        do {
            try await api.moveProjectItem(project: ref.id, item: itemId, field: fieldId, option: optionId)
        } catch {
            if let j = items.firstIndex(where: { $0.id == itemId }) { items[j] = backup }
            actionError = error.localizedDescription
        }
    }
}

struct ProjectBoardView: View {
    let project: ProjectRef
    @Environment(AppModel.self) private var app
    @State private var model: ProjectBoardModel?
    @State private var filter = ""

    var body: some View {
        Group {
            if let model {
                if model.detail != nil {
                    board(model)
                } else if let e = model.error {
                    ErrorStateView(message: e) { Task { await model.load() } }
                } else {
                    LoadingView()
                }
            } else {
                LoadingView()
            }
        }
        .navigationTitle(project.title)
        .task(id: project) {
            guard let api = app.api else { return }
            let m = ProjectBoardModel(ref: project, api: api)
            model = m
            await m.load()
        }
        .onChange(of: app.refreshTick) { Task { await model?.load() } }
        .onChange(of: app.liveTick) { Task { await model?.load() } }
    }

    private func board(_ model: ProjectBoardModel) -> some View {
        @Bindable var model = model
        let columns = columnsFor(model)
        return VStack(spacing: 0) {
            HStack {
                if model.groupFields.count > 1 {
                    Picker("Group by", selection: $model.fieldId) {
                        ForEach(model.groupFields) { Text($0.name).tag(Optional($0.id)) }
                    }
                    .foregroundStyle(Color.syMuted)
                    .frame(maxWidth: 260)
                }
                TextField("Filter cards", text: $filter).textFieldStyle(.roundedBorder).frame(maxWidth: 240)
                Spacer()
                ChipView(text: plural(model.items.count, "item", "items"), color: .syIndigo, icon: "square.stack")
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            Rectangle().fill(Color.syLine).frame(height: 1)
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: 12) {
                    ForEach(columns, id: \.id) { col in
                        ProjectColumn(model: model, column: col, items: col.items, options: model.field?.options ?? [])
                    }
                }
                .padding(14)
                .frame(maxHeight: .infinity, alignment: .top)
            }
            RefreshFooter(updated: model.lastUpdated, refreshing: model.loading, error: model.error) {
                Task { await model.load() }
            }
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button { openInBrowser(project.url) } label: { Label("Open in Browser", systemImage: "safari") }
            }
        }
        .alert("Error", isPresented: Binding(get: { model.actionError != nil }, set: { if !$0 { model.actionError = nil } })) {
            Button("OK") {}
        } message: {
            Text(model.actionError ?? "")
        }
    }

    struct Column {
        let id: String
        let optionId: String?
        let name: String
        let color: String
        let items: [ProjectItem]
    }

    private func columnsFor(_ model: ProjectBoardModel) -> [Column] {
        let items = model.items.filter { filter.isEmpty || ($0.content?.title ?? "").localizedCaseInsensitiveContains(filter) || ($0.content?.repository?.nameWithOwner ?? "").localizedCaseInsensitiveContains(filter) }
        guard let field = model.field, let fieldId = model.fieldId, let options = field.options else {
            return [Column(id: "all", optionId: nil, name: "All", color: "GRAY", items: items)]
        }
        var cols: [Column] = []
        let known = Set(options.map(\.id))
        let none = items.filter { $0.optionId(for: fieldId).map { !known.contains($0) } ?? true }
        if !none.isEmpty { cols.append(Column(id: "none", optionId: nil, name: "No \(field.name)", color: "GRAY", items: none)) }
        for o in options {
            cols.append(Column(id: o.id, optionId: o.id, name: o.name, color: o.color, items: items.filter { $0.optionId(for: fieldId) == o.id }))
        }
        return cols
    }
}

func projectColor(_ name: String) -> Color {
    switch name {
    case "BLUE": return .sySky
    case "GREEN": return .syGreen
    case "YELLOW": return .syAmber
    case "ORANGE": return .syCoral
    case "RED": return .syRed
    case "PINK": return .syCoral
    case "PURPLE": return .syViolet
    default: return .sySlate
    }
}

struct ProjectColumn: View {
    let model: ProjectBoardModel
    let column: ProjectBoardView.Column
    let items: [ProjectItem]
    let options: [ProjectOption]
    @State private var targeted = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Circle().stroke(projectColor(column.color), lineWidth: 2).frame(width: 10, height: 10)
                Text(column.name).fontWeight(.semibold).foregroundStyle(Color.syText).lineLimit(1)
                Text("\(items.count)").font(.caption.weight(.medium)).foregroundStyle(projectColor(column.color))
                    .padding(.horizontal, 6).background(Capsule().fill(projectColor(column.color).opacity(0.14)))
                Spacer()
            }
            .padding(10)
            .overlay(alignment: .top) {
                Rectangle().fill(projectColor(column.color)).frame(height: 3)
            }
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(items) { item in
                        ProjectCard(item: item)
                            .rowAppear(item.id, model.rows, radius: 8)
                            .draggable(item.id)
                            .contextMenu {
                                Menu("Move to") {
                                    ForEach(options) { o in
                                        Button(o.name) { Task { await model.move(item.id, to: o.id) } }
                                    }
                                    Divider()
                                    Button("No Status") { Task { await model.move(item.id, to: nil) } }
                                }
                                if let url = item.content?.url {
                                    Button("Open in Browser") { openInBrowser(url) }
                                    Button("Copy Link") { copyToPasteboard(url) }
                                }
                            }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 8)
            }
            .liveRefresh(updated: model.lastUpdated, refreshing: model.loading, footer: false) { await model.load() }
        }
        .frame(width: 290)
        .frame(maxHeight: .infinity, alignment: .top)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .card(radius: 12, fill: targeted ? Color.syAccent.opacity(0.10) : Color.sySunken)
        .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(targeted ? Color.syAccent : .clear, lineWidth: 2))
        .dropDestination(for: String.self) { ids, _ in
            guard model.fieldId != nil else { return false }
            for id in ids { Task { await model.move(id, to: column.optionId) } }
            return true
        } isTargeted: { targeted = $0 }
    }
}

struct ProjectCard: View {
    let item: ProjectItem

    var body: some View {
        let c = item.content
        let content = VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 5) {
                if let s = c?.visualState {
                    Image(systemName: s.icon).foregroundStyle(s.color).font(.caption)
                } else {
                    Image(systemName: "doc.text").foregroundStyle(Color.syDim).font(.caption)
                }
                Text(c?.repository.map { "\($0.nameWithOwner)\(c?.number.map { "#\($0)" } ?? "")" } ?? "Draft")
                    .font(.caption).foregroundStyle(Color.syMuted).lineLimit(1)
                Spacer()
                ForEach(c?.assignees?.nodes ?? [], id: \.login) { Avatar(url: $0.avatarUrl, size: 16).help($0.login) }
            }
            Text(c?.title ?? "—").font(.callout.weight(.medium)).foregroundStyle(Color.syText).lineLimit(3).multilineTextAlignment(.leading)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .card(radius: 8)
        .contentShape(Rectangle())

        if let route = c?.route {
            NavigationLink(value: route) { content }.buttonStyle(.plain)
        } else {
            content
        }
    }
}
