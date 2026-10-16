import AppKit
import SwiftUI
import UniformTypeIdentifiers

struct RequestGroup: Identifiable {
    var id: String { name }
    let name: String
    let kind: RequestKind
    let count: Int
    let avgMs: Double
    let p95Ms: Double
    let bytes: Int
    let errors: Int
}

struct RateLimitState: Identifiable {
    var id: String { bucket }
    let bucket: String
    let remaining: Int
    let reset: Int?

    static func latest(_ newestFirst: [RequestRecord]) -> [RateLimitState] {
        var seen: [String: RateLimitState] = [:]
        for r in newestFirst {
            guard let bucket = r.rateLimitBucket, seen[bucket] == nil, let remaining = r.rateLimitRemaining else { continue }
            seen[bucket] = RateLimitState(bucket: bucket, remaining: remaining, reset: r.rateLimitReset)
        }
        return seen.values.sorted { $0.bucket < $1.bucket }
    }

    var resetText: String? {
        reset.map { Date(timeIntervalSince1970: TimeInterval($0)).formatted(date: .omitted, time: .standard) }
    }

    var summary: String {
        "\(bucket) \(remaining)" + (resetText.map { " (resets \($0))" } ?? "")
    }
}

struct DiagnosticsStats {
    let count: Int
    let liveTick: Int
    let bytes: Int
    let avgMs: Double
    let p95Ms: Double

    init(_ requests: [RequestRecord]) {
        count = requests.count
        liveTick = requests.filter { $0.origin == .liveTick }.count
        bytes = requests.reduce(0) { $0 + $1.bytes }
        let durations = requests.map(\.durationMs)
        avgMs = durations.isEmpty ? 0 : durations.reduce(0, +) / Double(durations.count)
        p95Ms = Self.p95(durations)
    }

    static func p95(_ values: [Double]) -> Double {
        guard !values.isEmpty else { return 0 }
        let sorted = values.sorted()
        let index = Int((Double(sorted.count) * 0.95).rounded(.up)) - 1
        return sorted[min(max(index, 0), sorted.count - 1)]
    }

    static func groups(_ requests: [RequestRecord]) -> [RequestGroup] {
        Dictionary(grouping: requests, by: \.name).map { name, items in
            let durations = items.map(\.durationMs)
            return RequestGroup(
                name: name,
                kind: items[0].kind,
                count: items.count,
                avgMs: durations.reduce(0, +) / Double(durations.count),
                p95Ms: p95(durations),
                bytes: items.reduce(0) { $0 + $1.bytes },
                errors: items.filter(\.failed).count
            )
        }
        .sorted { $0.count == $1.count ? $0.name < $1.name : $0.count > $1.count }
    }
}

enum DiagnosticsTab: String, CaseIterable, Identifiable {
    case requests, grouped, ticks, slowUI

    var id: String { rawValue }

    var title: String {
        switch self {
        case .requests: return "Requests"
        case .grouped: return "By name"
        case .ticks: return "Live ticks"
        case .slowUI: return "Slow UI"
        }
    }
}

struct DiagnosticsView: View {
    @AppStorage(Diagnostics.enabledKey) private var enabled = false
    @State private var snapshot = DiagnosticsSnapshot(version: -1)
    @State private var tab = DiagnosticsTab.requests
    @State private var filter = ""
    @State private var paused = false
    @State private var selection = Set<RequestRecord.ID>()
    @State private var groupSelection = Set<RequestGroup.ID>()
    @State private var exportError: String?

