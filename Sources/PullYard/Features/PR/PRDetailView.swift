import SwiftUI

struct PRDetailView: View {
    let ref: ItemRef
    @Environment(AppModel.self) private var app
    @State private var model: PRDetailModel?

    var body: some View {
        Group {
            if let model {
                if model.notPR {
                    IssueDetailView(ref: ref)
                } else if let pr = model.pr {
                    PRDetailContent(model: model, pr: pr)
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
            let m = app.prModel(ref)
            model = m
            await m?.load(live: m?.pr != nil)
        }
        .onChange(of: app.refreshTick) {
            Task { await model?.load(full: true) }
        }
        .onLiveTick { await model?.load(live: true) }
    }
}

enum PRConfirm: Identifiable {
    case approve, close, deleteBranch, disableAutoMerge
    var id: Int { hashValue }
}

struct PRDetailContent: View {
    @Bindable var model: PRDetailModel
    let pr: PullRequestDetail
    @Environment(AppModel.self) private var app
    @State private var showReview = false
    @State private var showMerge = false
    @State private var showEdit = false
    @State private var showReviewers = false
    @State private var confirm: PRConfirm?
    @State private var showFix = false

    var body: some View {
        VStack(spacing: 0) {
            PRHeaderView(model: model, pr: pr, onMerge: { showMerge = true })
            ReReviewBanner(model: model, pr: pr) { showReviewers = true }
            ClaudeFixBanner(pr: pr) { showFix = true }
            TabStrip(items: [
                TabItem(id: PRTab.conversation, title: "Conversation", icon: "bubble.left.and.bubble.right", badge: commentsBadge),
                TabItem(id: PRTab.commits, title: "Commits", icon: "point.3.connected.trianglepath.dotted", badge: "\(pr.allCommits.totalCount ?? pr.allCommits.nodes.count)"),
                TabItem(id: PRTab.checks, title: "Checks", icon: "checklist", badge: checksSummary, badgeColor: checksColor),
                TabItem(id: PRTab.files, title: "Files changed", icon: "doc.text", badge: "\(pr.changedFiles)")
            ], selection: $model.tab, shortcuts: true)
            .padding(.top, 6)
            Divider()
            ZStack {
                ForEach(PRTab.allCases) { tab in
                    if model.visitedTabs.contains(tab) {
                        tabContent(tab)
                            .opacity(model.tab == tab ? 1 : 0)
                            .offset(y: model.tab == tab || Motion.reduced ? 0 : 4)
                            .animation(Motion.fade, value: model.tab)
                            .allowsHitTesting(model.tab == tab)
                            .disabled(model.tab != tab)
                            .accessibilityHidden(model.tab != tab)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        .navigationTitle(pr.title)
        .navigationSubtitle("\(model.ref.repo.full) #\(pr.number)")
        .task(id: pr.url) { app.noteCanonical(model.ref, url: pr.url) }
        .toolbar { toolbar }
        .sheet(isPresented: $showReview) { ReviewSheet(model: model, pr: pr) }
        .sheet(isPresented: $showMerge) { MergeSheet(model: model, pr: pr) }
        .sheet(isPresented: $showEdit) { EditPRSheet(model: model, pr: pr) }
        .sheet(isPresented: $showReviewers) { ReviewersSheet(model: model) }
        .sheet(isPresented: $showFix) { FixWithClaudeSheet(model: model, pr: pr) }
        .sheet(isPresented: $model.showResolveConflicts) { ResolveConflictsSheet(model: model, pr: pr) }
        .sheet(item: $model.replyThread) { ReplySheet(model: model, thread: $0) }
        .alert("Error", isPresented: Binding(get: { model.actionError != nil }, set: { if !$0 { model.actionError = nil } })) {
            Button("OK") {}
        } message: {
            Text(model.actionError ?? "")
        }
        .confirmationDialog(confirmTitle, isPresented: Binding(get: { confirm != nil }, set: { if !$0 { confirm = nil } }), presenting: confirm) { c in
            Button(confirmButton(c), role: c == .approve ? nil : .destructive) { perform(c) }
            Button("Cancel", role: .cancel) {}
        }
    }

    @ViewBuilder
    private func tabContent(_ tab: PRTab) -> some View {
        switch tab {
        case .conversation: ConversationTab(model: model, pr: pr)
        case .commits: CommitsTab(model: model, pr: pr)
        case .checks: ChecksTab(model: model, pr: pr)
        case .files: FilesTab(model: model)
        }
    }

    private var commentsBadge: String? {
        let unresolved = pr.reviewThreads.nodes.filter { !$0.isResolved && !$0.isOutdated }.count
        return unresolved > 0 ? "\(unresolved)" : nil
    }

    private var checksColor: Color {
        let checks = pr.checks
        if checks.contains(where: { $0.outcome == .failure }) { return .ghRed }
        if checks.contains(where: { $0.outcome == .pending }) { return .ghYellow }
        return checks.isEmpty ? .secondary : .ghGreen
    }

    private var checksSummary: String {
        let checks = pr.checks
        if checks.isEmpty { return "0" }
        let failed = checks.filter { $0.outcome == .failure }.count
        let pending = checks.filter { $0.outcome == .pending }.count
        if failed > 0 { return "✕\(failed)" }
        if pending > 0 { return "…\(pending)" }
        return "✓\(checks.count)"
    }

    private var confirmTitle: String {
        switch confirm {
        case .approve: return "Approve this pull request?"
        case .close: return "Close pull request without merging?"
        case .deleteBranch: return "Delete branch \(pr.headRefName)?"
        case .disableAutoMerge: return "Disable auto-merge?"
        case nil: return ""
        }
    }

    private func confirmButton(_ c: PRConfirm) -> String {
        switch c {
        case .approve: return "Approve"
        case .close: return "Close pull request"
        case .deleteBranch: return "Delete branch"
        case .disableAutoMerge: return "Disable"
        }
    }

    private func perform(_ c: PRConfirm) {
        Task {
            switch c {
            case .approve: _ = await model.submitReview(event: "APPROVE", body: "")
            case .close: await model.simple(Q.closePR)
            case .deleteBranch: await model.deleteBranch()
            case .disableAutoMerge: await model.simple(Q.disableAutoMerge)
            }
        }
    }

    private var canMerge: Bool {
        pr.isOpen && !pr.isDraft && model.canWrite
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            if model.busy {
                ProgressView().controlSize(.small)
            }
            RefreshStatus(updated: model.lastUpdated, refreshing: model.loading, error: model.refreshError, compact: true) {
                Task { await model.load(full: true) }
            }
            if pr.isOpen {
                Menu {
                    Button("Write review…") { showReview = true }
                    if !pr.viewerDidAuthor {
                        Button("Approve") { confirm = .approve }
                    }
                } label: {
                    Label(model.pending.isEmpty ? "Review" : "Review (\(model.pending.count))", systemImage: "checkmark.bubble")
                } primaryAction: {
                    showReview = true
                }
                .help("Submit review")
            }
            if pr.isOpen {
                Button { showReviewers = true } label: { Label("Request review", systemImage: "person.crop.circle.badge.plus") }
                    .help("Request or re-request review")
            }
            Button { showFix = true } label: { Label("Fix with Claude", systemImage: "sparkles") }
                .help("Send review comments to a new Claude Code session")
            if canMerge {
                Button { showMerge = true } label: { Label("Merge", systemImage: "arrow.triangle.merge") }
                    .help("Merge pull request")
            }
            Menu {
                moreMenu
            } label: {
                Label("More", systemImage: "ellipsis.circle")
            }
            Button { openInBrowser(pr.url) } label: { Label("Open in browser", systemImage: "safari") }
                .help("Open on github.com")
        }
    }

    @ViewBuilder
    private var moreMenu: some View {
        Button("Copy link") { copyToPasteboard(pr.url) }
        Button("Copy short link") { app.copyShortLink(ShortLink.ref(url: pr.url) ?? model.ref) }
        Button("Copy “gh pr checkout \(pr.number)”") { copyToPasteboard("gh pr checkout \(pr.number) -R \(model.ref.repo.full)") }
        Button("Copy branch name") { copyToPasteboard(pr.headRefName) }
        Divider()
        if pr.viewerCanUpdate {
            Button("Edit title and description…") { showEdit = true }
        }
        if pr.isOpen {
            Button("Request review…") { showReviewers = true }
            if pr.viewerCanUpdate {
                if pr.isDraft {
                    Button("Ready for review") { Task { await model.simple(Q.readyForReview) } }
                } else {
                    Button("Convert to draft") { Task { await model.simple(Q.convertToDraft) } }
                }
            }
            if MergeConflicts.applies(pr) {
                Button("Resolve conflicts with Claude Code…") { model.showResolveConflicts = true }
            }
            if pr.mergeStateStatus == "BEHIND" || pr.mergeStateStatus == "BLOCKED" || pr.mergeStateStatus == "UNSTABLE" {
                Button("Update branch from \(pr.baseRefName)") { Task { await model.simple(Q.updateBranch) } }
            }
            if pr.autoMergeRequest != nil {
                Button("Disable auto-merge") { confirm = .disableAutoMerge }
            }
            Divider()
            if pr.viewerCanUpdate {
                Button("Close pull request…") { confirm = .close }
            }
        } else if !pr.merged, pr.viewerCanUpdate {
            Button("Reopen pull request") { Task { await model.simple(Q.reopenPR) } }
        }
        if !pr.isOpen, pr.headRef != nil, model.canWrite {
            Button("Delete branch \(pr.headRefName)…") { confirm = .deleteBranch }
        }
    }
}

struct PRHeaderView: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    let onMerge: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(pr.title).font(.title2.weight(.bold)).textSelection(.enabled)
                Text("#\(pr.number)").font(.title2).foregroundStyle(Color.syDim)
                ShortLinkButton(ref: ShortLink.ref(url: pr.url) ?? model.ref)
            }
            HStack(spacing: 8) {
                StateBadge(state: pr.visualState)
                Avatar(url: pr.author?.avatarUrl, size: 18)
                Text(pr.author?.login ?? "ghost").fontWeight(.semibold)
                BranchChip(name: pr.baseRefName)
                Image(systemName: "arrow.left").font(.caption).foregroundStyle(Color.syDim)
                BranchChip(name: pr.headRefName, color: .syViolet)
                DiffStat(additions: pr.additions, deletions: pr.deletions)
                Spacer(minLength: 0)
                Text(Fmt.rel(pr.createdAt)).foregroundStyle(Color.syMuted).help(Fmt.full.string(from: pr.createdAt))
            }
            .font(.callout)
            .lineLimit(1)
            if !pr.labels.nodes.isEmpty || !model.reviewers.isEmpty || !pr.assignees.nodes.isEmpty {
                HStack(spacing: 6) {
                    ForEach(pr.labels.nodes, id: \.name) { LabelChip(label: $0) }
                    if !model.reviewers.isEmpty {
                        Text("Reviewers:").font(.caption).foregroundStyle(Color.syMuted).padding(.leading, pr.labels.nodes.isEmpty ? 0 : 6)
                        ForEach(model.reviewers, id: \.login) { r in
                            ReviewerBadge(login: r.login, avatar: r.avatar, state: r.state)
                        }
                    }
                    if !pr.assignees.nodes.isEmpty {
                        Text("Assignees:").font(.caption).foregroundStyle(Color.syMuted).padding(.leading, 6)
                        ForEach(pr.assignees.nodes, id: \.login) { a in
                            Avatar(url: a.avatarUrl, size: 18).help(a.login)
                        }
                    }
                }
            }
            if model.stack.count > 1 {
                StackStrip(stack: model.stack, current: pr.number, repo: model.ref.repo)
                    .transition(.opacity)
            }
            MergeStatusBar(model: model, pr: pr, onMerge: onMerge)
        }
        .animation(Motion.smooth, value: model.stack)
        .padding(.horizontal, 16)
        .padding(.top, 12)
    }
}

struct StackStrip: View {
    let stack: [StackPR]
    let current: Int
    let repo: RepoRef
    @Environment(AppModel.self) private var app
    @State private var expanded = false

