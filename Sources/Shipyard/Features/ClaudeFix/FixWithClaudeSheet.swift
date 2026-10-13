import SwiftUI

enum FixSource: Hashable {
    case reviewer, composed
}

struct FixWithClaudeSheet: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
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

    private var threads: [ReviewThread] { pr.reviewThreads.nodes.filter { !$0.isResolved } }
    private var general: [GeneralFeedback] { FixPrompt.generalFeedback(pr) }

    private var generated: String {
        if source == .reviewer, let found {
            return FixPromptExtractor.wrap(found, pr: pr, options: options)
        }
        let chosen = threads.filter { selected.contains($0.id) }
        return FixPrompt.build(pr: pr, repo: model.ref.repo, threads: chosen, general: general, options: options)
    }

    private var prompt: String { custom ?? generated }

    private var folderMatches: Bool {
        guard let folder else { return false }
        return LocalRepoStore.remoteMatches(folder, repo: model.ref.repo)
    }

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
            HStack(spacing: 16) {
                Toggle("Commit", isOn: $options.commit)
                Toggle("Push", isOn: $options.push)
                Toggle("Reply & resolve threads", isOn: $options.replyAndResolve)
                    .disabled(!options.push)
            }
            .toggleStyle(.checkbox)
            .onChange(of: options.push) { _, push in if !push { options.replyAndResolve = false } }
            promptEditor
            folderRow
            if let error { Text(error).font(.caption).foregroundStyle(Color.syRed) }
            launchRow
        }
        .padding(20)
        .frame(width: 780, height: 680)
        .surface(.panel)
        .onChange(of: source) { custom = nil }
        .onChange(of: selected) { custom = nil }
        .onChange(of: options.commit) { custom = nil }
        .onChange(of: options.push) { custom = nil }
        .onChange(of: options.replyAndResolve) { custom = nil }
        .onChange(of: options.includeGeneral) { custom = nil }
        .task {
            found = FixPromptExtractor.latest(in: pr)
            source = found != nil ? .reviewer : .composed
            selected = Set(threads.filter { !$0.isOutdated }.map(\.id))
            if selected.isEmpty { selected = Set(threads.map(\.id)) }
            folder = await LocalRepoStore.detect(model.ref.repo)
            detecting = false
        }
    }

    @ViewBuilder
    private var sourcePicker: some View {
        if let found {
            Picker("", selection: $source) {
                Text("Fix prompt by @\(found.author)" + (found.date.map { " · \(Fmt.rel($0))" } ?? "")).tag(FixSource.reviewer)
                Text("Build from comments" + " (\(threads.count))").tag(FixSource.composed)
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

    private var promptEditor: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Prompt").font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                if custom != nil {
                    ChipView(text: "edited", color: .syAmber)
                    Button("Reset") { custom = nil }.buttonStyle(.link).font(.caption)
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

    private var folderRow: some View {
        HStack(spacing: 8) {
            Image(systemName: "folder.fill").foregroundStyle(Color.syTeal)
            if detecting {
                ProgressView().controlSize(.small)
                Text("Looking for a local clone of \(model.ref.repo.full)…").font(.callout).foregroundStyle(Color.syMuted)
            } else if let folder {
                Text(folder.replacingOccurrences(of: FileManager.default.homeDirectoryForCurrentUser.path, with: "~"))
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(Color.syText)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if !folderMatches {
                    Label("remote doesn't match \(model.ref.repo.full)", systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(Color.syAmber)
                }
            } else {
                Text("No local clone found").font(.callout).foregroundStyle(Color.syAmber)
                Button("Copy “gh repo clone”") { copyToPasteboard("gh repo clone \(model.ref.repo.full)") }
                    .buttonStyle(.link)
                    .font(.caption)
            }
            Spacer()
            Button("Choose folder…") {
                if let p = LocalRepoStore.choose(model.ref.repo) { folder = p }
            }
            .controlSize(.small)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .card(radius: 8)
    }

    private var launchRow: some View {
        HStack(spacing: 10) {
            Picker("Open in", selection: $target) {
                ForEach(ClaudeTarget.allCases.filter(\.available)) { Text($0.title).tag($0) }
            }
            .frame(width: 220)
            Picker("Mode", selection: $mode) {
                ForEach(ClaudePermissionMode.allCases) { Text($0.title).tag($0) }
            }
            .frame(width: 250)
            .disabled(target == .desktop || target == .copy || target == .app)
            Spacer()
            Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            Button(target == .copy ? "Copy" : "Launch") { launch() }
                .buttonStyle(.borderedProminent)
                .tint(Color.syCoral)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || (target != .copy && folder == nil))
        }
        .overlay(alignment: .topLeading) {
            if target == .desktop {
                Text("Prompt will be copied — paste it into the new session (⌘V)")
                    .font(.caption)
                    .foregroundStyle(Color.syMuted)
                    .offset(y: -18)
            }
        }
    }

    private func launch() {
        if target == .app, let folder {
            app.claude.newSession(folder: folder, prompt: prompt, name: "Fix PR #\(pr.number): \(pr.title)")
            app.section = .claude
            dismiss()
            return
        }
        do {
            try ClaudeLauncher.launch(prompt: prompt, folder: folder ?? "", target: target, mode: mode, sessionName: "Fix PR #\(pr.number): \(pr.title)", configDir: ClaudeProfiles.shared.activeOverride)
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
        let unresolved = pr.reviewThreads.nodes.filter { !$0.isResolved }.count
        let found = FixPromptExtractor.latest(in: pr)
        if pr.isOpen, unresolved > 0 || found != nil {
            HStack(spacing: 8) {
                IconTile(systemImage: "sparkles", color: .syCoral, size: 18)
                if unresolved > 0 {
                    Text(plural(unresolved, "unresolved comment", "unresolved comments"))
                        .foregroundStyle(Color.syText)
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
            .overlay(RoundedRectangle(cornerRadius: 10).strokeBorder(Color.syCoral.opacity(0.28)))
            .padding(.horizontal, 16)
            .padding(.top, 6)
        }
    }
}
