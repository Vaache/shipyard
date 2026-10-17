import SwiftUI

struct LoginView: View {
    var embedded = false
    var onDone: (() -> Void)?
    @Environment(AppModel.self) private var app
    @AppStorage("loginProvider") private var providerRaw = GitProvider.github.rawValue
    @State private var host = ""
    @State private var email = ""
    @State private var token = ""
    @State private var clientID = ""
    @State private var busy = false
    @State private var error: String?
    @State private var device: DeviceCode?
    @State private var deviceTask: Task<Void, Never>?
    @State private var showDevice = false
    @State private var prefilled: GitAccount?

    private var provider: GitProvider { GitProvider(rawValue: providerRaw) ?? .github }

    private var effectiveHost: String {
        provider.hostEditable ? GitProvider.normalizeHost(host) : provider.defaultHost
    }

    private var clientIDKey: String {
        provider == .github ? "oauthClientID" : "oauthClientID.\(provider.rawValue)"
    }

    private var supportsDevice: Bool { provider.isGitHub || provider == .gitlab }

    private var cliBinary: String? {
        switch provider {
        case .github, .githubEnterprise: return GHCLI.binary
        case .gitlab: return GLabCLI.binary
        default: return nil
        }
    }

    var body: some View {
        ScrollView {
            VStack(spacing: 18) {
                if !embedded {
                    VStack(spacing: 6) {
                        Image(nsImage: NSApp.applicationIconImage)
                            .resizable()
                            .frame(width: 104, height: 104)
                            .shadow(color: Color.syAccent.opacity(0.25), radius: 18, y: 6)
                        Text("PullYard").font(.largeTitle.weight(.bold)).foregroundStyle(Color.syText)
                        Text("Fast PR browsing without a browser").foregroundStyle(Color.syMuted)
                    }
                    .padding(.bottom, 6)
                    savedAccounts
                }

                Picker("Service", selection: $providerRaw) {
                    ForEach(GitProvider.allCases) { Text($0.shortTitle).tag($0.rawValue) }
                }
                .pickerStyle(.segmented)
                .labelsHidden()

                if provider.hostEditable {
                    card {
                        cardTitle("\(provider.title) server", icon: "server.rack", color: .syTeal)
                        TextField(provider.hostPlaceholder, text: $host)
                            .textFieldStyle(.roundedBorder)
                    }
                }

                if let _ = cliBinary {
                    card {
                        cardTitle(provider == .gitlab ? "GitLab CLI" : "GitHub CLI", icon: "terminal.fill", color: .sySky)
                        Text(provider == .gitlab ? "Use the token you're signed in with in `glab`." : "Use the token you're signed in with in `gh`.").font(.callout).foregroundStyle(Color.syMuted)
                        Button {
                            signInWithCLI()
                        } label: {
                            Label(provider == .gitlab ? "Sign in with glab" : "Sign in with gh", systemImage: "terminal").frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)
                        .disabled(effectiveHost.isEmpty)
                    }
                }

                card {
                    cardTitle(provider == .bitbucket ? "API token" : "Personal Access Token", icon: "key.fill", color: .syAmber)
                    Text(provider.tokenHint).font(.callout).foregroundStyle(Color.syMuted)
                    if provider == .bitbucket {
                        TextField("Atlassian account email", text: $email)
                            .textFieldStyle(.roundedBorder)
                            .textContentType(.username)
                    }
                    HStack {
                        SecureField(provider.tokenPlaceholder, text: $token)
                            .textFieldStyle(.roundedBorder)
                            .onSubmit(loginWithToken)
                        Button("Sign in", action: loginWithToken)
                            .buttonStyle(.borderedProminent)
                            .disabled(!canSubmitToken)
                    }
                    if let url = provider.tokenCreateURL(host: effectiveHost) {
                        Button(provider == .bitbucket ? "Create an API token at id.atlassian.com" : "Create a token on \(effectiveHost)") {
                            openInBrowser(url)
                        }
                        .buttonStyle(.link)
                    }
                }

                if supportsDevice {
                    card {
                        DisclosureGroup(isExpanded: $showDevice) {
                            deviceContent.padding(.top, 6)
                        } label: {
                            cardTitle("Sign in via browser (OAuth Device Flow)", icon: "globe", color: .syIndigo)
                        }
                    }
                }

                if embedded {
                    Label("Adding an account switches to it. A new account gets its own Claude Code sign-in; the first account keeps the Claude you already use.", systemImage: "info.circle")
                        .font(.caption)
                        .foregroundStyle(Color.syMuted)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }

                if busy { ProgressView().controlSize(.small) }
                if let error {
                    Label(error, systemImage: "exclamationmark.triangle.fill")
                        .foregroundStyle(Color.syRed)
                        .multilineTextAlignment(.center)
                        .textSelection(.enabled)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                        .card(radius: 10, fill: Color.syRed.opacity(0.08))
                }
            }
            .padding(embedded ? 20 : 40)
            .frame(maxWidth: 520)
            .frame(maxWidth: .infinity)
        }
        .onAppear(perform: providerChanged)
        .onChange(of: providerRaw) { providerChanged() }
        .onChange(of: clientID) { UserDefaults.standard.set(clientID, forKey: clientIDKey) }
        .onDisappear { deviceTask?.cancel() }
    }