    var body: some View {
        let index = stack.firstIndex { $0.number == current } ?? 0
        VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(Motion.quick) { expanded.toggle() }
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "square.stack.3d.up.fill").foregroundStyle(Color.syIndigo)
                    Text("Stack").fontWeight(.semibold)
                    Text("\(index + 1) of \(stack.count)").foregroundStyle(Color.syMuted)
                    HStack(spacing: 3) {
                        ForEach(stack) { s in
                            Capsule()
                                .fill(s.number == current ? Color.syIndigo : s.visualState.color.opacity(0.55))
                                .frame(width: s.number == current ? 18 : 10, height: 5)
                        }
                    }
                    Spacer(minLength: 0)
                    if index > 0 { navButton(stack[index - 1], icon: "chevron.down", help: "Down the stack") }
                    if index < stack.count - 1 { navButton(stack[index + 1], icon: "chevron.up", help: "Up the stack") }
                    Image(systemName: "chevron.right")
                        .font(.caption2)
                        .foregroundStyle(Color.syDim)
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                }
                .font(.callout)
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if expanded {
                Rectangle().fill(Color.syLine).frame(height: 1)
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(stack.enumerated().reversed()), id: \.element.id) { i, s in
                        Button {
                            if s.number != current { app.open(.pr(ItemRef(repo: repo, number: s.number))) }
                        } label: {
                            HStack(spacing: 8) {
                                Text("\(i + 1)").font(.caption.monospacedDigit()).foregroundStyle(Color.syDim).frame(width: 14)
                                StateBadge(state: s.visualState, compact: true).scaleEffect(0.85)
                                Text("#\(s.number)").foregroundStyle(Color.syMuted)
                                Text(s.title).lineLimit(1).fontWeight(s.number == current ? .semibold : .regular)
                                Spacer(minLength: 6)
                                BranchChip(name: s.headRefName, color: s.number == current ? .syViolet : .sySlate)
                            }
                            .font(.callout)
                            .padding(.horizontal, 10)
                            .padding(.vertical, 4)
                            .background(s.number == current ? Color.syIndigo.opacity(0.12) : .clear)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .disabled(s.number == current)
                    }
                }
                .padding(.vertical, 4)
                .transition(.opacity)
            }
        }
        .background(RoundedRectangle(cornerRadius: 9).fill(Color.syIndigo.opacity(0.06)))
        .clipShape(RoundedRectangle(cornerRadius: 9))
    }

    private func navButton(_ s: StackPR, icon: String, help: String) -> some View {
        Button {
            app.open(.pr(ItemRef(repo: repo, number: s.number)))
        } label: {
            Label("#\(s.number)", systemImage: icon).labelStyle(.titleAndIcon).font(.caption.weight(.medium))
        }
        .buttonStyle(.borderless)
        .help("\(help): #\(s.number) \(s.title)")
    }
}

