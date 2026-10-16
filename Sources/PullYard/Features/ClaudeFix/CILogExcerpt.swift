import Foundation

enum CILogExcerpt {
    static let jobLimit = 40_000
    static let totalLimit = 120_000
    static let contextBefore = 10
    static let contextAfter = 5
    static let tailLines = 150
    static let headerLines = 15
    static let maxLine = 2_000

    static func clean(_ raw: String) -> String {
        raw.components(separatedBy: "\n").map(cleanLine).joined(separator: "\n")
    }

    static func cleanLine(_ line: String) -> String {
        var l = Substring(stripANSI(line))
        if l.first == "\u{FEFF}" { l = l.dropFirst() }
        if l.last == "\r" { l = l.dropLast() }
        if l.count > 20, l.dropFirst(4).first == "-", let space = l.firstIndex(of: " "), l.distance(from: l.startIndex, to: space) >= 20, l.distance(from: l.startIndex, to: space) <= 34 {
            l = l[l.index(after: space)...]
        }
        return String(l)
    }

    static func stripANSI(_ s: String) -> String {
        guard s.contains("\u{1B}") else { return s }
        let scalars = Array(s.unicodeScalars)
        var out = String.UnicodeScalarView()
        var i = 0
        func skipString() {
            while i < scalars.count {
                let p = scalars[i]
                i += 1
                if p == "\u{07}" { return }
                if p == "\u{1B}" {
                    if i < scalars.count, scalars[i] == "\\" { i += 1 }
                    return
                }
            }
        }
        while i < scalars.count {
            let c = scalars[i]
            i += 1
            guard c == "\u{1B}" else {
                out.append(c)
                continue
            }
            guard i < scalars.count else { break }
            let next = scalars[i]
            i += 1
            switch next {
            case "[":
                while i < scalars.count {
                    let p = scalars[i]
                    i += 1
                    if (0x40...0x7E).contains(p.value) { break }
                }
            case "]", "P", "X", "^", "_":
                skipString()
            default:
                if (0x20...0x2F).contains(next.value) {
                    while i < scalars.count, (0x20...0x2F).contains(scalars[i].value) { i += 1 }
                    if i < scalars.count { i += 1 }
                }
            }
        }
        return String(out)
    }

    static func isWarningLine(_ line: String) -> Bool {
        line.contains("##[warning]") || line.trimmingCharacters(in: .whitespaces).lowercased().hasPrefix("warning")
    }

    static func isErrorLine(_ line: String) -> Bool {
        if isWarningLine(line) { return false }
        let markers = ["##[error]", "error:", "Error:", "error[E", "error TS", "FAILED", "--- FAIL", "panicked", "npm ERR!", "Traceback (most recent call last)", "Process completed with exit code"]
        if markers.contains(where: line.contains) { return true }
        let t = line.trimmingCharacters(in: .whitespaces)
        return t.hasPrefix("FAIL ") || t.hasPrefix("FAIL\t") || t == "FAIL" || t.hasPrefix("● ")
    }

    static func isStepStart(_ line: String) -> Bool {
        line.hasPrefix("##[group]Run ") || line.hasPrefix("Post job cleanup.") || line.hasPrefix("Cleaning up orphan processes")
    }

    static func failingStep(_ lines: [String]) -> Range<Int>? {
        guard let fail = lines.lastIndex(where: { $0.contains("##[error]") }) ?? lines.lastIndex(where: { $0.contains("Process completed with exit code") }) else { return nil }
        let start = lines[...fail].lastIndex(where: isStepStart) ?? 0
        let end = lines[(fail + 1)...].firstIndex(where: isStepStart) ?? lines.count
        return start..<end
    }

