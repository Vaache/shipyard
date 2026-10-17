import SwiftUI

enum FixSource: Hashable {
    case reviewer, composed, ci
}

struct FixWithClaudeSheet: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    var ci: CIFixRequest?
    @Environment(\.dismiss) private var dismiss
    @Environment(AppModel.self) private var app
    @AppStorage("claudeTarget.v2") private var target: ClaudeTarget = .app
    @AppStorage("claudeMode") private var mode: ClaudePermissionMode = .standard
    @State private var found: FoundFixPrompt?
    @State private var source: FixSource = .composed
    @State private var selected: Set<String> = []
    @State private var options = FixOptions()
    @State private var custom: String?
    @State private var folder: String?
    @State private var detecting = true
    @State private var error: String?
    @State private var ciItems: [String: CIFailure] = [:]
    @State private var ciSelected: Set<String> = []
    @State private var ciLoading = false
    @State private var ciGeneration = 0
    @State private var ciHead: String?
    @State private var ciNonce = CIFixPrompt.nonce()
    @State private var ciStale = false

    private var allFailing: [CheckContext] { CIFailureCollector.failing(pr.checks) }
    private var failing: [CheckContext] { CIFailureCollector.shown(allFailing, focus: ci?.focus) }
    private var ciKey: String { source == .ci ? pr.headRefOid + "|" + failing.map(\.id).joined(separator: ",") : "" }
    private var threads: [ReviewThread] { pr.reviewThreads.nodes.filter { !$0.isResolved } }
    private var general: [GeneralFeedback] { FixPrompt.generalFeedback(pr) }

    private var generated: String {
        if source == .reviewer, let found {
            return FixPromptExtractor.wrap(found, pr: pr, options: options)
        }
        if source == .ci {
            let chosen = failing.filter { ciSelected.contains($0.id) }
            return CIFixPrompt.build(pr: pr, repo: model.ref.repo, failures: chosen.compactMap { ciItems[$0.id] }, pending: chosen.filter { ciItems[$0.id] == nil }.map { CIFailure(check: $0).name }, options: options, nonce: ciNonce)
        }
        let chosen = threads.filter { selected.contains($0.id) }
        return FixPrompt.build(pr: pr, repo: model.ref.repo, threads: chosen, general: general, options: options)
    }

    private var prompt: String { custom ?? generated }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                IconTile(systemImage: "sparkles", color: .syCoral, size: 30)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Fix with Claude Code").font(.headline).foregroundStyle(Color.syText)
                    HStack(spacing: 6) {
                        Text("#\(pr.number)").foregroundStyle(Color.syViolet).fontWeight(.semibold)
                        Text(pr.title).foregroundStyle(Color.syMuted).lineLimit(1)
                    }
                    .font(.callout)
                }
            }
            sourcePicker
            if source == .composed { composedOptions }
            if source == .ci { ciOptions }
            HStack(spacing: 16) {
                Toggle("Commit", isOn: $options.commit)
                Toggle("Push", isOn: $options.push)
                if source != .ci {
                    Toggle("Reply & resolve threads", isOn: $options.replyAndResolve)
                        .disabled(!options.push)
                }
            }
            .toggleStyle(.checkbox)
            .onChange(of: options.push) { _, push in if !push { options.replyAndResolve = false } }
            promptEditor
            ClaudeFolderRow(repo: model.ref.repo, folder: $folder, detecting: detecting)
            if let error { Text(error).font(.caption).foregroundStyle(Color.syRed) }
            ClaudeLaunchRow(target: $target, mode: $mode, canLaunch: !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && (target == .copy || folder != nil) && !(source == .ci && (ciLoading || ciSelected.isEmpty)), onLaunch: launch)
        }
        .padding(20)
        .frame(width: 780, height: 680)
        .surface(.panel)
        .onChange(of: source) { custom = nil }
        .onChange(of: custom == nil) { if custom == nil { ciStale = false } }
        .onChange(of: selected) { custom = nil }
        .onChange(of: ciSelected) { custom = nil }
        .onChange(of: options.commit) { custom = nil }
        .onChange(of: options.push) { custom = nil }
        .onChange(of: options.replyAndResolve) { custom = nil }
        .onChange(of: options.includeGeneral) { custom = nil }
        .task(id: ciKey) {
            guard source == .ci else { return }
            if ciHead != pr.headRefOid {
                ciHead = pr.headRefOid
                ciItems = [:]
            }
            let ids = Set(failing.map(\.id))
            ciSelected.formIntersection(ids)
            if ciSelected.isEmpty { ciSelected = ids }
            guard failing.contains(where: needsGather) else { return }
            await gatherCI()
        }
        .task {
            found = FixPromptExtractor.latest(in: pr)
            if let ci, !failing.isEmpty {
                source = .ci
                let focus = failing.filter { $0.id == ci.focus }.map(\.id)
                ciSelected = Set(focus.isEmpty ? failing.map(\.id) : focus)
            } else {
                source = found != nil ? .reviewer : .composed
                ciSelected = Set(failing.map(\.id))
            }
            selected = Set(threads.filter { !$0.isOutdated }.map(\.id))
            if selected.isEmpty { selected = Set(threads.map(\.id)) }
            folder = await LocalRepoStore.detect(model.ref.repo)
            detecting = false
        }
    }

    @ViewBuilder
    private var sourcePicker: some View {
        if found != nil || !failing.isEmpty {
            Picker("", selection: $source) {
                if let found {
                    Text("Fix prompt by @\(found.author)" + (found.date.map { " · \(Fmt.rel($0))" } ?? "")).tag(FixSource.reviewer)
                }
                Text("Build from comments" + " (\(threads.count))").tag(FixSource.composed)
                if !failing.isEmpty {
                    Text("Failing checks (\(failing.count))").tag(FixSource.ci)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        } else {
            Label("No Fix prompt in this PR — built from unresolved comments", systemImage: "info.circle")
                .font(.callout)
                .foregroundStyle(Color.syMuted)
        }
    }

    private var composedOptions: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Unresolved comments").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                Spacer()
                Button("All") { selected = Set(threads.map(\.id)) }.buttonStyle(.link).font(.caption)
                Button("None") { selected = [] }.buttonStyle(.link).font(.caption)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    if threads.isEmpty {
                        Text("No unresolved comments").font(.callout).foregroundStyle(Color.syDim)
                    }
                    ForEach(threads) { th in
                        Toggle(isOn: Binding(get: { selected.contains(th.id) }, set: { on in if on { selected.insert(th.id) } else { selected.remove(th.id) } })) {
                            HStack(spacing: 6) {
                                Text("\(th.path)\((th.line ?? th.originalLine).map { ":\($0)" } ?? "")")
                                    .font(.system(.caption, design: .monospaced))
                                    .foregroundStyle(Color.sySky)
                                    .lineLimit(1)
                                    .truncationMode(.head)
                                if th.isOutdated { ChipView(text: "outdated", color: .syAmber) }
                                Text("@\(th.comments.nodes.first?.author?.login ?? "?"): \(th.comments.nodes.first?.body.replacingOccurrences(of: "\n", with: " ") ?? "")")
                                    .font(.caption)
                                    .foregroundStyle(Color.syMuted)
                                    .lineLimit(1)
                            }
                        }
                        .toggleStyle(.checkbox)
                    }
                    if !general.isEmpty {
                        Toggle("General review feedback (\(general.count))", isOn: $options.includeGeneral)
                            .toggleStyle(.checkbox)
                            .font(.caption)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(8)
            }
            .frame(maxHeight: 130)
            .card(radius: 8)
        }
    }

    private var ciOptions: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Failing checks").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                if allFailing.count > failing.count {
                    Text("showing \(failing.count) of \(allFailing.count)").font(.caption).foregroundStyle(Color.syDim)
                }
                if ciLoading {
                    ProgressView().controlSize(.mini)
                    Text("Collecting logs…").font(.caption).foregroundStyle(Color.syDim)
                }
                Spacer()
                Button("All") { ciSelected = Set(failing.map(\.id)) }.buttonStyle(.link).font(.caption)
                Button("None") { ciSelected = [] }.buttonStyle(.link).font(.caption)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(failing) { c in
                        Toggle(isOn: Binding(get: { ciSelected.contains(c.id) }, set: { on in if on { ciSelected.insert(c.id) } else { ciSelected.remove(c.id) } })) {
                            HStack(spacing: 6) {
                                OutcomeIcon(outcome: c.outcome)
                                Text(c.workflowName.map { "\($0) / \(c.title)" } ?? c.title)
                                    .font(.caption)
                                    .foregroundStyle(Color.syText)
                                    .lineLimit(1)
                                ciStatus(c)
                            }
                        }
                        .toggleStyle(.checkbox)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(8)
            }
            .frame(maxHeight: 130)
            .card(radius: 8)
        }
    }

    @ViewBuilder
    private func ciStatus(_ c: CheckContext) -> some View {
        if let item = ciItems[c.id] {
            if let note = item.note {
                Label("partial", systemImage: "exclamationmark.triangle.fill")
                    .font(.caption)
                    .foregroundStyle(Color.syAmber)
                    .help(note)
            } else {
                Text(ciDetail(item)).font(.caption).foregroundStyle(Color.syDim).lineLimit(1)
            }
        } else if ciLoading {
            ProgressView().controlSize(.mini)
        }
    }

    private func ciDetail(_ item: CIFailure) -> String {
        var parts: [String] = []
        if let e = item.excerpt, !e.isEmpty { parts.append("log \(ByteCountFormatter.string(fromByteCount: Int64(e.utf8.count), countStyle: .file))") }
        if !item.annotations.isEmpty { parts.append(plural(item.annotations.count, "annotation", "annotations")) }
        if item.summary != nil, item.kind != .actions { parts.append("summary") }
        return parts.isEmpty ? "no details" : parts.joined(separator: " · ")
    }

    private func needsGather(_ c: CheckContext) -> Bool {
        guard let item = ciItems[c.id] else { return true }
        return item.note != nil
    }

    private func gatherCI() async {
        ciGeneration += 1
        let generation = ciGeneration
        ciLoading = true
        defer { if ciGeneration == generation { ciLoading = false } }
        guard let api = model.api else { return }
        let repo = model.ref.repo
        var queue = failing.filter(needsGather)[...]
        let seed = ci.flatMap { r in r.log.map { (r.focus, $0) } }
        await withTaskGroup(of: CIFailure.self) { group in
            func next() {
                guard let c = queue.popFirst() else { return }
                let log = seed?.0 == c.id ? seed?.1 : nil
                group.addTask { await CIFailureCollector.collect(api: api, repo: repo, check: c, log: log) }
            }
            for _ in 0..<CIFailureCollector.concurrency { next() }
            for await item in group {
                guard !Task.isCancelled, ciGeneration == generation else {
                    group.cancelAll()
                    return
                }
                ciItems[item.id] = item
                if custom != nil { ciStale = true }
                next()
            }
        }
    }

    private var promptEditor: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Prompt").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                if custom != nil {
                    ChipView(text: "edited", color: .syAmber)
                    Button("Reset") { custom = nil }.buttonStyle(.link).font(.caption)
                }
                if source == .ci, custom != nil, ciStale || ciLoading {
                    Label(ciLoading ? "Logs still loading — your edit won't include them" : "Logs loaded after your edit — Reset to include them", systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(Color.syAmber)
                }
                Spacer()
                Text("\(prompt.count)").font(.caption2.monospacedDigit()).foregroundStyle(Color.syDim)
            }
            TextEditor(text: Binding(get: { prompt }, set: { custom = $0 }))
                .font(.system(size: 11.5, design: .monospaced))
                .foregroundStyle(Color.syText)
                .scrollContentBackground(.hidden)
                .padding(8)
                .card(radius: 10, fill: .sySunken)
        }
    }

    private func launch() {
        do {
            let name = source == .ci ? "Fix CI PR #\(pr.number): \(pr.title)" : "Fix PR #\(pr.number): \(pr.title)"
            let tab = try ClaudeLauncher.start(app: app, prompt: prompt, folder: folder, target: target, mode: mode, name: name)
            if source == .ci, target != .copy {
                app.claude.ciFixes[model.ref] = CIFixLaunch(tabID: tab?.id, target: target, headOid: pr.headRefOid, at: Date())
            }
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct ClaudeFixBanner: View {
    let pr: PullRequestDetail
    let action: () -> Void

    var body: some View {
        let open = pr.reviewThreads.nodes.filter { !$0.isResolved }
        let unresolved = open.filter { !$0.isOutdated }.count
        let outdated = open.count - unresolved
        let lastCommit = pr.lastCommit.nodes.first?.commit.committedDate
        let found = FixPromptExtractor.latest(in: pr).flatMap { f -> FoundFixPrompt? in
            guard let d = f.date, let lastCommit, lastCommit > d else { return f }
            return unresolved > 0 ? f : nil
        }
        if pr.isOpen, unresolved > 0 || found != nil {
            HStack(spacing: 8) {
                IconTile(systemImage: "sparkles", color: .syCoral, size: 18)
                if unresolved > 0 {
                    Text(plural(unresolved, "unresolved comment", "unresolved comments"))
                        .foregroundStyle(Color.syText)
                }
                if outdated > 0 {
                    Text("· \(outdated) outdated").foregroundStyle(Color.syDim)
                        .help("Code these comments point to has changed since they were written")
                }
                if let found {
                    if unresolved > 0 { Text("·").foregroundStyle(Color.syDim) }
                    Text("Fix prompt by @\(found.author)" + (found.date.map { " · \(Fmt.rel($0))" } ?? ""))
                        .foregroundStyle(Color.syMuted)
                }
                Spacer()
                Button("Fix with Claude Code", action: action)
                    .controlSize(.small)
                    .buttonStyle(.borderedProminent)
                    .tint(Color.syCoral)
            }
            .font(.callout)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.syCoral.opacity(0.08)))
            .padding(.horizontal, 16)
            .padding(.top, 6)
        }
    }
}

