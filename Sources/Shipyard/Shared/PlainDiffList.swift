import SwiftUI

enum PlainDiffRow: Identifiable {
    case header(DiffFile)
    case line(DiffFile, DiffLine)
    case note(DiffFile, String)

    var id: String {
        switch self {
        case .header(let f): return "h:" + f.id
        case .line(let f, let l): return "l:" + f.id + ":\(l.id)"
        case .note(let f, _): return "n:" + f.id
        }
    }
}

struct PlainDiffList: View {
    let files: [DiffFile]
    @Binding var collapsed: Set<String>
    var scrollTarget: String?
    var onOpenFile: ((String) -> Void)?

    private var rows: [PlainDiffRow] {
        var out: [PlainDiffRow] = []
        out.reserveCapacity(files.reduce(0) { $0 + $1.lines.count } + files.count)
        for f in files {
            out.append(.header(f))
            guard !collapsed.contains(f.id) else { continue }
            if f.hasPatch {
                for l in f.lines { out.append(.line(f, l)) }
            } else {
                out.append(.note(f, t("Diff недоступен (бинарный или большой файл)", "Diff unavailable (binary or large file)")))
            }
        }
        return out
    }

    var body: some View {
        let table: [DiffTableRow] = rows.map { row in
            switch row {
            case .header(let f): return .view(id: row.id, group: true) { AnyView(header(f)) }
            case .line(_, let l): return .line(id: row.id, line: l, commentable: false)
            case .note(_, let text):
                return .view(id: row.id, group: false) {
                    AnyView(Text(text).font(.caption).foregroundStyle(Color.syMuted).padding(.horizontal, 12).padding(.vertical, 6).frame(maxWidth: .infinity, alignment: .leading))
                }
            }
        }
        return DiffTableView(rows: table, scrollTarget: scrollTarget.map { "h:" + $0 })
    }

    private func header(_ f: DiffFile) -> some View {
        let isCollapsed = collapsed.contains(f.id)
        return HStack(spacing: 8) {
            Image(systemName: isCollapsed ? "chevron.right" : "chevron.down").frame(width: 12).foregroundStyle(Color.syMuted)
            Text(f.statusLetter)
                .font(.caption2.weight(.bold).monospaced())
                .foregroundStyle(statusColor(f.status))
            Text(f.previousFilename.map { "\($0) → \(f.id)" } ?? f.id)
                .font(.system(.callout, design: .monospaced).weight(.medium))
                .lineLimit(1)
                .truncationMode(.head)
            DiffStat(additions: f.additions, deletions: f.deletions)
            Spacer()
            if let onOpenFile {
                Button { onOpenFile(f.id) } label: { Image(systemName: "arrow.up.forward.square") }
                    .buttonStyle(.borderless)
                    .help(t("Открыть файл", "Open file"))
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(Theme.isBrand ? AnyShapeStyle(Color.syRaised) : AnyShapeStyle(.bar))
        .overlay(alignment: .bottom) { Divider() }
        .contentShape(Rectangle())
        .onTapGesture {
            if isCollapsed { collapsed.remove(f.id) } else { collapsed.insert(f.id) }
        }
        .contextMenu {
            Button(t("Копировать путь", "Copy path")) { copyToPasteboard(f.id) }
        }
        .padding(.top, 6)
    }

    private func statusColor(_ s: String) -> Color {
        switch s {
        case "added": return .ghGreen
        case "removed": return .ghRed
        case "renamed": return .ghPurple
        default: return .ghYellow
        }
    }
}
