import Foundation

struct DiffLine: Identifiable {
    enum Kind { case hunk, context, add, del, meta }
    let id: Int
    let kind: Kind
    let text: String
    let oldLine: Int?
    let newLine: Int?

    var anchorSide: String { kind == .del ? "LEFT" : "RIGHT" }
    var anchorLine: Int? { kind == .del ? oldLine : newLine }
    var commentable: Bool { kind == .add || kind == .del || kind == .context }
}

struct DiffFile: Identifiable {
    let id: String
    let previousFilename: String?
    let status: String
    let additions: Int
    let deletions: Int
    let lines: [DiffLine]
    let hasPatch: Bool

    init(_ f: PRFile) {
        id = f.filename
        previousFilename = f.previousFilename
        status = f.status
        additions = f.additions
        deletions = f.deletions
        hasPatch = f.patch != nil
        lines = f.patch.map(DiffParser.parse) ?? []
    }

    var statusLetter: String {
        switch status {
        case "added": return "A"
        case "removed": return "D"
        case "renamed": return "R"
        case "copied": return "C"
        default: return "M"
        }
    }
}

struct DiffAnchor: Hashable {
    let path: String
    let line: Int
    let side: String
}

enum DiffParser {
    static func parse(_ patch: String) -> [DiffLine] {
        var result: [DiffLine] = []
        result.reserveCapacity(patch.count / 40)
        var old = 0, new = 0, idx = 0
        patch.enumerateLines { raw, _ in
            defer { idx += 1 }
            if raw.hasPrefix("@@") {
                let (o, n) = hunkStart(raw)
                old = o
                new = n
                result.append(DiffLine(id: idx, kind: .hunk, text: raw, oldLine: nil, newLine: nil))
                return
            }
            let first = raw.first
            let body = raw.isEmpty ? "" : String(raw.dropFirst())
            switch first {
            case "+":
                result.append(DiffLine(id: idx, kind: .add, text: body, oldLine: nil, newLine: new))
                new += 1
            case "-":
                result.append(DiffLine(id: idx, kind: .del, text: body, oldLine: old, newLine: nil))
                old += 1
            case "\\":
                result.append(DiffLine(id: idx, kind: .meta, text: raw, oldLine: nil, newLine: nil))
            default:
                result.append(DiffLine(id: idx, kind: .context, text: body, oldLine: old, newLine: new))
                old += 1
                new += 1
            }
        }
        return result
    }

    private static func hunkStart(_ header: String) -> (Int, Int) {
        let parts = header.split(separator: " ")
        func start(_ token: Substring?) -> Int {
            guard let token else { return 1 }
            let digits = token.dropFirst().split(separator: ",").first ?? "1"
            return Int(digits) ?? 1
        }
        return (start(parts.first { $0.hasPrefix("-") }), start(parts.first { $0.hasPrefix("+") }))
    }
}
