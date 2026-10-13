import AppKit
import SwiftUI

enum Motion {
    static var reduced: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    static var quick: Animation? { reduced ? nil : .snappy(duration: 0.16) }
    static var smooth: Animation? { reduced ? nil : .smooth(duration: 0.26) }
    static var spring: Animation? { reduced ? nil : .spring(response: 0.32, dampingFraction: 0.82) }
    static var fade: Animation? { reduced ? nil : .easeOut(duration: 0.14) }
}

extension AnyTransition {
    static var rise: AnyTransition {
        Motion.reduced ? .opacity : .opacity.combined(with: .offset(y: 8))
    }

    static var slideIn: AnyTransition {
        Motion.reduced ? .opacity : .asymmetric(insertion: .opacity.combined(with: .offset(x: 14)), removal: .opacity)
    }

    static var pop: AnyTransition {
        Motion.reduced ? .opacity : .opacity.combined(with: .scale(scale: 0.96))
    }
}

struct HoverHighlight: ViewModifier {
    var radius: CGFloat = 8
    var color: Color = .syText
    @State private var hovering = false

    func body(content: Content) -> some View {
        content
            .background(RoundedRectangle(cornerRadius: radius).fill(color.opacity(hovering ? 0.06 : 0)))
            .onHover { h in withAnimation(Motion.fade) { hovering = h } }
    }
}

struct PressScale: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed && !Motion.reduced ? 0.96 : 1)
            .animation(Motion.quick, value: configuration.isPressed)
    }
}

struct AppearFade: ViewModifier {
    var delay: Double = 0
    @State private var shown = false

    func body(content: Content) -> some View {
        content
            .opacity(shown ? 1 : 0)
            .offset(y: shown || Motion.reduced ? 0 : 6)
            .onAppear {
                guard !shown else { return }
                withAnimation(Motion.smooth?.delay(delay)) { shown = true }
            }
    }
}

extension View {
    func hoverHighlight(radius: CGFloat = 8, color: Color = .syText) -> some View { modifier(HoverHighlight(radius: radius, color: color)) }
    func appearFade(delay: Double = 0) -> some View { modifier(AppearFade(delay: delay)) }
}