struct BranchChip: View {
    let name: String
    var color: Color = .sySky

    var body: some View {
        Text(name)
            .font(.system(.caption, design: .monospaced))
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(Capsule().fill(color.opacity(0.12)))
            .foregroundStyle(color)
            .onTapGesture { copyToPasteboard(name) }
            .help("Click to copy")
    }
}

struct ReviewerBadge: View {
    let login: String
    let avatar: String?
    let state: String

    var body: some View {
        HStack(spacing: 3) {
            Avatar(url: avatar, size: 18)
            Group {
                switch state {
                case "APPROVED": Image(systemName: "checkmark").foregroundStyle(Color.ghGreen)
                case "CHANGES_REQUESTED": Image(systemName: "plusminus").foregroundStyle(Color.ghRed)
                case "COMMENTED": Image(systemName: "text.bubble").foregroundStyle(Color.syMuted)
                default: Image(systemName: "circle.fill").font(.system(size: 6)).foregroundStyle(Color.ghYellow)
                }
            }
            .font(.caption.weight(.bold))
        }
        .help("\(login): \(stateTitle)")
    }

    private var stateTitle: String {
        switch state {
        case "APPROVED": return "approved"
        case "CHANGES_REQUESTED": return "changes requested"
        case "COMMENTED": return "commented"
        case "DISMISSED": return "dismissed"
        default: return "review pending"
        }
    }
}

