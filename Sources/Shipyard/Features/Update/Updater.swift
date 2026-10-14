import AppKit
import Foundation
import Observation
import UserNotifications

struct Release: Equatable {
    let version: String
    let name: String
    let notes: String
    let pageURL: String
    let publishedAt: Date?
    let dmgURL: URL
    let dmgSize: Int64
}

enum UpdateError: LocalizedError {
    case http(Int)
    case noReleases
    case noAsset
    case download(String)
    case mount(String)
    case noApp
    case copy
    case bundleMismatch
    case versionMismatch(String, String)
    case install(String)
    case cancelled

    var errorDescription: String? {
        switch self {
        case .http(let code):
            return code == 403 || code == 429
                ? "GitHub rate limit reached. Try again later."
                : "GitHub responded with error \(code)."
        case .noReleases: return "No releases found."
        case .noAsset: return "The release has no Shipyard.dmg file."
        case .download(let s): return "Couldn't download the update: \(s)"
        case .mount(let s): return "Couldn't mount the disk image: \(s)"
        case .noApp: return "Shipyard.app wasn't found in the disk image."
        case .copy: return "Couldn't copy the new version."
        case .bundleMismatch: return "The app in the disk image has a different bundle identifier."
        case .versionMismatch(let expected, let found): return "Expected version \(expected), found \(found)."
        case .install(let s): return "Couldn't install the update: \(s)"
        case .cancelled: return "Installation was cancelled. Shipyard needs an administrator password to replace itself in this folder."
        }
    }
}

@Observable
@MainActor
final class Updater {
    enum State: Equatable {
        case idle
        case checking
        case upToDate
        case available(Release)
        case downloading(Double)
        case installing
        case readyToRelaunch
        case failed(String)
    }

    static let shared = Updater()
    static let autoCheckKey = "update.autoCheck"
    static let lastCheckKey = "update.lastCheck"
    static let skippedVersionKey = "update.skippedVersion"
    static let announcedVersionKey = "update.announcedVersion"
    static let checkInterval: TimeInterval = 6 * 3600
    static let releasesPage = "https://github.com/Vaache/shipyard/releases/latest"
    nonisolated static let latestAPI = URL(string: "https://api.github.com/repos/Vaache/shipyard/releases/latest")!
    nonisolated static let userAgent = "Shipyard-macOS"

    private(set) var state: State = .idle
    private(set) var release: Release?
    private(set) var presentRequest = 0
    private var launchCheckDone = false
    private var periodicTask: Task<Void, Never>?
    private var stagedApp: URL?

    let currentVersion = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "0"

    private init() {}

    var isBusy: Bool {
        switch state {
        case .checking, .downloading, .installing: return true
        default: return false
        }
    }

    var pageURL: String { release?.pageURL ?? Self.releasesPage }

    var availableVersion: String? {
        if case .available(let r) = state { return r.version }
        return nil
    }

    func present() {
        presentRequest += 1
    }

    func checkOnLaunch() async {
        guard !launchCheckDone else { return }
        launchCheckDone = true
        let defaults = UserDefaults.standard
        UpdateNotifier.shared.install()
        startPeriodicChecks()
        guard defaults.object(forKey: Self.autoCheckKey) as? Bool ?? true else { return }
        if let last = defaults.object(forKey: Self.lastCheckKey) as? Date, Date().timeIntervalSince(last) < Self.checkInterval { return }
        await check(userInitiated: false)
    }

