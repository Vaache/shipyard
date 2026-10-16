import Foundation

enum CILogExcerpt {
    static let jobLimit = 40_000
    static let totalLimit = 120_000
    static let contextBefore = 150
    static let contextAfter = 3
    static let tailWithoutMarker = 150
    static let maxLine = 2_000

    static func clean(_ raw: String) -> String {
        raw.components(separatedBy: "\n").map(cleanLine).joined(separator: "\n")
    }

    static func cleanLine(_ line: String) -> String {
        var l = Substring(stripANSI(line))
        if l.first == "\u{FEFF}" { l = l.dropFirst() }
        if l.last == "\r" { l = l.dropLast() }
        if l.count > 29, l.dropFirst(4).first == "-", let space = l.firstIndex(of: " "), l.distance(from: l.startIndex, to: space) >= 20, l.distance(from: l.startIndex, to: space) <= 34 {
            l = l[l.index(after: space)...]
        }
        return String(l)
    }

    static func stripANSI(_ s: String) -> String {
        guard s.contains("\u{1B}") else { return s }
        var out = ""
        out.reserveCapacity(s.count)
        var it = s.unicodeScalars.makeIterator()
        while let c = it.next() {
            guard c == "\u{1B}" else {
                out.unicodeScalars.append(c)
                continue
            }
            guard let next = it.next() else { break }
            if next == "[" {
                while let p = it.next() {
                    if (0x40...0x7E).contains(p.value) { break }
                }
            } else if next == "]" {
                while let p = it.next() {
                    if p == "\u{07}" { break }
                    if p == "\u{1B}" { _ = it.next(); break }
                }
            }
        }
        return out
    }

    static func isErrorLine(_ line: String) -> Bool {
        if line.contains("##[error]") || line.contains("FAILED") || line.contains("panicked") || line.contains("Process completed with exit code") { return true }
        return line.range(of: "error:", options: .caseInsensitive) != nil
    }

    static func extract(_ raw: String, limit: Int = jobLimit) -> String {
        var lines = clean(raw).components(separatedBy: "\n").map { clip($0, to: maxLine) }
        while lines.last?.trimmingCharacters(in: .whitespaces).isEmpty == true { lines.removeLast() }
        guard !lines.isEmpty else { return "" }
        guard let first = lines.firstIndex(where: isErrorLine) else {
            return fit(head: [], body: Array(lines.suffix(tailWithoutMarker)), limit: limit)
        }
        let before = Array(lines[max(0, first - contextBefore)..<first])
        var keep = IndexSet()
        for i in first..<lines.count where isErrorLine(lines[i]) {
            keep.insert(integersIn: i...min(lines.count - 1, i + contextAfter))
        }
        var body: [String] = []
        var last: Int?
        for i in keep {
            if let last, i > last + 1 { body.append("…") }
            body.append(lines[i])
            last = i
        }
        if let last, last < lines.count - 1 { body.append("…") }
        return fit(head: before, body: body, limit: limit)
    }

    static func fit(head: [String], body: [String], limit: Int) -> String {
        var head = ArraySlice(head)
        var size = (head + body).reduce(0) { $0 + $1.utf8.count + 1 }
        while size > limit, let line = head.first {
            size -= line.utf8.count + 1
            head = head.dropFirst()
        }
        let text = (Array(head) + body).joined(separator: "\n")
        return truncate(text, to: limit)
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
