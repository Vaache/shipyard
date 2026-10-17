import AppKit
import SwiftUI

struct ClaudeProfile: Codable, Identifiable, Hashable {
    var id: String
    var label: String
    var configDir: String?
    var email: String?
    var plan: String?
    var owner: String?

    var isDefault: Bool { configDir == nil }
    var initial: String { String((label.isEmpty ? (email ?? "C") : label).prefix(1)).uppercased() }
}

@Observable
@MainActor
final class ClaudeProfiles {
    static let shared = ClaudeProfiles()
    static let defaultID = "default"
    private static let listKey = "claude.profiles"
    private static let activeKey = "claude.activeProfile"
    private static let activeByOwnerKey = "claude.activeProfileByAccount"
    private static let shared_items = ["settings.json", "CLAUDE.md", "RTK.md", "skills", "commands", "plugins", "agents", "output-styles", "keybindings.json"]

    var profiles: [ClaudeProfile]
    var activeID: String
    var scope: String?
    private var activeByOwner: [String: String]

    private init() {
        var list: [ClaudeProfile] = []
        if let data = UserDefaults.standard.data(forKey: Self.listKey), let decoded = try? JSONDecoder().decode([ClaudeProfile].self, from: data) {
            list = decoded
        }
        if !list.contains(where: { $0.id == Self.defaultID }) {
            list.insert(ClaudeProfile(id: Self.defaultID, label: "Main", configDir: nil), at: 0)
        }
        profiles = list
        let saved = UserDefaults.standard.string(forKey: Self.activeKey) ?? Self.defaultID
        activeID = list.contains { $0.id == saved } ? saved : Self.defaultID
        activeByOwner = UserDefaults.standard.dictionary(forKey: Self.activeByOwnerKey) as? [String: String] ?? [:]
    }

    var active: ClaudeProfile { profiles.first { $0.id == activeID } ?? profiles[0] }

    var visible: [ClaudeProfile] {
        guard let scope else { return profiles }
        return profiles.filter { $0.owner == scope }
    }

    func owned(by owner: String) -> [ClaudeProfile] {
        profiles.filter { $0.owner == owner }
    }

    func preferred(for owner: String) -> ClaudeProfile? {
        let mine = owned(by: owner)
        return mine.first { $0.id == activeByOwner[owner] } ?? mine.first { $0.id == activeID } ?? mine.first
    }

    func claimUnowned(for owner: String) -> Bool {
        var claimed = false
        for i in profiles.indices where profiles[i].owner == nil {
            profiles[i].owner = owner
            claimed = true
        }
        if claimed { save() }
        return claimed
    }

    func release(_ id: String) {
        guard let i = profiles.firstIndex(where: { $0.id == id }) else { return }
        profiles[i].owner = nil
        profiles[i].email = nil
        profiles[i].plan = nil
        activeByOwner = activeByOwner.filter { $0.value != id }
        save()
    }

    static var baseDir: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("PullYard/ClaudeAccounts", isDirectory: true)
    }

    static var defaultConfigDir: String {
        if let env = ProcessInfo.processInfo.environment["CLAUDE_CONFIG_DIR"], !env.isEmpty { return env }
        return FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude").path
    }

    func configDir(for profile: ClaudeProfile? = nil) -> String {
        (profile ?? active).configDir ?? Self.defaultConfigDir
    }

    nonisolated static func apply(_ configDir: String?, to env: inout [String: String]) {
        if let configDir {
            env["CLAUDE_CONFIG_DIR"] = configDir
        }
    }

    var activeOverride: String? { active.configDir }

    private func save() {
        if let data = try? JSONEncoder().encode(profiles) { UserDefaults.standard.set(data, forKey: Self.listKey) }
        UserDefaults.standard.set(activeID, forKey: Self.activeKey)
        UserDefaults.standard.set(activeByOwner, forKey: Self.activeByOwnerKey)
    }

    func updateCachedInfo(email: String?, plan: String?) {
        guard let i = profiles.firstIndex(where: { $0.id == activeID }) else { return }
        if profiles[i].email != email || profiles[i].plan != plan {
            profiles[i].email = email
            profiles[i].plan = plan
            save()
        }
    }

    @discardableResult
    func add(label: String, shareSettings: Bool, owner: String? = nil) throws -> ClaudeProfile {
        let id = UUID().uuidString.lowercased()
        let dir = Self.baseDir.appendingPathComponent(id, isDirectory: true)
        let fm = FileManager.default
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        if shareSettings {
            let source = URL(fileURLWithPath: Self.defaultConfigDir)
            for item in Self.shared_items {
                let src = source.appendingPathComponent(item)
                guard fm.fileExists(atPath: src.path) else { continue }
                try? fm.createSymbolicLink(at: dir.appendingPathComponent(item), withDestinationURL: src)
            }
        }
        let name = label.trimmingCharacters(in: .whitespaces)
        let profile = ClaudeProfile(id: id, label: name.isEmpty ? "Account \(profiles.count + 1)" : name, configDir: dir.path, owner: owner ?? scope)
        profiles.append(profile)
        save()
        return profile
    }

    func rename(_ id: String, to label: String) {
        guard let i = profiles.firstIndex(where: { $0.id == id }) else { return }
        let name = label.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty else { return }
        profiles[i].label = name
        save()
    }

    func remove(_ id: String) {
        guard id != Self.defaultID else { return }
        profiles.removeAll { $0.id == id }
        activeByOwner = activeByOwner.filter { $0.value != id }
        if activeID == id { activeID = Self.defaultID }
        save()
    }

    func setActive(_ id: String) {
        guard let profile = profiles.first(where: { $0.id == id }) else { return }
        activeID = id
        if let owner = profile.owner { activeByOwner[owner] = id }
        save()
    }
}

