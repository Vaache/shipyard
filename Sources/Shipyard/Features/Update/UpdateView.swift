import AppKit
import SwiftUI

struct UpdateView: View {
    @Environment(\.dismissWindow) private var dismissWindow
    private var updater: Updater { Updater.shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            header
            notes
            if case .downloading(let progress) = updater.state {
                progressRow(progress)
                    .transition(.opacity)
            }
            if case .failed(let message) = updater.state {
                errorRow(message)
                    .transition(.rise)
            }
            if case .manualInstall = updater.state {
                Label("Shipyard can't replace itself in its current location. The disk image is open in Finder — drag Shipyard to Applications.", systemImage: "info.circle")
                    .font(.callout)
                    .foregroundStyle(Color.syMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .transition(.rise)
            }
            buttons
        }
        .padding(20)
        .frame(width: 520, height: 420)
        .surface(.panel)
        .animation(Motion.smooth, value: updater.state)
        .task {
            if updater.state == .idle { await updater.check(userInitiated: true) }
        }
    }

    private var header: some View {
        HStack(spacing: 14) {
            Image(nsImage: NSApp.applicationIconImage)
                .resizable()
                .frame(width: 64, height: 64)
            VStack(alignment: .leading, spacing: 4) {
                Text(title)
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(Color.syText)
                Text("You have \(updater.currentVersion)")
                    .foregroundStyle(Color.syMuted)
                if let release = updater.release, let date = release.publishedAt, showsRelease {
                    Text(Fmt.full.string(from: date))
                        .font(.caption)
                        .foregroundStyle(Color.syDim)
                }
            }
            Spacer(minLength: 0)
            if updater.state == .checking || updater.state == .installing {
                ProgressView().controlSize(.small)
            }
        }
    }

    private var title: String {
        switch updater.state {
        case .idle, .checking: return "Checking for updates…"
        case .upToDate: return "You're up to date"
        case .failed: return updater.release.map { "Shipyard \($0.version) is available" } ?? "Couldn't check for updates"
        case .installing: return "Preparing update…"
        case .readyToRelaunch: return "Shipyard \(updater.release?.version ?? "") is ready to install"
        case .available, .downloading, .manualInstall:
            return "Shipyard \(updater.release?.version ?? "") is available"
        }
    }

    private var showsRelease: Bool {
        switch updater.state {
        case .idle, .checking, .upToDate: return false
        case .failed: return updater.release.map { Updater.compare($0.version, updater.currentVersion) == .orderedDescending } ?? false
        default: return true
        }
    }

    @ViewBuilder
    private var notes: some View {
        ScrollView {
            Group {
                if showsRelease, let release = updater.release {
                    VStack(alignment: .leading, spacing: 8) {
                        Text(release.name).font(.headline).foregroundStyle(Color.syText)
                        if release.notes.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            Text("No release notes.").foregroundStyle(Color.syMuted)
                        } else {
                            MarkdownView(text: release.notes)
                        }
                    }
                } else if updater.state == .upToDate {
                    Text("Shipyard \(updater.currentVersion) is the newest version available.")
                        .foregroundStyle(Color.syMuted)
                } else {
                    Color.clear.frame(height: 1)
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(12)
        }
        .frame(maxHeight: .infinity)
        .card(radius: 10, fill: .sySunken)
    }

    private func progressRow(_ progress: Double) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            ProgressView(value: progress)
                .tint(Color.syAccent)
            HStack {
                Text("Downloading…")
                Spacer()
                if let size = updater.release?.dmgSize, size > 0 {
                    Text("\(ByteCountFormatter.string(fromByteCount: Int64(Double(size) * progress), countStyle: .file)) / \(ByteCountFormatter.string(fromByteCount: size, countStyle: .file))")
                        .monospacedDigit()
                }
            }
            .font(.caption)
            .foregroundStyle(Color.syMuted)
        }
    }

    private func errorRow(_ message: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: "exclamationmark.triangle.fill")
            Text(message)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
            Spacer(minLength: 0)
            Button("View on GitHub") { openInBrowser(updater.pageURL) }
                .buttonStyle(.link)
        }
        .font(.callout)
        .foregroundStyle(Color.syRed)
    }

    @ViewBuilder
    private var buttons: some View {
        HStack(spacing: 8) {
            switch updater.state {
            case .available, .downloading:
                installButtons
            case .failed where showsRelease:
                installButtons
            case .installing:
                Spacer()
                Button("Later") { close() }
                    .disabled(true)
            case .readyToRelaunch:
                Spacer()
                Button("Later") { close() }
                    .keyboardShortcut(.cancelAction)
                Button("Restart to Update") { updater.relaunch() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            case .manualInstall:
                Button("View on GitHub") { openInBrowser(updater.pageURL) }
                Spacer()
                Button("Done") { close() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
            default:
                Spacer()
                if case .failed = updater.state {
                    Button("Try Again") {
                        Task { await updater.check(userInitiated: true) }
                    }
                }
                Button("OK") { close() }
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(updater.state == .checking)
            }
        }
    }

    @ViewBuilder
    private var installButtons: some View {
        Button("Skip This Version") {
            updater.skip()
            close()
        }
        .disabled(updater.isBusy)
        Spacer()
        Button("Later") { close() }
            .keyboardShortcut(.cancelAction)
        Button("Download & Install") {
            Task { await updater.downloadAndInstall() }
        }
        .buttonStyle(.borderedProminent)
        .keyboardShortcut(.defaultAction)
        .disabled(updater.isBusy)
    }

    private func close() {
        updater.later()
        dismissWindow(id: "update")
    }
}

struct CheckForUpdatesButton: View {
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Button("Check for Updates…") {
            openWindow(id: "update")
            Task { await Updater.shared.check(userInitiated: true) }
        }
    }
}
