import SwiftUI

enum TokenFormat {
    static func short(_ n: Int) -> String {
        if n >= 1_000_000 { return String(format: n % 1_000_000 == 0 ? "%.0fM" : "%.1fM", Double(n) / 1_000_000) }
        if n >= 1000 { return String(format: "%.1fk", Double(n) / 1000) }
        return "\(n)"
    }

    static func color(_ name: String, kind: String) -> Color {
        if kind == "free" { return .clear }
        if kind == "buffer" || kind == "deferred" { return Color.secondary.opacity(0.35) }
        switch name {
        case "claude": return .orange
        case "warning": return .yellow
        case "error": return .red
        case "inactive": return Color.secondary.opacity(0.6)
        case "promptBorder": return Color.gray
        default:
            if name.hasPrefix("green") { return .green }
            if name.hasPrefix("purple") { return .accentColor }
            if name.hasPrefix("blue") { return .blue }
            if name.hasPrefix("red") { return .red }
            if name.hasPrefix("orange") { return .orange }
            if name.hasPrefix("pink") { return .pink }
            if name.hasPrefix("cyan") { return .cyan }
            return .teal
        }
    }

    static func resets(_ date: Date?) -> String {
        guard let date else { return "" }
        let interval = date.timeIntervalSinceNow
        if interval <= 0 { return t("Сбрасывается", "Resetting") }
        if interval < 24 * 3600 {
            let h = Int(interval) / 3600, m = (Int(interval) % 3600) / 60
            return h > 0 ? t("Сброс через \(h) ч \(m) мин", "Resets in \(h) hr \(m) min") : t("Сброс через \(m) мин", "Resets in \(m) min")
        }
        let f = DateFormatter()
        f.locale = Loc.shared.lang.locale
        f.setLocalizedDateFormatFromTemplate("EEE jj:mm")
        return t("Сброс ", "Resets ") + f.string(from: date)
    }

    static func windowTitle(_ key: String) -> String {
        switch key {
        case "five_hour": return t("Лимит сессии (5 ч)", "Session limit")
        case "seven_day": return t("Неделя · все модели", "Weekly · all models")
        default:
            let model = key.replacingOccurrences(of: "seven_day_", with: "").replacingOccurrences(of: "_", with: " ").capitalized
            return key.hasPrefix("seven_day") ? t("Неделя · \(model)", "Weekly · \(model)") : key.replacingOccurrences(of: "_", with: " ").capitalized
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
                Circle().stroke(Color.secondary.opacity(0.25), lineWidth: 2.5)
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
        .help(t("Контекст \(pct)% — нажмите, чтобы посмотреть лимиты", "Context \(pct)% — click for usage and limits"))
        .popover(isPresented: $open, arrowEdge: .top) {
            UsageOverlay(chat: chat)
        }
    }

    private func ringColor(_ pct: Int) -> Color {
        pct >= 85 ? .red : pct >= 65 ? .orange : .accentColor
    }
}

struct UsageOverlay: View {
    let chat: ChatSession
    @State private var detailed = false
    private var usage: ClaudeUsageStore { ClaudeUsageStore.shared }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            contextSection
            Divider()
            limitsSection
            Divider()
            HStack {
                Button(detailed ? t("Скрыть разбивку", "Hide breakdown") : t("Подробная разбивка", "See detailed breakdown")) { detailed.toggle() }
                    .controlSize(.small)
                Spacer()
                if chat.totalCost > 0 {
                    Text(t("Сессия: ", "Session: ") + String(format: "$%.2f", chat.totalCost))
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.secondary)
                }
            }
            if detailed { breakdown }
        }
        .padding(16)
        .frame(width: 380)
    }

    @ViewBuilder
    private var contextSection: some View {
        if let ctx = chat.context {
            VStack(alignment: .leading, spacing: 8) {
                HStack {
                    Text(t("Окно контекста", "Context window")).foregroundStyle(.secondary)
                    Spacer()
                    Text("\(TokenFormat.short(ctx.total)) / \(TokenFormat.short(ctx.max)) (\(ctx.percentage)%)")
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                }
                .font(.callout)
                ContextBar(ctx: ctx)
                HStack {
                    if ctx.autoCompactEnabled, let threshold = ctx.autoCompactThreshold, threshold > ctx.total {
                        Text(t("\(TokenFormat.short(threshold - ctx.total)) до auto-compact", "\(TokenFormat.short(threshold - ctx.total)) until auto-compact"))
                    } else if !ctx.autoCompactEnabled {
                        Text(t("Auto-compact выключен", "Auto-compact is off")).foregroundStyle(.secondary)
                    }
                    Spacer()
                    Button(t("Сжать сессию", "Compact session")) { chat.compact() }
                        .controlSize(.small)
                        .disabled(!chat.running || chat.busy)
                }
                .font(.callout)
            }
        } else {
            HStack {
                Text(t("Окно контекста", "Context window")).foregroundStyle(.secondary)
                Spacer()
                Text(chat.running ? t("загрузка…", "loading…") : t("сессия спит", "session is sleeping")).foregroundStyle(.tertiary)
            }
            .font(.callout)
        }
    }

    @ViewBuilder
    private var limitsSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Text(t("Лимиты", "Your usage limits") + (usage.subscription.map { " · \($0)" } ?? "")).foregroundStyle(.secondary)
                Spacer()
                Button { openInBrowser("https://claude.ai/settings/usage") } label: { Image(systemName: "arrow.up.right") }
                    .buttonStyle(.borderless)
                    .help(t("Открыть страницу использования", "Open usage page"))
            }
            .font(.callout)
            if usage.windows.isEmpty {
                Text(t("Данные появятся после первого ответа Claude", "Shows up after Claude's first reply")).font(.caption).foregroundStyle(.tertiary)
            }
            ForEach(usage.windows) { w in
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(TokenFormat.windowTitle(w.key))
                        Spacer()
                        Text(TokenFormat.resets(w.resetsAt)).foregroundStyle(.secondary)
                        Text("\(Int((w.utilization * 100).rounded()))%").monospacedDigit().frame(minWidth: 34, alignment: .trailing)
                    }
                    .font(.callout)
                    ProgressBar(value: w.utilization, color: w.utilization >= 0.9 ? .red : w.utilization >= 0.7 ? .orange : .accentColor)
                }
            }
            if let note = usage.overageNote {
                Label(note, systemImage: "exclamationmark.triangle.fill").font(.caption).foregroundStyle(.orange)
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
                            .fill(c.kind == "free" ? Color.secondary.opacity(0.15) : TokenFormat.color(c.color, kind: c.kind))
                            .frame(width: 10, height: 10)
                        Text(c.name + (c.kind == "deferred" ? t(" (отложено)", " (deferred)") : ""))
                            .foregroundStyle(c.kind == "used" ? .primary : .secondary)
                        Spacer()
                        Text(TokenFormat.short(c.tokens)).monospacedDigit().foregroundStyle(.secondary)
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
            .background(Capsule().fill(Color.secondary.opacity(0.18)))
            .clipShape(Capsule())
        }
        .frame(height: 6)
    }
}

struct ProgressBar: View {
    let value: Double
    var color: Color = .accentColor

    var body: some View {
        GeometryReader { g in
            ZStack(alignment: .leading) {
                Capsule().fill(Color.secondary.opacity(0.18))
                Capsule().fill(color).frame(width: max(0, min(1, value)) * g.size.width)
            }
        }
        .frame(height: 5)
    }
}
