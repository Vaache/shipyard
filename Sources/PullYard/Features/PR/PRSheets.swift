import SwiftUI

enum MergeMethod: String, CaseIterable, Identifiable {
    case MERGE, SQUASH, REBASE
    var id: String { rawValue }

    var title: String {
        switch self {
        case .MERGE: return "Merge commit"
        case .SQUASH: return "Squash and merge"
        case .REBASE: return "Rebase and merge"
        }
    }
}

struct SheetFrame<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.title3.weight(.semibold))
            content
        }
        .padding(20)
        .frame(width: 560)
        .surface(.panel)
    }
}

struct ReviewSheet: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var event = "COMMENT"

    var body: some View {
        SheetFrame(title: "Review #\(pr.number)") {
            if !model.pending.isEmpty {
                Label("Inline comments to be submitted: \(model.pending.count)", systemImage: "text.bubble")
                    .foregroundStyle(Color.syMuted)
            }
            ComposerBox(text: $text, placeholder: "Review summary (optional when approving)", minHeight: 140)
                .frame(maxHeight: 260)
            Picker("", selection: $event) {
                Text("Comment — without approval").tag("COMMENT")
                Text("Approve").tag("APPROVE").disabled(pr.viewerDidAuthor)
                Text("Request changes").tag("REQUEST_CHANGES").disabled(pr.viewerDidAuthor)
            }
            .pickerStyle(.radioGroup)
            .labelsHidden()
            HStack {
                if !model.pending.isEmpty {
                    Button("Discard pending comments", role: .destructive) { model.pending.removeAll() }
                }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Submit review") {
                    let body = text, ev = event
                    Task { if await model.submitReview(event: ev, body: body) { dismiss() } }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .buttonStyle(.borderedProminent)
                .disabled(model.busy || (event != "APPROVE" && text.trimmingCharacters(in: .whitespaces).isEmpty && model.pending.isEmpty))
            }
        }
    }
}

struct MergeSheet: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    @Environment(\.dismiss) private var dismiss
    @State private var method: MergeMethod = .MERGE
    @State private var title = ""
    @State private var message = ""
    @State private var deleteBranch = true

    private var allowed: [MergeMethod] {
        guard let r = model.repo else { return MergeMethod.allCases }
        return MergeMethod.allCases.filter {
            switch $0 {
            case .MERGE: return r.mergeCommitAllowed
            case .SQUASH: return r.squashMergeAllowed
            case .REBASE: return r.rebaseMergeAllowed
            }
        }
    }

    var body: some View {
        SheetFrame(title: "Merge #\(pr.number) into \(pr.baseRefName)") {
            Picker("Method", selection: $method) {
                ForEach(allowed) { Text($0.title).tag($0) }
            }
            .onChange(of: method) { _, m in prefill(m) }
            if method != .REBASE {
                TextField("Commit title (GitHub default)", text: $title)
                ComposerBox(text: $message, placeholder: "Commit description (optional)", minHeight: 80)
                    .frame(maxHeight: 160)
            }
            if !(model.repo?.deleteBranchOnMerge ?? false), pr.headRepository?.nameWithOwner == model.ref.repo.full {
                Toggle("Delete branch \(pr.headRefName) after merge", isOn: $deleteBranch)
            }
            HStack {
                if model.repo?.autoMergeAllowed == true, pr.autoMergeRequest == nil, pr.mergeStateStatus != "CLEAN" {
                    Button("Enable auto-merge") {
                        Task { if await model.enableAutoMerge(method: method.rawValue) { dismiss() } }
                    }
                    .help("Merge automatically when checks and reviews pass")
                }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Merge") {
                    let t = title.trimmingCharacters(in: .whitespaces)
                    let b = message.trimmingCharacters(in: .whitespacesAndNewlines)
                    let del = deleteBranch && !(model.repo?.deleteBranchOnMerge ?? false) && pr.headRepository?.nameWithOwner == model.ref.repo.full
                    Task {
                        if await model.merge(method: method.rawValue, title: t.isEmpty ? nil : t, body: b.isEmpty ? nil : b, deleteBranch: del) { dismiss() }
                    }
                }
                .buttonStyle(.borderedProminent)
                .tint(Color.ghGreen)
                .disabled(model.busy || allowed.isEmpty)
            }
        }
        .onAppear {
            let def = MergeMethod(rawValue: model.repo?.viewerDefaultMergeMethod ?? "MERGE") ?? .MERGE
            method = allowed.contains(def) ? def : (allowed.first ?? .MERGE)
            prefill(method)
        }
    }

    private func prefill(_ m: MergeMethod) {
        switch m {
        case .SQUASH: title = "\(pr.title) (#\(pr.number))"
        case .MERGE: title = ""
        case .REBASE: title = ""
        }
    }
}

