import SwiftUI

enum TokenFormat {
    static func short(_ n: Int) -> String {
        if n >= 1_000_000 { return String(format: n % 1_000_000 == 0 ? "%.0fM" : "%.1fM", Double(n) / 1_000_000) }
        if n >= 1000 { return String(format: "%.1fk", Double(n) / 1000) }
        return "\(n)"
    }

    static func color(_ name: String, kind: String) -> Color {
        if kind == "free" { return .clear }
        if kind == "buffer" || kind == "deferred" { return Color.syDim.opacity(0.5) }
        switch name {
        case "claude": return .syCoral
        case "warning": return .syAmber
        case "error": return .syRed
        case "inactive": return Color.syMuted.opacity(0.6)
        case "promptBorder": return .sySlate
        default:
            if name.hasPrefix("green") { return .syGreen }
            if name.hasPrefix("purple") { return .syViolet }
            if name.hasPrefix("blue") { return .syAccent }
            if name.hasPrefix("red") { return .syRed }
            if name.hasPrefix("orange") { return .syAmber }
            if name.hasPrefix("pink") { return .syCoral }
            if name.hasPrefix("cyan") { return .sySky }
            return .syTeal
        }
    }

    static func resets(_ date: Date?) -> String {
        guard let date else { return "" }
        let interval = date.timeIntervalSinceNow
        if interval <= 0 { return "Resetting" }
        if interval < 24 * 3600 {
            let h = Int(interval) / 3600, m = (Int(interval) % 3600) / 60
            return h > 0 ? "Resets in \(h) hr \(m) min" : "Resets in \(m) min"
        }
        let f = DateFormatter()
        f.locale = .app
        f.setLocalizedDateFormatFromTemplate("EEE jj:mm")
        return "Resets " + f.string(from: date)
    }

    static func windowTitle(_ key: String) -> String {
        switch key {
        case "five_hour": return "Session limit"
        case "seven_day": return "Weekly · all models"
        default:
            let model = key.replacingOccurrences(of: "seven_day_", with: "").replacingOccurrences(of: "_", with: " ").capitalized
            return key.hasPrefix("seven_day") ? "Weekly · \(model)" : key.replacingOccurrences(of: "_", with: " ").capitalized
        }
    }
}

struct UsageRingButton: View {
    let chat: ChatSession
    @State private var open = false