    private func startPeriodicChecks() {
        guard periodicTask == nil else { return }
        periodicTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 3600 * 1_000_000_000)
                guard let self, !Task.isCancelled else { return }
                let defaults = UserDefaults.standard
                guard defaults.object(forKey: Self.autoCheckKey) as? Bool ?? true else { continue }
                if let last = defaults.object(forKey: Self.lastCheckKey) as? Date, Date().timeIntervalSince(last) < Self.checkInterval { continue }
                if case .available = self.state { continue }
                await self.check(userInitiated: false)
            }
        }
    }

    func check(userInitiated: Bool) async {
        switch state {
        case .checking, .downloading, .installing, .readyToRelaunch:
            if userInitiated { presentRequest += 1 }
            return
        default:
            break
        }
        state = .checking
        do {
            let latest = try await Self.fetchLatest()
            UserDefaults.standard.set(Date(), forKey: Self.lastCheckKey)
            release = latest
            if Self.compare(latest.version, currentVersion) == .orderedDescending {
                state = .available(latest)
                let defaults = UserDefaults.standard
                let skipped = defaults.string(forKey: Self.skippedVersionKey)
                let announced = defaults.string(forKey: Self.announcedVersionKey)
                if userInitiated {
                    presentRequest += 1
                } else if skipped != latest.version, announced != latest.version {
                    defaults.set(latest.version, forKey: Self.announcedVersionKey)
                    if NSApp.isActive {
                        presentRequest += 1
                    } else {
                        UpdateNotifier.shared.post(version: latest.version)
                    }
                }
            } else {
                state = .upToDate
                if userInitiated { presentRequest += 1 }
            }
        } catch {
            state = .failed(error.localizedDescription)
            if userInitiated { presentRequest += 1 }
        }
    }

    func skip() {
        guard let release else { return }
        UserDefaults.standard.set(release.version, forKey: Self.skippedVersionKey)
        state = .idle
    }

    func later() {
        switch state {
        case .upToDate, .failed: state = .idle
        default: break
        }
    }

    func downloadAndInstall() async {
        guard let release, !isBusy else { return }
        state = .downloading(0)
        do {
            let dir = Self.updatesDir
            let destination = dir.appendingPathComponent("Shipyard-\(release.version).dmg")
            try await Task.detached {
                let fm = FileManager.default
                try? fm.removeItem(at: dir)
                try fm.createDirectory(at: dir, withIntermediateDirectories: true)
            }.value
            let dmg = try await DMGDownloader.download(release.dmgURL, to: destination, expectedSize: release.dmgSize) { progress in
                Task { @MainActor in
                    if case .downloading = Updater.shared.state { Updater.shared.state = .downloading(progress) }
                }
            }
            state = .installing
            let bundleID = Bundle.main.bundleIdentifier
            let version = release.version
            stagedApp = try await Task.detached { try Self.stage(dmg: dmg, version: version, bundleID: bundleID) }.value
            state = .readyToRelaunch
        } catch {
            state = .failed(error.localizedDescription)
        }
        presentRequest += 1
    }

    func relaunch() async {
        guard case .readyToRelaunch = state, let staged = stagedApp else { return }
        let target = Self.installedBundleURL
        state = .installing
        do {
            if await Task.detached(operation: { Self.canReplace(target) }).value {
                try Self.spawn(Self.swapScript, [String(ProcessInfo.processInfo.processIdentifier), staged.path, target.path])
            } else {
                try await Task.detached { try Self.installAsAdmin(staged: staged, target: target) }.value
                try Self.spawn(Self.reopenScript, [String(ProcessInfo.processInfo.processIdentifier), target.path])
            }
        } catch {
            state = .failed(error.localizedDescription)
            presentRequest += 1
            return
        }
        stagedApp = nil
        NSApp.terminate(nil)
    }

    private static let swapScript = """
    while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
    if /usr/bin/ditto "$2" "$3.update"; then
      rm -rf "$3" && mv "$3.update" "$3"
    fi
    rm -rf "$3.update" "$2"
    /usr/bin/xattr -dr com.apple.quarantine "$3" 2>/dev/null
    /usr/bin/open "$3"
    rm -f "$0"
    """

    private static let reopenScript = """
    while kill -0 "$1" 2>/dev/null; do sleep 0.2; done
    /usr/bin/open "$2"
    rm -f "$0"
    """

    private static func spawn(_ script: String, _ args: [String]) throws {
        let scriptURL = FileManager.default.temporaryDirectory.appendingPathComponent("shipyard-update-\(UUID().uuidString).sh")
        do {
            try script.write(to: scriptURL, atomically: true, encoding: .utf8)
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/bin/sh")
            p.arguments = [scriptURL.path] + args
            p.standardInput = FileHandle.nullDevice
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
            try p.run()
        } catch {
            throw UpdateError.install(error.localizedDescription)
        }
    }

    nonisolated static func installAsAdmin(staged: URL, target: URL) throws {
        func q(_ s: String) -> String { "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'" }
        let update = target.path + ".update"
        let command = [
            "/usr/bin/ditto \(q(staged.path)) \(q(update))",
            "rm -rf \(q(target.path))",
            "mv \(q(update)) \(q(target.path))",
            "/usr/bin/xattr -dr com.apple.quarantine \(q(target.path))",
            "chown -R \(getuid()):staff \(q(target.path))"
        ].joined(separator: " && ")
        let escaped = command.replacingOccurrences(of: "\\", with: "\\\\").replacingOccurrences(of: "\"", with: "\\\"")
        let result = run("/usr/bin/osascript", ["-e", "do shell script \"\(escaped)\" with administrator privileges with prompt \"Shipyard wants to install an update.\""])
        try? FileManager.default.removeItem(at: staged.deletingLastPathComponent())
        guard result.ok else {
            if result.out.contains("-128") { throw UpdateError.cancelled }
            throw UpdateError.install(result.out.trimmingCharacters(in: .whitespacesAndNewlines))
        }
    }

    nonisolated static var installedBundleURL: URL {
        let current = Bundle.main.bundleURL
        guard current.path.contains("/AppTranslocation/") else { return current }
        typealias OriginalPath = @convention(c) (CFURL, UnsafeMutablePointer<Unmanaged<CFError>?>?) -> Unmanaged<CFURL>?
        if let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "SecTranslocateCreateOriginalPathForURL") {
            let original = unsafeBitCast(symbol, to: OriginalPath.self)
            if let url = original(current as CFURL, nil)?.takeRetainedValue() { return url as URL }
        }
        return URL(fileURLWithPath: "/Applications", isDirectory: true).appendingPathComponent(current.lastPathComponent)
    }

    nonisolated static func clearQuarantine() {
        let target = installedBundleURL
        guard target.pathExtension == "app",
              getxattr(target.path, "com.apple.quarantine", nil, 0, 0, 0) >= 0 else { return }
        _ = run("/usr/bin/xattr", ["-dr", "com.apple.quarantine", target.path])
    }

    nonisolated static var updatesDir: URL {
        let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first ?? FileManager.default.temporaryDirectory
        return caches.appendingPathComponent("Shipyard/Updates", isDirectory: true)
    }

    nonisolated static var stagingDir: URL {
        updatesDir.deletingLastPathComponent().appendingPathComponent("Staged", isDirectory: true)
    }

    nonisolated static func fetchLatest() async throws -> Release {
        var req = URLRequest(url: latestAPI)
        req.setValue("application/vnd.github+json", forHTTPHeaderField: "Accept")
        req.setValue(userAgent, forHTTPHeaderField: "User-Agent")
        req.cachePolicy = .reloadIgnoringLocalCacheData
        req.timeoutInterval = 20
        let (data, response) = try await URLSession.shared.data(for: req)
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        if code == 404 { throw UpdateError.noReleases }
        guard (200..<300).contains(code) else { throw UpdateError.http(code) }
        return try parseRelease(data)
    }

    nonisolated static func parseRelease(_ data: Data) throws -> Release {
        struct Asset: Decodable {
            let name: String
            let browserDownloadUrl: URL
            let size: Int64
        }
        struct Payload: Decodable {
            let tagName: String
            let name: String?
            let body: String?
            let htmlUrl: String
            let publishedAt: Date?
            let assets: [Asset]
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        decoder.dateDecodingStrategy = .iso8601
        let payload = try decoder.decode(Payload.self, from: data)
        guard let asset = payload.assets.first(where: { $0.name == "Shipyard.dmg" }) else { throw UpdateError.noAsset }
        var version = payload.tagName.trimmingCharacters(in: .whitespaces)
        if version.hasPrefix("v") || version.hasPrefix("V") { version.removeFirst() }
        let name = payload.name?.trimmingCharacters(in: .whitespaces) ?? ""
        return Release(
            version: version,
            name: name.isEmpty ? "Shipyard \(version)" : name,
            notes: payload.body ?? "",
            pageURL: payload.htmlUrl,
            publishedAt: payload.publishedAt,
            dmgURL: asset.browserDownloadUrl,
            dmgSize: asset.size
        )
    }

    nonisolated static func compare(_ a: String, _ b: String) -> ComparisonResult {
        let pa = versionComponents(a), pb = versionComponents(b)
        for i in 0..<max(pa.count, pb.count) {
            let x = i < pa.count ? pa[i] : 0
            let y = i < pb.count ? pb[i] : 0
            if x != y { return x < y ? .orderedAscending : .orderedDescending }
        }
        return .orderedSame
    }

    nonisolated static func versionComponents(_ v: String) -> [Int] {
        var s = v.trimmingCharacters(in: .whitespaces)
        if s.hasPrefix("v") || s.hasPrefix("V") { s.removeFirst() }
        let core = s.split(whereSeparator: { $0 == "-" || $0 == "+" }).first.map(String.init) ?? s
        return core.split(separator: ".").map { Int($0.prefix(while: \.isNumber)) ?? 0 }
    }

    nonisolated static func canReplace(_ target: URL) -> Bool {
        guard target.pathExtension == "app" else { return false }
        let fm = FileManager.default
        guard fm.isWritableFile(atPath: target.deletingLastPathComponent().path) else { return false }
        return !fm.fileExists(atPath: target.path) || fm.isWritableFile(atPath: target.path)
    }

    nonisolated static func stage(dmg: URL, version: String, bundleID: String?) throws -> URL {
        let fm = FileManager.default
        let mount = fm.temporaryDirectory.appendingPathComponent("ShipyardUpdate-\(UUID().uuidString)", isDirectory: true)
        try fm.createDirectory(at: mount, withIntermediateDirectories: true)
        let attach = run("/usr/bin/hdiutil", ["attach", "-nobrowse", "-readonly", "-noautoopen", "-mountpoint", mount.path, dmg.path])
        guard attach.ok else {
            try? fm.removeItem(at: mount)
            throw UpdateError.mount(attach.out.trimmingCharacters(in: .whitespacesAndNewlines))
        }
        defer {
            if run("/usr/bin/hdiutil", ["detach", mount.path, "-force"]).ok { try? fm.removeItem(at: mount) }
        }
        let source = mount.appendingPathComponent("Shipyard.app", isDirectory: true)
        guard fm.fileExists(atPath: source.path) else { throw UpdateError.noApp }
        let root = stagingDir
        try? fm.removeItem(at: root)
        try fm.createDirectory(at: root, withIntermediateDirectories: true)
        let staged = root.appendingPathComponent("Shipyard.app", isDirectory: true)
        guard run("/usr/bin/ditto", [source.path, staged.path]).ok else { throw UpdateError.copy }
        let info = NSDictionary(contentsOf: staged.appendingPathComponent("Contents/Info.plist"))
        guard let bundleID, info?["CFBundleIdentifier"] as? String == bundleID else {
            try? fm.removeItem(at: root)
            throw UpdateError.bundleMismatch
        }
        let found = info?["CFBundleShortVersionString"] as? String ?? "?"
        guard found == version else {
            try? fm.removeItem(at: root)
            throw UpdateError.versionMismatch(version, found)
        }
        _ = run("/usr/bin/xattr", ["-dr", "com.apple.quarantine", staged.path])
        return staged
    }

    nonisolated private static func run(_ tool: String, _ args: [String]) -> (out: String, ok: Bool) {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: tool)
        p.arguments = args
        let out = Pipe()
        p.standardOutput = out
        p.standardError = out
        do { try p.run() } catch { return (error.localizedDescription, false) }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return (String(decoding: data, as: UTF8.self), p.terminationStatus == 0)
    }
}