struct EditPRSheet: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var text = ""

    var body: some View {
        SheetFrame(title: "Edit #\(pr.number)") {
            TextField("Title", text: $title).textFieldStyle(.roundedBorder)
            ComposerBox(text: $text, placeholder: "Description (Markdown)", minHeight: 260)
                .frame(maxHeight: 420)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") {
                    let t = title, b = text
                    Task { if await model.edit(title: t, body: b) { dismiss() } }
                }
                .keyboardShortcut(.return, modifiers: .command)
                .buttonStyle(.borderedProminent)
                .disabled(title.trimmingCharacters(in: .whitespaces).isEmpty || model.busy)
            }
        }
        .frame(width: 680)
        .onAppear {
            title = pr.title
            text = pr.body
        }
    }
}

struct ReviewersSheet: View {
    let model: PRDetailModel
    @Environment(\.dismiss) private var dismiss
    @State private var logins = ""
    @State private var selected: Set<String> = []

    private var candidates: [(login: String, avatar: String?, state: String)] {
        model.reviewers.filter { $0.login != model.viewerLogin && !$0.login.hasSuffix("[bot]") && !$0.login.contains("/") }
    }

    private var typed: [String] {
        logins.split(whereSeparator: { $0 == "," || $0 == " " }).map { $0.trimmingCharacters(in: CharacterSet(charactersIn: "@ ")) }.filter { !$0.isEmpty }
    }

    private var requested: [String] {
        var seen = Set<String>()
        return (candidates.map(\.login).filter(selected.contains) + typed).filter { seen.insert($0).inserted }
    }

    var body: some View {
        SheetFrame(title: "Request review") {
            if !candidates.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Reviewers on this pull request").font(.caption).foregroundStyle(Color.syMuted)
                    ForEach(candidates, id: \.login) { r in
                        Toggle(isOn: Binding(get: { selected.contains(r.login) }, set: { if $0 { selected.insert(r.login) } else { selected.remove(r.login) } })) {
                            HStack(spacing: 6) {
                                ReviewerBadge(login: r.login, avatar: r.avatar, state: r.state)
                                Text(r.login)
                                if model.reviewersToReRequest.contains(r.login) {
                                    Text("reviewed before latest push").font(.caption).foregroundStyle(Color.syAmber)
                                } else if r.state == "PENDING" {
                                    Text("already requested").font(.caption).foregroundStyle(Color.syMuted)
                                }
                            }
                        }
                        .toggleStyle(.checkbox)
                    }
                }
            }
            TextField("Other logins, comma-separated, e.g. alice, bob", text: $logins).textFieldStyle(.roundedBorder)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button(requested.isEmpty ? "Request" : "Request (\(requested.count))") {
                    let list = requested
                    Task { if await model.requestReviewers(list) { dismiss() } }
                }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
                .disabled(requested.isEmpty || model.busy)
            }
        }
        .onAppear { selected = Set(model.reviewersToReRequest) }
    }
}

struct ReplySheet: View {
    let model: PRDetailModel
    let thread: ReviewThread
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""

