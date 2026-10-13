import SwiftUI

enum MDBlock: Hashable {
    case heading(Int, String)
    case paragraph(String)
    case code(String, String)
    case bullet([MDListItem])
    case quote(String)
    case table([[String]])
    case rule
}

struct MDListItem: Hashable {
    let indent: Int
    let marker: String
    let text: String
}

enum MarkdownParser {
    private static let cache = NSCache<NSString, MDBox>()

    final class MDBox {
        let blocks: [MDBlock]
        init(_ b: [MDBlock]) { blocks = b }
    }

    static func parse(_ text: String) -> [MDBlock] {
        if let hit = cache.object(forKey: text as NSString) { return hit.blocks }
        let blocks = parseUncached(text)
        if text.count < 200_000 { cache.setObject(MDBox(blocks), forKey: text as NSString) }
        return blocks
    }

    private static func parseUncached(_ text: String) -> [MDBlock] {
        var blocks: [MDBlock] = []
        let lines = text.components(separatedBy: "\n")
        var i = 0
        var para: [String] = []
        var list: [MDListItem] = []

        func flushPara() {
            if !para.isEmpty { blocks.append(.paragraph(para.joined(separator: "\n"))); para = [] }
        }
        func flushList() {
            if !list.isEmpty { blocks.append(.bullet(list)); list = [] }
        }

        while i < lines.count {
            let raw = lines[i]
            let line = raw.trimmingCharacters(in: .whitespaces)
            if line.hasPrefix("```") || line.hasPrefix("~~~") {
                flushPara(); flushList()
                let fence = String(line.prefix(3))
                let lang = String(line.dropFirst(3)).trimmingCharacters(in: .whitespaces)
                var code: [String] = []
                i += 1
                while i < lines.count, !lines[i].trimmingCharacters(in: .whitespaces).hasPrefix(fence) {
                    code.append(lines[i])
                    i += 1
                }
                blocks.append(.code(lang, code.joined(separator: "\n")))
                i += 1
                continue
            }
            if line.isEmpty {
                flushPara(); flushList()
                i += 1
                continue
            }
            if let h = line.firstIndex(where: { $0 != "#" }), line.hasPrefix("#"), line[h] == " " {
                flushPara(); flushList()
                let level = line.distance(from: line.startIndex, to: h)
                blocks.append(.heading(min(level, 4), String(line[h...]).trimmingCharacters(in: .whitespaces)))
                i += 1
                continue
            }
            if line == "---" || line == "***" || line == "___" {
                flushPara(); flushList()
                blocks.append(.rule)
                i += 1
                continue
            }
            if line.hasPrefix(">") {
                flushPara(); flushList()
                var q: [String] = []
                while i < lines.count, lines[i].trimmingCharacters(in: .whitespaces).hasPrefix(">") {
                    q.append(String(lines[i].trimmingCharacters(in: .whitespaces).dropFirst()).trimmingCharacters(in: .whitespaces))
                    i += 1
                }
                blocks.append(.quote(q.joined(separator: "\n")))
                continue
            }
            if line.hasPrefix("|"), i + 1 < lines.count, lines[i + 1].contains("---") {
                flushPara(); flushList()
                var rows: [[String]] = []
                while i < lines.count, lines[i].trimmingCharacters(in: .whitespaces).hasPrefix("|") {
                    let row = lines[i].trimmingCharacters(in: .whitespaces)
                    if !row.replacingOccurrences(of: "|", with: "").trimmingCharacters(in: CharacterSet(charactersIn: "-: ")).isEmpty {
                        rows.append(row.trimmingCharacters(in: CharacterSet(charactersIn: "|")).components(separatedBy: "|").map { $0.trimmingCharacters(in: .whitespaces) })
                    }
                    i += 1
                }
                blocks.append(.table(rows))
                continue
            }
            if let item = listItem(raw) {
                flushPara()
                list.append(item)
                i += 1
                continue
            }
            if !list.isEmpty, raw.hasPrefix("  ") {
                let last = list.removeLast()
                list.append(MDListItem(indent: last.indent, marker: last.marker, text: last.text + " " + line))
                i += 1
                continue
            }
            flushList()
            para.append(line)
            i += 1
        }
        flushPara(); flushList()
        return blocks
    }

    private static func listItem(_ raw: String) -> MDListItem? {
        let indent = raw.prefix { $0 == " " }.count
        let s = raw.dropFirst(indent)
        for m in ["- ", "* ", "+ "] where s.hasPrefix(m) {
            var text = String(s.dropFirst(2))
            var marker = "•"
            if text.hasPrefix("[ ] ") { marker = "☐"; text = String(text.dropFirst(4)) }
            if text.hasPrefix("[x] ") || text.hasPrefix("[X] ") { marker = "☑"; text = String(text.dropFirst(4)) }
            return MDListItem(indent: indent / 2, marker: marker, text: text)
        }
        if let dot = s.firstIndex(of: "."), s[s.startIndex..<dot].allSatisfy(\.isNumber), !s[s.startIndex..<dot].isEmpty, s[s.index(after: dot)...].hasPrefix(" ") {
            return MDListItem(indent: indent / 2, marker: String(s[s.startIndex...dot]), text: String(s[s.index(dot, offsetBy: 2)...]))
        }
        return nil
    }
}