struct MergeStatusBar: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    let onMerge: () -> Void

    var body: some View {
        if pr.isOpen {
            let s = status
            HStack(spacing: 8) {
                Image(systemName: s.icon).foregroundStyle(s.color)
                Text(s.text).font(.callout)
                if let auto = pr.autoMergeRequest {
                    Text("· auto-merge enabled" + (auto.mergeMethod.map { " (\(MergeMethod(rawValue: $0)?.title ?? $0))" } ?? ""))
                        .font(.callout).foregroundStyle(Color.ghGreen)
                }
                checksBadge
                Spacer()
                if MergeConflicts.applies(pr) {
                    Button(conflictsTitle) {
                        model.tab = .conversation
                        model.scrollToBottomToken += 1
                    }
                    .controlSize(.small)
                    .help("Show the conflicting files at the end of the conversation")
                }
                if pr.mergeStateStatus == "BEHIND", model.canWrite || pr.viewerCanUpdate {
                    Button("Update branch") { Task { await model.simple(Q.updateBranch) } }.controlSize(.small)
                }
                if model.canWrite, !pr.isDraft {
                    Button("Merge…", action: onMerge)
                        .controlSize(.small)
                        .buttonStyle(.borderedProminent)
                        .tint(s.ready ? Color.ghGreen : .gray)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 8).fill(s.color.opacity(0.08)))
        }
    }

    @ViewBuilder
    private var checksBadge: some View {
        let checks = pr.checks
        if !checks.isEmpty {
            let failed = checks.filter { $0.outcome == .failure }.count
            let pending = checks.filter { $0.outcome == .pending }.count
            let ok = checks.filter { $0.outcome == .success }.count
            Button {
                model.tab = .checks
            } label: {
                HStack(spacing: 6) {
                    if failed > 0 { Label("\(failed)", systemImage: "xmark.circle.fill").foregroundStyle(Color.ghRed) }
                    if pending > 0 { Label("\(pending)", systemImage: "clock.fill").foregroundStyle(Color.ghYellow) }
                    Label("\(ok)", systemImage: "checkmark.circle.fill").foregroundStyle(Color.ghGreen)
                }
                .font(.caption)
            }
            .buttonStyle(.plain)
            .help("Checks")
        }
    }

    private var conflictsTitle: String {
        let count = model.conflicts?.files.count ?? 0
        return count > 0 ? "View \(plural(count, "conflict", "conflicts"))" : "View conflicts"
    }

    private var failedChecks: Int { pr.checks.filter { $0.outcome == .failure }.count }
    private var pendingChecks: Int { pr.checks.filter { $0.outcome == .pending }.count }
    private var failedChecksText: String { failedChecks == 1 ? "1 check failed" : "\(failedChecks) checks failed" }
    private var pendingChecksText: String { pendingChecks == 1 ? "1 check is still running" : "\(pendingChecks) checks are still running" }

    private var status: (icon: String, color: Color, text: String, ready: Bool) {
        if pr.isDraft { return ("pencil.circle", .secondary, "Draft — merging is unavailable", false) }
        if pr.mergeable == "CONFLICTING" || pr.mergeStateStatus == "DIRTY" {
            return ("exclamationmark.triangle.fill", .ghRed, "Has conflicts with \(pr.baseRefName)", false)
        }
        switch pr.mergeStateStatus {
        case "BEHIND": return ("arrow.down.circle", .ghYellow, "Branch is behind \(pr.baseRefName)", false)
        case "BLOCKED":
            switch pr.reviewDecision {
            case "CHANGES_REQUESTED": return ("plusminus.circle.fill", .ghRed, "Changes requested", false)
            case "REVIEW_REQUIRED": return ("eye.circle", .ghYellow, "Review required", false)
            default:
                if failedChecks > 0 { return ("xmark.circle.fill", .ghRed, failedChecksText, false) }
                if pendingChecks > 0 { return ("clock", .ghYellow, pendingChecksText, false) }
                return ("lock.circle", .ghYellow, "Blocked by branch protection", false)
            }
        case "UNSTABLE":
            if failedChecks > 0 { return ("exclamationmark.circle", .ghYellow, "Can be merged, but " + failedChecksText.lowercased(), true) }
            if pendingChecks > 0 { return ("clock", .ghYellow, pendingChecksText, true) }
            return ("checkmark.circle.fill", .ghGreen, "Ready to merge", true)
        case "CLEAN", "HAS_HOOKS": return ("checkmark.circle.fill", .ghGreen, "Ready to merge", true)
        default: return ("hourglass", .secondary, "Checking mergeability…", false)
        }
    }
}