final class DMGDownloader: NSObject, URLSessionDownloadDelegate {
    private let destination: URL
    private let expectedSize: Int64
    private let onProgress: @Sendable (Double) -> Void
    private var continuation: CheckedContinuation<URL, Error>?
    private var lastReported = -1.0

    private init(destination: URL, expectedSize: Int64, onProgress: @escaping @Sendable (Double) -> Void) {
        self.destination = destination
        self.expectedSize = expectedSize
        self.onProgress = onProgress
    }

    static func download(_ url: URL, to destination: URL, expectedSize: Int64, progress: @escaping @Sendable (Double) -> Void) async throws -> URL {
        let delegate = DMGDownloader(destination: destination, expectedSize: expectedSize, onProgress: progress)
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        let session = URLSession(configuration: .ephemeral, delegate: delegate, delegateQueue: queue)
        defer { session.finishTasksAndInvalidate() }
        var req = URLRequest(url: url)
        req.setValue(Updater.userAgent, forHTTPHeaderField: "User-Agent")
        req.setValue("application/octet-stream", forHTTPHeaderField: "Accept")
        return try await withCheckedThrowingContinuation { c in
            queue.addOperation {
                delegate.continuation = c
                session.downloadTask(with: req).resume()
            }
        }
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didWriteData bytesWritten: Int64, totalBytesWritten: Int64, totalBytesExpectedToWrite: Int64) {
        let total = totalBytesExpectedToWrite > 0 ? totalBytesExpectedToWrite : expectedSize
        guard total > 0 else { return }
        let fraction = min(1, Double(totalBytesWritten) / Double(total))
        guard fraction - lastReported >= 0.005 || fraction >= 1 else { return }
        lastReported = fraction
        onProgress(fraction)
    }

