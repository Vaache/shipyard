import AppKit
import SwiftUI

enum ThemeStyle: String, CaseIterable, Identifiable {
    case pullyard, system
    var id: String { rawValue }

    var title: String {
        switch self {
        case .pullyard: return "PullYard"
        case .system: return "System"
        }
    }
}

enum AppearanceMode: String, CaseIterable, Identifiable {
    case auto, light, dark
    var id: String { rawValue }

    var title: String {
        switch self {
        case .auto: return "Match system"
        case .light: return "Light"
        case .dark: return "Dark"
        }
    }

    var nsAppearance: NSAppearance? {
        switch self {
        case .auto: return nil
        case .light: return NSAppearance(named: .aqua)
        case .dark: return NSAppearance(named: .darkAqua)
        }
    }
}

extension NSColor {
    convenience init(hex: UInt32, alpha: CGFloat = 1) {
        self.init(srgbRed: CGFloat((hex >> 16) & 0xff) / 255, green: CGFloat((hex >> 8) & 0xff) / 255, blue: CGFloat(hex & 0xff) / 255, alpha: alpha)
    }
}

enum Theme {
    static let styleKey = "theme.style"
    static let appearanceKey = "theme.appearance"

    nonisolated(unsafe) static var style: ThemeStyle = ThemeStyle(rawValue: UserDefaults.standard.string(forKey: styleKey) ?? "") ?? .pullyard
    static var isBrand: Bool { style == .pullyard }

    static var appearance: AppearanceMode {
        AppearanceMode(rawValue: UserDefaults.standard.string(forKey: appearanceKey) ?? "") ?? .auto
    }

    @MainActor
    static func applyAppearance() {
        NSApp.appearance = appearance.nsAppearance
        for window in NSApp.windows where window.styleMask.contains(.titled) && !(window is NSPanel) {
            window.backgroundColor = isBrand ? NSColor.syWindow : .windowBackgroundColor
        }
    }

    fileprivate static func color(_ name: String, light: NSColor, dark: NSColor, system: NSColor) -> NSColor {
        NSColor(name: NSColor.Name("sy." + name)) { appearance in
            guard isBrand else { return system }
            return appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
        }
    }

    fileprivate static func color(_ name: String, light: UInt32, dark: UInt32, lightAlpha: CGFloat = 1, darkAlpha: CGFloat = 1, system: NSColor) -> NSColor {
        color(name, light: NSColor(hex: light, alpha: lightAlpha), dark: NSColor(hex: dark, alpha: darkAlpha), system: system)
    }
}

extension NSColor {
    static let syWindow = Theme.color("window", light: 0xF7F5F0, dark: 0x0C1220, system: .windowBackgroundColor)
    static let sySidebar = Theme.color("sidebar", light: 0xEFECE5, dark: 0x0A0F1B, system: .clear)
    static let syList = Theme.color("list", light: 0xF3F1EC, dark: 0x0E1526, system: .clear)
    static let syPanel = Theme.color("panel", light: 0xFDFCFA, dark: 0x101828, system: .clear)
    static let syRaised = Theme.color("raised", light: 0xFFFFFF, dark: 0x152036, system: .controlBackgroundColor)
    static let sySunken = Theme.color("sunken", light: 0xF1EEE8, dark: 0x0A101C, system: .textBackgroundColor)
    static let syBar = Theme.color("bar", light: 0xF5F3EE, dark: 0x111A2C, system: .windowBackgroundColor)
    static let syLine = Theme.color("line", light: 0x1A2333, dark: 0xA0AEC4, lightAlpha: 0.08, darkAlpha: 0.10, system: .separatorColor)
    static let syLineStrong = Theme.color("lineStrong", light: 0x1A2333, dark: 0xA0AEC4, lightAlpha: 0.15, darkAlpha: 0.18, system: .separatorColor)

    static let syText = Theme.color("text", light: 0x1A2333, dark: 0xE6EBF2, system: .labelColor)
    static let syMuted = Theme.color("muted", light: 0x5F6B7E, dark: 0x8693A8, system: .secondaryLabelColor)
    static let syDim = Theme.color("dim", light: 0x939AA8, dark: 0x5F6B80, system: .tertiaryLabelColor)

    static let syAccent = Theme.color("accent", light: 0x2A8BC8, dark: 0x5BB8EC, system: .controlAccentColor)
    static let sySky = Theme.color("sky", light: 0x1F7FB8, dark: 0x5BB8EC, system: .systemBlue)
    static let syViolet = Theme.color("violet", light: 0x6B55D6, dark: 0x9D8CF0, system: NSColor(srgbRed: 0.55, green: 0.36, blue: 0.9, alpha: 1))
    static let syCoral = Theme.color("coral", light: 0xD8513F, dark: 0xEE6F5F, system: .systemPink)
    static let syGreen = Theme.color("green", light: 0x2E8A57, dark: 0x5DBE8A, system: NSColor(srgbRed: 0.12, green: 0.6, blue: 0.27, alpha: 1))
    static let syRed = Theme.color("red", light: 0xC2412F, dark: 0xE8746A, system: NSColor(srgbRed: 0.85, green: 0.2, blue: 0.22, alpha: 1))
    static let syAmber = Theme.color("amber", light: 0xB7791F, dark: 0xE8B44C, system: NSColor(srgbRed: 0.8, green: 0.6, blue: 0.1, alpha: 1))
    static let syTeal = Theme.color("teal", light: 0x23877A, dark: 0x56C2B0, system: .systemTeal)
    static let syIndigo = Theme.color("indigo", light: 0x4E5BC4, dark: 0x8C97E8, system: .systemIndigo)
    static let sySlate = Theme.color("slate", light: 0x6B7486, dark: 0x939CB0, system: .systemGray)

