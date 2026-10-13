import SwiftUI

@Observable
@MainActor
final class IssueDetailModel {
    let ref: ItemRef
    let api: GitHubAPI
    var issue: IssueDetail?
    var isPR = false
    var html = ""
    var loading = false
    var busy = false
    var error: String?
    var actionError: String?
    var scrollToBottomToken = 0

    init(ref: ItemRef, api: GitHubAPI) {
        self.ref = ref
        self.api = api
    }

    func load() async {
        loading = true
        defer { loading = false }
        do {
            let r = try await api.issue(ref)
            guard let node = r.repository?.issueOrPullRequest else {
                error = t("Issue \(ref.title) не найден", "Issue \(ref.title) not found")
                return
            }
            if node.typename == "PullRequest" {
                isPR = true
                return
            }
            guard let issue = node.issue else { return }
            self.issue = issue
            let h = ConversationHTML.render(issue: issue)
            if h != html { html = h }
            error = nil
        } catch {
            if isCancellation(error) { return }
            if issue == nil { self.error = error.localizedDescription } else { actionError = error.localizedDescription }
        }
    }

    private func run(_ op: () async throws -> Void) async -> Bool {
        busy = true
        defer { busy = false }
        do {
            try await op()
            await load()
            return true
        } catch {
            actionError = error.localizedDescription
            return false
        }
    }

    func comment(_ body: String) async -> Bool {
        guard let issue else { return false }
        let ok = await run { try await api.addComment(subject: issue.id, body: body) }
        if ok { scrollToBottomToken += 1 }
        return ok
    }

    func setOpen(_ open: Bool) async {
        guard let issue else { return }
        _ = await run { try await api.mutate(open ? Q.reopenIssue : Q.closeIssue, ["id": issue.id]) }
    }
}

struct IssueDetailView: View {
    let ref: ItemRef
    @Environment(AppModel.self) private var app
    @State private var model: IssueDetailModel?
    @State private var text = ""

    var body: some View {
        Group {
            if let model {
                if model.isPR {
                    PRDetailView(ref: ref)
                } else if let issue = model.issue {
                    content(model, issue)
                } else if let e = model.error {
                    ErrorStateView(message: e) { Task { await model.load() } }
                } else {
                    LoadingView()
                }
            } else {
                LoadingView()
            }
        }
        .task(id: ref) {
            guard let api = app.api else { return }
            let m = IssueDetailModel(ref: ref, api: api)
            model = m
            await m.load()
        }
        .onChange(of: app.refreshTick) { Task { await model?.load() } }
    }

    private func content(_ model: IssueDetailModel, _ issue: IssueDetail) -> some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(issue.title).font(.title2.weight(.semibold)).textSelection(.enabled)
                    Text("#\(issue.number)").font(.title2).foregroundStyle(.secondary)
                }
                HStack(spacing: 8) {
                    StateBadge(state: issue.visualState)
                    Avatar(url: issue.author?.avatarUrl, size: 18)
                    Text(issue.author?.login ?? "ghost").fontWeight(.semibold)
                    Text("· \(Fmt.rel(issue.createdAt)) · \(tp(issue.comments.totalCount, "комментарий", "комментария", "комментариев", "comment", "comments"))").foregroundStyle(.secondary)
                    Spacer()
                    ForEach(issue.assignees.nodes, id: \.login) { Avatar(url: $0.avatarUrl, size: 18).help($0.login) }
                }
                .font(.callout)
                if !issue.labels.nodes.isEmpty {
                    HStack(spacing: 6) { ForEach(issue.labels.nodes, id: \.name) { LabelChip(label: $0) } }
                }
            }
            .padding(16)
            Divider()
            HTMLView(html: model.html, scrollToBottomToken: model.scrollToBottomToken, onLink: { url in
                if let route = GitHubURL.parse(url.absoluteString, allowShort: false) { app.open(route, push: true) } else { LinkBrowser.open(url) }
            })
            Divider()
            HStack(alignment: .bottom, spacing: 8) {
                ComposerBox(text: $text, placeholder: t("Комментарий (Markdown) — ⌘↩ отправить", "Comment (Markdown) — ⌘↩ to send"), minHeight: 56).frame(height: 64)
                Button(t("Комментировать", "Comment")) {
                    let body = text
                    Task { if await model.comment(body) { text = "" } }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .buttonStyle(.borderedProminent)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy)
            }
            .padding(10)
        }
        .navigationTitle(issue.title)
        .navigationSubtitle("\(ref.repo.full) #\(issue.number)")
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                if model.busy || model.loading { ProgressView().controlSize(.small) }
                if issue.viewerCanUpdate {
                    if issue.issueState == "OPEN" {
                        Button { Task { await model.setOpen(false) } } label: { Label(t("Закрыть", "Close"), systemImage: "checkmark.circle") }
                            .help(t("Закрыть issue", "Close issue"))
                    } else {
                        Button { Task { await model.setOpen(true) } } label: { Label(t("Переоткрыть", "Reopen"), systemImage: "arrow.uturn.backward.circle") }
                            .help(t("Переоткрыть issue", "Reopen issue"))
                    }
                }
                Button { copyToPasteboard(issue.url) } label: { Label(t("Копировать ссылку", "Copy Link"), systemImage: "link") }
                Button { Task { await model.load() } } label: { Label(t("Обновить", "Refresh"), systemImage: "arrow.clockwise") }
                Button { openInBrowser(issue.url) } label: { Label(t("В браузере", "Open in Browser"), systemImage: "safari") }
            }
        }
        .alert(t("Ошибка", "Error"), isPresented: Binding(get: { model.actionError != nil }, set: { if !$0 { model.actionError = nil } })) {
            Button("OK") {}
        } message: {
            Text(model.actionError ?? "")
        }
    }
}
