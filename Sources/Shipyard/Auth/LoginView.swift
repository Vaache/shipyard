import SwiftUI

struct LoginView: View {
    @Environment(AppModel.self) private var app
    @AppStorage("oauthClientID") private var clientID = ""
    @State private var token = ""
    @State private var busy = false
    @State private var error: String?
    @State private var device: DeviceCode?
    @State private var deviceTask: Task<Void, Never>?
    @State private var showDevice = false

    var body: some View {
        ScrollView {
            VStack(spacing: 18) {
                VStack(spacing: 6) {
                    Image(nsImage: NSApp.applicationIconImage)
                        .resizable()
                        .frame(width: 104, height: 104)
                        .shadow(color: Color.syAccent.opacity(0.25), radius: 18, y: 6)
                    Text("Shipyard").font(.largeTitle.weight(.bold)).foregroundStyle(Color.syText)
                    Text(t("Быстрый просмотр PR без браузера", "Fast PR browsing without a browser")).foregroundStyle(Color.syMuted)
                }
                .padding(.bottom, 6)

                if GHCLI.binary != nil {
                    card {
                        cardTitle("GitHub CLI", icon: "terminal.fill", color: .sySky)
                        Text(t("Использовать токен, под которым вы вошли в `gh`.", "Use the token you're signed in with in `gh`.")).font(.callout).foregroundStyle(Color.syMuted)
                        Button {
                            run { try await app.login(token: try await GHCLI.token()) }
                        } label: {
                            Label(t("Войти через gh", "Sign in with gh"), systemImage: "terminal").frame(maxWidth: .infinity)
                        }
                        .buttonStyle(.borderedProminent)
                        .controlSize(.large)
                    }
                }

                card {
                    cardTitle("Personal Access Token", icon: "key.fill", color: .syAmber)
                    Text(t("Нужны права: repo, read:org, read:project, project, notifications, workflow.", "Required scopes: repo, read:org, read:project, project, notifications, workflow.")).font(.callout).foregroundStyle(Color.syMuted)
                    HStack {
                        SecureField(t("ghp_… или github_pat_…", "ghp_… or github_pat_…"), text: $token)
                            .textFieldStyle(.roundedBorder)
                            .onSubmit(loginWithToken)
                        Button(t("Войти", "Sign in"), action: loginWithToken)
                            .buttonStyle(.borderedProminent)
                            .disabled(token.trimmingCharacters(in: .whitespaces).isEmpty)
                    }
                    Button(t("Создать токен на github.com", "Create a token on github.com")) {
                        openInBrowser("https://github.com/settings/tokens/new?scopes=repo,read:org,read:project,project,notifications,workflow&description=Shipyard")
                    }
                    .buttonStyle(.link)
                }

                card {
                    DisclosureGroup(isExpanded: $showDevice) {
                        VStack(alignment: .leading, spacing: 8) {
                            Text(t("Нужен Client ID своего OAuth App (Settings → Developer settings → OAuth Apps, включите «Enable Device Flow»).", "Requires the Client ID of your own OAuth App (Settings → Developer settings → OAuth Apps, enable “Enable Device Flow”)."))
                                .font(.callout).foregroundStyle(Color.syMuted)
                            TextField("Client ID", text: $clientID).textFieldStyle(.roundedBorder)
                            if let device {
                                HStack(spacing: 12) {
                                    Text(device.userCode)
                                        .font(.system(.title, design: .monospaced).weight(.bold))
                                        .foregroundStyle(Color.syText)
                                        .textSelection(.enabled)
                                        .padding(.horizontal, 10)
                                        .padding(.vertical, 4)
                                        .card(radius: 8, fill: .sySunken)
                                    Button(t("Скопировать и открыть", "Copy and open")) {
                                        copyToPasteboard(device.userCode)
                                        openInBrowser(device.verificationUri)
                                    }
                                    .buttonStyle(.borderedProminent)
                                    ProgressView().controlSize(.small)
                                }
                                Text(t("Введите код на github.com/login/device — вход произойдёт автоматически.", "Enter the code at github.com/login/device — you'll be signed in automatically.")).font(.caption).foregroundStyle(Color.syMuted)
                            } else {
                                Button(t("Получить код", "Get code")) { startDevice() }
                                    .disabled(clientID.trimmingCharacters(in: .whitespaces).isEmpty)
                            }
                        }
                        .padding(.top, 6)
                    } label: {
                        cardTitle(t("Вход через браузер (OAuth Device Flow)", "Sign in via browser (OAuth Device Flow)"), icon: "globe", color: .syIndigo)
                    }
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
            .padding(40)
            .frame(maxWidth: 520)
            .frame(maxWidth: .infinity)
        }
        .overlay(alignment: .topTrailing) {
            Picker("", selection: Binding(get: { Loc.shared.lang }, set: { Loc.shared.set($0) })) {
                ForEach(AppLanguage.allCases) { l in
                    Text(l.title).tag(l)
                }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 160)
            .padding(12)
        }
        .onDisappear { deviceTask?.cancel() }
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

    private func run(_ op: @escaping () async throws -> Void) {
        busy = true
        error = nil
        Task {
            do { try await op() } catch { self.error = error.localizedDescription }
            busy = false
        }
    }

    private func loginWithToken() {
        let t = token.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !t.isEmpty else { return }
        run { try await app.login(token: t) }
    }

    private func startDevice() {
        let id = clientID.trimmingCharacters(in: .whitespaces)
        error = nil
        deviceTask?.cancel()
        deviceTask = Task {
            do {
                let code = try await DeviceFlow.start(clientID: id)
                device = code
                copyToPasteboard(code.userCode)
                openInBrowser(code.verificationUri)
                let token = try await DeviceFlow.poll(clientID: id, code: code)
                try await app.login(token: token)
            } catch is CancellationError {
            } catch {
                self.error = error.localizedDescription
            }
            device = nil
        }
    }
}