    static func extract(_ raw: String, limit: Int = jobLimit) -> String {
        var lines = clean(raw).components(separatedBy: "\n").map { clip($0, to: maxLine) }
        while lines.last?.trimmingCharacters(in: .whitespaces).isEmpty == true { lines.removeLast() }
        guard !lines.isEmpty else { return "" }
        var header: [String] = []
        var bodyStart = 0
        var bodyEnd = lines.count
        if let step = failingStep(lines) {
            bodyStart = step.lowerBound
            bodyEnd = step.upperBound
            if lines[bodyStart].hasPrefix("##[group]Run ") {
                let close = lines[bodyStart..<bodyEnd].firstIndex(of: "##[endgroup]")
                let commandEnd = close ?? bodyStart + 1
                header = Array(lines[bodyStart..<min(commandEnd, bodyStart + headerLines)])
                if commandEnd - bodyStart > headerLines { header.append("…") }
                bodyStart = close.map { $0 + 1 } ?? bodyStart + 1
            }
        }
        let tailStart = max(bodyStart, bodyEnd - tailLines)
        var keep = IndexSet()
        for i in bodyStart..<tailStart where isErrorLine(lines[i]) {
            keep.insert(integersIn: max(bodyStart, i - contextBefore)..<min(tailStart, i + contextAfter + 1))
        }
        var windows: [[String]] = []
        var last: Int?
        for i in keep {
            if let last, i == last + 1 { windows[windows.count - 1].append(lines[i]) } else { windows.append([lines[i]]) }
            last = i
        }
        let tail = Array(lines[tailStart..<bodyEnd])
        return assemble(header: header, windows: windows, tail: tail, gapBeforeTail: (last.map { $0 + 1 } ?? bodyStart) < tailStart, limit: limit)
    }

    static func assemble(header: [String], windows: [[String]], tail: [String], gapBeforeTail: Bool, limit: Int) -> String {
        func size(_ ls: [String]) -> Int { ls.reduce(0) { $0 + $1.utf8.count + 1 } }
        var windows = windows
        var gap = gapBeforeTail
        var total = size(header) + size(tail) + windows.reduce(0) { $0 + size($1) + 4 } + 8
        while total > limit, !windows.isEmpty {
            total -= size(windows.removeFirst()) + 4
            gap = true
        }
        var out = header
        for w in windows {
            if !out.isEmpty { out.append("…") }
            out += w
        }
        if gap, !out.isEmpty, !tail.isEmpty { out.append("…") }
        let head = out.joined(separator: "\n")
        let room = limit - head.utf8.count - 1
        guard room > 200 else { return truncate(head, to: limit) }
        let body = truncateTail(tail.joined(separator: "\n"), to: room)
        return head.isEmpty ? body : head + "\n" + body
    }

    static func truncate(_ text: String, to limit: Int) -> String {
        guard text.utf8.count > limit else { return text }
        let marker = "… (truncated)"
        var budget = limit - marker.utf8.count - 1
        var kept: [String] = []
        for line in text.components(separatedBy: "\n") {
            let n = line.utf8.count + 1
            if n > budget {
                if kept.isEmpty { kept.append(clip(line, to: max(0, budget - 1))) }
                break
            }
            kept.append(line)
            budget -= n
        }
        return (kept + [marker]).joined(separator: "\n")
    }

    static func truncateTail(_ text: String, to limit: Int) -> String {
        guard text.utf8.count > limit else { return text }
        let marker = "… (earlier lines omitted)"
        var budget = limit - marker.utf8.count - 1
        var kept: [String] = []
        for line in text.components(separatedBy: "\n").reversed() {
            let n = line.utf8.count + 1
            if n > budget {
                if kept.isEmpty { kept.append(clip(line, to: max(0, budget - 1))) }
                break
            }
            kept.append(line)
            budget -= n
        }
        return ([marker] + kept.reversed()).joined(separator: "\n")
    }

    static func clip(_ line: String, to bytes: Int) -> String {
        guard line.utf8.count > bytes else { return line }
        let ellipsis = "…"
        var out = ""
        var size = 0
        for ch in line {
            let n = String(ch).utf8.count
            if size + n > bytes - ellipsis.utf8.count { break }
            out.append(ch)
            size += n
        }
        return out + ellipsis
    }
}
