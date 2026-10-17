import SwiftUI

struct MenuBarLabel: View {
    let app: AppModel

    var body: some View {
        let count = app.reviewRequestCount
        if count > 0 {
            HStack(spacing: 2) {
                Image(systemName: "ferry")
                Text("\(count)")
            }
        } else {
            Image(systemName: "ferry")
        }
    }
}

struct MenuBarView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.openWindow) private var openWindow
    @State private var rows = RowAppearTracker()
    @State private var ready = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Image(nsImage: NSApp.applicationIconImage).resizable().frame(width: 20, height: 20)
                Text("PullYard").font(.headline).foregroundStyle(Color.syText)
                if let u = app.user { Text("@\(u.login)").foregroundStyle(Color.syMuted) }
                Spacer()
                Button { Task { await app.refreshBadges() } } label: { Image(systemName: "arrow.clockwise").foregroundStyle(Color.syMuted) }
                    .buttonStyle(.borderless)
                    .help("Refresh")
            }
            .padding(12)
            .background(Theme.isBrand ? Color.syBar : Color.clear)
            Rectangle().fill(Color.syLine).frame(height: 1)
            if let until = app.rateLimitedUntil {
                Label("GitHub rate limit — live updates paused until \(until.formatted(date: .omitted, time: .shortened))", systemImage: "hourglass")
                    .font(.caption)
                    .foregroundStyle(Color.syMuted)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                Rectangle().fill(Color.syLine).frame(height: 1)
            }
            if let version = Updater.shared.availableVersion {
                Button {
                    openWindow(id: "update")
                    NSApp.activate(ignoringOtherApps: true)
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: "arrow.down.circle.fill").foregroundStyle(Color.sySky)
                        Text("Update to \(version)").foregroundStyle(Color.syText)
                        Spacer()
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(MenuRowStyle())
                .padding(6)
                Rectangle().fill(Color.syLine).frame(height: 1)
            }
            if app.phase != .loggedIn {
                Button("Sign in…") { show(nil) }.padding(12)
            } else if ready {
                ScrollView {
                    VStack(alignment: .leading, spacing: 2) {
                        section("Review requests", count: app.reviewRequestCount, items: app.reviewRequests, filter: .reviewRequested)
                        section("My open \(app.provider.requestShort)s", count: app.myPRCount, items: app.myPRs, filter: .mine)
                        if app.forge == nil {
                            Button {
                                app.section = .notifications
                                show(nil)
                            } label: {
                                HStack(spacing: 8) {
                                    IconTile(systemImage: "bell.fill", color: .syAmber, size: 16)
                                    Text("Notifications").foregroundStyle(Color.syText)
                                    Spacer()
                                    if app.unreadCount > 0 { ChipView(text: app.unreadCount >= 50 ? "50+" : "\(app.unreadCount)", color: .syAmber) }
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(MenuRowStyle())
                        }
                    }
                    .padding(6)
                }
                .frame(maxHeight: 460)
            } else {
                Color.clear.frame(height: 120)
            }
            Rectangle().fill(Color.syLine).frame(height: 1)
            HStack {
                Button("Open window") { show(nil) }
                Spacer()
                Button("Buy me a pizza 🍕") { openInBrowser(Support.pizzaURL) }.foregroundStyle(Color.syMuted)
                Button("Quit") { NSApp.terminate(nil) }.foregroundStyle(Color.syMuted)
            }
            .buttonStyle(.borderless)
            .padding(12)
            .background(Theme.isBrand ? Color.syBar : Color.clear)
        }
        .frame(width: 380)
        .surface(.panel)
        .overlay(alignment: .bottom) { ToastBanner(bottomInset: 54) }
        .onAppear {
            rows.ingest((app.reviewRequests + app.myPRs).map(\.id), mode: .cascade)
            ready = true
        }
        .onChange(of: (app.reviewRequests + app.myPRs).map(\.id)) { _, ids in
            rows.ingest(ids, mode: .refresh)
        }
    }

    @ViewBuilder
    private func section(_ title: String, count: Int, items: [SearchItem], filter: PRFilter) -> some View {
        Button {
            app.section = .prs(filter)
            show(nil)
        } label: {
            HStack(spacing: 6) {
                IconTile(systemImage: filter.tileIcon, color: filter.tint, size: 16)
                Text(title.uppercased()).font(.caption.weight(.semibold)).foregroundStyle(Color.syMuted)
                Spacer()
                Text("\(count)").font(.caption.weight(.semibold)).foregroundStyle(count > 0 ? filter.tint : Color.syDim)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(MenuRowStyle())
        if items.isEmpty {
            Text("Nothing here").font(.callout).foregroundStyle(Color.syDim).padding(.horizontal, 8).padding(.bottom, 6)
        }
        ForEach(items) { item in
            Button { open(item) } label: {
                HStack(alignment: .top, spacing: 8) {
                    StateBadge(state: item.visualState, compact: true)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(item.title).fontWeight(.medium).foregroundStyle(Color.syText).lineLimit(1)
                        Text("\(item.repository.nameWithOwner)\(app.provider.numberPrefix)\(item.number) · \(item.author?.login ?? "") · \(Fmt.rel(item.updatedAt))")
                            .font(.caption).foregroundStyle(Color.syMuted).lineLimit(1)
                    }
                    Spacer(minLength: 4)
                    CIIcon(state: item.ciState).font(.caption)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(MenuRowStyle())
            .contextMenu {
                Button("Open in Browser") { openInBrowser(item.url) }
                Button("Copy Link") { copyToPasteboard(item.url) }
                if app.forge == nil {
                    Button("Copy Short Link") { app.copyShortLink(item.ref) }
                }
            }
            .rowAppear(item.id, rows, bleed: EdgeInsets(top: 0, leading: 0, bottom: 0, trailing: 0))
        }
        Spacer().frame(height: 6)
    }

    private func open(_ item: SearchItem) {
        guard app.forge != nil else { return show(item.route) }
        openWindow(id: "main")
        app.forgeSelection = item
        NSApp.activate(ignoringOtherApps: true)
    }

    private func show(_ route: Route?) {
        openWindow(id: "main")
        if let route { app.select(route) }
        NSApp.activate(ignoringOtherApps: true)
    }
}

struct MenuRowStyle: ButtonStyle {
    @State private var hover = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 8)
            .padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: 7).fill(hover || configuration.isPressed ? Color.syAccent.opacity(0.14) : .clear))
            .onHover { hover = $0 }
    }
}
