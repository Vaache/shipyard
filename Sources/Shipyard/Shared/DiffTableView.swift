import AppKit
import SwiftUI

enum DiffTableRow {
    case line(id: String, line: DiffLine, commentable: Bool)
    case view(id: String, group: Bool, content: () -> AnyView)

    var id: String {
        switch self {
        case .line(let id, _, _): return id
        case .view(let id, _, _): return id
        }
    }
}

private enum DiffStyle {
    static let font = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
    static let gutterWidth: CGFloat = 112
    static let vPad: CGFloat = 1.5
    static let addBG = NSColor.syAddBG
    static let delBG = NSColor.syDelBG
    static let hunkBG = NSColor.syHunkBG
    static let paragraph: NSParagraphStyle = {
        let p = NSMutableParagraphStyle()
        p.lineBreakMode = .byCharWrapping
        return p
    }()
    static let lineHeight: CGFloat = ceil(font.ascender - font.descender + font.leading)

    static func codeAttributes(_ kind: DiffLine.Kind) -> [NSAttributedString.Key: Any] {
        [.font: font, .paragraphStyle: paragraph, .foregroundColor: color(kind)]
    }

    static func color(_ kind: DiffLine.Kind) -> NSColor {
        switch kind {
        case .add: return .syAddText
        case .del: return .syDelText
        case .hunk, .meta: return .syHunkText
        default: return .syText
        }
    }

    static let gutterAttributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: NSColor.syGutter]

    static func codeText(_ line: DiffLine) -> String {
        switch line.kind {
        case .add: return "+" + line.text
        case .del: return "-" + line.text
        case .context: return " " + line.text
        default: return line.text
        }
    }

    static func height(_ line: DiffLine, width: CGFloat) -> CGFloat {
        let text = codeText(line)
        let available = max(80, width - gutterWidth - 12)
        let approxChars = Int(available / 7.2)
        if text.count < approxChars - 2 { return lineHeight + vPad * 2 + (line.kind == .hunk ? 4 : 0) }
        let rect = (text as NSString).boundingRect(with: NSSize(width: available, height: .greatestFiniteMagnitude), options: [.usesLineFragmentOrigin], attributes: codeAttributes(line.kind))
        return ceil(rect.height) + vPad * 2 + (line.kind == .hunk ? 4 : 0)
    }
}

final class DiffLineCell: NSView {
    var line: DiffLine?
    var commentable = false
    var onComment: (() -> Void)?

    override var isFlipped: Bool { true }

    override func draw(_ dirtyRect: NSRect) {
        guard let line else { return }
        switch line.kind {
        case .add: DiffStyle.addBG.setFill(); bounds.fill()
        case .del: DiffStyle.delBG.setFill(); bounds.fill()
        case .hunk: DiffStyle.hunkBG.setFill(); bounds.fill()
        default: break
        }
        let pad: CGFloat = line.kind == .hunk ? 3.5 : DiffStyle.vPad
        if line.kind != .hunk {
            let o = line.oldLine.map(String.init) ?? ""
            let n = line.newLine.map(String.init) ?? ""
            let gutter = String(repeating: " ", count: max(0, 5 - o.count)) + o + " " + String(repeating: " ", count: max(0, 5 - n.count)) + n + (commentable && line.commentable ? " +" : "  ")
            (gutter as NSString).draw(at: NSPoint(x: 4, y: pad), withAttributes: DiffStyle.gutterAttributes)
        }
        let x = DiffStyle.gutterWidth
        let rect = NSRect(x: x, y: pad, width: bounds.width - x - 8, height: bounds.height - pad)
        (DiffStyle.codeText(line) as NSString).draw(with: rect, options: [.usesLineFragmentOrigin], attributes: DiffStyle.codeAttributes(line.kind))
    }

    override func mouseDown(with event: NSEvent) {
        let p = convert(event.locationInWindow, from: nil)
        if p.x < DiffStyle.gutterWidth, commentable, line?.commentable == true {
            onComment?()
        } else {
            super.mouseDown(with: event)
        }
    }

    override func resetCursorRects() {
        if commentable, line?.commentable == true {
            addCursorRect(NSRect(x: 0, y: 0, width: DiffStyle.gutterWidth, height: bounds.height), cursor: .pointingHand)
        }
    }

