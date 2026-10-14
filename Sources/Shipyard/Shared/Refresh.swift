import AppKit
import SwiftUI
import WebKit

enum RefreshText {
    static func ago(_ updated: Date, now: Date) -> String {
        let s = max(0, Int(now.timeIntervalSince(updated)))
        switch s {
        case ..<10: return "just now"
        case ..<60: return "\(s / 5 * 5)s ago"
        case ..<3600: return "\(s / 60)m ago"
        default: return "\(s / 3600)h ago"
        }
    }
}

struct RefreshStatus: View {
    let updated: Date?
    let refreshing: Bool
    var error: String?
    var compact = false
    var retry: (() -> Void)?

    var body: some View {
        HStack(spacing: 5) {
            ZStack {
                if refreshing {
                    ProgressView().controlSize(.mini)
                } else if error != nil {
                    Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(Color.syAmber)
                } else {
                    Image(systemName: "arrow.clockwise")
                }
            }
            .frame(width: 12, height: 12)
            TimelineView(.periodic(from: .now, by: 5)) { context in
                Text(label(now: context.date))
                    .monospacedDigit()
                    .lineLimit(1)
            }
            if error != nil, !refreshing, let retry {
                Button("Retry", action: retry)
                    .buttonStyle(.link)
                    .font(.caption.weight(.medium))
            }
        }
        .font(.caption)
        .foregroundStyle(Color.syMuted)
        .help(error.map { "Last refresh failed: \($0)" } ?? "Pull down to refresh (⌘R)")
    }

    private func label(now: Date) -> String {
        if refreshing { return "Updating…" }
        if error != nil { return compact ? "Offline" : "Couldn't refresh" }
        guard let updated else { return compact ? "" : "Not updated yet" }
        let ago = RefreshText.ago(updated, now: now)
        return compact ? ago : "Updated \(ago)"
    }
}

struct RefreshFooter: View {
    let updated: Date?
    let refreshing: Bool
    var error: String?
    var retry: (() -> Void)?

    var body: some View {
        VStack(spacing: 0) {
            Rectangle().fill(Color.syLine).frame(height: 1)
            RefreshStatus(updated: updated, refreshing: refreshing, error: error, retry: retry)
                .frame(maxWidth: .infinity)
                .frame(height: 24)
        }
    }
}

extension View {
    func liveRefresh(updated: Date?, refreshing: Bool, error: String? = nil, footer: Bool = true, web: Bool = false, action: @escaping @MainActor () async -> Void) -> some View {
        modifier(LiveRefreshModifier(updated: updated, refreshing: refreshing, error: error, footer: footer, web: web, action: action))
    }
}

private let pullThreshold: CGFloat = 44
private let heldMargin: CGFloat = 48

enum PullPhase: Equatable {
    case idle, loading, done
}

private struct LiveRefreshModifier: ViewModifier {
    let updated: Date?
    let refreshing: Bool
    let error: String?
    let footer: Bool
    let web: Bool
    let action: @MainActor () async -> Void
    @State private var pull: CGFloat = 0
    @State private var phase: PullPhase = .idle

    private var held: Bool { phase != .idle }

    func body(content: Content) -> some View {
        VStack(spacing: 0) {
            scrollArea(content)
                .background(ScrollPullProbe(pull: $pull, onTrigger: pulled))
                .overlay(alignment: .top) {
                    PullIndicator(pull: pull, phase: phase)
                        .allowsHitTesting(false)
                }
                .clipped()
                .onChange(of: pull >= pullThreshold) { _, armed in
                    if armed { NSHapticFeedbackManager.defaultPerformer.perform(.alignment, performanceTime: .now) }
                }
            if footer {
                RefreshFooter(updated: updated, refreshing: refreshing && phase == .idle, error: error) {
                    Task { await action() }
                }
            }
        }
    }

    @ViewBuilder
    private func scrollArea(_ content: Content) -> some View {
        if web {
            content.padding(.top, held ? heldMargin : 0)
        } else {
            content
                .scrollBounceBehavior(.always, axes: .vertical)
                .contentMargins(.top, held ? heldMargin : nil, for: .scrollContent)
        }
    }

