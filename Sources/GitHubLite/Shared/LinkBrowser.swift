import AppKit

struct BrowserApp: Identifiable, Hashable {
    let id: String
    let name: String
}

enum LinkBrowser {
    static let systemDefault = "system"
    private static let key = "linkBrowser"
    static let preferredDefault = "company.thebrowser.dia"

    static var installed: [BrowserApp] {
        guard let probe = URL(string: "https://github.com") else { return [] }
        var seen = Set<String>()
        return NSWorkspace.shared.urlsForApplications(toOpen: probe).compactMap { url in
            guard let bundle = Bundle(url: url), let id = bundle.bundleIdentifier, seen.insert(id).inserted else { return nil }
            let name = FileManager.default.displayName(atPath: url.path).replacingOccurrences(of: ".app", with: "")
            return BrowserApp(id: id, name: name)
        }.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    static var selected: String {
        get {
            if let v = UserDefaults.standard.string(forKey: key) { return v }
            return NSWorkspace.shared.urlForApplication(withBundleIdentifier: preferredDefault) != nil ? preferredDefault : systemDefault
        }
        set { UserDefaults.standard.set(newValue, forKey: key) }
    }

    static var bundleID: String? {
        let s = selected
        guard s != systemDefault, NSWorkspace.shared.urlForApplication(withBundleIdentifier: s) != nil else { return nil }
        return s
    }

    static func open(_ url: URL) {
        guard url.scheme == "http" || url.scheme == "https", let id = bundleID, let app = NSWorkspace.shared.urlForApplication(withBundleIdentifier: id) else {
            NSWorkspace.shared.open(url)
            return
        }
        let config = NSWorkspace.OpenConfiguration()
        config.activates = true
        NSWorkspace.shared.open([url], withApplicationAt: app, configuration: config)
    }

    static func helperScript() -> String? {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("GitHubLite", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let file = dir.appendingPathComponent("open-browser.sh")
        let body: String
        if let id = bundleID {
            body = "#!/bin/bash\nexec /usr/bin/open -b \(id) \"$@\"\n"
        } else {
            body = "#!/bin/bash\nexec /usr/bin/open \"$@\"\n"
        }
        do {
            try body.write(to: file, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: file.path)
            return file.path
        } catch {
            return nil
        }
    }
}
