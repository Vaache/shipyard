import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var app
    @Environment(\.openWindow) private var openWindow
    @State private var splashHold = true

    var body: some View {
        Group {
            if splashHold || app.phase == .checking {
                SplashView().transition(.opacity)
            } else if app.phase == .loggedOut {
                LoginView().transition(.rise)
            } else {
                MainView().transition(.opacity)
            }
        }
        .animation(Motion.smooth, value: app.phase)
        .animation(Motion.smooth, value: splashHold)
        .task {
            try? await Task.sleep(nanoseconds: 750_000_000)
            splashHold = false
        }
        .task { await Updater.shared.checkOnLaunch() }
        .onChange(of: Updater.shared.presentRequest) { openWindow(id: "update") }
        .environment(\.locale, .app)
    }
}

struct ThemedRoot<Content: View>: View {
    @AppStorage(Theme.styleKey) private var style = ThemeStyle.shipyard.rawValue
    @AppStorage(Theme.appearanceKey) private var appearance = AppearanceMode.auto.rawValue
    @ViewBuilder let content: Content

    var body: some View {
        let current = ThemeStyle(rawValue: style) ?? .shipyard
        let _ = Theme.style = current
        content
            .tint(Color.syAccent)
            .id(current)
            .onAppear { Theme.applyAppearance() }
            .onChange(of: style) { Theme.applyAppearance() }
            .onChange(of: appearance) { Theme.applyAppearance() }
    }
}

struct MainView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        @Bindable var app = app
        NavigationSplitView {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 300)
                .surface(.sidebar)
        } content: {
            ContentColumn()
                .id(app.section)
                .transition(.opacity)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .animation(Motion.fade, value: app.section)
                .surface(.list)
                .navigationSplitViewColumnWidth(min: 300, ideal: 380, max: 560)
        } detail: {
            Group {
                if app.section == .claude {
                    ClaudeWorkspaceView()
                        .surface(.panel)
                        .transition(.opacity)
                } else {
                    NavigationStack(path: $app.detailPath) {
                        Group {
                            if let root = app.detailRoot {
                                RouteView(route: root).id(root).transition(.rise)
                            } else {
                                VStack(spacing: 12) {
                                    IconTile(systemImage: "arrow.triangle.pull", color: .syViolet, size: 56)
                                        .opacity(0.9)
                                    Text("Select a pull request, issue or project").foregroundStyle(Color.syMuted)
                                    Text("⌘L — open by link").font(.caption).foregroundStyle(Color.syDim)
                                }
                                .frame(maxWidth: .infinity, maxHeight: .infinity)
                                .transition(.opacity)
                            }
                        }
                        .animation(Motion.smooth, value: app.detailRoot)
                        .navigationDestination(for: Route.self) { RouteView(route: $0) }
                    }
                    .surface(.panel)
                    .transition(.opacity)
                }
            }
            .animation(Motion.fade, value: app.section == .claude)
        }
        .modifier(BrandToolbar())
        .sheet(isPresented: $app.showOpenLink) { OpenLinkSheet() }
        .overlay(alignment: .bottom) { ClipboardBanner() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            app.checkClipboard()
            app.didBecomeActive()
        }
        .onAppear { app.checkClipboard() }
    }
}

struct RouteView: View {
    let route: Route

    var body: some View {
        switch route {
        case .pr(let ref): PRDetailView(ref: ref)
        case .issue(let ref): IssueDetailView(ref: ref)
        case .repo(let repo): RepoDetailView(repo: repo)
        case .project(let p): ProjectBoardView(project: p)
        case .commit(let repo, let sha): CommitDetailView(repo: repo, sha: sha)
        }
    }
}

