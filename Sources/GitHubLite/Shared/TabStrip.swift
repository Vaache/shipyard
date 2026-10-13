import SwiftUI

struct TabItem<ID: Hashable>: Identifiable {
    let id: ID
    let title: String
    let icon: String
    var badge: String?
    var badgeColor: Color = .secondary
}

struct TabStrip<ID: Hashable>: View {
    let items: [TabItem<ID>]
    @Binding var selection: ID
    var shortcuts = false
    @Namespace private var ns

    var body: some View {
        HStack(spacing: 2) {
            ForEach(Array(items.enumerated()), id: \.element.id) { index, item in
                let selected = item.id == selection
                Button {
                    selection = item.id
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: item.icon).imageScale(.small)
                        Text(item.title)
                        if let badge = item.badge {
                            Text(badge)
                                .font(.caption.monospacedDigit().weight(.semibold))
                                .foregroundStyle(item.badgeColor)
                                .padding(.horizontal, 6)
                                .padding(.vertical, 1)
                                .background(Capsule().fill(Color.secondary.opacity(0.15)))
                        }
                    }
                    .font(.callout.weight(selected ? .semibold : .regular))
                    .foregroundStyle(selected ? Color.primary : Color.secondary)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .contentShape(Rectangle())
                    .overlay(alignment: .bottom) {
                        if selected {
                            Capsule()
                                .fill(Color.accentColor)
                                .frame(height: 2)
                                .matchedGeometryEffect(id: "underline", in: ns)
                        }
                    }
                }
                .buttonStyle(.plain)
                .modifier(OptionalShortcut(key: shortcuts && index < 9 ? Character("\(index + 1)") : nil))
                .help(shortcuts && index < 9 ? "\(item.title) (⌃\(index + 1))" : item.title)
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 8)
        .animation(.easeOut(duration: 0.15), value: selection)
    }
}

struct OptionalShortcut: ViewModifier {
    let key: Character?

    func body(content: Content) -> some View {
        if let key {
            content.keyboardShortcut(KeyEquivalent(key), modifiers: .control)
        } else {
            content
        }
    }
}