    @ViewBuilder
    private var savedAccounts: some View {
        let accounts = GitAccounts.shared.accounts
        if !accounts.isEmpty {
            card {
                cardTitle("Your accounts", icon: "person.2.fill", color: .syGreen)
                ForEach(accounts) { a in
                    HStack(spacing: 10) {
                        Avatar(url: a.avatarUrl, size: 24)
                        VStack(alignment: .leading, spacing: 0) {
                            Text(a.name ?? a.login).font(.callout.weight(.medium)).foregroundStyle(Color.syText)
                            Text("\(a.provider.title) · \(a.subtitle)").font(.caption).foregroundStyle(Color.syMuted)
                        }
                        Spacer()
                        if GitAccounts.shared.hasCredential(a) {
                            Button("Continue") { app.requestSwitch(to: a.id) }
                        } else {
                            Button("Sign in again") { prefill(a) }
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var deviceContent: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(deviceHelp).font(.callout).foregroundStyle(Color.syMuted)
            TextField(provider == .gitlab ? "Application ID" : "Client ID", text: $clientID).textFieldStyle(.roundedBorder)
            if let device {
                HStack(spacing: 12) {
                    Text(device.userCode)
                        .font(.system(.title, design: .monospaced).weight(.bold))
                        .foregroundStyle(Color.syText)
                        .textSelection(.enabled)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 4)
                        .card(radius: 8, fill: .sySunken)
                    Button("Copy and open") {
                        copyToPasteboard(device.userCode)
                        openInBrowser(device.verificationUri)
                    }
                    .buttonStyle(.borderedProminent)
                    ProgressView().controlSize(.small)
                }
                Text("Enter the code at \(device.verificationUri) — you'll be signed in automatically.").font(.caption).foregroundStyle(Color.syMuted)
            } else {
                Button("Get code") { startDevice() }
                    .disabled(clientID.trimmingCharacters(in: .whitespaces).isEmpty || effectiveHost.isEmpty)
            }
        }
    }

    private var deviceHelp: String {
        switch provider {
        case .gitlab:
            return "Requires GitLab 17.1 or later and the Application ID of your own OAuth application (User settings → Applications): not confidential, scope api, “Device authorization grant” enabled."
        default:
            return "Requires the Client ID of your own OAuth App (Settings → Developer settings → OAuth Apps, enable “Enable Device Flow”)."
        }
    }

    private var canSubmitToken: Bool {
        let hasToken = !token.trimmingCharacters(in: .whitespaces).isEmpty
        let hasEmail = provider != .bitbucket || !email.trimmingCharacters(in: .whitespaces).isEmpty
        return hasToken && hasEmail && !effectiveHost.isEmpty
    }

    private func card<C: View>(@ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 8, content: content)
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .card(radius: 12)
    }

    private func cardTitle(_ title: String, icon: String, color: Color) -> some View {
        HStack(spacing: 8) {
            IconTile(systemImage: icon, color: color, size: 22)
            Text(title).font(.headline).foregroundStyle(Color.syText)
        }
    }

    private func providerChanged() {
        host = provider.defaultHost
        if let a = prefilled, a.provider == provider {
            host = a.host
            email = a.email ?? ""
        }
        prefilled = nil
        token = ""
        error = nil
        deviceTask?.cancel()
        device = nil
        clientID = UserDefaults.standard.string(forKey: clientIDKey) ?? ""
    }

    private func prefill(_ a: GitAccount) {
        prefilled = a
        if providerRaw == a.provider.rawValue {
            providerChanged()
        } else {
            providerRaw = a.provider.rawValue
        }
    }

    private func run(_ op: @escaping () async throws -> Void) {
        busy = true
        error = nil
        Task {
            do { try await op() } catch { self.error = error.localizedDescription }
            busy = false
        }
    }

    private func finish(_ credential: GitCredential) async throws {
        try await app.addAccount(provider, host: effectiveHost, credential: credential, email: provider == .bitbucket ? email : nil)
        onDone?()
    }

    private func signInWithCLI() {
        let host = effectiveHost
        let provider = provider
        run {
            let t = provider == .gitlab ? try await GLabCLI.token(host: host) : try await GHCLI.token(host: host)
            try await finish(GitCredential(token: t))
        }
    }

    private func loginWithToken() {
        let t = token.trimmingCharacters(in: .whitespacesAndNewlines)
        guard canSubmitToken else { return }
        run { try await finish(GitCredential(token: t)) }
    }

    private func startDevice() {
        let id = clientID.trimmingCharacters(in: .whitespaces)
        let endpoint = provider == .gitlab ? DeviceFlow.gitlab(host: effectiveHost) : DeviceFlow.github(host: effectiveHost)
        let keepRefresh = provider == .gitlab
        error = nil
        deviceTask?.cancel()
        deviceTask = Task {
            do {
                let code = try await DeviceFlow.start(clientID: id, endpoint: endpoint)
                device = code
                copyToPasteboard(code.userCode)
                openInBrowser(code.verificationUri)
                let token = try await DeviceFlow.poll(clientID: id, code: code, endpoint: endpoint)
                var credential = keepRefresh ? token.credential : GitCredential(token: token.accessToken)
                if keepRefresh { credential.clientID = id }
                try await finish(credential)
            } catch is CancellationError {
            } catch {
                self.error = error.localizedDescription
            }
            device = nil
        }
    }
}
