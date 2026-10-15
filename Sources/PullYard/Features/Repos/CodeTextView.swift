import AppKit
import SwiftUI

enum SyntaxHighlighter {
    private static let keywords: Set<String> = [
        "func", "let", "var", "if", "else", "for", "while", "return", "class", "struct", "enum", "protocol", "extension", "import", "guard", "switch", "case", "default", "break", "continue", "in", "is", "as", "try", "catch", "throw", "throws", "async", "await", "static", "private", "public", "internal", "final", "override", "init", "self", "super", "nil", "true", "false", "null", "undefined", "new", "this", "const", "function", "export", "from", "interface", "type", "implements", "extends", "abstract", "void", "int", "double", "bool", "String", "late", "required", "factory", "get", "set", "with", "yield", "def", "elif", "pass", "lambda", "None", "True", "False", "and", "or", "not", "package", "fun", "val", "object", "when", "data", "sealed", "go", "chan", "defer", "map", "range", "fn", "mut", "pub", "impl", "trait", "use", "mod", "match", "where", "some", "any", "do", "repeat", "typealias", "mixin", "dynamic", "final", "of", "delete", "typeof", "instanceof", "static", "unsafe", "fileprivate", "open", "inout", "where", "echo", "then", "fi", "esac", "done", "local"
    ]

    private static let hashComment: Set<String> = ["py", "sh", "bash", "zsh", "rb", "yaml", "yml", "toml", "mk", "makefile", "dockerfile", "r", "pl", "conf", "cfg", "ini", "gitignore", "env"]
    private static let slashComment: Set<String> = ["swift", "dart", "js", "jsx", "ts", "tsx", "kt", "kts", "java", "go", "rs", "c", "h", "cc", "cpp", "hpp", "m", "mm", "cs", "scala", "groovy", "gradle", "php", "css", "scss", "less", "json5", "proto"]

