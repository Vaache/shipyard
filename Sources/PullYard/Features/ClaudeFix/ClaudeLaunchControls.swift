import SwiftUI

extension ClaudeLauncher {
    @MainActor
    @discardableResult
    static func start(app: AppModel, prompt: String, folder: String?, target: ClaudeTarget, mode: ClaudePermissionMode, name: String) throws -> ClaudeTab? {
        if target == .app, let folder {
            let tab = app.claude.newSession(folder: folder, prompt: prompt, name: name)
            app.section = .claude
            return tab
        }
        try launch(prompt: prompt, folder: folder ?? "", target: target, mode: mode, sessionName: name, configDir: ClaudeProfiles.shared.activeOverride)
        return nil
    }
}

struct ClaudeFolderRow: View {
    let repo: RepoRef
    @Binding var folder: String?
    let detecting: Bool

    private var folderMatches: Bool {
        guard let folder else { return false }
        return LocalRepoStore.remoteMatches(folder, repo: repo)
    }

    var body: some View {
        HStack(spacing: 8) {
            Image(systemName: "folder.fill").foregroundStyle(Color.syTeal)
            if detecting {
                ProgressView().controlSize(.small)
                Text("Looking for a local clone of \(repo.full)…").font(.callout).foregroundStyle(Color.syMuted)
            } else if let folder {
                Text(folder.replacingOccurrences(of: FileManager.default.homeDirectoryForCurrentUser.path, with: "~"))
                    .font(.system(.callout, design: .monospaced))
                    .foregroundStyle(Color.syText)
                    .lineLimit(1)
                    .truncationMode(.middle)
                if !folderMatches {
                    Label("remote doesn't match \(repo.full)", systemImage: "exclamationmark.triangle.fill")
                        .font(.caption)
                        .foregroundStyle(Color.syAmber)
                }
            } else {
                Text("No local clone found").font(.callout).foregroundStyle(Color.syAmber)
                Button("Copy “gh repo clone”") { copyToPasteboard("gh repo clone \(repo.full)") }
                    .buttonStyle(.link)
                    .font(.caption)
            }
            Spacer()
            Button("Choose folder…") {
                if let p = LocalRepoStore.choose(repo) { folder = p }
            }
            .controlSize(.small)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .card(radius: 8)
    }
}

struct ClaudeLaunchRow: View {
    @Binding var target: ClaudeTarget
    @Binding var mode: ClaudePermissionMode
    let canLaunch: Bool
    let onLaunch: () -> Void
    @Environment(\.dismiss) private var dismiss

    var body: some View {
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
            Button(target == .copy ? "Copy" : "Launch", action: onLaunch)
                .buttonStyle(.borderedProminent)
                .tint(Color.syCoral)
                .keyboardShortcut(.return, modifiers: .command)
                .disabled(!canLaunch)
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
}
