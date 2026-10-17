import SwiftUI

struct UnlockView: View {
    let account: GitAccount
    var title = "Enter PIN"
    var sheet = false
    let onSuccess: () -> Void
    @Environment(AppModel.self) private var app
    @Environment(\.dismiss) private var dismiss
    @State private var pin = ""
    @State private var message: String?
    @State private var waitUntil: Date?
    @State private var blocked = false
    @State private var busy = false
    @FocusState private var focused: Bool

    private var lock: AccountLock { AccountLock.shared }
    private var touchID: Bool { lock.biometricsEnabled(account.id) && SystemAuth.biometricsAvailable && !blocked }

    var body: some View {
        VStack(spacing: 14) {
            Avatar(url: account.avatarUrl, size: 56)
            VStack(spacing: 2) {
                Text(account.name ?? account.login).font(.title3.weight(.semibold)).foregroundStyle(Color.syText)
                Text("\(account.provider.title) · \(account.subtitle)").font(.callout).foregroundStyle(Color.syMuted)
            }
            Text(title).font(.headline).foregroundStyle(Color.syText).padding(.top, 4)
            if blocked {
                Text("Too many wrong PINs. Reset the PIN with your Mac password to continue. This also signs out the Claude Code accounts linked to this account.")
                    .font(.callout)
                    .foregroundStyle(Color.syRed)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 340)
            } else {
                HStack {
                    SecureField("PIN", text: $pin)
                        .textFieldStyle(.roundedBorder)
                        .font(.title3.monospacedDigit())
                        .frame(width: 160)
                        .focused($focused)
                        .onSubmit(submit)
                        .onChange(of: pin) { pin = String(pin.filter(\.isNumber).prefix(AccountLock.pinLength.upperBound)) }
                        .disabled(waiting)
                    Button("Unlock", action: submit)
                        .buttonStyle(.borderedProminent)
                        .disabled(!AccountLock.isValidPIN(pin) || waiting)
                        .keyboardShortcut(.defaultAction)
                }
                if touchID {
                    Button {
                        Task { await tryTouchID() }
                    } label: {
                        Label("Use Touch ID", systemImage: "touchid")
                    }
                }
            }
            if let message {
                Text(message).font(.callout).foregroundStyle(Color.syAmber).multilineTextAlignment(.center)
            }
            TimelineView(.periodic(from: .now, by: 1)) { context in
                if let waitUntil, waitUntil > context.date {
                    Text("Try again in \(Int(waitUntil.timeIntervalSince(context.date).rounded(.up))) s")
                        .font(.callout.monospacedDigit())
                        .foregroundStyle(Color.syMuted)
                }
            }
            HStack(spacing: 16) {
                Button(blocked ? "Reset PIN with Mac password…" : "Forgot PIN?") { Task { await reset() } }
                    .buttonStyle(.link)
                if sheet {
                    Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                } else {
                    Button("Use another account") {
                        app.lockedAccount = nil
                        app.phase = .loggedOut
                    }
                    .buttonStyle(.link)
                }
            }
            .padding(.top, 6)
            if busy { ProgressView().controlSize(.small) }
        }
        .padding(32)
        .frame(width: sheet ? 440 : nil)
        .frame(maxWidth: .infinity, maxHeight: sheet ? nil : .infinity)
        .surface(.panel)
        .onAppear {
            refreshState()
            focused = true
            if touchID { Task { await tryTouchID() } }
        }
    }

    private var waiting: Bool { (waitUntil ?? .distantPast) > Date() }

    private func refreshState() {
        switch lock.state(account.id) {
        case .blocked?: blocked = true
        case .wait(let until)?: waitUntil = until
        default: break
        }
    }

    private func submit() {
        guard AccountLock.isValidPIN(pin), !waiting else { return }
        let result = lock.verify(pin, for: account.id)
        pin = ""
        switch result {
        case .ok:
            finish()
        case .wrong(let left):
            message = left == 1 ? "Wrong PIN. 1 attempt left before a pause." : "Wrong PIN. \(left) attempts left before a pause."
        case .wait(let until):
            waitUntil = until
            message = "Wrong PIN."
        case .blocked:
            blocked = true
            message = nil
        }
    }

    private func tryTouchID() async {
        guard let state = await SystemAuth.biometrics("unlock \(account.subtitle) in PullYard") else { return }
        if lock.biometryMatches(state, for: account.id) {
            finish()
        } else {
            message = "Touch ID changed on this Mac since it was turned on for this account. Enter the PIN; you can turn Touch ID on again in Settings."
        }
    }

    private func reset() async {
        busy = true
        defer { busy = false }
        if await app.resetLock(account) {
            finish()
        }
    }

    private func finish() {
        if sheet { dismiss() }
        onSuccess()
    }
}