struct ConversationTab: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    @Environment(AppModel.self) private var app
    @State private var text = ""

    var body: some View {
        VStack(spacing: 0) {
            HTMLView(html: model.html, scrollToBottomToken: model.scrollToBottomToken, onAction: handle, onLink: openLink)
                .liveRefresh(updated: model.lastUpdated, refreshing: model.loading, footer: false, web: true) { await model.load(full: true) }
            Divider()
            HStack(alignment: .bottom, spacing: 8) {
                ComposerBox(text: $text, placeholder: "Comment (Markdown) — ⌘↩ to send", minHeight: 56).frame(height: 64)
                VStack(spacing: 6) {
                    Button("Comment") { send() }
                        .keyboardShortcut(.return, modifiers: .command)
                        .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy)
                        .buttonStyle(.borderedProminent)
                }
            }
            .padding(10)
        }
    }

    private func send() {
        let body = text
        Task {
            if await model.comment(body) { text = "" }
        }
    }

    private func handle(_ type: String, _ id: String) {
        switch type {
        case "reply":
            model.replyThread = pr.reviewThreads.nodes.first { $0.id == id }
        case "resolve":
            Task { await model.setResolved(id, true) }
        case "unresolve":
            Task { await model.setResolved(id, false) }
        case "file":
            model.revealFile(id)
            Task { await model.loadFiles() }
        case "resolveConflicts":
            model.showResolveConflicts = true
        case "chooseClone":
            model.chooseLocalClone()
        case "rescanConflicts":
            model.rescanConflicts()
        default:
            break
        }
    }

    private func openLink(_ url: URL) {
        if url.host == "github.com", url.path == "/" || url.path.isEmpty { return }
        if let route = GitHubURL.parse(url.absoluteString, allowShort: false) {
            app.open(route, push: true)
        } else {
            LinkBrowser.open(url)
        }
    }
}

