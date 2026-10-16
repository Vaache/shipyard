import Foundation

struct BlameMap {
    let id: String
    let ranges: [BlameRange]
    private let owners: [Int32]

    init(id: String, ranges: [BlameRange]) {
        self.id = id
        let sorted = ranges.sorted { $0.startingLine < $1.startingLine }
        self.ranges = sorted
        var owners = [Int32](repeating: -1, count: sorted.map(\.endingLine).max() ?? 0)
        for (i, r) in sorted.enumerated() where r.startingLine >= 1 && r.endingLine >= r.startingLine {
            for line in r.startingLine...r.endingLine { owners[line - 1] = Int32(i) }
        }
        self.owners = owners
    }

    var lineCount: Int { owners.count }

    func range(atLine index: Int) -> BlameRange? {
        guard index >= 0, index < owners.count else { return nil }
        let owner = owners[index]
        return owner < 0 ? nil : ranges[Int(owner)]
    }

    func isBlockStart(_ index: Int) -> Bool {
        guard let current = range(atLine: index) else { return false }
        guard let previous = range(atLine: index - 1) else { return true }
        return previous.commit.oid != current.commit.oid
    }

    static func heat(age: Int) -> Double {
        min(max(Double(11 - age) / 10, 0.1), 1)
    }
}