struct SetPINSheet: View {
    let account: GitAccount
    @Environment(\.dismiss) private var dismiss
    @State private var pin = ""
    @State private var confirm = ""
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Protect \(account.subtitle) with a PIN").font(.headline)
            Text("PullYard asks for the PIN when it starts with this account and when someone switches to it. Use 4 to 6 digits.")
                .font(.callout)
                .foregroundStyle(Color.syMuted)
            SecureField("New PIN", text: $pin)
                .textFieldStyle(.roundedBorder)
                .onChange(of: pin) { pin = String(pin.filter(\.isNumber).prefix(AccountLock.pinLength.upperBound)) }
            SecureField("Repeat PIN", text: $confirm)
                .textFieldStyle(.roundedBorder)
                .onChange(of: confirm) { confirm = String(confirm.filter(\.isNumber).prefix(AccountLock.pinLength.upperBound)) }
                .onSubmit(save)
            if let error { Text(error).font(.caption).foregroundStyle(Color.syRed) }
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Set PIN", action: save)
                    .buttonStyle(.borderedProminent)
                    .keyboardShortcut(.defaultAction)
                    .disabled(!AccountLock.isValidPIN(pin) || confirm.isEmpty)
            }
        }
        .padding(20)
        .frame(width: 420)
    }

    private func save() {
        guard AccountLock.isValidPIN(pin) else {
            error = "Use 4 to 6 digits."
            return
        }
        guard pin == confirm else {
            error = "The PINs don't match."
            return
        }
        AccountLock.shared.setPIN(pin, for: account.id)
        dismiss()
    }
}

struct AccountLockSettings: View {
    let account: GitAccount
    @Environment(AppModel.self) private var app
    @State private var setting = false
    @State private var removing = false
    @State private var tick = 0

    var body: some View {
        let lock = AccountLock.shared
        let _ = tick
        let protected = lock.isProtected(account.id)
        Toggle("Require PIN for this account", isOn: Binding(get: { protected }, set: { on in
            if on { setting = true } else { removing = true }
        }))
        .sheet(isPresented: $setting, onDismiss: { tick += 1 }) { SetPINSheet(account: account) }
        .sheet(isPresented: $removing, onDismiss: { tick += 1 }) {
            UnlockView(account: account, title: "Enter the PIN to turn it off", sheet: true) {
                lock.removeLock(account.id)
            }
            .environment(app)
        }
        if protected, SystemAuth.biometricsAvailable {
            Toggle("Unlock with Touch ID", isOn: Binding(get: { lock.biometricsEnabled(account.id) }, set: { on in
                Task {
                    if on {
                        if let state = await SystemAuth.biometrics("turn on Touch ID for \(account.subtitle)") {
                            lock.setBiometryState(state, for: account.id)
                        }
                    } else {
                        lock.setBiometryState(nil, for: account.id)
                    }
                    tick += 1
                }
            }))
            Text("Touch ID accepts any fingerprint added on this Mac. If someone else's finger is added later, Touch ID turns off for this account until the PIN is entered.")
                .font(.caption)
                .foregroundStyle(Color.syMuted)
        }
    }
}