    var body: some View {
        let pct = chat.context?.percentage ?? chat.contextPercent ?? 0
        Button {
            chat.refreshContext()
            open.toggle()
        } label: {
            ZStack {
                Circle().stroke(Color.syLineStrong, lineWidth: 2.5)
                Circle()
                    .trim(from: 0, to: CGFloat(min(max(pct, 1), 100)) / 100)
                    .stroke(ringColor(pct), style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                    .rotationEffect(.degrees(-90))
            }
            .frame(width: 16, height: 16)
            .padding(3)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("Context \(pct)% — click for usage and limits")
        .popover(isPresented: $open, arrowEdge: .top) {
            UsageOverlay(chat: chat)
        }
    }

    private func ringColor(_ pct: Int) -> Color {
        pct >= 85 ? .syRed : pct >= 65 ? .syAmber : .sySky
    }
}

struct UsageOverlay: View {
    let chat: ChatSession
    @State private var detailed = false
    private var usage: ClaudeUsageStore { ClaudeUsageStore.shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            contextSection
                .padding(12)
                .card(radius: 10)
            limitsSection
                .padding(12)
                .card(radius: 10)
            HStack {
                Button(detailed ? "Hide breakdown" : "See detailed breakdown") { detailed.toggle() }
                    .controlSize(.small)
                Spacer()
                if chat.totalCost > 0 {
                    Text("Session: " + String(format: "$%.2f", chat.totalCost))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(Color.syMuted)
                }
            }
            if detailed {
                breakdown
                    .padding(12)
                    .card(radius: 10, fill: .sySunken)
            }
        }
        .padding(12)
        .frame(width: 380)
    }

    @ViewBuilder
    private var contextSection: some View {
        if let ctx = chat.context {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text("Context window").fontWeight(.medium).foregroundStyle(Color.syText)
                    Spacer()
                    Text("\(TokenFormat.short(ctx.total)) / \(TokenFormat.short(ctx.max)) (\(ctx.percentage)%)")
                        .monospacedDigit()
                        .foregroundStyle(Color.syMuted)
                }
                .font(.callout)
                ContextBar(ctx: ctx)
                HStack {
                    if ctx.autoCompactEnabled, let threshold = ctx.autoCompactThreshold, threshold > ctx.total {
                        Text("\(TokenFormat.short(threshold - ctx.total)) until auto-compact")
                    } else if !ctx.autoCompactEnabled {
                        Text("Auto-compact is off").foregroundStyle(Color.syMuted)
                    }
                    Spacer()
                    Button("Compact session") { chat.compact() }
                        .controlSize(.small)
                        .disabled(!chat.running || chat.busy)
                }
                .font(.callout)
            }
        } else {
            HStack {
                Text("Context window").foregroundStyle(Color.syMuted)
                Spacer()
                Text(chat.running ? "loading…" : "session is sleeping").foregroundStyle(Color.syDim)
            }
            .font(.callout)
        }
    }

    @ViewBuilder
    private var limitsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text("Your usage limits" + (usage.subscription.map { " · \($0)" } ?? "")).fontWeight(.medium).foregroundStyle(Color.syText)
                Spacer()
                Button { openInBrowser("https://claude.ai/settings/usage") } label: { Image(systemName: "arrow.up.right") }
                    .buttonStyle(.borderless)
                    .help("Open usage page")
            }
            .font(.callout)
            if usage.windows.isEmpty {
                Text("Shows up after Claude's first reply").font(.caption).foregroundStyle(Color.syDim)
            }
            ForEach(usage.windows) { w in
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(TokenFormat.windowTitle(w.key))
                        Spacer()
                        Text(TokenFormat.resets(w.resetsAt)).foregroundStyle(Color.syMuted)
                        Text("\(Int((w.utilization * 100).rounded()))%").monospacedDigit().frame(minWidth: 34, alignment: .trailing)
                    }
                    .font(.callout)
                    ProgressBar(value: w.utilization, color: w.utilization >= 0.9 ? .syRed : w.utilization >= 0.7 ? .syAmber : .sySky)
                }
            }
            if let note = usage.overageNote {
                Label(note, systemImage: "exclamationmark.triangle.fill").font(.caption).foregroundStyle(Color.syAmber)
            }
        }
    }

    @ViewBuilder
    private var breakdown: some View {
        if let ctx = chat.context {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(ctx.categories) { c in
                    HStack(spacing: 8) {
                        RoundedRectangle(cornerRadius: 2)
                            .fill(c.kind == "free" ? Color.syLineStrong : TokenFormat.color(c.color, kind: c.kind))
                            .frame(width: 10, height: 10)
                        Text(c.name + (c.kind == "deferred" ? " (deferred)" : ""))
                            .foregroundStyle(c.kind == "used" ? Color.syText : Color.syMuted)
                        Spacer()
                        Text(TokenFormat.short(c.tokens)).monospacedDigit().foregroundStyle(Color.syMuted)
                    }
                    .font(.caption)
                }
            }
        }
    }
}

struct ContextBar: View {
    let ctx: ContextUsage

    var body: some View {
        GeometryReader { g in
            let used = ctx.categories.filter { $0.kind == "used" || $0.kind == "buffer" }
            HStack(spacing: 1.5) {
                ForEach(used) { c in
                    let w = ctx.max > 0 ? g.size.width * CGFloat(c.tokens) / CGFloat(ctx.max) : 0
                    if w >= 1 {
                        Rectangle().fill(TokenFormat.color(c.color, kind: c.kind)).frame(width: w)
                    }
                }
                Spacer(minLength: 0)
            }
            .frame(height: 6)
            .background(Capsule().fill(Color.syLineStrong))
            .clipShape(Capsule())
        }
        .frame(height: 6)
    }
}

struct ProgressBar: View {
    let value: Double
    var color: Color = .sySky

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.syLineStrong)
                Capsule().fill(color).frame(width: max(0, min(1, value)) * g.size.width)
            }
        }
        .frame(height: 5)
    }
}
