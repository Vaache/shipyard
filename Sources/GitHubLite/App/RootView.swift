import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        Group {
            switch app.phase {
            case .checking: LoadingView()
            case .loggedOut: LoginView()
            case .loggedIn: MainView()
            }
        }
        .environment(\.locale, Loc.shared.lang.locale)
    }
}

struct MainView: View {
    @Environment(AppModel.self) private var app

    var body: some View {
        @Bindable var app = app
        NavigationSplitView {
            SidebarView()
                .navigationSplitViewColumnWidth(min: 190, ideal: 220, max: 300)
        } content: {
            ContentColumn()
                .navigationSplitViewColumnWidth(min: 300, ideal: 380, max: 560)
        } detail: {
            if app.section == .claude {
                ClaudeWorkspaceView()
            } else {
                NavigationStack(path: $app.detailPath) {
                    Group {
                        if let root = app.detailRoot {
                            RouteView(route: root).id(root)
                        } else {
                            VStack(spacing: 10) {
                                Image(systemName: "arrow.triangle.pull").font(.system(size: 42)).foregroundStyle(.tertiary)
                                Text(t("Выберите pull request, issue или проект", "Select a pull request, issue or project")).foregroundStyle(.secondary)
                                Text(t("⌘L — открыть по ссылке", "⌘L — open by link")).font(.caption).foregroundStyle(.tertiary)
                            }
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                        }
                    }
                    .navigationDestination(for: Route.self) { RouteView(route: $0) }
                }
            }
        }
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
            Section(t("Входящие", "Inbox")) {
                Label(t("Уведомления", "Notifications"), systemImage: "bell")
                    .badge(app.unreadCount)
                    .tag(SidebarSection.notifications)
            }
            Section("Pull requests") {
                ForEach(PRFilter.allCases) { f in
                    Label(f.title, systemImage: f.icon)
                        .badge(f == .reviewRequested ? app.reviewRequestCount : (f == .mine ? app.myPRCount : 0))
                        .tag(SidebarSection.prs(f))
                }
            }
            Section("Issues") {
                ForEach(IssueFilter.allCases) { f in
                    Label(f.title, systemImage: f.icon).tag(SidebarSection.issues(f))
                }
            }
            Section(t("Код", "Code")) {
                Label(t("Репозитории", "Repositories"), systemImage: "book.closed").tag(SidebarSection.repos)
                Label(t("Проекты", "Projects"), systemImage: "rectangle.split.3x1").tag(SidebarSection.projects)
            }
            Section("AI") {
                Label("Claude Code", systemImage: "sparkles")
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
                        Text("@\(user.login)").font(.caption).foregroundStyle(.secondary)
                    }
                    Spacer()
                    SettingsLink { Image(systemName: "gearshape") }.buttonStyle(.borderless)
                }
                .padding(10)
            }
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
            Text(t("Выберите раздел", "Select a section")).foregroundStyle(.secondary)
        }
    }
}

struct OpenLinkSheet: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var text = ""
    @State private var invalid = false

    var body: some View {
        SheetFrame(title: t("Открыть по ссылке", "Open by link")) {
            TextField(t("https://github.com/org/repo/pull/123 или org/repo#123", "https://github.com/org/repo/pull/123 or org/repo#123"), text: $text)
                .textFieldStyle(.roundedBorder)
                .onSubmit(open)
            if invalid { Text(t("Не удалось распознать ссылку", "Couldn't recognize the link")).font(.caption).foregroundStyle(.red) }
            HStack {
                Spacer()
                Button(t("Отмена", "Cancel")) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(t("Открыть", "Open"), action: open).buttonStyle(.borderedProminent).keyboardShortcut(.defaultAction)
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
                Text(t("В буфере: \(title(route))", "In clipboard: \(title(route))")).lineLimit(1)
                Button(t("Открыть", "Open")) {
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
