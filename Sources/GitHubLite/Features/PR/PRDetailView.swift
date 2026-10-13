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
            await m?.load()
        }
        .onChange(of: app.refreshTick) {
            Task { await model?.load() }
        }
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
            ClaudeFixBanner(pr: pr) { showFix = true }
            TabStrip(items: [
                TabItem(id: PRTab.conversation, title: t("Обсуждение", "Conversation"), icon: "bubble.left.and.bubble.right", badge: commentsBadge),
                TabItem(id: PRTab.commits, title: t("Коммиты", "Commits"), icon: "point.3.connected.trianglepath.dotted", badge: "\(pr.allCommits.totalCount ?? pr.allCommits.nodes.count)"),
                TabItem(id: PRTab.checks, title: t("Проверки", "Checks"), icon: "checklist", badge: checksSummary, badgeColor: checksColor),
                TabItem(id: PRTab.files, title: t("Файлы", "Files changed"), icon: "doc.text", badge: "\(pr.changedFiles)")
            ], selection: $model.tab, shortcuts: true)
            .padding(.top, 6)
            Divider()
            ZStack {
                ForEach(PRTab.allCases) { tab in
                    if model.visitedTabs.contains(tab) {
                        tabContent(tab)
                            .opacity(model.tab == tab ? 1 : 0)
                            .allowsHitTesting(model.tab == tab)
                            .disabled(model.tab != tab)
                            .accessibilityHidden(model.tab != tab)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .transaction { $0.animation = nil }
        }
        .navigationTitle(pr.title)
        .navigationSubtitle("\(model.ref.repo.full) #\(pr.number)")
        .toolbar { toolbar }
        .sheet(isPresented: $showReview) { ReviewSheet(model: model, pr: pr) }
        .sheet(isPresented: $showMerge) { MergeSheet(model: model, pr: pr) }
        .sheet(isPresented: $showEdit) { EditPRSheet(model: model, pr: pr) }
        .sheet(isPresented: $showReviewers) { ReviewersSheet(model: model) }
        .sheet(isPresented: $showFix) { FixWithClaudeSheet(model: model, pr: pr) }
        .sheet(item: $model.replyThread) { ReplySheet(model: model, thread: $0) }
        .alert(t("Ошибка", "Error"), isPresented: Binding(get: { model.actionError != nil }, set: { if !$0 { model.actionError = nil } })) {
            Button("OK") {}
        } message: {
            Text(model.actionError ?? "")
        }
        .confirmationDialog(confirmTitle, isPresented: Binding(get: { confirm != nil }, set: { if !$0 { confirm = nil } }), presenting: confirm) { c in
            Button(confirmButton(c), role: c == .approve ? nil : .destructive) { perform(c) }
            Button(t("Отмена", "Cancel"), role: .cancel) {}
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
        let unresolved = pr.reviewThreads.nodes.filter { !$0.isResolved }.count
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
        case .approve: return t("Одобрить этот PR?", "Approve this pull request?")
        case .close: return t("Закрыть PR без слияния?", "Close pull request without merging?")
        case .deleteBranch: return t("Удалить ветку \(pr.headRefName)?", "Delete branch \(pr.headRefName)?")
        case .disableAutoMerge: return t("Отключить автослияние?", "Disable auto-merge?")
        case nil: return ""
        }
    }

    private func confirmButton(_ c: PRConfirm) -> String {
        switch c {
        case .approve: return t("Одобрить", "Approve")
        case .close: return t("Закрыть PR", "Close pull request")
        case .deleteBranch: return t("Удалить ветку", "Delete branch")
        case .disableAutoMerge: return t("Отключить", "Disable")
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
            if model.busy || model.loading {
                ProgressView().controlSize(.small)
            }
            if pr.isOpen {
                Menu {
                    Button(t("Написать ревью…", "Write review…")) { showReview = true }
                    if !pr.viewerDidAuthor {
                        Button(t("Одобрить", "Approve")) { confirm = .approve }
                    }
                } label: {
                    Label(model.pending.isEmpty ? t("Ревью", "Review") : t("Ревью (\(model.pending.count))", "Review (\(model.pending.count))"), systemImage: "checkmark.bubble")
                } primaryAction: {
                    showReview = true
                }
                .help(t("Отправить ревью", "Submit review"))
            }
            Button { showFix = true } label: { Label(t("Исправить с Claude", "Fix with Claude"), systemImage: "sparkles") }
                .help(t("Отправить комментарии ревью в новую сессию Claude Code", "Send review comments to a new Claude Code session"))
            if canMerge {
                Button { showMerge = true } label: { Label(t("Слить", "Merge"), systemImage: "arrow.triangle.merge") }
                    .help(t("Слить pull request", "Merge pull request"))
            }
            Menu {
                moreMenu
            } label: {
                Label(t("Ещё", "More"), systemImage: "ellipsis.circle")
            }
            Button { Task { await model.load() } } label: { Label(t("Обновить", "Refresh"), systemImage: "arrow.clockwise") }
                .help(t("Обновить", "Refresh"))
            Button { openInBrowser(pr.url) } label: { Label(t("В браузере", "Open in browser"), systemImage: "safari") }
                .help(t("Открыть на github.com", "Open on github.com"))
        }
    }

    @ViewBuilder
    private var moreMenu: some View {
        Button(t("Копировать ссылку", "Copy link")) { copyToPasteboard(pr.url) }
        Button(t("Копировать «gh pr checkout \(pr.number)»", "Copy “gh pr checkout \(pr.number)”")) { copyToPasteboard("gh pr checkout \(pr.number) -R \(model.ref.repo.full)") }
        Button(t("Копировать имя ветки", "Copy branch name")) { copyToPasteboard(pr.headRefName) }
        Divider()
        if pr.viewerCanUpdate {
            Button(t("Редактировать заголовок и описание…", "Edit title and description…")) { showEdit = true }
        }
        if pr.isOpen {
            Button(t("Запросить ревью…", "Request review…")) { showReviewers = true }
            if pr.viewerCanUpdate {
                if pr.isDraft {
                    Button(t("Готово к ревью", "Ready for review")) { Task { await model.simple(Q.readyForReview) } }
                } else {
                    Button(t("Перевести в черновик", "Convert to draft")) { Task { await model.simple(Q.convertToDraft) } }
                }
            }
            if pr.mergeStateStatus == "BEHIND" || pr.mergeStateStatus == "BLOCKED" || pr.mergeStateStatus == "UNSTABLE" {
                Button(t("Обновить ветку из \(pr.baseRefName)", "Update branch from \(pr.baseRefName)")) { Task { await model.simple(Q.updateBranch) } }
            }
            if pr.autoMergeRequest != nil {
                Button(t("Отключить автослияние", "Disable auto-merge")) { confirm = .disableAutoMerge }
            }
            Divider()
            if pr.viewerCanUpdate {
                Button(t("Закрыть PR…", "Close pull request…")) { confirm = .close }
            }
        } else if !pr.merged, pr.viewerCanUpdate {
            Button(t("Переоткрыть PR", "Reopen pull request")) { Task { await model.simple(Q.reopenPR) } }
        }
        if !pr.isOpen, pr.headRef != nil, model.canWrite {
            Button(t("Удалить ветку \(pr.headRefName)…", "Delete branch \(pr.headRefName)…")) { confirm = .deleteBranch }
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
                Text(pr.title).font(.title2.weight(.semibold)).textSelection(.enabled)
                Text("#\(pr.number)").font(.title2).foregroundStyle(.secondary)
            }
            HStack(spacing: 8) {
                StateBadge(state: pr.visualState)
                Avatar(url: pr.author?.avatarUrl, size: 18)
                Text(pr.author?.login ?? "ghost").fontWeight(.semibold)
                BranchChip(name: pr.baseRefName)
                Image(systemName: "arrow.left").font(.caption).foregroundStyle(.secondary)
                BranchChip(name: pr.headRefName)
                DiffStat(additions: pr.additions, deletions: pr.deletions)
                Spacer(minLength: 0)
                Text(Fmt.rel(pr.createdAt)).foregroundStyle(.secondary).help(Fmt.full.string(from: pr.createdAt))
            }
            .font(.callout)
            .lineLimit(1)
            if !pr.labels.nodes.isEmpty || !model.reviewers.isEmpty || !pr.assignees.nodes.isEmpty {
                HStack(spacing: 6) {
                    ForEach(pr.labels.nodes, id: \.name) { LabelChip(label: $0) }
                    if !model.reviewers.isEmpty {
                        Text(t("Ревьюеры:", "Reviewers:")).font(.caption).foregroundStyle(.secondary).padding(.leading, pr.labels.nodes.isEmpty ? 0 : 6)
                        ForEach(model.reviewers, id: \.login) { r in
                            ReviewerBadge(login: r.login, avatar: r.avatar, state: r.state)
                        }
                    }
                    if !pr.assignees.nodes.isEmpty {
                        Text(t("Исполнители:", "Assignees:")).font(.caption).foregroundStyle(.secondary).padding(.leading, 6)
                        ForEach(pr.assignees.nodes, id: \.login) { a in
                            Avatar(url: a.avatarUrl, size: 18).help(a.login)
                        }
                    }
                }
            }
            MergeStatusBar(model: model, pr: pr, onMerge: onMerge)
        }
        .padding(.horizontal, 16)
        .padding(.top, 12)
    }
}