    private var requests: [RequestRecord] {
        let q = filter.trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return snapshot.requests }
        return snapshot.requests.filter { $0.name.localizedCaseInsensitiveContains(q) }
    }

    var body: some View {
        let filtered = requests
        let stats = DiagnosticsStats(filtered)
        VStack(alignment: .leading, spacing: 12) {
            if !enabled {
                Label("Recording is off. Turn on “Record network diagnostics” in Settings.", systemImage: "pause.circle")
                    .font(.callout)
                    .foregroundStyle(Color.syAmber)
            }
            summary(stats)
            toolbar(stats, filtered)
            content(filtered)
        }
        .padding(16)
        .frame(minWidth: 820, minHeight: 480)
        .surface(.panel)
        .task {
            while !Task.isCancelled {
                if !isHolding { refreshSnapshot() }
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .alert("Couldn't export diagnostics", isPresented: Binding(get: { exportError != nil }, set: { if !$0 { exportError = nil } })) {
            Button("OK") {}
        } message: {
            Text(exportError ?? "")
        }
    }

    private var isHolding: Bool {
        paused || !selection.isEmpty || !groupSelection.isEmpty
    }

    private func refreshSnapshot() {
        guard let s = Diagnostics.shared.snapshot(ifNewerThan: snapshot.version) else { return }
        Diagnostics.shared.markPanelRefresh()
        snapshot = s
    }

    private var activeTicks: [TickRecord] {
        snapshot.ticks.filter { $0.requests > 0 }
    }

    private func summary(_ stats: DiagnosticsStats) -> some View {
        Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 6) {
            GridRow {
                metric("Requests", "\(stats.count)", detail: "\(stats.liveTick) live-tick · \(stats.count - stats.liveTick) user")
                metric("Data", Self.bytes(stats.bytes))
                metric("Avg", Self.ms(stats.avgMs), detail: "p95 \(Self.ms(stats.p95Ms))")
                metric("Live tick avg", activeTicks.isEmpty ? "—" : Self.ms(activeTicks.map(\.durationMs).reduce(0, +) / Double(activeTicks.count)), detail: "\(activeTicks.count) ticks with requests")
            }
            GridRow {
                metric("Rate limit", rateLimits.isEmpty ? "—" : rateLimits.map { "\($0.bucket) \($0.remaining)" }.joined(separator: "  "), detail: rateLimits.isEmpty ? "no headers yet" : rateLimits.compactMap { s in s.resetText.map { "\(s.bucket) resets \($0)" } }.joined(separator: " · "))
                    .gridCellColumns(4)
            }
            GridRow {
                metric("Live ticks", tickSummary, detail: "ms/requests, last \(min(activeTicks.count, 10)) with requests, newest first")
                    .gridCellColumns(3)
                metric("Slow UI", "\(snapshot.slowUI.count)", detail: snapshot.slowUI.map(\.durationMs).max().map { "max \(Self.ms($0))" } ?? "main thread > 16 ms")
            }
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color.syRaised, in: RoundedRectangle(cornerRadius: 8))
    }

    private func metric(_ title: String, _ value: String, detail: String? = nil) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title).font(.caption).foregroundStyle(Color.syMuted)
            Text(value).font(.system(.title3, design: .monospaced).weight(.semibold)).foregroundStyle(Color.syText).lineLimit(1)
            if let detail {
                Text(detail).font(.caption2).foregroundStyle(Color.syDim).lineLimit(1)
            }
        }
    }

    private var rateLimits: [RateLimitState] {
        RateLimitState.latest(snapshot.requests)
    }

    private var tickSummary: String {
        let last = activeTicks.prefix(10)
        guard !last.isEmpty else { return "—" }
        return last.map { "\(Int($0.durationMs.rounded()))/\($0.requests)" }.joined(separator: "  ")
    }

    private func toolbar(_ stats: DiagnosticsStats, _ filtered: [RequestRecord]) -> some View {
        HStack {
            Picker("View", selection: $tab) {
                ForEach(DiagnosticsTab.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 360)
            TextField("Filter by name", text: $filter)
                .textFieldStyle(.roundedBorder)
                .frame(maxWidth: 240)
            Spacer()
            Toggle(isHolding && !paused ? "Paused (selection)" : "Pause", isOn: $paused)
                .toggleStyle(.button)
            Button("Copy Summary") { copySummary(stats, filtered) }
            Button("Export…") { export() }
            Button("Clear", role: .destructive) {
                Diagnostics.shared.clear()
                selection = []
                groupSelection = []
                refreshSnapshot()
            }
        }
    }

    @ViewBuilder
    private func content(_ filtered: [RequestRecord]) -> some View {
        switch tab {
        case .requests:
            Table(filtered, selection: $selection) {
                TableColumn("Time") { r in Text(r.date.formatted(date: .omitted, time: .standard)).monospacedDigit() }
                    .width(min: 70, ideal: 80)
                TableColumn("Kind") { r in Text(r.kind.rawValue) }
                    .width(min: 50, ideal: 60)
                TableColumn("Name") { r in Text(r.name).lineLimit(1).help(r.path) }
                    .width(min: 180, ideal: 320)
                TableColumn("Origin") { r in Text(r.origin.rawValue).foregroundStyle(r.origin == .liveTick ? Color.syMuted : Color.syText) }
                    .width(min: 60, ideal: 70)
                TableColumn("ms") { r in Text(Self.number(r.durationMs)).monospacedDigit() }
                    .width(min: 50, ideal: 60)
                TableColumn("Bytes") { r in Text(Self.bytes(r.bytes)).monospacedDigit() }
                    .width(min: 60, ideal: 70)
                TableColumn("Status") { r in statusText(r) }
                    .width(min: 50, ideal: 120)
                TableColumn("Remaining") { r in Text(r.rateLimitRemaining.map(String.init) ?? "").monospacedDigit().help(r.rateLimitBucket ?? "") }
                    .width(min: 60, ideal: 70)
                TableColumn("Path") { r in Text(r.path).lineLimit(1).truncationMode(.middle).foregroundStyle(Color.syMuted).help(r.path) }
                    .width(min: 120, ideal: 260)
            }
        case .grouped:
            Table(DiagnosticsStats.groups(filtered), selection: $groupSelection) {
                TableColumn("Name") { g in Text(g.name).lineLimit(1).help(g.name) }
                    .width(min: 200, ideal: 360)
                TableColumn("Kind") { g in Text(g.kind.rawValue) }
                    .width(min: 50, ideal: 60)
                TableColumn("Count") { g in Text("\(g.count)").monospacedDigit() }
                    .width(min: 50, ideal: 60)
                TableColumn("Avg ms") { g in Text(Self.number(g.avgMs)).monospacedDigit() }
                    .width(min: 60, ideal: 70)
                TableColumn("p95 ms") { g in Text(Self.number(g.p95Ms)).monospacedDigit() }
                    .width(min: 60, ideal: 70)
                TableColumn("Total bytes") { g in Text(Self.bytes(g.bytes)).monospacedDigit() }
                    .width(min: 70, ideal: 90)
                TableColumn("Errors") { g in Text(g.errors == 0 ? "" : "\(g.errors)").foregroundStyle(Color.syCoral) }
                    .width(min: 50, ideal: 60)
            }
        case .ticks:
            Table(snapshot.ticks) {
                TableColumn("Time") { t in Text(t.date.formatted(date: .omitted, time: .standard)).monospacedDigit() }
                TableColumn("Duration ms") { t in Text(Self.number(t.durationMs)).monospacedDigit() }
                TableColumn("Requests") { t in Text("\(t.requests)").monospacedDigit() }
            }
        case .slowUI:
            Table(snapshot.slowUI) {
                TableColumn("Time") { s in Text(s.date.formatted(date: .omitted, time: .standard)).monospacedDigit() }
                TableColumn("Main thread busy ms") { s in Text(Self.number(s.durationMs)).monospacedDigit() }
            }
        }
    }

    private func statusText(_ r: RequestRecord) -> some View {
        let text = [r.status.map(String.init), r.error, r.note].compactMap { $0 }.joined(separator: " · ")
        return Text(text)
            .lineLimit(1)
            .help(text)
            .foregroundStyle(r.failed ? Color.syCoral : r.note != nil ? Color.syAmber : Color.syText)
    }

    private func copySummary(_ stats: DiagnosticsStats, _ filtered: [RequestRecord]) {
        var lines = ["PullYard diagnostics, \(Date().formatted(date: .abbreviated, time: .standard))"]
        if !filter.isEmpty { lines.append("Filter: \(filter)") }
        lines.append("Requests: \(stats.count) (\(stats.liveTick) live-tick, \(stats.count - stats.liveTick) user), \(Self.bytes(stats.bytes)), avg \(Self.ms(stats.avgMs)), p95 \(Self.ms(stats.p95Ms))")
        if !rateLimits.isEmpty {
            lines.append("Rate limit: " + rateLimits.map(\.summary).joined(separator: ", "))
        }
        if !activeTicks.isEmpty {
            lines.append("Live ticks: \(activeTicks.count) with requests, avg \(Self.ms(activeTicks.map(\.durationMs).reduce(0, +) / Double(activeTicks.count)))")
        }
        lines.append("Live ticks (ms/requests, newest first): \(tickSummary)")
        lines.append("Slow UI (>16 ms): \(snapshot.slowUI.count)" + (snapshot.slowUI.map(\.durationMs).max().map { ", max \(Self.ms($0))" } ?? ""))
        lines.append("")
        lines.append(["name", "kind", "count", "avg ms", "p95 ms", "bytes", "errors"].joined(separator: "\t"))
        for g in DiagnosticsStats.groups(filtered) {
            lines.append([g.name, g.kind.rawValue, "\(g.count)", Self.number(g.avgMs), Self.number(g.p95Ms), "\(g.bytes)", "\(g.errors)"].joined(separator: "\t"))
        }
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(lines.joined(separator: "\n"), forType: .string)
    }

    private func export() {
        let panel = NSSavePanel()
        panel.allowedContentTypes = [UTType(filenameExtension: "jsonl") ?? .json]
        panel.nameFieldStringValue = "pullyard-diagnostics-\(Self.fileStamp.string(from: Date())).jsonl"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        let current = Diagnostics.shared.snapshot(ifNewerThan: -1) ?? snapshot
        do {
            try DiagnosticsExport.jsonLines(current).write(to: url, options: .atomic)
        } catch {
            exportError = error.localizedDescription
        }
    }

    private static let fileStamp: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyyMMdd-HHmmss"
        return f
    }()

    static func number(_ v: Double) -> String {
        v >= 100 ? String(Int(v.rounded())) : String(format: "%.1f", v)
    }

    static func ms(_ v: Double) -> String {
        number(v) + " ms"
    }

    static func bytes(_ n: Int) -> String {
        ByteCountFormatter.string(fromByteCount: Int64(n), countStyle: .file)
    }
}

