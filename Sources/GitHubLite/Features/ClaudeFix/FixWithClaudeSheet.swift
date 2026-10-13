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
            HStack(spacing: 8) {
                Image(systemName: "sparkles").foregroundStyle(Color.accentColor)
                Text(t("Исправить с Claude Code", "Fix with Claude Code")).font(.headline)
                Text("#\(pr.number) \(pr.title)").foregroundStyle(.secondary).lineLimit(1)
            }
            sourcePicker
            if source == .composed { composedOptions }
            HStack(spacing: 16) {
                Toggle(t("Коммит", "Commit"), isOn: $options.commit)
                Toggle(t("Пуш", "Push"), isOn: $options.push)
                Toggle(t("Ответить в тредах и решить", "Reply & resolve threads"), isOn: $options.replyAndResolve)
                    .disabled(!options.push)
            }
            .toggleStyle(.checkbox)
            .onChange(of: options.push) { _, push in if !push { options.replyAndResolve = false } }
            promptEditor
            folderRow
            if let error { Text(error).font(.caption).foregroundStyle(.red) }
            launchRow
        }
        .padding(20)
        .frame(width: 780, height: 680)
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
                Text(t("Fix prompt от @\(found.author)", "Fix prompt by @\(found.author)") + (found.date.map { " · \(Fmt.rel($0))" } ?? "")).tag(FixSource.reviewer)
                Text(t("Собрать из комментариев", "Build from comments") + " (\(threads.count))").tag(FixSource.composed)
            }
            .pickerStyle(.segmented)
            .labelsHidden()
        } else {
            Label(t("Fix prompt в PR не найден — промпт собран из нерешённых комментариев", "No Fix prompt in this PR — built from unresolved comments"), systemImage: "info.circle")
                .font(.callout)
                .foregroundStyle(.secondary)
        }
    }

    private var composedOptions: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(t("Нерешённые комментарии", "Unresolved comments")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                Spacer()
                Button(t("Все", "All")) { selected = Set(threads.map(\.id)) }.buttonStyle(.link).font(.caption)
                Button(t("Никаких", "None")) { selected = [] }.buttonStyle(.link).font(.caption)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    if threads.isEmpty {
                        Text(t("Нерешённых комментариев нет", "No unresolved comments")).font(.callout).foregroundStyle(.tertiary)
                    }
                    ForEach(threads) { th in
                        Toggle(isOn: Binding(get: { selected.contains(th.id) }, set: { on in if on { selected.insert(th.id) } else { selected.remove(th.id) } })) {
                            HStack(spacing: 6) {
                                Text("\(th.path)\((th.line ?? th.originalLine).map { ":\($0)" } ?? "")")
                                    .font(.system(.caption, design: .monospaced))
                                    .lineLimit(1)
                                    .truncationMode(.head)
                                if th.isOutdated { Text(t("устарело", "outdated")).font(.caption2).foregroundStyle(.secondary) }
                                Text("@\(th.comments.nodes.first?.author?.login ?? "?"): \(th.comments.nodes.first?.body.replacingOccurrences(of: "\n", with: " ") ?? "")")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        .toggleStyle(.checkbox)
                    }
                    if !general.isEmpty {
                        Toggle(t("Общие замечания ревью (\(general.count))", "General review feedback (\(general.count))"), isOn: $options.includeGeneral)
                            .toggleStyle(.checkbox)
                            .font(.caption)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .frame(maxHeight: 130)
        }
    }

    private var promptEditor: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(t("Промпт", "Prompt")).font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                if custom != nil {
                    Text(t("изменён", "edited")).font(.caption2).foregroundStyle(.orange)
                    Button(t("Сбросить", "Reset")) { custom = nil }.buttonStyle(.link).font(.caption)
                }
                Spacer()
                Text("\(prompt.count)").font(.caption2.monospacedDigit()).foregroundStyle(.tertiary)
            }
            TextEditor(text: Binding(get: { prompt }, set: { custom = $0 }))
                .font(.system(size: 11.5, design: .monospaced))
                .scrollContentBackground(.hidden)
                .padding(6)
                .background(RoundedRectangle(cornerRadius: 6).fill(Color(nsColor: .textBackgroundColor)))
                .overlay(RoundedRectangle(cornerRadius: 6).stroke(Color(nsColor: .separatorColor)))
        }
    }

    private var folderRow: some View {
        HStack(spacing: 8) {
            Image(systemName: "folder").foregroundStyle(.secondary)
            if detecting {
                ProgressView().controlSize(.small)
                Text(t("Ищу локальную копию \(model.ref.repo.full)…", "Looking for a local clone of \(model.ref.repo.full)…")).font(.callout).foregroundStyle(.secondary)
            } else if let folder {
                Text(folder.replacingOccurrences(of: FileManager.default.homeDirectoryForCurrentUser.path, with: "~"))
                    .font(.system(.callout, design: .monospaced))
                    .lineLimit(1)
                    .truncationMode(.middle)
                if !folderMatches {
                    Label(t("remote не совпадает с \(model.ref.repo.full)", "remote doesn't match \(model.ref.repo.full)"), systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(.orange)
                }
            } else {
                Text(t("Локальная копия не найдена", "No local clone found")).font(.callout).foregroundStyle(.orange)
                Button(t("Копировать «gh repo clone»", "Copy “gh repo clone”")) { copyToPasteboard("gh repo clone \(model.ref.repo.full)") }
                    .buttonStyle(.link)
                    .font(.caption)
            }
            Spacer()
            Button(t("Выбрать папку…", "Choose folder…")) {
                if let p = LocalRepoStore.choose(model.ref.repo) { folder = p }
            }
            .controlSize(.small)
        }
    }

    private var launchRow: some View {
        HStack(spacing: 10) {
            Picker(t("Где", "Open in"), selection: $target) {
                ForEach(ClaudeTarget.allCases.filter(\.available)) { Text($0.title).tag($0) }
            }
            .frame(width: 220)
            Picker(t("Режим", "Mode"), selection: $mode) {
                ForEach(ClaudePermissionMode.allCases) { Text($0.title).tag($0) }
            }
            .frame(width: 250)
            .disabled(target == .desktop || target == .copy || target == .app)
            Spacer()
            Button(t("Отмена", "Cancel")) { dismiss() }.keyboardShortcut(.cancelAction)
            Button(target == .copy ? t("Скопировать", "Copy") : t("Запустить", "Launch")) { launch() }
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || (target != .copy && folder == nil))
        }
        .overlay(alignment: .topLeading) {
            if target == .desktop {
                Text(t("Промпт будет скопирован — вставьте его в новую сессию (⌘V)", "Prompt will be copied — paste it into the new session (⌘V)"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
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
                Image(systemName: "sparkles").foregroundStyle(Color.accentColor)
                if unresolved > 0 {
                    Text(tp(unresolved, "нерешённый комментарий", "нерешённых комментария", "нерешённых комментариев", "unresolved comment", "unresolved comments"))
                }
                if let found {
                    if unresolved > 0 { Text("·").foregroundStyle(.secondary) }
                    Text(t("Fix prompt от @\(found.author)", "Fix prompt by @\(found.author)") + (found.date.map { " · \(Fmt.rel($0))" } ?? ""))
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Button(t("Исправить с Claude Code", "Fix with Claude Code"), action: action)
                    .controlSize(.small)
                    .buttonStyle(.borderedProminent)
            }
            .font(.callout)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.accentColor.opacity(0.08)))
            .overlay(RoundedRectangle(cornerRadius: 8).stroke(Color.accentColor.opacity(0.25)))
            .padding(.horizontal, 16)
            .padding(.top, 6)
        }
    }
}