    static let syAddBG = Theme.color("addBG", light: 0x2E8A57, dark: 0x5DBE8A, lightAlpha: 0.11, darkAlpha: 0.10, system: NSColor.systemGreen.withAlphaComponent(0.14))
    static let syDelBG = Theme.color("delBG", light: 0xC2412F, dark: 0xE8746A, lightAlpha: 0.10, darkAlpha: 0.11, system: NSColor.systemRed.withAlphaComponent(0.14))
    static let syHunkBG = Theme.color("hunkBG", light: 0x1F7FB8, dark: 0x5BB8EC, lightAlpha: 0.08, darkAlpha: 0.08, system: NSColor.controlAccentColor.withAlphaComponent(0.08))
    static let syAddText = Theme.color("addText", light: 0x1E5A3A, dark: 0xC2E9D2, system: .textColor)
    static let syDelText = Theme.color("delText", light: 0x7A2A20, dark: 0xF3C4BE, system: .textColor)
    static let syHunkText = Theme.color("hunkText", light: 0x1F6E9E, dark: 0x8CCBEE, system: .secondaryLabelColor)
    static let syGutter = Theme.color("gutter", light: 0x9AA2B0, dark: 0x4A5670, system: .tertiaryLabelColor)
}

extension Color {
    static let syWindow = Color(nsColor: .syWindow)
    static let sySidebar = Color(nsColor: .sySidebar)
    static let syList = Color(nsColor: .syList)
    static let syPanel = Color(nsColor: .syPanel)
    static let syRaised = Color(nsColor: .syRaised)
    static let sySunken = Color(nsColor: .sySunken)
    static let syBar = Color(nsColor: .syBar)
    static let syLine = Color(nsColor: .syLine)
    static let syLineStrong = Color(nsColor: .syLineStrong)
    static let syText = Color(nsColor: .syText)
    static let syMuted = Color(nsColor: .syMuted)
    static let syDim = Color(nsColor: .syDim)
    static let syAccent = Color(nsColor: .syAccent)
    static let sySky = Color(nsColor: .sySky)
    static let syViolet = Color(nsColor: .syViolet)
    static let syCoral = Color(nsColor: .syCoral)
    static let syGreen = Color(nsColor: .syGreen)
    static let syRed = Color(nsColor: .syRed)
    static let syAmber = Color(nsColor: .syAmber)
    static let syTeal = Color(nsColor: .syTeal)
    static let syIndigo = Color(nsColor: .syIndigo)
    static let sySlate = Color(nsColor: .sySlate)
    static let syAddBG = Color(nsColor: .syAddBG)
    static let syDelBG = Color(nsColor: .syDelBG)
}

enum Surface {
    case sidebar, list, panel, raised, sunken, bar

    var color: Color {
        switch self {
        case .sidebar: return .sySidebar
        case .list: return .syList
        case .panel: return .syPanel
        case .raised: return .syRaised
        case .sunken: return .sySunken
        case .bar: return .syBar
        }
    }
}

struct SurfaceBackground: ViewModifier {
    let surface: Surface

    func body(content: Content) -> some View {
        if Theme.isBrand {
            content.scrollContentBackground(.hidden).background(surface.color)
        } else {
            content
        }
    }
}

struct CardStyle: ViewModifier {
    var radius: CGFloat = 10
    var fill: Color = .syRaised

    func body(content: Content) -> some View {
        content
            .background(RoundedRectangle(cornerRadius: radius).fill(fill))
    }
}

extension View {
    func surface(_ s: Surface) -> some View { modifier(SurfaceBackground(surface: s)) }
    func card(radius: CGFloat = 10, fill: Color = .syRaised) -> some View { modifier(CardStyle(radius: radius, fill: fill)) }
}

struct IconTile: View {
    let systemImage: String
    let color: Color
    var size: CGFloat = 20

    var body: some View {
        Image(systemName: systemImage)
            .font(.system(size: size * 0.55, weight: .semibold))
            .foregroundStyle(color)
            .frame(width: size, height: size)
            .background(RoundedRectangle(cornerRadius: size * 0.28).fill(color.opacity(0.14)))
    }
}

struct ChipView: View {
    let text: String
    var color: Color = .syMuted
    var icon: String?
    var mono = false

    var body: some View {
        HStack(spacing: 4) {
            if let icon { Image(systemName: icon).imageScale(.small) }
            Text(text).lineLimit(1).truncationMode(.middle)
        }
        .font(mono ? .caption.monospaced() : .caption.weight(.medium))
        .foregroundStyle(color)
        .padding(.horizontal, 7)
        .padding(.vertical, 2)
        .background(Capsule().fill(color.opacity(0.12)))
    }
}