struct SidebarView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        List {
            Section("Inbox") {
                SidebarRow(section: .notifications, title: "Notifications", icon: "bell.fill", color: .syAmber, count: app.unreadCount)
            }
            Section("Pull requests") {
                ForEach(PRFilter.allCases) { f in
                    SidebarRow(section: .prs(f), title: f.title, icon: f.tileIcon, color: f.tint, count: f == .reviewRequested ? app.reviewRequestCount : (f == .mine ? app.myPRCount : 0))
                }
            }
            Section("Issues") {
                ForEach(IssueFilter.allCases) { f in
                    SidebarRow(section: .issues(f), title: f.title, icon: f.tileIcon, color: f.tint)
                }
            }
            Section("Code") {
                SidebarRow(section: .repos, title: "Repositories", icon: "book.closed.fill", color: .syTeal)
                SidebarRow(section: .projects, title: "Projects", icon: "rectangle.split.3x1.fill", color: .syIndigo)
            }
            Section("AI") {
                SidebarRow(section: .claude, title: "Claude Code", icon: "sparkles", color: .syCoral, count: app.claude.attentionCount > 0 ? app.claude.attentionCount : app.claude.tabs.filter(\.running).count)
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            VStack(spacing: 0) {
                if let version = Updater.shared.availableVersion {
                    UpdateAvailableRow(version: version)
                }
                userFooter
            }
        }
    }

    @ViewBuilder
    private var userFooter: some View {
        if let user = app.user {
            HStack(spacing: 8) {
                Avatar(url: user.avatarUrl, size: 24)
                VStack(alignment: .leading, spacing: 0) {
                    Text(user.name ?? user.login).font(.callout.weight(.medium)).lineLimit(1)
                    Text("@\(user.login)").font(.caption).foregroundStyle(Color.syMuted)
                }
                Spacer()
                SettingsLink { Image(systemName: "gearshape") }.buttonStyle(.borderless)
            }
            .padding(10)
            .background(Theme.isBrand ? AnyShapeStyle(Color.sySidebar) : AnyShapeStyle(.clear))
            .overlay(alignment: .top) { if Theme.isBrand { Rectangle().fill(Color.syLine).frame(height: 1) } }
        }
    }
}

struct UpdateAvailableRow: View {
    let version: String

    var body: some View {
        Button { Updater.shared.present() } label: {
            HStack(spacing: 8) {
                Image(systemName: "arrow.down.circle.fill").foregroundStyle(Color.sySky)
                Text("Update to \(version)").foregroundStyle(Color.syText)
                Spacer()
                Image(systemName: "chevron.right").font(.caption2.weight(.semibold)).foregroundStyle(Color.syDim)
            }
            .font(.callout.weight(.medium))
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.sySky.opacity(0.12)))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .padding(.horizontal, 8)
        .padding(.bottom, 8)
        .help("A new version of Shipyard is available")
    }
}

struct SidebarRow: View {
    @Environment(AppModel.self) private var app
    let section: SidebarSection
    let title: String
    let icon: String
    let color: Color
    var count = 0
    @State private var hovering = false

    var body: some View {
        let selected = app.section == section
        HStack(spacing: 8) {
            SidebarIcon(systemImage: icon, color: color)
            Text(title)
                .foregroundStyle(Color.syText)
                .fontWeight(selected ? .medium : .regular)
                .lineLimit(1)
            Spacer(minLength: 4)
            if count > 0 {
                Text("\(count)").font(.callout.monospacedDigit()).foregroundStyle(Color.syMuted)
            }
        }
        .padding(.leading, 9)
        .padding(.trailing, 8)
        .padding(.vertical, 5)
        .background {
            ZStack(alignment: .leading) {
                Color.syText.opacity(selected ? 0.06 : hovering ? 0.035 : 0)
                if selected { color.frame(width: 3) }
            }
            .clipShape(RoundedRectangle(cornerRadius: 7))
        }
        .contentShape(Rectangle())
        .onTapGesture { app.section = section }
        .onHover { hovering = $0 }
        .animation(Motion.fade, value: selected)
        .listRowInsets(EdgeInsets(top: 1, leading: 6, bottom: 1, trailing: 6))
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
    }
}

struct SidebarIcon: View {
    let systemImage: String
    let color: Color

