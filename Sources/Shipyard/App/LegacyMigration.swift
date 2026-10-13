import Foundation

enum LegacyMigration {
    private static let legacyDomain = "dev.vaache.githublite"
    private static let doneKey = "legacyDefaultsMigrated"

    static func run() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: doneKey) else { return }
        defaults.set(true, forKey: doneKey)
        guard Bundle.main.bundleIdentifier != legacyDomain, let old = defaults.persistentDomain(forName: legacyDomain) else { return }
        for (key, value) in old where defaults.object(forKey: key) == nil && !key.hasPrefix("NS") {
            defaults.set(value, forKey: key)
        }
    }
}