    func urlSession(_ session: URLSession, downloadTask: URLSessionDownloadTask, didFinishDownloadingTo location: URL) {
        let code = (downloadTask.response as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else {
            finish(.failure(UpdateError.download("HTTP \(code)")))
            return
        }
        do {
            let fm = FileManager.default
            try? fm.removeItem(at: destination)
            try fm.moveItem(at: location, to: destination)
            onProgress(1)
            finish(.success(destination))
        } catch {
            finish(.failure(UpdateError.download(error.localizedDescription)))
        }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        if let error { finish(.failure(UpdateError.download(error.localizedDescription))) }
        else { finish(.failure(UpdateError.download("the download was interrupted"))) }
    }

    private func finish(_ result: Result<URL, Error>) {
        guard let c = continuation else { return }
        continuation = nil
        c.resume(with: result)
    }
}

@MainActor
final class UpdateNotifier: NSObject, UNUserNotificationCenterDelegate {
    static let shared = UpdateNotifier()
    private var installed = false

    func install() {
        guard !installed else { return }
        installed = true
        UNUserNotificationCenter.current().delegate = self
    }

    func post(version: String) {
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound]) { granted, _ in
            guard granted else { return }
            let content = UNMutableNotificationContent()
            content.title = "Shipyard \(version) is available"
            content.body = "Click to see what's new and update."
            content.sound = .default
            center.add(UNNotificationRequest(identifier: "update-\(version)", content: content, trigger: nil))
        }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse, withCompletionHandler completionHandler: @escaping () -> Void) {
        Task { @MainActor in
            NSApp.activate(ignoringOtherApps: true)
            Updater.shared.present()
        }
        completionHandler()
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification, withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void) {
        completionHandler([.banner, .sound])
    }
}