enum DiagnosticsExport {
    private struct Line: Encodable {
        let type: String
        let ts: Date
        var kind: String?
        var name: String?
        var path: String?
        var origin: String?
        let durationMs: Double
        var bytes: Int?
        var status: Int?
        var error: String?
        var note: String?
        var authenticated: Bool?
        var rateLimitRemaining: Int?
        var rateLimitReset: Int?
        var rateLimitResource: String?
        var requests: Int?
    }

    private static let iso: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    static func jsonLines(_ s: DiagnosticsSnapshot) -> Data {
        var lines: [Line] = []
        for r in s.requests {
            lines.append(Line(type: "request", ts: r.date, kind: r.kind.rawValue, name: r.name, path: r.path, origin: r.origin.rawValue, durationMs: r.durationMs, bytes: r.bytes, status: r.status, error: r.error, note: r.note, authenticated: r.authenticated ? nil : false, rateLimitRemaining: r.rateLimitRemaining, rateLimitReset: r.rateLimitReset, rateLimitResource: r.rateLimitResource))
        }
        for t in s.ticks {
            lines.append(Line(type: "tick", ts: t.date, durationMs: t.durationMs, requests: t.requests))
        }
        for u in s.slowUI {
            lines.append(Line(type: "slow-ui", ts: u.date, durationMs: u.durationMs))
        }
        lines.sort { $0.ts < $1.ts }
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        encoder.dateEncodingStrategy = .custom { date, enc in
            var c = enc.singleValueContainer()
            try c.encode(iso.string(from: date))
        }
        var out = Data()
        for line in lines {
            guard let d = try? encoder.encode(line) else { continue }
            out.append(d)
            out.append(0x0A)
        }
        return out
    }
}
