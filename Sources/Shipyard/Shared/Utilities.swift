import AppKit
import SwiftUI

enum GitHubURL {
    private static let reserved: Set<String> = ["orgs", "settings", "notifications", "users", "marketplace", "features", "pulls", "issues", "search", "login", "logout", "apps", "sponsors", "topics", "explore", "new", "codespaces", "organizations", "enterprise", "about", "pricing", "collections", "trending"]

    static func parse(_ raw: String, allowShort: Bool = true) -> Route? {
        let s = raw.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !s.isEmpty, !s.contains(" ") else { return nil }
        if allowShort, !s.contains("://"), let hash = s.firstIndex(of: "#") {
            let repoPart = String(s[..<hash])
            let num = Int(s[s.index(after: hash)...])
            if let num, let repo = RepoRef(full: repoPart), !repo.name.contains("/") {
                return .pr(ItemRef(repo: repo, number: num))
            }
            return nil
        }
        guard let url = URL(string: s.hasPrefix("http") ? s : "https://" + s), let host = url.host?.lowercased() else { return nil }
        let parts = url.pathComponents.filter { $0 != "/" }
        if host == "github.com" || host == "www.github.com" {
            guard parts.count >= 2, !reserved.contains(parts[0].lowercased()) else { return nil }
            let repo = RepoRef(owner: parts[0], name: parts[1])
            if parts.count >= 4 {
                if let n = Int(parts[3]) {
                    if parts[2] == "pull" { return .pr(ItemRef(repo: repo, number: n)) }
                    if parts[2] == "issues" { return .issue(ItemRef(repo: repo, number: n)) }
                }
                if parts[2] == "commit" { return .commit(repo, parts[3]) }
                return nil
            }
            if parts.count == 2 { return .repo(repo) }
            if parts.count == 3, parts[2] == "pulls" || parts[2] == "issues" { return .repo(repo) }
            return nil
        }
        if host == "api.github.com", parts.count >= 5, parts[0] == "repos", let n = Int(parts[4]) {
            let repo = RepoRef(owner: parts[1], name: parts[2])
            if parts[3] == "pulls" { return .pr(ItemRef(repo: repo, number: n)) }
            if parts[3] == "issues" { return .issue(ItemRef(repo: repo, number: n)) }
        }
        return nil
    }
}

enum Fmt {
    private static let relative: RelativeDateTimeFormatter = {
        let f = RelativeDateTimeFormatter()
        f.locale = .app
        f.unitsStyle = .short
        return f
    }()

    static let full: DateFormatter = {
        let f = DateFormatter()
        f.locale = .app
        f.dateStyle = .medium
        f.timeStyle = .short
        return f
    }()

    static func rel(_ date: Date?) -> String {
        guard let date else { return "" }
        if abs(date.timeIntervalSinceNow) < 45 { return "just now" }
        return relative.localizedString(for: date, relativeTo: Date())
    }
}

func openInBrowser(_ s: String?) {
    guard let s, let u = URL(string: s) else { return }
    LinkBrowser.open(u)
}

func copyToPasteboard(_ s: String) {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(s, forType: .string)
}

extension Color {
    init(hex: String) {
        var s = hex.trimmingCharacters(in: CharacterSet(charactersIn: "#"))
        if s.count == 3 { s = s.map { "\($0)\($0)" }.joined() }
        let v = UInt64(s, radix: 16) ?? 0x888888
        self.init(red: Double((v >> 16) & 0xff) / 255, green: Double((v >> 8) & 0xff) / 255, blue: Double(v & 0xff) / 255)
    }

    static var ghGreen: Color { .syGreen }
    static var ghPurple: Color { .syViolet }
    static var ghRed: Color { .syRed }
    static var ghYellow: Color { .syAmber }
}

enum VisualState {
    case open, draft, merged, closed, issueOpen, completed, notPlanned

    static func pr(state: String, draft: Bool) -> VisualState {
        switch state {
        case "MERGED": return .merged
        case "CLOSED": return .closed
        default: return draft ? .draft : .open
        }
    }

    static func issue(state: String, reason: String?) -> VisualState {
        guard state == "CLOSED" else { return .issueOpen }
        return reason == "NOT_PLANNED" ? .notPlanned : .completed
    }

    var color: Color {
        switch self {
        case .open, .issueOpen: return .ghGreen
        case .draft, .notPlanned: return .secondary
        case .merged, .completed: return .ghPurple
        case .closed: return .ghRed
        }
    }

    var icon: String {
        switch self {
        case .open, .draft: return "arrow.triangle.pull"
        case .merged: return "arrow.triangle.merge"
        case .closed: return "xmark.circle"
        case .issueOpen: return "smallcircle.filled.circle"
        case .completed: return "checkmark.circle"
        case .notPlanned: return "slash.circle"
        }
    }

    var title: String {
        switch self {
        case .open, .issueOpen: return "Open"
        case .draft: return "Draft"
        case .merged: return "Merged"
        case .closed: return "Closed"
        case .completed: return "Completed"
        case .notPlanned: return "Not planned"
        }
    }
}

