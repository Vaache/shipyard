import SwiftUI

struct ProjectsListView: View {
    @Environment(AppModel.self) private var app
    @State private var projects: [ProjectSummary] = []
    @State private var loading = false
    @State private var error: String?
    @State private var showClosed = false

    var body: some View {
        List(selection: Binding(get: { app.detailRoot }, set: { app.select($0) })) {
            if !app.hasProjectScope || (error?.contains("scope") ?? false) || (error?.contains("прав") ?? false) {
                ScopeHint()
            } else if let error {
                Text(error).foregroundStyle(.red).font(.caption)
            }
            ForEach(projects.filter { showClosed || !$0.closed }) { p in
                VStack(alignment: .leading, spacing: 3) {
                    HStack(spacing: 5) {
                        Image(systemName: "rectangle.split.3x1").foregroundStyle(.secondary).font(.caption)
                        Text(p.title).fontWeight(.medium).lineLimit(1)
                        if p.closed { Text(t("закрыт", "closed")).font(.caption2).foregroundStyle(.secondary) }
                    }
                    if let d = p.shortDescription, !d.isEmpty { Text(d).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                    HStack(spacing: 6) {
                        Text("\(p.owner?.login ?? "") · #\(p.number)")
                        Text("·")
                        Text(tp(p.items.totalCount, "элемент", "элемента", "элементов", "item", "items"))
                        Spacer()
                        Text(Fmt.rel(p.updatedAt))
                    }
                    .font(.caption)
                    .foregroundStyle(.secondary)
                }
                .padding(.vertical, 3)
                .tag(Route.project(p.ref))
                .contextMenu { Button(t("Открыть в браузере", "Open in Browser")) { openInBrowser(p.url) } }
            }
            if projects.isEmpty, !loading, error == nil {
                Text(t("Проектов нет", "No projects")).foregroundStyle(.secondary)
            }
        }
        .overlay { if loading && projects.isEmpty { ProgressView().controlSize(.small) } }
        .navigationTitle(t("Проекты", "Projects"))
        .toolbar {
            ToolbarItem {
                Toggle(isOn: $showClosed) { Label(t("Закрытые", "Closed"), systemImage: "archivebox") }.help(t("Показывать закрытые", "Show closed"))
            }
        }
        .task { await load() }
        .onChange(of: app.refreshTick) { Task { await load() } }
    }

    private func load() async {
        guard let api = app.api else { return }
        loading = true
        defer { loading = false }
        do {
            projects = try await api.projects()
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
        VStack(alignment: .leading, spacing: 6) {
            Label(t("Токену не хватает прав на Projects", "Token is missing Projects scopes"), systemImage: "key").font(.callout.weight(.semibold))
            Text(t("Выполните в терминале и затем нажмите «Обновить токен»:", "Run this in Terminal, then click “Refresh Token”:")).font(.caption).foregroundStyle(.secondary)
            Text("gh auth refresh -s read:project,project")
                .font(.system(.caption, design: .monospaced))
                .textSelection(.enabled)
                .padding(6)
                .background(RoundedRectangle(cornerRadius: 5).fill(.quaternary))
            HStack {
                Button(t("Копировать команду", "Copy Command")) { copyToPasteboard("gh auth refresh -s read:project,project") }
                Button(t("Обновить токен из gh", "Refresh Token from gh")) {
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
        .padding(.vertical, 6)
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
    var error: String?
    var actionError: String?

    init(ref: ProjectRef, api: GitHubAPI) {
        self.ref = ref
        self.api = api
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
            self.items = items.filter { !$0.isArchived }
            if fieldId == nil || !groupFields.contains(where: { $0.id == fieldId }) {
                fieldId = (groupFields.first { $0.name.lowercased() == "status" } ?? groupFields.first)?.id
            }
            error = nil
        } catch {
            if isCancellation(error) { return }
            self.error = error.localizedDescription
        }
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
    }

    private func board(_ model: ProjectBoardModel) -> some View {
        @Bindable var model = model
        let columns = columnsFor(model)
        return VStack(spacing: 0) {
            HStack {
                if model.groupFields.count > 1 {
                    Picker(t("Группировать по", "Group by"), selection: $model.fieldId) {
                        ForEach(model.groupFields) { Text($0.name).tag(Optional($0.id)) }
                    }
                    .frame(maxWidth: 260)
                }
                TextField(t("Фильтр карточек", "Filter cards"), text: $filter).textFieldStyle(.roundedBorder).frame(maxWidth: 240)
                Spacer()
                Text(tp(model.items.count, "элемент", "элемента", "элементов", "item", "items")).font(.caption).foregroundStyle(.secondary)
                if model.loading { ProgressView().controlSize(.small) }
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            Divider()
            ScrollView(.horizontal) {
                HStack(alignment: .top, spacing: 12) {
                    ForEach(columns, id: \.id) { col in
                        ProjectColumn(model: model, column: col, items: col.items, options: model.field?.options ?? [])
                    }
                }
                .padding(14)
                .frame(maxHeight: .infinity, alignment: .top)
            }
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button { Task { await model.load() } } label: { Label(t("Обновить", "Refresh"), systemImage: "arrow.clockwise") }
                Button { openInBrowser(project.url) } label: { Label(t("В браузере", "Open in Browser"), systemImage: "safari") }
            }
        }
        .alert(t("Ошибка", "Error"), isPresented: Binding(get: { model.actionError != nil }, set: { if !$0 { model.actionError = nil } })) {
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
            return [Column(id: "all", optionId: nil, name: t("Все", "All"), color: "GRAY", items: items)]
        }
        var cols: [Column] = []
        let known = Set(options.map(\.id))
        let none = items.filter { $0.optionId(for: fieldId).map { !known.contains($0) } ?? true }
        if !none.isEmpty { cols.append(Column(id: "none", optionId: nil, name: t("Без \(field.name)", "No \(field.name)"), color: "GRAY", items: none)) }
        for o in options {
            cols.append(Column(id: o.id, optionId: o.id, name: o.name, color: o.color, items: items.filter { $0.optionId(for: fieldId) == o.id }))
        }
        return cols
    }
}

func projectColor(_ name: String) -> Color {
    switch name {
    case "BLUE": return .blue
    case "GREEN": return .ghGreen
    case "YELLOW": return .ghYellow
    case "ORANGE": return .orange
    case "RED": return .ghRed
    case "PINK": return .pink
    case "PURPLE": return .ghPurple
    default: return .gray
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
                Text(column.name).fontWeight(.semibold).lineLimit(1)
                Text("\(items.count)").font(.caption).foregroundStyle(.secondary)
                    .padding(.horizontal, 6).background(Capsule().fill(.quaternary))
                Spacer()
            }
            .padding(10)
            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(items) { item in
                        ProjectCard(item: item)
                            .draggable(item.id)
                            .contextMenu {
                                Menu(t("Переместить в", "Move to")) {
                                    ForEach(options) { o in
                                        Button(o.name) { Task { await model.move(item.id, to: o.id) } }
                                    }
                                    Divider()
                                    Button(t("Без статуса", "No Status")) { Task { await model.move(item.id, to: nil) } }
                                }
                                if let url = item.content?.url {
                                    Button(t("Открыть в браузере", "Open in Browser")) { openInBrowser(url) }
                                    Button(t("Копировать ссылку", "Copy Link")) { copyToPasteboard(url) }
                                }
                            }
                    }
                }
                .padding(.horizontal, 8)
                .padding(.bottom, 8)
            }
        }
        .frame(width: 290)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.secondary.opacity(targeted ? 0.16 : 0.07)))
        .overlay(RoundedRectangle(cornerRadius: 10).stroke(targeted ? Color.accentColor : .clear, lineWidth: 2))
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
                    Image(systemName: "doc.text").foregroundStyle(.secondary).font(.caption)
                }
                Text(c?.repository.map { "\($0.nameWithOwner)\(c?.number.map { "#\($0)" } ?? "")" } ?? t("Черновик", "Draft"))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
                Spacer()
                ForEach(c?.assignees?.nodes ?? [], id: \.login) { Avatar(url: $0.avatarUrl, size: 16).help($0.login) }
            }
            Text(c?.title ?? "—").font(.callout).lineLimit(3).multilineTextAlignment(.leading)
        }
        .padding(10)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color(nsColor: .controlBackgroundColor)))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color(nsColor: .separatorColor)))
        .contentShape(Rectangle())

        if let route = c?.route {
            NavigationLink(value: route) { content }.buttonStyle(.plain)
        } else {
            content
        }
    }
}
