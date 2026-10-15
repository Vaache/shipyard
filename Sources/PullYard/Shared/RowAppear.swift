import SwiftUI

enum RowAppearMode {
    case cascade, refresh, append, silent
}

enum RowEntrance {
    case cascade(delay: Double)
    case insert
    case highlight
}

@Observable
@MainActor
final class RowAppearTracker {
    private static var registry: [String: RowAppearTracker] = [:]
    private static let cascadeLimit = 12
    private static let stagger = 0.07

    private(set) var epoch = 0
    @ObservationIgnored private var seen: Set<String> = []
    @ObservationIgnored private var cascade: [String: Int] = [:]
    @ObservationIgnored private var fresh: [String: Date] = [:]
    @ObservationIgnored private var cascadeStart = Date.distantPast

    init() {}

    static func shared(_ key: String) -> RowAppearTracker {
        if let t = registry[key] { return t }
        let t = RowAppearTracker()
        registry[key] = t
        return t
    }

    static func resetAll() {
        registry.values.forEach { $0.reset() }
        registry = [:]
    }

    var isEmpty: Bool { seen.isEmpty }

    func seed<S: Sequence>(_ ids: S) where S.Element == String {
        ingest(ids, mode: seen.isEmpty ? .silent : .refresh)
    }

    func ingest<S: Sequence>(_ ids: S, mode: RowAppearMode) where S.Element == String {
        let list = Array(ids)
        var mode = mode
        if mode == .refresh, seen.isEmpty { mode = .cascade }
        switch mode {
        case .cascade:
            cascade = [:]
            fresh = [:]
            for (i, id) in list.prefix(Self.cascadeLimit).enumerated() { cascade[id] = i }
            cascadeStart = Date()
        case .refresh:
            let now = Date()
            for id in list where !seen.contains(id) { fresh[id] = now }
        case .append, .silent:
            break
        }
        seen.formUnion(list)
        epoch += 1
    }

    func isPending(_ id: String) -> Bool {
        if cascade[id] != nil { return true }
        if let at = fresh[id] { return Date().timeIntervalSince(at) < 1 }
        return false
    }

    func claim(_ id: String) -> RowEntrance? {
        if let i = cascade.removeValue(forKey: id) {
            let elapsed = Date().timeIntervalSince(cascadeStart)
            return .cascade(delay: max(0, Double(i) * Self.stagger - elapsed))
        }
        if let at = fresh.removeValue(forKey: id) {
            let age = Date().timeIntervalSince(at)
            if age < 1 { return .insert }
            return age < 60 ? .highlight : nil
        }
        return nil
    }

    func reset() {
        seen = []
        cascade = [:]
        fresh = [:]
        epoch += 1
    }
}

struct RowAppear: ViewModifier {
    let id: String
    let tracker: RowAppearTracker
    var bleed: EdgeInsets
    var radius: CGFloat?
    @State private var entered: Bool?
    @State private var glow = false

    func body(content: Content) -> some View {
        let visible = entered ?? !tracker.isPending(id)
        content
            .opacity(visible ? 1 : 0)
            .offset(x: visible || Motion.reduced ? 0 : 60)
            .background {
                if glow, radius == nil { glowLayer.padding(bleed) }
            }
            .overlay {
                if glow, let radius { glowLayer.clipShape(RoundedRectangle(cornerRadius: radius)) }
            }
            .onAppear(perform: run)
            .onChange(of: tracker.epoch) { run() }
    }

    private var glowLayer: some View {
        HStack(spacing: 0) {
            Rectangle().fill(Color.sySky).frame(width: 3)
            Rectangle().fill(Color.sySky.opacity(0.10))
        }
        .allowsHitTesting(false)
        .transition(.opacity)
    }

    private func run() {
        guard let entrance = tracker.claim(id) else {
            if entered == nil { entered = true }
            return
        }
        switch entrance {
        case .cascade(let delay):
            slideIn(Motion.rowIn(delay: delay))
        case .insert:
            slideIn(Motion.insert)
            fadeGlow()
        case .highlight:
            entered = true
            fadeGlow()
        }
    }

    private func slideIn(_ animation: Animation) {
        var t = Transaction()
        t.disablesAnimations = true
        withTransaction(t) { entered = false }
        DispatchQueue.main.async {
            withAnimation(animation) { entered = true }
        }
    }

    private func fadeGlow() {
        var t = Transaction()
        t.disablesAnimations = true
        withTransaction(t) { glow = true }
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 1_200_000_000)
            withAnimation(.easeOut(duration: 1.2)) { glow = false }
        }
    }
}

extension View {
    func rowAppear(_ id: String, _ tracker: RowAppearTracker, bleed: EdgeInsets = EdgeInsets(top: -4, leading: -10, bottom: -4, trailing: -10), radius: CGFloat? = nil) -> some View {
        modifier(RowAppear(id: id, tracker: tracker, bleed: bleed, radius: radius))
    }
}