    private func pulled() {
        guard phase == .idle else { return }
        withAnimation(Motion.insert) { phase = .loading }
        Task { @MainActor in
            await action()
            withAnimation(Motion.fade) { phase = .done }
            try? await Task.sleep(nanoseconds: 800_000_000)
            guard phase == .done else { return }
            withAnimation(Motion.insert) { phase = .idle }
        }
    }
}

private struct PullIndicator: View {
    let pull: CGFloat
    let phase: PullPhase
    @State private var spin = false

    private var progress: CGFloat { min(1, pull / pullThreshold) }

    var body: some View {
        let visible = phase != .idle || pull > 2
        Group {
            switch phase {
            case .done:
                HStack(spacing: 5) {
                    Image(systemName: "checkmark").font(.caption2.weight(.bold)).foregroundStyle(Color.syGreen)
                    Text("Updated just now").font(.caption.weight(.medium)).foregroundStyle(Color.syMuted)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 5)
                .background(.regularMaterial, in: Capsule())
                .overlay(Capsule().strokeBorder(Color.syLine))
                .transition(.opacity)
            case .loading:
                ring(trim: 0.28)
                    .rotationEffect(.degrees(spin ? 360 : 0))
                    .onAppear {
                        guard !Motion.reduced else { return }
                        withAnimation(.linear(duration: 0.8).repeatForever(autoreverses: false)) { spin = true }
                    }
                    .onDisappear { spin = false }
            case .idle:
                ring(trim: progress)
                    .scaleEffect(0.6 + 0.4 * progress)
                    .rotationEffect(.degrees(Double(progress) * 270))
            }
        }
        .frame(height: 26)
        .padding(.top, phase == .idle ? max(4, min(pull, pullThreshold) / 2 - 13) : (heldMargin - 26) / 2)
        .opacity(visible ? (phase == .idle ? Double(min(1, pull / 16)) : 1) : 0)
        .animation(Motion.fade, value: phase)
    }

    private func ring(trim: CGFloat) -> some View {
        ZStack {
            Circle().stroke(Color.syLine, lineWidth: 2.5)
            Circle()
                .trim(from: 0, to: max(0.001, trim))
                .stroke(Color.sySky, style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                .rotationEffect(.degrees(-90))
        }
        .frame(width: 22, height: 22)
        .padding(2)
        .background(Circle().fill(.regularMaterial))
    }
}

final class PullWebView: WKWebView {
    var atTop = true
    var onPull: ((CGFloat) -> Void)?
    var onTrigger: (() -> Void)?
    private var accumulated: CGFloat = 0
    private var tracking = false

    override func scrollWheel(with event: NSEvent) {
        super.scrollWheel(with: event)
        guard onPull != nil, event.momentumPhase == [] else { return }
        if event.phase.contains(.began) || event.phase.contains(.mayBegin) {
            accumulated = 0
            tracking = true
        }
        if tracking, event.phase.contains(.changed) {
            if atTop {
                accumulated = max(0, accumulated + event.scrollingDeltaY * (event.hasPreciseScrollingDeltas ? 0.8 : 0))
            } else {
                accumulated = 0
            }
            onPull?(accumulated)
        }
        if event.phase.contains(.ended) || event.phase.contains(.cancelled) {
            let fire = tracking && accumulated >= pullThreshold * 0.85 && event.phase.contains(.ended)
            tracking = false
            accumulated = 0
            onPull?(0)
            if fire { onTrigger?() }
        }
    }
}

private struct ScrollPullProbe: NSViewRepresentable {
    @Binding var pull: CGFloat
    let onTrigger: () -> Void

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> ProbeView {
        let v = ProbeView()
        v.coordinator = context.coordinator
        return v
    }

    func updateNSView(_ view: ProbeView, context: Context) {
        context.coordinator.onPull = { value in
            if abs(value - pull) >= 1 || (value == 0) != (pull == 0) { pull = value }
        }
        context.coordinator.onTrigger = onTrigger
        view.coordinator = context.coordinator
    }

    static func dismantleNSView(_ view: ProbeView, coordinator: Coordinator) {
        coordinator.detach()
    }

    final class ProbeView: NSView {
        weak var coordinator: Coordinator?
        private var attempts = 0

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            guard window != nil else {
                coordinator?.detach()
                return
            }
            attempts = 0
            DispatchQueue.main.async { [weak self] in self?.attach() }
        }

