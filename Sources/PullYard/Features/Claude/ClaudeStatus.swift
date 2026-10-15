import Foundation
import SwiftUI

enum ProcessMemory {
    struct Entry {
        let ppid: pid_t
        let rss: Int
    }

    static func snapshot() -> [pid_t: Entry] {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/bin/ps")
        p.arguments = ["-axo", "pid=,ppid=,rss="]
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        guard (try? p.run()) != nil else { return [:] }
        DispatchQueue.global(qos: .utility).asyncAfter(deadline: .now() + 30) { [weak p] in
            if p?.isRunning == true { p?.terminate() }
        }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        var map: [pid_t: Entry] = [:]
        for line in String(decoding: data, as: UTF8.self).split(separator: "\n") {
            let f = line.split(separator: " ", omittingEmptySubsequences: true)
            guard f.count >= 3, let pid = pid_t(f[0]), let ppid = pid_t(f[1]), let rss = Int(f[2]) else { continue }
            map[pid] = Entry(ppid: ppid, rss: rss * 1024)
        }
        return map
    }

    static func treeBytes(_ root: pid_t, in snap: [pid_t: Entry]) -> Int {
        guard root > 0, let rootEntry = snap[root] else { return 0 }
        var children: [pid_t: [pid_t]] = [:]
        for (pid, e) in snap { children[e.ppid, default: []].append(pid) }
        var total = rootEntry.rss
        var stack = children[root] ?? []
        var seen: Set<pid_t> = [root]
        while let pid = stack.popLast() {
            guard seen.insert(pid).inserted, let e = snap[pid] else { continue }
            total += e.rss
            stack += children[pid] ?? []
        }
        return total
    }

    static func format(_ bytes: Int) -> String {
        let mb = Double(bytes) / 1_048_576
        return mb >= 1024 ? String(format: "%.1f ", mb / 1024) + "GB" : "\(Int(mb.rounded())) " + "MB"
    }
}

enum ProjectPalette {
    private static let colors: [Color] = [
        .sySky,
        .syViolet,
        .syCoral,
        Color(nsColor: .systemOrange),
        .syTeal,
        .syGreen,
        .syIndigo,
        .syAmber
    ]

    static func color(for path: String) -> Color {
        var h: UInt64 = 1469598103934665603
        for b in path.utf8 { h = (h ^ UInt64(b)) &* 1099511628211 }
        return colors[Int(h % UInt64(colors.count))]
    }
}

enum BranchStyle {
    static func color(_ branch: String) -> Color {
        let b = branch.lowercased()
        if ["main", "master", "develop", "dev", "staging", "release"].contains(b) { return .sySlate }
        if b.hasPrefix("feat") || b.hasPrefix("feature") { return .syGreen }
        if b.hasPrefix("fix") || b.hasPrefix("hotfix") || b.hasPrefix("bug") { return .syAmber }
        if b.hasPrefix("chore") || b.hasPrefix("refactor") || b.hasPrefix("docs") { return .syTeal }
        if b.range(of: "^[a-z]+-[0-9]+", options: .regularExpression) != nil { return .syViolet }
        return .syMuted
    }
}

struct BranchTag: View {
    let name: String

    var body: some View {
        let c = BranchStyle.color(name)
        HStack(spacing: 3) {
            Image(systemName: "arrow.branch").imageScale(.small)
            Text(name).lineLimit(1).truncationMode(.middle)
        }
        .font(.caption)
        .foregroundStyle(c)
        .padding(.horizontal, 6)
        .padding(.vertical, 1)
        .background(Capsule().fill(c.opacity(0.12)))
    }
}

struct PRTag: View {
    let number: Int

    var body: some View {
        HStack(spacing: 2) {
            Image(systemName: "arrow.triangle.pull").imageScale(.small)
            Text("#\(number)")
        }
        .font(.caption.weight(.medium))
        .foregroundStyle(Color.syViolet)
        .padding(.horizontal, 6)
        .padding(.vertical, 1)
        .background(Capsule().fill(Color.syViolet.opacity(0.12)))
    }
}

enum ClaudeTabState {
    case working, waiting, active, sleeping, stopped

    var color: Color {
        switch self {
        case .working: return .sySky
        case .waiting: return .syAmber
        case .active: return .syGreen
        case .sleeping: return .syIndigo
        case .stopped: return .syDim
        }
    }

    var icon: String {
        switch self {
        case .working: return "bolt.fill"
        case .waiting: return "hand.raised.fill"
        case .active: return "circle.fill"
        case .sleeping: return "moon.zzz.fill"
        case .stopped: return "stop.circle"
        }
    }

    var title: String {
        switch self {
        case .working: return "Working"
        case .waiting: return "Needs you"
        case .active: return "Active"
        case .sleeping: return "Sleeping"
        case .stopped: return "Stopped"
        }
    }

    var help: String {
        switch self {
        case .sleeping: return "Process stopped, memory freed. A new message resumes the session."
        case .active: return "Process is running and uses memory. Sleeps after idle time."
        default: return title
        }
    }
}

struct TabStateBadge: View {
    let state: ClaudeTabState
    var memory: Int?
    var compact = false

    var body: some View {
        HStack(spacing: 4) {
            if state == .working {
                ProgressView().controlSize(.mini).frame(width: 10, height: 10)
            } else {
                Image(systemName: state.icon).font(.system(size: state == .active ? 6 : 9))
            }
            if !compact {
                Text(state.title)
                if let memory, memory > 0, state != .sleeping {
                    Text("· " + ProcessMemory.format(memory)).foregroundStyle(Color.syMuted)
                }
            }
        }
        .font(.caption)
        .foregroundStyle(state.color)
        .help(state.help + (memory.map { $0 > 0 ? " — " + ProcessMemory.format($0) : "" } ?? ""))
    }
}
