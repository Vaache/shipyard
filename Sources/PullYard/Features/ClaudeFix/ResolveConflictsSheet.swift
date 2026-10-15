import SwiftUI

struct ResolveConflictsSheet: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    @Environment(\.dismiss) private var dismiss
    @Environment(AppModel.self) private var app
    @AppStorage("claudeTarget.v2") private var target: ClaudeTarget = .app
    @AppStorage("claudeMode") private var mode: ClaudePermissionMode = .standard
    @State private var push = true
    @State private var custom: String?
    @State private var folder: String?
    @State private var detecting = true
    @State private var error: String?

    private var files: [String] { model.conflicts?.files ?? [] }
    private var generated: String { ConflictPrompt.build(pr: pr, repo: model.ref.repo, files: files, push: push) }
    private var prompt: String { custom ?? generated }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                IconTile(systemImage: "arrow.triangle.merge", color: .syCoral, size: 30)
                VStack(alignment: .leading, spacing: 1) {
                    Text("Resolve Conflicts with Claude Code").font(.headline).foregroundStyle(Color.syText)
                    HStack(spacing: 6) {
                        Text("#\(pr.number)").foregroundStyle(Color.syViolet).fontWeight(.semibold)
                        Text(pr.title).foregroundStyle(Color.syMuted).lineLimit(1)
                    }
                    .font(.callout)
                }
                Spacer()
                HStack(spacing: 6) {
                    BranchChip(name: pr.baseRefName)
                    Image(systemName: "arrow.left").font(.caption).foregroundStyle(Color.syDim)
                    BranchChip(name: pr.headRefName, color: .syViolet)
                }
            }
            fileList
            Toggle("Push when done", isOn: $push).toggleStyle(.checkbox)
            promptEditor
            ClaudeFolderRow(repo: model.ref.repo, folder: $folder, detecting: detecting)
            if let error { Text(error).font(.caption).foregroundStyle(Color.syRed) }
            ClaudeLaunchRow(target: $target, mode: $mode, canLaunch: !prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && (target == .copy || folder != nil), onLaunch: launch)
        }
        .padding(20)
        .frame(width: 780, height: 640)
        .surface(.panel)
        .onChange(of: push) { custom = nil }
        .onChange(of: files) { custom = nil }
        .task {
            folder = await LocalRepoStore.detect(model.ref.repo)
            detecting = false
        }
    }

    private var fileList: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text(files.isEmpty ? "Conflicting files" : "Conflicting files (\(files.count))")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(Color.syMuted)
                Spacer()
                if model.conflicts == .checking { ProgressView().controlSize(.mini) }
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 4) {
                    if files.isEmpty {
                        Text(model.conflicts == .checking ? "Checking which files conflict…" : "File list unavailable — Claude will find the conflicts while merging")
                            .font(.callout)
                            .foregroundStyle(Color.syDim)
                    }
                    ForEach(files, id: \.self) { f in
                        HStack(spacing: 6) {
                            Image(systemName: "exclamationmark.triangle.fill").font(.caption2).foregroundStyle(Color.syRed)
                            Text(f)
                                .font(.system(.caption, design: .monospaced))
                                .foregroundStyle(Color.sySky)
                                .lineLimit(1)
                                .truncationMode(.head)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(8)
            }
            .frame(maxHeight: 110)
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

    private func launch() {
        do {
            try ClaudeLauncher.start(app: app, prompt: prompt, folder: folder, target: target, mode: mode, name: "Resolve conflicts PR #\(pr.number): \(pr.title)")
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
