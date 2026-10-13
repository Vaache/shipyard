import AppKit
import SwiftUI

struct ClaudeProfile: Codable, Identifiable, Hashable {
    var id: String
    var label: String
    var configDir: String?
    var email: String?
    var plan: String?

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
    private static let shared_items = ["settings.json", "CLAUDE.md", "RTK.md", "skills", "commands", "plugins", "agents", "output-styles", "keybindings.json"]

    var profiles: [ClaudeProfile]
    var activeID: String

    private init() {
        var list: [ClaudeProfile] = []
        if let data = UserDefaults.standard.data(forKey: Self.listKey), let decoded = try? JSONDecoder().decode([ClaudeProfile].self, from: data) {
            list = decoded
        }
        if !list.contains(where: { $0.id == Self.defaultID }) {
            list.insert(ClaudeProfile(id: Self.defaultID, label: t("Основной", "Main"), configDir: nil), at: 0)
        }
        profiles = list
        let saved = UserDefaults.standard.string(forKey: Self.activeKey) ?? Self.defaultID
        activeID = list.contains { $0.id == saved } ? saved : Self.defaultID
    }

    var active: ClaudeProfile { profiles.first { $0.id == activeID } ?? profiles[0] }

    static var baseDir: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("GitHubLite/ClaudeAccounts", isDirectory: true)
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
    func add(label: String, shareSettings: Bool) throws -> ClaudeProfile {
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
        let profile = ClaudeProfile(id: id, label: name.isEmpty ? t("Аккаунт \(profiles.count + 1)", "Account \(profiles.count + 1)") : name, configDir: dir.path)
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
        if activeID == id { activeID = Self.defaultID }
        save()
    }

    func setActive(_ id: String) {
        guard profiles.contains(where: { $0.id == id }) else { return }
        activeID = id
        save()
    }
}

extension AppModel {
    func switchClaudeProfile(to id: String) {
        guard id != ClaudeProfiles.shared.activeID else { return }
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
        guard let profile = profiles.profiles.first(where: { $0.id == id }), !profile.isDefault else { return }
        if profiles.activeID == id { switchClaudeProfile(to: ClaudeProfiles.defaultID) }
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
        Section(t("Аккаунты Claude", "Claude accounts")) {
            ForEach(profiles.profiles) { p in
                Button {
                    app.switchClaudeProfile(to: p.id)
                } label: {
                    let text = p.label + (p.email.map { " — \($0)" } ?? "")
                    if p.id == profiles.activeID { Label(text, systemImage: "checkmark") } else { Text(text) }
                }
            }
        }
        Button(t("Добавить аккаунт…", "Add account…")) { showAdd = true }
        if !profiles.active.isDefault {
            Button(t("Убрать «\(profiles.active.label)»…", "Remove “\(profiles.active.label)”…"), role: .destructive) { removeTarget = profiles.active }
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
                if ClaudeProfiles.shared.profiles.count > 1 {
                    Text("\(ClaudeProfiles.shared.profiles.count)").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .help(t("Переключить аккаунт Claude", "Switch Claude account"))
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
            .confirmationDialog(t("Убрать аккаунт?", "Remove account?"), isPresented: Binding(get: { removeTarget != nil }, set: { if !$0 { removeTarget = nil } }), presenting: removeTarget) { p in
                Button(t("Выйти и убрать «\(p.label)»", "Sign out and remove “\(p.label)”"), role: .destructive) { app.removeClaudeProfile(p.id) }
                Button(t("Отмена", "Cancel"), role: .cancel) {}
            } message: { p in
                Text(t("Приложение выйдет из этого аккаунта и уберёт его из списка. Папка с его историей останется: \(p.configDir ?? "")", "The app signs out of this account and removes it from the list. Its history folder stays on disk: \(p.configDir ?? "")"))
            }
    }
}

struct ProfileBadge: View {
    let profile: ClaudeProfile
    var size: CGFloat = 22

    var body: some View {
        let color = profile.isDefault ? Color.orange : ProjectPalette.color(for: profile.id)
        ZStack {
            Circle().fill(color.opacity(0.2))
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

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(t("Новый аккаунт Claude", "New Claude account")).font(.headline)
            Text(t("У аккаунта будет свой вход, своя история сессий и свои лимиты. После создания откроется вход.", "The account gets its own sign-in, session history and limits. Sign-in opens right after."))
                .font(.callout)
                .foregroundStyle(.secondary)
            TextField(t("Название, например «Личный» или «Работа»", "Name, e.g. “Personal” or “Work”"), text: $label)
                .textFieldStyle(.roundedBorder)
                .onSubmit(create)
            Toggle(t("Использовать мои настройки, CLAUDE.md, скиллы и плагины из основного аккаунта", "Use my settings, CLAUDE.md, skills and plugins from the main account"), isOn: $share)
                .toggleStyle(.checkbox)
            if let error { Text(error).font(.caption).foregroundStyle(.red) }
            HStack {
                Spacer()
                Button(t("Отмена", "Cancel")) { dismiss() }.keyboardShortcut(.cancelAction)
                Button(t("Создать и войти", "Create and sign in"), action: create)
                .buttonStyle(.borderedProminent)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 460)
    }

    private func create() {
        do {
            let p = try ClaudeProfiles.shared.add(label: label, shareSettings: share)
            app.switchClaudeProfile(to: p.id)
            app.section = .claude
            dismiss()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
