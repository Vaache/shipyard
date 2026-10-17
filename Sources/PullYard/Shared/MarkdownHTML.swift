import Foundation

enum MarkdownHTML {
    static func render(_ markdown: String, base: String? = nil) -> String {
        MarkdownParser.parse(markdown).map { block(base: base, $0) }.joined()
    }

    private static func block(base: String?, _ b: MDBlock) -> String {
        switch b {
        case .heading(let level, let text):
            return "<h\(level)>\(inline(text, base: base))</h\(level)>"
        case .paragraph(let text):
            return "<p>\(inline(text, base: base))</p>"
        case .code(let lang, let code):
            let cls = lang.isEmpty ? "" : " class=\"language-\(ConversationHTML.esc(lang))\""
            return "<pre><code\(cls)>\(ConversationHTML.esc(code))</code></pre>"
        case .bullet(let items):
            let ordered = items.first.map { $0.marker.hasSuffix(".") } ?? false
            let rows = items.map { item -> String in
                let box: String
                switch item.marker {
                case "☐": box = "<input type=\"checkbox\" disabled> "
                case "☑": box = "<input type=\"checkbox\" checked disabled> "
                default: box = ""
                }
                let indent = item.indent > 0 ? " style=\"margin-left:\(item.indent * 16)px\"" : ""
                return "<li\(indent)>\(box)\(inline(item.text, base: base))</li>"
            }.joined()
            return ordered ? "<ol>\(rows)</ol>" : "<ul>\(rows)</ul>"
        case .quote(let text):
            return "<blockquote>\(render(text, base: base))</blockquote>"
        case .table(let rows):
            guard let head = rows.first else { return "" }
            let th = head.map { "<th>\(inline($0, base: base))</th>" }.joined()
            let body = rows.dropFirst().map { row in "<tr>" + row.map { "<td>\(inline($0, base: base))</td>" }.joined() + "</tr>" }.joined()
            return "<table><thead><tr>\(th)</tr></thead><tbody>\(body)</tbody></table>"
        case .rule:
            return "<hr>"
        }
    }

    static func inline(_ text: String, base: String? = nil) -> String {
        var options = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
        options.failurePolicy = .returnPartiallyParsedIfPossible
        guard let attributed = try? AttributedString(markdown: text, options: options) else {
            return ConversationHTML.esc(text).replacingOccurrences(of: "\n", with: "<br>")
        }
        var out = ""
        for run in attributed.runs {
            let raw = String(attributed[run.range].characters)
            if let image = run.imageURL {
                out += "<img src=\"\(ConversationHTML.esc(absolute(image.absoluteString, base: base)))\" alt=\"\(ConversationHTML.esc(raw))\">"
                continue
            }
            var html = ConversationHTML.esc(raw).replacingOccurrences(of: "\n", with: "<br>")
            let intent = run.inlinePresentationIntent ?? []
            if intent.contains(.code) { html = "<code>\(html)</code>" }
            if intent.contains(.stronglyEmphasized) { html = "<strong>\(html)</strong>" }
            if intent.contains(.emphasized) { html = "<em>\(html)</em>" }
            if intent.contains(.strikethrough) { html = "<del>\(html)</del>" }
            if let link = run.link {
                html = "<a href=\"\(ConversationHTML.esc(absolute(link.absoluteString, base: base)))\">\(html)</a>"
            }
            out += html
        }
        return out
    }

    static func absolute(_ url: String, base: String?) -> String {
        guard url.hasPrefix("/"), !url.hasPrefix("//"), let base, let root = URL(string: base) else { return url }
        if url.hasPrefix("/uploads/") { return base.trimmingCharacters(in: CharacterSet(charactersIn: "/")) + url }
        guard let scheme = root.scheme, let host = root.host else { return url }
        return "\(scheme)://\(host)\(root.port.map { ":\($0)" } ?? "")\(url)"
    }

    static func absolutizeHTML(_ html: String, base: String) -> String {
        var out = html.replacingOccurrences(of: #" src="data:image/gif;base64,[^"]*""#, with: "", options: .regularExpression)
        out = out.replacingOccurrences(of: " data-src=\"", with: " src=\"")
        for attr in ["href", "src"] {
            let pattern = "\(attr)=\"(/[^/\"][^\"]*)\""
            guard let regex = try? NSRegularExpression(pattern: pattern) else { continue }
            let ns = out as NSString
            var result = ""
            var last = 0
            for m in regex.matches(in: out, range: NSRange(location: 0, length: ns.length)) {
                result += ns.substring(with: NSRange(location: last, length: m.range.location - last))
                result += "\(attr)=\"\(absolute(ns.substring(with: m.range(at: 1)), base: base))\""
                last = m.range.location + m.range.length
            }
            result += ns.substring(from: last)
            out = result
        }
        return out
    }
}