struct BranchChip: View {
    let name: String

    var body: some View {
        Text(name)
            .font(.system(.caption, design: .monospaced))
            .padding(.horizontal, 6)
            .padding(.vertical, 2)
            .background(RoundedRectangle(cornerRadius: 5).fill(Color.accentColor.opacity(0.12)))
            .foregroundStyle(Color.accentColor)
            .onTapGesture { copyToPasteboard(name) }
            .help(t("Нажмите, чтобы скопировать", "Click to copy"))
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
                case "COMMENTED": Image(systemName: "text.bubble").foregroundStyle(.secondary)
                default: Image(systemName: "circle.fill").font(.system(size: 6)).foregroundStyle(Color.ghYellow)
                }
            }
            .font(.caption.weight(.bold))
        }
        .help("\(login): \(stateTitle)")
    }

    private var stateTitle: String {
        switch state {
        case "APPROVED": return t("одобрено", "approved")
        case "CHANGES_REQUESTED": return t("запрошены изменения", "changes requested")
        case "COMMENTED": return t("комментарии", "commented")
        case "DISMISSED": return t("отклонено", "dismissed")
        default: return t("ожидается ревью", "review pending")
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
                    Text(t("· автослияние включено", "· auto-merge enabled") + (auto.mergeMethod.map { " (\(MergeMethod(rawValue: $0)?.title ?? $0))" } ?? ""))
                        .font(.callout).foregroundStyle(Color.ghGreen)
                }
                checksBadge
                Spacer()
                if pr.mergeStateStatus == "BEHIND", model.canWrite || pr.viewerCanUpdate {
                    Button(t("Обновить ветку", "Update branch")) { Task { await model.simple(Q.updateBranch) } }.controlSize(.small)
                }
                if model.canWrite, !pr.isDraft {
                    Button(t("Слить…", "Merge…"), action: onMerge)
                        .controlSize(.small)
                        .buttonStyle(.borderedProminent)
                        .tint(s.ready ? Color.ghGreen : .gray)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 8).fill(s.color.opacity(0.08)))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(s.color.opacity(0.3)))
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
            .help(t("Проверки", "Checks"))
        }
    }

    private var status: (icon: String, color: Color, text: String, ready: Bool) {
        if pr.isDraft { return ("pencil.circle", .secondary, t("Черновик — слияние недоступно", "Draft — merging is unavailable"), false) }
        if pr.mergeable == "CONFLICTING" || pr.mergeStateStatus == "DIRTY" {
            return ("exclamationmark.triangle.fill", .ghRed, t("Есть конфликты с \(pr.baseRefName)", "Has conflicts with \(pr.baseRefName)"), false)
        }
        switch pr.mergeStateStatus {
        case "BEHIND": return ("arrow.down.circle", .ghYellow, t("Ветка отстаёт от \(pr.baseRefName)", "Branch is behind \(pr.baseRefName)"), false)
        case "BLOCKED":
            switch pr.reviewDecision {
            case "CHANGES_REQUESTED": return ("plusminus.circle.fill", .ghRed, t("Запрошены изменения", "Changes requested"), false)
            case "REVIEW_REQUIRED": return ("eye.circle", .ghYellow, t("Нужно одобрение ревьюера", "Review required"), false)
            default: return ("lock.circle", .ghYellow, t("Заблокировано правилами ветки или проверками", "Blocked by branch protection or checks"), false)
            }
        case "UNSTABLE": return ("exclamationmark.circle", .ghYellow, t("Слияние возможно, но есть непрошедшие проверки", "Can be merged, but some checks failed"), true)
        case "CLEAN", "HAS_HOOKS": return ("checkmark.circle.fill", .ghGreen, t("Можно сливать", "Ready to merge"), true)
        default: return ("hourglass", .secondary, t("Проверка возможности слияния…", "Checking mergeability…"), false)
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
            Divider()
            HStack(alignment: .bottom, spacing: 8) {
                ComposerBox(text: $text, placeholder: t("Комментарий (Markdown) — ⌘↩ отправить", "Comment (Markdown) — ⌘↩ to send"), minHeight: 56).frame(height: 64)
                VStack(spacing: 6) {
                    Button(t("Комментировать", "Comment")) { send() }
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
                                .font(.caption).foregroundStyle(.secondary)
                        }
                        Spacer()
                        CIIcon(state: c.statusCheckRollup?.state)
                        Text(c.abbreviatedOid).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 2)
                }
                .contextMenu {
                    Button(t("Копировать SHA", "Copy SHA")) { copyToPasteboard(c.oid) }
                    Button(t("Открыть в браузере", "Open in browser")) { openInBrowser(c.url) }
                }
            }
            if (pr.allCommits.totalCount ?? 0) > pr.allCommits.nodes.count {
                Button(t("Показать все \(pr.allCommits.totalCount ?? 0) коммитов в браузере", "Show all \(pr.allCommits.totalCount ?? 0) commits in browser")) { openInBrowser(pr.url + "/commits") }
            }
        }
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
                Text(t("Нет проверок для последнего коммита", "No checks for the latest commit")).foregroundStyle(.secondary)
            }
            if !failedRuns.isEmpty, model.canWrite {
                Button {
                    Task { await model.rerunFailed() }
                } label: {
                    Label(t("Перезапустить упавшие jobs (\(failedRuns.count) workflow)", "Re-run failed jobs (\(failedRuns.count) workflow)"), systemImage: "arrow.clockwise")
                }
            }
            ForEach(checks) { c in
                HStack(spacing: 10) {
                    OutcomeIcon(outcome: c.outcome)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(c.workflowName.map { "\($0) / \(c.title)" } ?? c.title).lineLimit(1)
                        Text(subtitle(c)).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                    Spacer()
                    if c.isActionsJob, c.outcome != .pending, c.outcome != .skipped {
                        Button(t("Лог", "Log")) { logTarget = c }.controlSize(.small)
                    }
                    if let link = c.link {
                        Button { openInBrowser(link) } label: { Image(systemName: "arrow.up.right.square") }
                            .buttonStyle(.borderless)
                            .help(t("Открыть в браузере", "Open in browser"))
                    }
                }
                .padding(.vertical, 2)
            }
        }
        .sheet(item: $logTarget) { c in
            JobLogSheet(api: model.api, repo: model.ref.repo, check: c)
        }
    }

    private func subtitle(_ c: CheckContext) -> String {
        var parts: [String] = []
        switch c.outcome {
        case .success: parts.append(t("Успешно", "Successful"))
        case .failure: parts.append(t("Ошибка", "Failed"))
        case .pending: parts.append(c.status == "QUEUED" ? t("В очереди", "Queued") : t("Выполняется", "In progress"))
        case .neutral: parts.append(c.conclusion == "CANCELLED" ? t("Отменено", "Cancelled") : t("Нейтрально", "Neutral"))
        case .skipped: parts.append(t("Пропущено", "Skipped"))
        }
        if let d = c.duration { parts.append(d) }
        if let desc = c.description, !desc.isEmpty { parts.append(desc) }
        return parts.joined(separator: " · ")
    }
}