    override func menu(for event: NSEvent) -> NSMenu? {
        guard let line else { return nil }
        let menu = NSMenu()
        let copy = NSMenuItem(title: t("Копировать строку", "Copy line"), action: #selector(copyLine), keyEquivalent: "")
        copy.target = self
        menu.addItem(copy)
        if commentable, line.commentable {
            let c = NSMenuItem(title: t("Комментировать строку", "Comment on line"), action: #selector(comment), keyEquivalent: "")
            c.target = self
            menu.addItem(c)
        }
        return menu
    }

    @objc private func copyLine() { if let line { copyToPasteboard(line.text) } }
    @objc private func comment() { onComment?() }
}

final class HostingCell: NSTableCellView {
    let host = NSHostingView(rootView: AnyView(EmptyView()))

    override init(frame: NSRect) {
        super.init(frame: frame)
        host.translatesAutoresizingMaskIntoConstraints = false
        addSubview(host)
        NSLayoutConstraint.activate([
            host.leadingAnchor.constraint(equalTo: leadingAnchor),
            host.trailingAnchor.constraint(equalTo: trailingAnchor),
            host.topAnchor.constraint(equalTo: topAnchor),
            host.bottomAnchor.constraint(equalTo: bottomAnchor)
        ])
    }

    required init?(coder: NSCoder) { fatalError() }
}

private struct HeightReporter: ViewModifier {
    let report: (CGFloat) -> Void

    func body(content: Content) -> some View {
        content
            .fixedSize(horizontal: false, vertical: true)
            .background(GeometryReader { g in
                Color.clear
                    .onAppear { report(g.size.height) }
                    .onChange(of: g.size.height) { _, h in report(h) }
            })
            .frame(maxHeight: .infinity, alignment: .top)
    }
}

final class DiffTable: NSTableView {
    override func drawGrid(inClipRect clipRect: NSRect) {}
}

struct DiffTableView: NSViewRepresentable {
    let rows: [DiffTableRow]
    var version: Int = 0
    var scrollTarget: String?
    var onComment: ((String, DiffLine) -> Void)?

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSScrollView {
        let table = DiffTable()
        table.headerView = nil
        table.backgroundColor = .clear
        table.intercellSpacing = .zero
        table.selectionHighlightStyle = .none
        table.floatsGroupRows = true
        table.usesAlternatingRowBackgroundColors = false
        table.style = .plain
        table.columnAutoresizingStyle = .uniformColumnAutoresizingStyle
        let col = NSTableColumn(identifier: .init("c"))
        col.resizingMask = .autoresizingMask
        table.addTableColumn(col)
        table.dataSource = context.coordinator
        table.delegate = context.coordinator

        let scroll = NSScrollView()
        scroll.documentView = table
        scroll.hasVerticalScroller = true
        scroll.drawsBackground = false
        scroll.autohidesScrollers = true
        scroll.contentView.postsBoundsChangedNotifications = true
        context.coordinator.table = table
        context.coordinator.observeWidth(scroll)
        context.coordinator.update(rows: rows, version: version, onComment: onComment)
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        context.coordinator.update(rows: rows, version: version, onComment: onComment)
        if let target = scrollTarget, target != context.coordinator.lastScrollTarget {
            context.coordinator.lastScrollTarget = target
            context.coordinator.scroll(to: target)
        }
    }

    final class Coordinator: NSObject, NSTableViewDataSource, NSTableViewDelegate {
        weak var table: NSTableView?
        var rows: [DiffTableRow] = []
        var ids: [String] = []
        var signature: [Int] = []
        var version = -1
        var onComment: ((String, DiffLine) -> Void)?
        var lastScrollTarget: String?
        private var lineHeights: [String: CGFloat] = [:]
        private var viewHeights: [String: CGFloat] = [:]
        private var width: CGFloat = 0
        private var widthObserver: NSObjectProtocol?

        deinit {
            if let widthObserver { NotificationCenter.default.removeObserver(widthObserver) }
        }

        func observeWidth(_ scroll: NSScrollView) {
            widthObserver = NotificationCenter.default.addObserver(forName: NSView.frameDidChangeNotification, object: scroll.contentView, queue: .main) { [weak self] _ in
                guard let self, let table = self.table else { return }
                let w = table.bounds.width
                if abs(w - self.width) > 1 {
                    self.width = w
                    self.lineHeights.removeAll()
                    table.noteHeightOfRows(withIndexesChanged: IndexSet(integersIn: 0..<self.rows.count))
                }
            }
            scroll.contentView.postsFrameChangedNotifications = true
        }

        func update(rows newRows: [DiffTableRow], version newVersion: Int, onComment: ((String, DiffLine) -> Void)?) {
            self.onComment = onComment
            let newIds = newRows.map(\.id)
            let newSignature = newRows.map { row -> Int in
                switch row {
                case .line(let id, let line, let commentable):
                    var h = Hasher()
                    h.combine(id)
                    h.combine(line.text)
                    h.combine(line.oldLine)
                    h.combine(line.newLine)
                    h.combine(commentable)
                    return h.finalize()
                case .view(let id, _, _):
                    return id.hashValue
                }
            }
            guard newIds != ids || newVersion != version || newSignature != signature else {
                rows = newRows
                refreshVisibleHosts()
                return
            }
            rows = newRows
            ids = newIds
            signature = newSignature
            version = newVersion
            lineHeights.removeAll()
            table?.reloadData()
        }

        private func refreshVisibleHosts() {
            guard let table else { return }
            let visible = table.rows(in: table.visibleRect)
            for i in visible.location..<min(rows.count, visible.location + visible.length) {
                if case .view(let id, _, let content) = rows[i], let cell = table.view(atColumn: 0, row: i, makeIfNecessary: false) as? HostingCell {
                    cell.host.rootView = wrapped(id: id, content: content)
                }
            }
        }

        func scroll(to id: String) {
            guard let table, let i = ids.firstIndex(of: id) else { return }
            let rect = table.rect(ofRow: i)
            table.enclosingScrollView?.contentView.scroll(to: NSPoint(x: 0, y: rect.minY))
            table.enclosingScrollView?.reflectScrolledClipView(table.enclosingScrollView!.contentView)
        }

        func numberOfRows(in tableView: NSTableView) -> Int { rows.count }

        func tableView(_ tableView: NSTableView, isGroupRow row: Int) -> Bool {
            if case .view(_, let group, _) = rows[row] { return group }
            return false
        }

        func tableView(_ tableView: NSTableView, heightOfRow row: Int) -> CGFloat {
            if width == 0 { width = tableView.bounds.width }
            switch rows[row] {
            case .line(let id, let line, _):
                if let h = lineHeights[id] { return h }
                let h = DiffStyle.height(line, width: width)
                lineHeights[id] = h
                return h
            case .view(let id, _, _):
                return viewHeights[id] ?? 34
            }
        }

        func tableView(_ tableView: NSTableView, viewFor tableColumn: NSTableColumn?, row: Int) -> NSView? {
            switch rows[row] {
            case .line(let id, let line, let commentable):
                let cell = (tableView.makeView(withIdentifier: .init("line"), owner: nil) as? DiffLineCell) ?? {
                    let c = DiffLineCell()
                    c.identifier = .init("line")
                    return c
                }()
                cell.line = line
                cell.commentable = commentable
                cell.onComment = { [weak self] in self?.onComment?(id, line) }
                cell.needsDisplay = true
                cell.window?.invalidateCursorRects(for: cell)
                return cell
            case .view(let id, _, let content):
                let cell = (tableView.makeView(withIdentifier: .init("host"), owner: nil) as? HostingCell) ?? {
                    let c = HostingCell()
                    c.identifier = .init("host")
                    return c
                }()
                cell.host.rootView = wrapped(id: id, content: content)
                return cell
            }
        }

        func tableView(_ tableView: NSTableView, rowViewForRow row: Int) -> NSTableRowView? {
            let v = NSTableRowView()
            v.isGroupRowStyle = false
            return v
        }

        private func wrapped(id: String, content: () -> AnyView) -> AnyView {
            AnyView(content().modifier(HeightReporter { [weak self] h in self?.report(id: id, height: h) }))
        }

        private func report(id: String, height: CGFloat) {
            let h = max(1, ceil(height))
            guard abs((viewHeights[id] ?? 0) - h) > 0.5 else { return }
            viewHeights[id] = h
            DispatchQueue.main.async { [weak self] in
                guard let self, let table = self.table, let i = self.ids.firstIndex(of: id) else { return }
                NSAnimationContext.runAnimationGroup { ctx in
                    ctx.duration = 0
                    table.noteHeightOfRows(withIndexesChanged: IndexSet(integer: i))
                }
            }
        }
    }
}
