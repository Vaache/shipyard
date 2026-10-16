import SwiftUI

struct CIFixRequest: Identifiable {
    let id = UUID()
    var focus: String?
}

struct CIFixLaunch {
    let tabID: UUID?
    let target: ClaudeTarget
    let headOid: String
    let at: Date
}

struct CIFailureBanner: View {
    let model: PRDetailModel
    let pr: PullRequestDetail
    let onFix: () -> Void
    let onRerun: () -> Void
    @Environment(AppModel.self) private var app

    var body: some View {
        let failing = CIFailureCollector.failing(pr.checks)
        if pr.isOpen, !failing.isEmpty {
            let launch = app.claude.ciFixes[model.ref].flatMap { $0.headOid == pr.headRefOid ? $0 : nil }
            let tab = launch?.tabID.flatMap { id in app.claude.tabs.first { $0.id == id } }
            HStack(spacing: 8) {
                IconTile(systemImage: "xmark.octagon.fill", color: .syRed, size: 18)
                Text(plural(failing.count, "check failing", "checks failing"))
                    .foregroundStyle(Color.syText)
                if failing.count == 1, let only = failing.first {
                    Text("· " + (only.workflowName.map { "\($0) / \(only.title)" } ?? only.title))
                        .foregroundStyle(Color.syMuted)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
                if let launch { status(launch, tab: tab) }
                Spacer()
                if model.canWrite, failing.contains(where: { $0.runId != nil }) {
                    Button("Re-run failed…", action: onRerun)
                        .controlSize(.small)
                        .disabled(model.busy)
                }
                if let tab {
                    Button("Open session") {
                        app.claude.selectedTabID = tab.id
                        app.section = .claude
                    }
                    .controlSize(.small)
                }
                Button(launch == nil ? "Fix with Claude Code" : "Fix again", action: onFix)
                    .controlSize(.small)
                    .buttonStyle(.borderedProminent)
                    .tint(Color.syCoral)
            }
            .font(.callout)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.syRed.opacity(0.07)))
            .padding(.horizontal, 16)
            .padding(.top, 6)
        }
    }

    @ViewBuilder
    private func status(_ launch: CIFixLaunch, tab: ClaudeTab?) -> some View {
        if let tab {
            let state = tab.state
            ChipView(text: label(state), color: state.color, icon: state == .working ? "sparkles" : state.icon)
                .help("Claude Code session started \(Fmt.rel(launch.at))")
        } else if launch.tabID != nil {
            Text("· Fix session closed").foregroundStyle(Color.syDim)
        } else {
            Text("· Fix launched in \(launch.target.title) \(Fmt.rel(launch.at))")
                .foregroundStyle(Color.syDim)
                .lineLimit(1)
        }
    }

    private func label(_ state: ClaudeTabState) -> String {
        switch state {
        case .working, .active: return "Fix running"
        case .waiting: return "Fix needs input"
        case .sleeping: return "Fix session asleep"
        case .stopped: return "Fix session ended"
        }
    }
}
