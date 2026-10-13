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
        @Bindable var app = app
        List(selection: $app.section) {
            Section("Inbox") {
                SidebarRow(title: "Notifications", icon: "bell.fill", color: .syAmber)
                    .badge(app.unreadCount)
                    .tag(SidebarSection.notifications)
            }
            Section("Pull requests") {
                ForEach(PRFilter.allCases) { f in
                    SidebarRow(title: f.title, icon: f.tileIcon, color: f.tint)
                        .badge(f == .reviewRequested ? app.reviewRequestCount : (f == .mine ? app.myPRCount : 0))
                        .tag(SidebarSection.prs(f))
                }
            }
            Section("Issues") {
                ForEach(IssueFilter.allCases) { f in
                    SidebarRow(title: f.title, icon: f.tileIcon, color: f.tint).tag(SidebarSection.issues(f))
                }
            }
            Section("Code") {
                SidebarRow(title: "Repositories", icon: "book.closed.fill", color: .syTeal).tag(SidebarSection.repos)
                SidebarRow(title: "Projects", icon: "rectangle.split.3x1.fill", color: .syIndigo).tag(SidebarSection.projects)
            }
            Section("AI") {
                SidebarRow(title: "Claude Code", icon: "sparkles", color: .syCoral)
                    .badge(app.claude.attentionCount > 0 ? app.claude.attentionCount : app.claude.tabs.filter(\.running).count)
                    .tag(SidebarSection.claude)
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
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
}

struct SidebarRow: View {
    let title: String
    let icon: String
    let color: Color

    var body: some View {
        Label {
            Text(title)
        } icon: {
            IconTile(systemImage: icon, color: color, size: 20)
        }
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