    var body: some View {
        Image(systemName: systemImage.hasSuffix(".fill") ? String(systemImage.dropLast(5)) : systemImage)
            .font(.system(size: 10.5, weight: .semibold))
            .foregroundStyle(color)
            .frame(width: 21, height: 21)
            .background(RoundedRectangle(cornerRadius: 6).fill(Color.syText.opacity(0.05)))
            .overlay(
                RoundedRectangle(cornerRadius: 6)
                    .strokeBorder(LinearGradient(colors: [Color.white.opacity(0.28), .clear], startPoint: .top, endPoint: UnitPoint(x: 0.5, y: 0.35)), lineWidth: 1)
            )
    }
}

struct BrandToolbar: ViewModifier {
    func body(content: Content) -> some View {
        if Theme.isBrand {
            content.toolbarBackground(Color.syBar, for: .windowToolbar)
        } else {
            content
        }
    }
}

extension PRFilter {
    var tint: Color {
        switch self {
        case .reviewRequested: return .sySky
        case .mine: return .syViolet
        case .assigned: return .syTeal
        case .mentioned: return .syIndigo
        case .reviewed: return .syGreen
        }
    }

    var tileIcon: String {
        switch self {
        case .reviewRequested: return "eye.fill"
        case .mine: return "person.fill"
        case .assigned: return "person.badge.clock.fill"
        case .mentioned: return "at"
        case .reviewed: return "checkmark.bubble.fill"
        }
    }
}

extension IssueFilter {
    var tint: Color {
        switch self {
        case .assigned: return .syTeal
        case .mine: return .syGreen
        case .mentioned: return .syIndigo
        }
    }

    var tileIcon: String {
        switch self {
        case .assigned: return "person.badge.clock.fill"
        case .mine: return "smallcircle.filled.circle"
        case .mentioned: return "at"
        }
    }
}

struct ContentColumn: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        switch app.section {
        case .notifications:
            NotificationsView()
        case .prs(let f):
            SearchListView(title: f.title, kind: "pr", baseQuery: f.query).id("pr-\(f.rawValue)")
        case .issues(let f):
            SearchListView(title: "Issues: \(f.title)", kind: "issue", baseQuery: f.query).id("issue-\(f.rawValue)")
        case .repos:
            ReposView()
        case .projects:
            ProjectsListView()
        case .claude:
            ClaudeProjectsColumn()
        case nil:
            Text("Select a section").foregroundStyle(Color.syMuted)
        }
    }
}

struct OpenLinkSheet: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var invalid = false

    var body: some View {
        SheetFrame(title: "Open by link") {
            TextField("https://github.com/org/repo/pull/123 or org/repo#123", text: $text)
                .textFieldStyle(.roundedBorder)
                .onSubmit(open)
            if invalid { Text("Couldn't recognize the link").font(.caption).foregroundStyle(.red) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Open", action: open).buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
            }
        }
        .onAppear {
            if let s = NSPasteboard.general.string(forType: .string), GitHubURL.parse(s) != nil { text = s }
        }
    }

    private func open() {
        guard let r = GitHubURL.parse(text) else {
            invalid = true
            return
        }
        app.open(r)
        dismiss()
    }
}

struct ClipboardBanner: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        if let route = app.clipboardRoute {
            HStack(spacing: 10) {
                Image(systemName: "doc.on.clipboard")
                Text("In clipboard: \(title(route))").lineLimit(1)
                Button("Open") {
                    app.open(route)
                    app.clipboardRoute = nil
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.small)
                Button {
                    app.clipboardRoute = nil
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 8)
            .background(.regularMaterial, in: Capsule())
            .overlay(Capsule().stroke(Color(nsColor: .separatorColor)))
            .shadow(radius: 8, y: 2)
            .padding(.bottom, 16)
            .transition(.move(edge: .bottom).combined(with: .opacity))
        }
    }

    private func title(_ r: Route) -> String {
        switch r {
        case .pr(let ref): return "PR \(ref.title)"
        case .issue(let ref): return "Issue \(ref.title)"
        default: return ""
        }
    }
}