func inlineMarkdown(_ text: String) -> AttributedString {
    var opts = AttributedString.MarkdownParsingOptions(interpretedSyntax: .inlineOnlyPreservingWhitespace)
    opts.failurePolicy = .returnPartiallyParsedIfPossible
    var attr = (try? AttributedString(markdown: text, options: opts)) ?? AttributedString(text)
    for run in attr.runs where run.inlinePresentationIntent?.contains(.code) == true {
        attr[run.range].font = .system(.body, design: .monospaced)
        attr[run.range].backgroundColor = Color.syLineStrong
    }
    return attr
}

struct MarkdownView: View {
    let text: String

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ForEach(Array(MarkdownParser.parse(text).enumerated()), id: \.offset) { _, block in
                blockView(block)
            }
        }
        .textSelection(.enabled)
        .environment(\.openURL, OpenURLAction { url in
            LinkBrowser.open(url)
            return .handled
        })
    }

    @ViewBuilder
    private func blockView(_ block: MDBlock) -> some View {
        switch block {
        case .heading(let level, let s):
            Text(inlineMarkdown(s))
                .font(level == 1 ? .title2.weight(.bold) : level == 2 ? .title3.weight(.semibold) : .headline)
                .padding(.top, 4)
        case .paragraph(let s):
            Text(inlineMarkdown(s)).fixedSize(horizontal: false, vertical: true)
        case .code(let lang, let code):
            CodeBlockView(language: lang, code: code)
        case .bullet(let items):
            VStack(alignment: .leading, spacing: 3) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(item.marker).foregroundStyle(Color.syDim).frame(minWidth: 14, alignment: .trailing)
                        Text(inlineMarkdown(item.text)).fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.leading, CGFloat(item.indent) * 16)
                }
            }
        case .quote(let s):
            HStack(spacing: 8) {
                RoundedRectangle(cornerRadius: 1.5).fill(Color.syLineStrong).frame(width: 3)
                Text(inlineMarkdown(s)).foregroundStyle(Color.syMuted).fixedSize(horizontal: false, vertical: true)
            }
        case .table(let rows):
            ScrollView(.horizontal, showsIndicators: false) {
                Grid(alignment: .leading, horizontalSpacing: 14, verticalSpacing: 4) {
                    ForEach(Array(rows.enumerated()), id: \.offset) { i, row in
                        GridRow {
                            ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
                                Text(inlineMarkdown(cell)).fontWeight(i == 0 ? .semibold : .regular)
                            }
                        }
                        if i == 0 { Rectangle().fill(Color.syLine).frame(height: 1) }
                    }
                }
                .padding(8)
            }
            .card(radius: 8, fill: .sySunken)
        case .rule:
            Rectangle().fill(Color.syLine).frame(height: 1)
        }
    }
}

struct CodeBlockView: View {
    let language: String
    let code: String
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(language.isEmpty ? "code" : language).font(.caption.monospaced()).foregroundStyle(Color.syMuted)
                Spacer()
                Button {
                    copyToPasteboard(code)
                    copied = true
                    DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) { copied = false }
                } label: {
                    Label(copied ? t("Скопировано", "Copied") : t("Копировать", "Copy"), systemImage: copied ? "checkmark" : "doc.on.doc")
                        .font(.caption)
                }
                .buttonStyle(.borderless)
                .foregroundStyle(Color.syMuted)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .background(Color.syText.opacity(0.04))
            .overlay(alignment: .bottom) { Rectangle().fill(Color.syLine).frame(height: 1) }
            ScrollView(.horizontal, showsIndicators: false) {
                Text(highlighted)
                    .font(.system(size: 12, design: .monospaced))
                    .fixedSize(horizontal: true, vertical: true)
                    .padding(10)
            }
        }
        .background(Color.sySunken)
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color.syLine))
    }

    private var highlighted: AttributedString {
        let aliases = ["python": "py", "javascript": "js", "typescript": "ts", "kotlin": "kt", "shell": "sh", "console": "sh", "zsh": "sh", "yml": "yaml", "rust": "rs", "golang": "go", "objective-c": "m", "objc": "m", "c++": "cpp", "csharp": "cs", "jsonc": "js", "json": "js"]
        let ext = aliases[language.lowercased()] ?? (language.isEmpty ? "txt" : language.lowercased())
        let ns = SyntaxHighlighter.attributed(code, ext: ext, font: .monospacedSystemFont(ofSize: 12, weight: .regular))
        return (try? AttributedString(ns, including: \.appKit)) ?? AttributedString(code)
    }
}
