import SwiftUI

enum ShortLink {
    static func make(_ ref: ItemRef) -> String {
        "\(ref.repo.owner)/\(ref.repo.name)#\(ref.number)"
    }

    static func make(url: String) -> String? {
        ref(url: url).map(make)
    }

    static func ref(url: String) -> ItemRef? {
        switch GitHubURL.parse(url, allowShort: false) {
        case .pr(let ref), .issue(let ref): return ref
        default: return nil
        }
    }

    static func ref(route: Route?) -> ItemRef? {
        switch route {
        case .pr(let ref), .issue(let ref): return ref
        default: return nil
        }
    }
}

struct ShortLinkButton: View {
    let ref: ItemRef
    @Environment(AppModel.self) private var app

    var body: some View {
        Button { app.copyShortLink(ref) } label: {
            Image(systemName: "link").font(.callout.weight(.semibold)).foregroundStyle(Color.syMuted)
        }
        .buttonStyle(.borderless)
        .help("Copy short link \(ShortLink.make(ref)) (⇧⌘C)")
        .accessibilityLabel("Copy short link")
    }
}

struct ToastBanner: View {
    var bottomInset: CGFloat?
    @Environment(AppModel.self) private var app

    var body: some View {
        if let text = app.toast {
            HStack(spacing: 6) {
                Image(systemName: "checkmark.circle.fill").foregroundStyle(Color.syGreen)
                Text(text).foregroundStyle(Color.syText).lineLimit(1)
            }
            .font(.callout)
            .padding(.horizontal, 12)
            .padding(.vertical, 6)
            .background(.regularMaterial, in: Capsule())
            .overlay(Capsule().stroke(Color(nsColor: .separatorColor)))
            .shadow(radius: 6, y: 2)
            .padding(.bottom, bottomInset ?? (app.clipboardRoute == nil && app.rateLimitedUntil == nil ? 16 : 0))
            .allowsHitTesting(false)
            .transition(.move(edge: .bottom).combined(with: .opacity))
            .id(text)
        }
    }
}
