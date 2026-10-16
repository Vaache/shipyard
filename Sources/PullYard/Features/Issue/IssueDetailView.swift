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
    var lastUpdated: Date?
    var refreshError: String?
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
                error = "Issue \(ref.title) not found"
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
            lastUpdated = Date()
            error = nil
            refreshError = nil
        } catch {
            if isCancellation(error) { return }
            if issue == nil { self.error = error.localizedDescription } else { refreshError = error.localizedDescription }
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
        .onLiveTick { await model?.load() }
    }

    private func content(_ model: IssueDetailModel, _ issue: IssueDetail) -> some View {
        VStack(spacing: 0) {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(issue.title).font(.title2.weight(.bold)).foregroundStyle(Color.syText).textSelection(.enabled)
                    Text("#\(issue.number)").font(.title2).foregroundStyle(Color.syDim)
                    ShortLinkButton(ref: ShortLink.ref(url: issue.url) ?? ref)
                }
                HStack(spacing: 8) {
                    StateBadge(state: issue.visualState)
                    Avatar(url: issue.author?.avatarUrl, size: 18)
                    Text(issue.author?.login ?? "ghost").fontWeight(.semibold).foregroundStyle(Color.syText)
                    ChipView(text: ref.repo.full, color: .syTeal, icon: "book.closed.fill")
                    ChipView(text: plural(issue.comments.totalCount, "comment", "comments"), color: .sySky, icon: "bubble.left.fill")
                    Spacer(minLength: 0)
                    Text(Fmt.rel(issue.createdAt)).foregroundStyle(Color.syMuted).help(Fmt.full.string(from: issue.createdAt))
                }
                .font(.callout)
                .lineLimit(1)
                if !issue.labels.nodes.isEmpty || !issue.assignees.nodes.isEmpty {
                    HStack(spacing: 6) {
                        ForEach(issue.labels.nodes, id: \.name) { LabelChip(label: $0) }
                        if !issue.assignees.nodes.isEmpty {
                            Text("Assignees:").font(.caption).foregroundStyle(Color.syMuted).padding(.leading, issue.labels.nodes.isEmpty ? 0 : 6)
                            ForEach(issue.assignees.nodes, id: \.login) { Avatar(url: $0.avatarUrl, size: 18).help($0.login) }
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(16)
            Rectangle().fill(Color.syLine).frame(height: 1)
            HTMLView(html: model.html, scrollToBottomToken: model.scrollToBottomToken, onLink: { url in
                if let route = GitHubURL.parse(url.absoluteString, allowShort: false) { app.open(route, push: true) } else { LinkBrowser.open(url) }
            })
            .liveRefresh(updated: model.lastUpdated, refreshing: model.loading, footer: false, web: true) { await model.load() }
            Rectangle().fill(Color.syLine).frame(height: 1)
            HStack(alignment: .bottom, spacing: 8) {
                ComposerBox(text: $text, placeholder: "Comment (Markdown) — ⌘↩ to send", minHeight: 56).frame(height: 64)
                Button("Comment") {
                    let body = text
                    Task { if await model.comment(body) { text = "" } }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .buttonStyle(.borderedProminent)
                .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy)
            }
            .padding(10)
            .background(Theme.isBrand ? Color.syBar : Color.clear)
        }
        .navigationTitle(issue.title)
        .navigationSubtitle("\(ref.repo.full) #\(issue.number)")
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                if model.busy { ProgressView().controlSize(.small) }
                RefreshStatus(updated: model.lastUpdated, refreshing: model.loading, error: model.refreshError, compact: true) {
                    Task { await model.load() }
                }
                if issue.viewerCanUpdate {
                    if issue.issueState == "OPEN" {
                        Button { Task { await model.setOpen(false) } } label: { Label("Close", systemImage: "checkmark.circle") }
                            .help("Close issue")
                    } else {
                        Button { Task { await model.setOpen(true) } } label: { Label("Reopen", systemImage: "arrow.uturn.backward.circle") }
                            .help("Reopen issue")
                    }
                }
                Button { copyToPasteboard(issue.url) } label: { Label("Copy Link", systemImage: "link") }
                Button { openInBrowser(issue.url) } label: { Label("Open in Browser", systemImage: "safari") }
            }
        }
        .alert("Error", isPresented: Binding(get: { model.actionError != nil }, set: { if !$0 { model.actionError = nil } })) {
            Button("OK") {}
        } message: {
            Text(model.actionError ?? "")
        }
    }
}
