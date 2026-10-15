import AppKit
import Foundation

enum LegacyMigration {
    private static let legacyDomain = "dev.vaache.githublite"
    private static let doneKey = "legacyDefaultsMigrated"

    static func run() {
        renameBundleOnQuit()
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: doneKey) else { return }
        defaults.set(true, forKey: doneKey)
        guard Bundle.main.bundleIdentifier != legacyDomain, let old = defaults.persistentDomain(forName: legacyDomain) else { return }
        for (key, value) in old where defaults.object(forKey: key) == nil && !key.hasPrefix("NS") {
            defaults.set(value, forKey: key)
        }
    }

    private static func renameBundleOnQuit() {
        let installed = Updater.installedBundleURL
        let target = Updater.renamedTarget(installed)
        guard target != installed else { return }
        NotificationCenter.default.addObserver(forName: NSApplication.willTerminateNotification, object: nil, queue: .main) { _ in
            let installing = MainActor.assumeIsolated {
                if case .installing = Updater.shared.state { return true }
                return false
            }
            let fm = FileManager.default
            guard !installing, !fm.fileExists(atPath: target.path),
                  Updater.canReplace(target), Updater.canReplace(installed) else { return }
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/bin/sh")
            p.arguments = ["-c", "while kill -0 \"$1\" 2>/dev/null; do sleep 0.2; done; mv \"$2\" \"$3\"", "sh",
                           String(ProcessInfo.processInfo.processIdentifier), installed.path, target.path]
            p.standardInput = FileHandle.nullDevice
            p.standardOutput = FileHandle.nullDevice
            p.standardError = FileHandle.nullDevice
            try? p.run()
        }
    }
}