        override func layout() {
            super.layout()
            if coordinator?.attached == false, window != nil, attempts < 20 { attach() }
        }

        private func attach() {
            attempts += 1
            guard let coordinator, window != nil else { return }
            switch findTarget() {
            case let scroll as NSScrollView: coordinator.attach(scroll)
            case let web as PullWebView: coordinator.attach(web)
            default: break
            }
        }

        private func findTarget() -> NSView? {
            let target = convert(bounds, to: nil)
            guard target.width > 0, target.height > 0 else { return nil }
            var ancestor = superview
            while let a = ancestor {
                var best: (NSView, CGFloat)?
                var queue: [NSView] = [a]
                while !queue.isEmpty {
                    let v = queue.removeFirst()
                    let candidate = (v as? NSScrollView).flatMap { $0.hasVerticalScroller || $0.documentView != nil ? $0 : nil } ?? (v as? PullWebView)
                    if let c = candidate {
                        let r = c.convert(c.bounds, to: nil)
                        let overlap = r.intersection(target)
                        if !overlap.isNull, overlap.width * overlap.height > target.width * target.height * 0.5 {
                            let diff = abs(r.minX - target.minX) + abs(r.minY - target.minY) + abs(r.width - target.width) + abs(r.height - target.height)
                            if best == nil || diff < best!.1 { best = (c, diff) }
                        }
                        continue
                    }
                    queue.append(contentsOf: v.subviews)
                }
                if let best { return best.0 }
                ancestor = a.superview
            }
            return nil
        }
    }

    @MainActor
    final class Coordinator {
        weak var scrollView: NSScrollView?
        weak var webView: PullWebView?
        var onPull: (CGFloat) -> Void = { _ in }
        var onTrigger: () -> Void = {}
        private var observers: [NSObjectProtocol] = []
        private var maxPull: CGFloat = 0
        private var tracking = false

        var attached: Bool { scrollView != nil || webView != nil }

        func attach(_ web: PullWebView) {
            guard web !== webView else { return }
            detach()
            webView = web
            web.onPull = { [weak self] in self?.onPull($0) }
            web.onTrigger = { [weak self] in self?.onTrigger() }
        }

        func attach(_ scroll: NSScrollView) {
            guard scroll !== scrollView else { return }
            detach()
            scrollView = scroll
            scroll.verticalScrollElasticity = .allowed
            let clip = scroll.contentView
            clip.postsBoundsChangedNotifications = true
            let center = NotificationCenter.default
            observers.append(center.addObserver(forName: NSView.boundsDidChangeNotification, object: clip, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.boundsChanged() }
            })
            observers.append(center.addObserver(forName: NSScrollView.willStartLiveScrollNotification, object: scroll, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated {
                    self?.tracking = true
                    self?.maxPull = 0
                }
            })
            observers.append(center.addObserver(forName: NSScrollView.didEndLiveScrollNotification, object: scroll, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.endTracking() }
            })
        }

        func detach() {
            observers.forEach(NotificationCenter.default.removeObserver)
            observers = []
            scrollView = nil
            webView?.onPull = nil
            webView?.onTrigger = nil
            webView = nil
        }

        private var currentPull: CGFloat {
            guard let scroll = scrollView else { return 0 }
            let clip = scroll.contentView
            guard clip.isFlipped else { return 0 }
            return max(0, -(clip.bounds.minY + clip.contentInsets.top))
        }

        private func boundsChanged() {
            let value = currentPull
            let event = NSApp.currentEvent
            let fingerDown = event?.type == .scrollWheel
                && event?.momentumPhase == []
                && event?.hasPreciseScrollingDeltas == true
                && !(event?.phase ?? []).isDisjoint(with: [.began, .changed, .stationary, .mayBegin])
            if tracking, fingerDown {
                maxPull = max(maxPull, value)
                onPull(value)
                return
            }
            onPull(0)
            if tracking, maxPull >= pullThreshold * 0.85 { fire() }
        }

        private func endTracking() {
            if maxPull >= pullThreshold * 0.85 { fire() }
            tracking = false
            maxPull = 0
            onPull(0)
        }

        private func fire() {
            maxPull = 0
            tracking = false
            onPull(0)
            onTrigger()
        }
    }
}