struct CommitsTab: View {
    let model: PRDetailModel
    let pr: PullRequestDetail

    var body: some View {
        List {
            ForEach(pr.allCommits.nodes.reversed(), id: \.commit.oid) { node in
                let c = node.commit
                NavigationLink(value: Route.commit(model.ref.repo, c.oid)) {
                    HStack(spacing: 10) {
                        Avatar(url: c.author?.avatarUrl, size: 22)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(c.messageHeadline).lineLimit(2)
                            Text("\(c.author?.user?.login ?? c.author?.name ?? "") · \(Fmt.rel(c.committedDate))")
                                .font(.caption).foregroundStyle(Color.syMuted)
                        }
                        Spacer()
                        CIIcon(state: c.statusCheckRollup?.state)
                        Text(c.abbreviatedOid).font(.system(.caption, design: .monospaced)).foregroundStyle(Color.syMuted)
                    }
                    .padding(.vertical, 2)
                    .rowAppear(c.oid, model.commitRows)
                }
                .contextMenu {
                    Button("Copy SHA") { copyToPasteboard(c.oid) }
                    Button("Open in browser") { openInBrowser(c.url) }
                }
            }
            if (pr.allCommits.totalCount ?? 0) > pr.allCommits.nodes.count {
                Button("Show all \(pr.allCommits.totalCount ?? 0) commits in browser") { openInBrowser(pr.url + "/commits") }
            }
        }
        .liveRefresh(updated: model.lastUpdated, refreshing: model.loading, footer: false) { await model.load(full: true) }
    }
}

struct ChecksTab: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    @State private var logTarget: CheckContext?

    var body: some View {
        let checks = pr.checks.sorted { ($0.outcome.rawValue, $0.title) < ($1.outcome.rawValue, $1.title) }
        let failedRuns = Set(checks.filter { $0.outcome == .failure }.compactMap(\.runId))
        List {
            if checks.isEmpty {
                Text("No checks for the latest commit").foregroundStyle(Color.syMuted)
            }
            if !failedRuns.isEmpty, model.canWrite {
                Button {
                    Task { await model.rerunFailed() }
                } label: {
                    Label("Re-run failed jobs (\(failedRuns.count) workflow)", systemImage: "arrow.clockwise")
                }
            }
            ForEach(checks) { c in
                HStack(spacing: 10) {
                    OutcomeIcon(outcome: c.outcome)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(c.workflowName.map { "\($0) / \(c.title)" } ?? c.title).lineLimit(1)
                        Text(subtitle(c)).font(.caption).foregroundStyle(Color.syMuted).lineLimit(1)
                    }
                    Spacer()
                    if c.isActionsJob, c.outcome != .pending, c.outcome != .skipped {
                        Button("Log") { logTarget = c }.controlSize(.small)
                    }
                    if let link = c.link {
                        Button { openInBrowser(link) } label: { Image(systemName: "arrow.up.right.square") }
                            .buttonStyle(.borderless)
                            .help("Open in browser")
                    }
                }
                .padding(.vertical, 2)
                .rowAppear(c.id, model.checkRows)
            }
        }
        .liveRefresh(updated: model.lastUpdated, refreshing: model.loading, footer: false) { await model.load(full: true) }
        .sheet(item: $logTarget) { c in
            JobLogSheet(api: model.api, repo: model.ref.repo, check: c)
        }
    }

    private func subtitle(_ c: CheckContext) -> String {
        var parts: [String] = []
        switch c.outcome {
        case .success: parts.append("Successful")
        case .failure: parts.append("Failed")
        case .pending: parts.append(c.status == "QUEUED" ? "Queued" : "In progress")
        case .neutral: parts.append(c.conclusion == "CANCELLED" ? "Cancelled" : "Neutral")
        case .skipped: parts.append("Skipped")
        }
        if let d = c.duration { parts.append(d) }
        if let desc = c.description, !desc.isEmpty { parts.append(desc) }
        return parts.joined(separator: " · ")
    }
}