    var body: some View {
        SheetFrame(title: "Reply to thread") {
            Text("\(thread.path)\(thread.line.map { ":\($0)" } ?? "")").font(.system(.caption, design: .monospaced)).foregroundStyle(Color.syMuted)
            if let last = thread.comments.nodes.last {
                HStack(alignment: .top, spacing: 8) {
                    Avatar(url: last.author?.avatarUrl, size: 18)
                    MarkdownText(text: last.body).lineLimit(6).font(.callout)
                }
                .padding(8)
                .background(RoundedRectangle(cornerRadius: 6).fill(.quaternary.opacity(0.5)))
            }
            ComposerBox(text: $text, placeholder: "Reply (Markdown)", minHeight: 120).frame(maxHeight: 240)
            HStack {
                if thread.viewerCanResolve, !thread.isResolved {
                    Button("Reply and resolve") { send(resolve: true) }
                        .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy)
                }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Reply") { send(resolve: false) }
                    .keyboardShortcut(.return, modifiers: .command)
                    .buttonStyle(.borderedProminent)
                    .disabled(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || model.busy)
            }
        }
    }

    private func send(resolve: Bool) {
        let body = text
        let id = thread.id
        Task {
            if await model.reply(id, body: body) {
                if resolve { await model.setResolved(id, true) }
                dismiss()
            }
        }
    }
}

struct JobLogSheet: View {
    let api: GitHubAPI
    let repo: RepoRef
    let check: CheckContext
    var onFix: ((String?) -> Void)?
    @Environment(\.dismiss) private var dismiss
    @State private var log: String?
    @State private var error: String?
    @State private var errorsOnly = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                OutcomeIcon(outcome: check.outcome)
                Text(check.workflowName.map { "\($0) / \(check.title)" } ?? check.title).font(.headline).lineLimit(1)
                Spacer()
                Toggle("Errors only", isOn: $errorsOnly).toggleStyle(.checkbox)
                if let log { Button("Copy") { copyToPasteboard(log) } }
                if let link = check.link { Button("Open in browser") { openInBrowser(link) } }
                if let onFix {
                    Button("Fix with Claude Code") { onFix(log) }
                        .buttonStyle(.borderedProminent)
                        .tint(Color.syCoral)
                }
                Button("Close") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding(12)
            Divider()
            if let log {
                LogTextView(text: errorsOnly ? filterErrors(log) : log)
            } else if let error {
                ErrorStateView(message: error)
            } else {
                LoadingView()
            }
        }
        .frame(width: 980, height: 640)
        .task {
            guard let id = check.databaseId else { return }
            do {
                let raw = try await api.jobLog(repo: repo, jobId: id)
                log = await Task.detached { Self.clean(raw) }.value
            } catch {
                if isCancellation(error) { return }
                self.error = error.localizedDescription
            }
        }
    }

    private func filterErrors(_ s: String) -> String {
        let lines = s.components(separatedBy: "\n")
        var keep: [String] = []
        for (i, l) in lines.enumerated() where l.contains("##[error]") || l.localizedCaseInsensitiveContains("error") || l.contains("FAILED") || l.contains("✗") {
            let from = max(0, i - 3)
            if let last = keep.last, last == "…" {} else if !keep.isEmpty { keep.append("…") }
            keep += lines[from...i]
        }
        return keep.isEmpty ? "No error lines found" : keep.joined(separator: "\n")
    }

    nonisolated static func clean(_ raw: String) -> String {
        CILogExcerpt.clean(raw)
    }
}

struct LogTextView: NSViewRepresentable {
    let text: String

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        let tv = scroll.documentView as! NSTextView
        tv.isEditable = false
        tv.isSelectable = true
        tv.font = .monospacedSystemFont(ofSize: 11, weight: .regular)
        tv.textContainerInset = NSSize(width: 8, height: 8)
        tv.isAutomaticSpellingCorrectionEnabled = false
        set(tv)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let tv = scroll.documentView as? NSTextView, tv.string != text else { return }
        set(tv)
    }

    private func set(_ tv: NSTextView) {
        let attr = NSMutableAttributedString(string: text, attributes: [.font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular), .foregroundColor: NSColor.textColor])
        let ns = text as NSString
        ns.enumerateSubstrings(in: NSRange(location: 0, length: ns.length), options: .byLines) { sub, range, _, _ in
            guard let sub else { return }
            if sub.contains("##[error]") { attr.addAttribute(.foregroundColor, value: NSColor.systemRed, range: range) }
            else if sub.contains("##[warning]") { attr.addAttribute(.foregroundColor, value: NSColor.systemOrange, range: range) }
            else if sub.hasPrefix("##[group]") { attr.addAttribute(.foregroundColor, value: NSColor.systemBlue, range: range) }
        }
        tv.textStorage?.setAttributedString(attr)
        tv.scrollToEndOfDocument(nil)
    }
}
