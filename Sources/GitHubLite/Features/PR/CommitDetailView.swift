import SwiftUI

struct CommitDetailView: View {
    let repo: RepoRef
    let sha: String
    @Environment(AppModel.self) private var app
    @State private var commit: CommitDetailREST?
    @State private var files: [DiffFile] = []
    @State private var collapsed: Set<String> = []
    @State private var error: String?

    var body: some View {
        Group {
            if let commit {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0, pinnedViews: [.sectionHeaders]) {
                        header(commit)
                        ForEach(files) { file in
                            Section {
                                if !collapsed.contains(file.id) {
                                    if file.hasPatch {
                                        ForEach(file.lines) { DiffLineView(line: $0) }
                                    } else {
                                        Text(t("Diff недоступен", "Diff unavailable")).font(.caption).foregroundStyle(.secondary).padding(12)
                                    }
                                }
                            } header: {
                                HStack(spacing: 8) {
                                    Button {
                                        if collapsed.contains(file.id) { collapsed.remove(file.id) } else { collapsed.insert(file.id) }
                                    } label: {
                                        Image(systemName: collapsed.contains(file.id) ? "chevron.right" : "chevron.down").frame(width: 12)
                                    }
                                    .buttonStyle(.plain)
                                    Text(file.id).font(.system(.callout, design: .monospaced).weight(.medium)).lineLimit(1).truncationMode(.head)
                                    DiffStat(additions: file.additions, deletions: file.deletions)
                                    Spacer()
                                }
                                .padding(.horizontal, 12)
                                .padding(.vertical, 6)
                                .background(.bar)
                                .overlay(alignment: .bottom) { Divider() }
                            }
                        }
                    }
                    .padding(.bottom, 30)
                }
            } else if let error {
                ErrorStateView(message: error) { Task { await load() } }
            } else {
                LoadingView()
            }
        }
        .navigationTitle(String(sha.prefix(7)))
        .navigationSubtitle(repo.full)
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                Button { copyToPasteboard(sha) } label: { Label(t("Копировать SHA", "Copy SHA"), systemImage: "doc.on.doc") }
                if let url = commit?.htmlUrl {
                    Button { openInBrowser(url) } label: { Label(t("В браузере", "Open in browser"), systemImage: "safari") }
                }
            }
        }
        .task(id: sha) { await load() }
    }

    private func header(_ c: CommitDetailREST) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            let lines = c.commit.message.components(separatedBy: "\n")
            Text(lines.first ?? "").font(.title3.weight(.semibold)).textSelection(.enabled)
            let rest = lines.dropFirst().joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
            if !rest.isEmpty {
                Text(rest).font(.system(.callout, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
            }
            HStack(spacing: 8) {
                Avatar(url: c.author?.avatarUrl, size: 18)
                Text(c.author?.login ?? c.commit.author?.name ?? "").fontWeight(.semibold)
                Text(Fmt.rel(c.commit.author?.date)).foregroundStyle(.secondary)
                Text(c.sha).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary).textSelection(.enabled)
                if let s = c.stats { DiffStat(additions: s.additions, deletions: s.deletions) }
            }
            .font(.callout)
        }
        .padding(16)
    }

    private func load() async {
        guard let api = app.api else { return }
        do {
            let c = try await api.commit(repo, sha: sha)
            let raw = c.files ?? []
            files = await Task.detached { raw.map(DiffFile.init) }.value
            collapsed = Set(files.filter { $0.lines.count > 1500 }.map(\.id))
            commit = c
        } catch {
            self.error = error.localizedDescription
        }
    }
}