extension AppModel {
    func switchClaudeProfile(to id: String) {
        guard id != ClaudeProfiles.shared.activeID, ClaudeProfiles.shared.visible.contains(where: { $0.id == id }) else { return }
        claude.closeAllForProfileSwitch()
        ClaudeProfiles.shared.setActive(id)
        ClaudeUsageStore.shared.windows = []
        ClaudeUsageStore.shared.subscription = nil
        ClaudeAccountStore.shared.status = .checking
        Task {
            await ClaudeAccountStore.shared.refresh()
            await claude.reloadProjects()
        }
    }

    func removeClaudeProfile(_ id: String) {
        let profiles = ClaudeProfiles.shared
        guard let profile = profiles.visible.first(where: { $0.id == id }), !profile.isDefault else { return }
        if profiles.activeID == id {
            if let other = profiles.visible.first(where: { $0.id != id }) {
                switchClaudeProfile(to: other.id)
            } else if let fresh = try? profiles.add(label: "Claude", shareSettings: false) {
                switchClaudeProfile(to: fresh.id)
            }
        }
        Task {
            await ClaudeAccountStore.shared.logout(configDir: profile.configDir)
            profiles.remove(id)
        }
    }
}

struct ClaudeProfileMenuItems: View {
    @Environment(AppModel.self) private var app
    @Binding var showAdd: Bool
    @Binding var removeTarget: ClaudeProfile?

    var body: some View {
        let profiles = ClaudeProfiles.shared
        Section("Claude accounts") {
            ForEach(profiles.visible) { p in
                Button {
                    app.switchClaudeProfile(to: p.id)
                } label: {
                    let text = p.label + (p.email.map { " — \($0)" } ?? "")
                    if p.id == profiles.activeID { Label(text, systemImage: "checkmark") } else { Text(text) }
                }
            }
        }
        Button("Add account…") { showAdd = true }
        if !profiles.active.isDefault {
            Button("Remove “\(profiles.active.label)”…", role: .destructive) { removeTarget = profiles.active }
        }
    }
}

struct ClaudeProfileSwitcher: View {
    @Environment(AppModel.self) private var app
    @State private var showAdd = false
    @State private var removeTarget: ClaudeProfile?

    var body: some View {
        let active = ClaudeProfiles.shared.active
        Menu {
            ClaudeProfileMenuItems(showAdd: $showAdd, removeTarget: $removeTarget)
        } label: {
            HStack(spacing: 6) {
                ProfileBadge(profile: active, size: 18)
                Text(active.label).lineLimit(1)
                if ClaudeProfiles.shared.visible.count > 1 {
                    Text("\(ClaudeProfiles.shared.visible.count)").font(.caption2).foregroundStyle(Color.syMuted)
                }
            }
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help("Switch Claude account")
        .modifier(ProfileSheets(showAdd: $showAdd, removeTarget: $removeTarget))
    }
}

struct ProfileSheets: ViewModifier {
    @Environment(AppModel.self) private var app
    @Binding var showAdd: Bool
    @Binding var removeTarget: ClaudeProfile?

    func body(content: Content) -> some View {
        content
            .sheet(isPresented: $showAdd) { AddClaudeProfileSheet() }
            .confirmationDialog("Remove account?", isPresented: Binding(get: { removeTarget != nil }, set: { if !$0 { removeTarget = nil } }), presenting: removeTarget) { p in
                Button("Sign out and remove “\(p.label)”", role: .destructive) { app.removeClaudeProfile(p.id) }
                Button("Cancel", role: .cancel) {}
            } message: { p in
                Text("The app signs out of this account and removes it from the list. Its history folder stays on disk: \(p.configDir ?? "")")
            }
    }
}

struct ProfileBadge: View {
    let profile: ClaudeProfile
    var size: CGFloat = 22

    var body: some View {
        let color = profile.isDefault ? Color.syCoral : ProjectPalette.color(for: profile.id)
        ZStack {
            Circle().fill(color.opacity(0.18))
            Circle().strokeBorder(color.opacity(0.35))
            Text(profile.initial).font(.system(size: size * 0.5, weight: .bold, design: .rounded)).foregroundStyle(color)
        }
        .frame(width: size, height: size)
    }
}

struct AddClaudeProfileSheet: View {
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var label = ""
    @State private var share = true
    @State private var error: String?
    private var ownsMain: Bool { ClaudeProfiles.shared.visible.contains { $0.isDefault } }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("New Claude account").font(.headline)
            Text("The account gets its own sign-in, session history and limits. Sign-in opens right after.")
                .font(.callout)
                .foregroundStyle(Color.syMuted)
            TextField("Name, e.g. “Personal” or “Work”", text: $label)
                .textFieldStyle(.roundedBorder)
                .onSubmit(create)
            if ownsMain {
                Toggle("Use my settings, CLAUDE.md, skills and plugins from the main account", isOn: $share)
                    .toggleStyle(.checkbox)
            }
            if let error { Text(error).font(.caption).foregroundStyle(Color.syRed) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Create and sign in", action: create)
                .buttonStyle(.borderedProminent)
                .tint(.syCoral)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func create() {
        do {
            let p = try ClaudeProfiles.shared.add(label: label, shareSettings: share && ownsMain)
            app.switchClaudeProfile(to: p.id)
            app.section = .claude
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