struct ReReviewBanner: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    var inset = true
    let more: () -> Void
    @State private var sending = false
    @State private var requested: [String]?

    var body: some View {
        let stale = model.staleReviews
        let logins = stale.map(\.login)
        let canAct = pr.isOpen && (pr.viewerDidAuthor || pr.viewerCanUpdate)
        Group {
            if let requested {
                banner {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.syGreen)
                    Text("Requested").fontWeight(.semibold).foregroundStyle(Color.syText)
                    Text("· Review re-requested from " + mentions(requested))
                        .foregroundStyle(Color.syMuted)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Spacer()
                }
                .transition(.opacity)
            } else if canAct, !logins.isEmpty {
                banner {
                    IconTile(systemImage: "arrow.triangle.2.circlepath", color: .sySky, size: 18)
                    Text(message(stale))
                        .foregroundStyle(Color.syText)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    if let d = model.lastCommitDate {
                        Text("· \(Fmt.rel(d))").foregroundStyle(Color.syMuted).lineLimit(1)
                    }
                    Spacer()
                    Button("Other…", action: more)
                        .controlSize(.small)
                    Button {
                        send(logins)
                    } label: {
                        HStack(spacing: 5) {
                            if sending {
                                ProgressView().controlSize(.mini)
                            } else {
                                Image(systemName: "arrow.triangle.2.circlepath")
                            }
                            Text(logins.count == 1 ? "Re-request review" : "Re-request review (\(logins.count))")
                        }
                    }
                    .controlSize(.small)
                    .buttonStyle(.borderedProminent)
                    .tint(Color.sySky)
                    .disabled(sending || model.busy)
                }
                .transition(.opacity)
            }
        }
        .animation(Motion.smooth, value: requested)
        .animation(Motion.smooth, value: logins)
    }

    private func banner<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: 8) { content() }
            .font(.callout)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.sySky.opacity(0.08)))
            .padding(.horizontal, inset ? 16 : 0)
            .padding(.top, inset ? 6 : 0)
    }

    private func mentions(_ logins: [String]) -> String {
        logins.map { "@\($0)" }.joined(separator: ", ")
    }

    private func message(_ stale: [(login: String, commits: Int?)]) -> String {
        let who = stale.count == 1 ? "@\(stale[0].login)" : stale.dropLast().map { "@\($0.login)" }.joined(separator: ", ") + " and @\(stale.last!.login)"
        let counts = stale.compactMap(\.commits)
        let what: String
        if counts.count == stale.count, let n = counts.max() {
            what = plural(n, "commit", "commits")
        } else {
            what = "new commits"
        }
        return pr.viewerDidAuthor ? "You pushed \(what) after \(who) reviewed" : "\(what.prefix(1).uppercased() + what.dropFirst()) pushed after \(who) reviewed"
    }

    private func send(_ logins: [String]) {
        sending = true
        Task {
            let ok = await model.requestReviewers(logins)
            sending = false
            guard ok else { return }
            requested = logins
            try? await Task.sleep(nanoseconds: 2_500_000_000)
            requested = nil
        }
    }
}