    private static let stringRegex = try! NSRegularExpression(pattern: #""(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*'|`(?:\\.|[^`\\])*`"#)
    private static let numberRegex = try! NSRegularExpression(pattern: #"\b(?:0x[0-9a-fA-F]+|\d+(?:\.\d+)?)\b"#)
    private static let wordRegex = try! NSRegularExpression(pattern: #"\b[A-Za-z_][A-Za-z0-9_]*\b"#)
    private static let slashCommentRegex = try! NSRegularExpression(pattern: #"//[^\n]*|/\*[\s\S]*?\*/"#)
    private static let hashCommentRegex = try! NSRegularExpression(pattern: #"(?m)(?:^|\s)#[^\n]*"#)
    private static let htmlCommentRegex = try! NSRegularExpression(pattern: #"<!--[\s\S]*?-->"#)

    static func attributed(_ text: String, ext: String, font: NSFont) -> NSAttributedString {
        let result = NSMutableAttributedString(string: text, attributes: [.font: font, .foregroundColor: NSColor.syText])
        guard text.utf16.count < 400_000 else { return result }
        let ns = text as NSString
        let full = NSRange(location: 0, length: ns.length)
        let e = ext.lowercased()
        let code = slashComment.contains(e) || hashComment.contains(e) || ["html", "xml", "vue", "svelte", "lua", "sql"].contains(e)
        guard code else { return result }

        wordRegex.enumerateMatches(in: text, range: full) { m, _, _ in
            guard let r = m?.range, keywords.contains(ns.substring(with: r)) else { return }
            result.addAttribute(.foregroundColor, value: NSColor.syCoral, range: r)
        }
        numberRegex.enumerateMatches(in: text, range: full) { m, _, _ in
            if let r = m?.range { result.addAttribute(.foregroundColor, value: NSColor.syViolet, range: r) }
        }
        stringRegex.enumerateMatches(in: text, range: full) { m, _, _ in
            if let r = m?.range { result.addAttribute(.foregroundColor, value: NSColor.syGreen, range: r) }
        }
        let commentRegex: NSRegularExpression? = slashComment.contains(e) ? slashCommentRegex : hashComment.contains(e) ? hashCommentRegex : ["html", "xml", "vue", "svelte"].contains(e) ? htmlCommentRegex : nil
        commentRegex?.enumerateMatches(in: text, range: full) { m, _, _ in
            if let r = m?.range { result.addAttribute(.foregroundColor, value: NSColor.syDim, range: r) }
        }
        return result
    }
}

final class LineNumberRuler: NSRulerView {
    private weak var textView: NSTextView?
    private var lineStarts: [Int] = [0]
    private let font = NSFont.monospacedDigitSystemFont(ofSize: 10.5, weight: .regular)

    init(textView: NSTextView, scrollView: NSScrollView) {
        self.textView = textView
        super.init(scrollView: scrollView, orientation: .verticalRuler)
        clientView = textView
        ruleThickness = 44
        NotificationCenter.default.addObserver(self, selector: #selector(refresh), name: NSView.boundsDidChangeNotification, object: scrollView.contentView)
        NotificationCenter.default.addObserver(self, selector: #selector(refresh), name: NSText.didChangeNotification, object: textView)
    }

    required init(coder: NSCoder) { fatalError() }

    func reindex() {
        guard let s = textView?.string as NSString? else { return }
        var starts = [0]
        var idx = 0
        while idx < s.length {
            let r = s.lineRange(for: NSRange(location: idx, length: 0))
            idx = NSMaxRange(r)
            if idx < s.length || (idx == s.length && s.length > 0 && s.character(at: s.length - 1) == 10) { starts.append(idx) }
        }
        lineStarts = starts
        let digits = max(3, String(starts.count).count)
        ruleThickness = CGFloat(digits) * 7.5 + 14
        needsDisplay = true
    }

    @objc private func refresh() { needsDisplay = true }

    private func lineIndex(for location: Int) -> Int {
        var lo = 0, hi = lineStarts.count - 1
        while lo < hi {
            let mid = (lo + hi + 1) / 2
            if lineStarts[mid] <= location { lo = mid } else { hi = mid - 1 }
        }
        return lo
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let tv = textView, let lm = tv.layoutManager, let tc = tv.textContainer else { return }
        NSColor.sySunken.setFill()
        bounds.fill()
        NSColor.syLine.setFill()
        NSRect(x: bounds.maxX - 1, y: bounds.minY, width: 1, height: bounds.height).fill()
        guard lm.numberOfGlyphs > 0 else { return }
        let visible = tv.visibleRect
        let glyphs = lm.glyphRange(forBoundingRect: visible, in: tc)
        let chars = lm.characterRange(forGlyphRange: glyphs, actualGlyphRange: nil)
        let attrs: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.syGutter]
        var line = lineIndex(for: chars.location)
        let total = (tv.string as NSString).length
        while line < lineStarts.count {
            let start = lineStarts[line]
            if start > NSMaxRange(chars) { break }
            if start > total { break }
            let glyph = start < total ? lm.glyphIndexForCharacter(at: start) : max(0, lm.numberOfGlyphs - 1)
            var frag = lm.lineFragmentRect(forGlyphAt: glyph, effectiveRange: nil)
            if start == total, total > 0 { frag = lm.extraLineFragmentRect.isEmpty ? frag : lm.extraLineFragmentRect }
            let y = convert(NSPoint(x: 0, y: frag.minY + tv.textContainerInset.height), from: tv).y
            let label = "\(line + 1)" as NSString
            let size = label.size(withAttributes: attrs)
            label.draw(at: NSPoint(x: ruleThickness - size.width - 6, y: y + (frag.height - size.height) / 2), withAttributes: attrs)
            line += 1
        }
    }
}

struct CodeTextView: NSViewRepresentable {
    let text: String
    let ext: String
    var wrap = false

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        let tv = scroll.documentView as! NSTextView
        _ = tv.layoutManager
        tv.isEditable = false
        tv.isSelectable = true
        tv.isRichText = false
        tv.usesFindBar = true
        tv.isIncrementalSearchingEnabled = true
        tv.textContainerInset = NSSize(width: 6, height: 8)
        tv.backgroundColor = .sySunken
        tv.insertionPointColor = .syAccent
        tv.selectedTextAttributes = [.backgroundColor: NSColor.syAccent.withAlphaComponent(0.25)]
        scroll.backgroundColor = .sySunken
        scroll.drawsBackground = true
        scroll.hasHorizontalScroller = true
        let ruler = LineNumberRuler(textView: tv, scrollView: scroll)
        scroll.verticalRulerView = ruler
        scroll.hasVerticalRuler = true
        scroll.rulersVisible = true
        apply(scroll, tv)
        context.coordinator.wrap = wrap
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let tv = scroll.documentView as? NSTextView else { return }
        if tv.string != text || context.coordinator.wrap != wrap {
            apply(scroll, tv)
            context.coordinator.wrap = wrap
        }
    }

    func makeCoordinator() -> Coordinator { Coordinator() }

    final class Coordinator {
        var wrap: Bool?
    }

    private func apply(_ scroll: NSScrollView, _ tv: NSTextView) {
        let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        if wrap {
            tv.isHorizontallyResizable = false
            tv.textContainer?.widthTracksTextView = true
            tv.textContainer?.containerSize = NSSize(width: scroll.contentSize.width, height: .greatestFiniteMagnitude)
        } else {
            tv.isHorizontallyResizable = true
            tv.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
            tv.textContainer?.widthTracksTextView = false
            tv.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        }
        tv.textStorage?.setAttributedString(SyntaxHighlighter.attributed(text, ext: ext, font: font))
        tv.scroll(.zero)
        (scroll.verticalRulerView as? LineNumberRuler)?.reindex()
    }
}
