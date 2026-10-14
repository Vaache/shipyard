import AppKit
import SwiftUI

struct RefreshStatus: View {
    let updated: Date?
    let refreshing: Bool
    var error: String?
    var compact = false
    let action: () -> Void

    var body: some View {
        Button(action: action) {
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
            }
            .font(.caption)
            .foregroundStyle(Color.syMuted)
            .contentShape(Rectangle())
        }
        .buttonStyle(.borderless)
        .help(error.map { "Last refresh failed: \($0)\nClick to retry" } ?? "Refresh now (⌘R)")
    }

    private func label(now: Date) -> String {
        if refreshing { return "Updating…" }
        if error != nil { return compact ? "Offline" : "Couldn't refresh" }
        guard let updated else { return compact ? "" : "Not updated yet" }
        let s = max(0, Int(now.timeIntervalSince(updated)))
        let ago: String
        switch s {
        case ..<10: ago = "just now"
        case ..<60: ago = "\(s / 5 * 5)s ago"
        case ..<3600: ago = "\(s / 60)m ago"
        default: ago = "\(s / 3600)h ago"
        }
        return compact ? ago : "Updated \(ago)"
    }
}

struct RefreshFooter: View {
    let updated: Date?
    let refreshing: Bool
    var error: String?
    let action: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            Rectangle().fill(Color.syLine).frame(height: 1)
            RefreshStatus(updated: updated, refreshing: refreshing, error: error, action: action)
                .frame(maxWidth: .infinity)
                .frame(height: 24)
        }
    }
}

extension View {
    func pullToRefresh(_ action: @escaping @MainActor () async -> Void) -> some View {
        modifier(PullToRefreshModifier(action: action))
    }
}

private let pullThreshold: CGFloat = 64

private struct PullToRefreshModifier: ViewModifier {
    let action: @MainActor () async -> Void
    @State private var pull: CGFloat = 0
    @State private var refreshing = false

    func body(content: Content) -> some View {
        content
            .background(ScrollPullProbe(pull: $pull) {
                guard !refreshing else { return }
                refreshing = true
                Task {
                    await action()
                    refreshing = false
                }
            })
            .overlay(alignment: .top) {
                PullIndicator(pull: pull, refreshing: refreshing)
                    .allowsHitTesting(false)
            }
    }
}

private struct PullIndicator: View {
    let pull: CGFloat
    let refreshing: Bool

    private var armed: Bool { pull >= pullThreshold }

    var body: some View {
        let visible = refreshing || pull > 6
        HStack(spacing: 6) {
            if refreshing {
                ProgressView().controlSize(.mini)
                Text("Refreshing…")
            } else {
                Image(systemName: "arrow.down")
                    .rotationEffect(.degrees(armed ? 180 : 0))
                    .animation(Motion.spring, value: armed)
                Text(armed ? "Release to refresh" : "Pull to refresh")
            }
        }
        .font(.caption.weight(.medium))
        .foregroundStyle(Color.syMuted)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(.regularMaterial, in: Capsule())
        .overlay(Capsule().strokeBorder(Color.syLine))
        .padding(.top, refreshing ? 8 : max(4, min(pull, pullThreshold) / 2 - 8))
        .opacity(visible ? (refreshing ? 1 : min(1, pull / pullThreshold)) : 0)
        .animation(.easeOut(duration: 0.15), value: refreshing)
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
            if coordinator?.scrollView == nil, window != nil, attempts < 20 { attach() }
        }

        private func attach() {
            attempts += 1
            guard let coordinator, window != nil, let scroll = findScrollView() else { return }
            coordinator.attach(scroll)
        }

        private func findScrollView() -> NSScrollView? {
            let target = convert(bounds, to: nil)
            guard target.width > 0, target.height > 0 else { return nil }
            var ancestor = superview
            while let a = ancestor {
                var best: (NSScrollView, CGFloat)?
                var queue: [NSView] = [a]
                while !queue.isEmpty {
                    let v = queue.removeFirst()
                    if let s = v as? NSScrollView, s.hasVerticalScroller || s.documentView != nil {
                        let r = s.convert(s.bounds, to: nil)
                        let overlap = r.intersection(target)
                        if !overlap.isNull, overlap.width * overlap.height > target.width * target.height * 0.5 {
                            let diff = abs(r.minX - target.minX) + abs(r.minY - target.minY) + abs(r.width - target.width) + abs(r.height - target.height)
                            if best == nil || diff < best!.1 { best = (s, diff) }
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
        var onPull: (CGFloat) -> Void = { _ in }
        var onTrigger: () -> Void = {}
        private var observers: [NSObjectProtocol] = []
        private var maxPull: CGFloat = 0
        private var tracking = false

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
                && !(event?.phase ?? []).isDisjoint(with: [.began, .changed, .stationary, .mayBegin])
            if tracking, fingerDown {
                maxPull = max(maxPull, value)
                onPull(value)
                return
            }
            onPull(0)
            if tracking, maxPull >= pullThreshold { fire() }
        }

        private func endTracking() {
            if maxPull >= pullThreshold { fire() }
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