struct Avatar: View {
    let url: String?
    var size: CGFloat = 20

    var body: some View {
        AsyncImage(url: resolved) { image in
            image.resizable().interpolation(.medium)
        } placeholder: {
            Circle().fill(.quaternary)
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
    }

    private var resolved: URL? {
        guard let url, var c = URLComponents(string: url) else { return nil }
        let px = Int(size * 2)
        var items = c.queryItems?.filter { $0.name != "s" } ?? []
        items.append(URLQueryItem(name: "s", value: "\(px)"))
        c.queryItems = items
        return c.url
    }
}

struct StateBadge: View {
    let state: VisualState
    var compact = false

    var body: some View {
        if compact {
            Image(systemName: state.icon)
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(state.color)
                .frame(width: 22, height: 22)
                .background(Circle().fill(state.color.opacity(0.15)))
        } else {
            Label(state.title, systemImage: state.icon)
                .font(.callout.weight(.semibold))
                .foregroundStyle(.white)
                .padding(.horizontal, 10)
                .padding(.vertical, 3)
                .background(Capsule().fill((state.color == .secondary ? Color.sySlate : state.color).gradient))
        }
    }
}

struct LabelChip: View {
    let label: LabelInfo

    var body: some View {
        let c = Color(hex: label.color)
        Text(label.name)
            .font(.caption2.weight(.medium))
            .lineLimit(1)
            .padding(.horizontal, 7)
            .padding(.vertical, 1.5)
            .background(Capsule().fill(c.opacity(0.22)))
            .overlay(Capsule().stroke(c.opacity(0.7), lineWidth: 0.5))
    }
}

struct CIIcon: View {
    let state: String?

    var body: some View {
        switch state {
        case "SUCCESS": Image(systemName: "checkmark").foregroundStyle(Color.ghGreen)
        case "FAILURE", "ERROR": Image(systemName: "xmark").foregroundStyle(Color.ghRed)
        case "PENDING", "EXPECTED": Image(systemName: "circle.dotted").foregroundStyle(Color.ghYellow)
        default: EmptyView()
        }
    }
}

struct OutcomeIcon: View {
    let outcome: CheckOutcome

    var body: some View {
        switch outcome {
        case .success: Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.ghGreen)
        case .failure: Image(systemName: "xmark.circle.fill").foregroundStyle(Color.ghRed)
        case .pending: Image(systemName: "clock.fill").foregroundStyle(Color.ghYellow)
        case .neutral: Image(systemName: "minus.circle.fill").foregroundStyle(Color.syMuted)
        case .skipped: Image(systemName: "arrow.uturn.right.circle").foregroundStyle(Color.syMuted)
        }
    }
}

struct DiffStat: View {
    let additions: Int
    let deletions: Int

    var body: some View {
        HStack(spacing: 4) {
            Text("+\(additions)").foregroundStyle(Color.ghGreen)
            Text("−\(deletions)").foregroundStyle(Color.ghRed)
        }
        .font(.caption.monospacedDigit())
    }
}

struct ErrorStateView: View {
    let message: String
    var retry: (() -> Void)?

    var body: some View {
        VStack(spacing: 10) {
            IconTile(systemImage: "exclamationmark.triangle.fill", color: .syAmber, size: 44)
            Text(message).multilineTextAlignment(.center).textSelection(.enabled).foregroundStyle(Color.syMuted)
            if let retry { Button("Retry", action: retry) }
        }
        .padding(30)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

struct LoadingView: View {
    @State private var visible = false

    var body: some View {
        ProgressView()
            .controlSize(.small)
            .opacity(visible ? 1 : 0)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .task {
                try? await Task.sleep(nanoseconds: 180_000_000)
                withAnimation(Motion.fade) { visible = true }
            }
    }
}

struct MarkdownText: View {
    let text: String

    var body: some View {
        Text(attributed).textSelection(.enabled)
    }

    private var attributed: AttributedString {
        (try? AttributedString(markdown: text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(text)
    }
}

struct ComposerBox: View {
    @Binding var text: String
    var placeholder: String
    var minHeight: CGFloat = 60

    var body: some View {
        ZStack(alignment: .topLeading) {
            TextEditor(text: $text)
                .font(.body)
                .scrollContentBackground(.hidden)
                .padding(4)
            if text.isEmpty {
                Text(placeholder).foregroundStyle(Color.syDim).padding(.horizontal, 9).padding(.vertical, 4).allowsHitTesting(false)
            }
        }
        .frame(minHeight: minHeight)
        .background(RoundedRectangle(cornerRadius: 8).fill(Color.sySunken))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.syLineStrong))
    }
}

func isCancellation(_ error: Error) -> Bool {
    if error is CancellationError { return true }
    if let u = error as? URLError, u.code == .cancelled { return true }
    let ns = error as NSError
    return ns.domain == NSURLErrorDomain && ns.code == NSURLErrorCancelled
}
