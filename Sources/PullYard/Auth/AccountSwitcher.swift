import SwiftUI

struct AccountMenuItems: View {
    @Environment(AppModel.self) private var app
    @Binding var pendingSwitch: GitAccount?
    @Binding var confirmSignOut: Bool
    @Binding var showAdd: Bool

    var body: some View {
        Section("Accounts") {
            ForEach(GitAccounts.shared.accounts) { a in
                Button {
                    guard a.id != app.account?.id else { return }
                    if app.claude.tabs.contains(where: \.running) {
                        pendingSwitch = a
                    } else {
                        app.requestSwitch(to: a.id)
                    }
                } label: {
                    let text = "\(a.provider.title) — \(a.subtitle)"
                    if a.id == app.account?.id { Label(text, systemImage: "checkmark") } else { Text(text) }
                }
            }
        }
        Button("Add Account…") { showAdd = true }
        Divider()
        Button("Sign Out…", role: .destructive) { confirmSignOut = true }
    }
}

struct AccountDialogs: ViewModifier {
    @Environment(AppModel.self) private var app
    @Binding var pendingSwitch: GitAccount?
    @Binding var confirmSignOut: Bool
    @Binding var showAdd: Bool

    func body(content: Content) -> some View {
        content
            .sheet(isPresented: $showAdd) { AddAccountSheet().environment(app) }
            .confirmationDialog(
                pendingSwitch.map { "Switch to \($0.subtitle)?" } ?? "",
                isPresented: Binding(get: { pendingSwitch != nil }, set: { if !$0 { pendingSwitch = nil } }),
                presenting: pendingSwitch
            ) { a in
                Button("Switch") { app.requestSwitch(to: a.id) }
                Button("Cancel", role: .cancel) {}
            } message: { _ in
                Text("Running Claude sessions stop. Their history stays, and this account's Claude Code sign-in is kept for when you switch back.")
            }
            .confirmationDialog("Sign out of \(app.account?.subtitle ?? "this account")?", isPresented: $confirmSignOut) {
                Button("Sign Out", role: .destructive) {
                    Task { await app.signOut() }
                }
                Button("Cancel", role: .cancel) {}
            } message: {
                Text("The account is removed from PullYard, and the Claude Code accounts linked to it are signed out. Other accounts and their Claude Code sign-ins are not affected.")
            }
    }
}

struct AccountSwitcherFooter: View {
    @Environment(AppModel.self) private var app
    @State private var pendingSwitch: GitAccount?
    @State private var confirmSignOut = false

    var body: some View {
        @Bindable var app = app
        if let user = app.user {
            HStack(spacing: 8) {
                Avatar(url: user.avatarUrl, size: 24)
                VStack(alignment: .leading, spacing: 0) {
                    Text(user.name ?? user.login).font(.callout.weight(.medium)).lineLimit(1)
                    Text(app.accountSubtitle ?? "").font(.caption).foregroundStyle(Color.syMuted).lineLimit(1)
                }
                Spacer()
                Menu {
                    AccountMenuItems(pendingSwitch: $pendingSwitch, confirmSignOut: $confirmSignOut, showAdd: $app.showAddAccount)
                } label: {
                    Image(systemName: "person.2")
                }
                .menuStyle(.borderlessButton)
                .menuIndicator(.hidden)
                .fixedSize()
                .help("Switch account")
                SettingsLink { Image(systemName: "gearshape") }.buttonStyle(.borderless)
            }
            .modifier(AccountDialogs(pendingSwitch: $pendingSwitch, confirmSignOut: $confirmSignOut, showAdd: $app.showAddAccount))
        }
    }
}

struct AddAccountSheet: View {
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Add account").font(.headline)
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            .padding(.horizontal, 20)
            .padding(.top, 16)
            LoginView(embedded: true) { dismiss() }
        }
        .frame(width: 560, height: 620)
        .surface(.panel)
    }
}
