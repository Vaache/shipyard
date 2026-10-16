import XCTest
@testable import PullYard

final class BlameMapTests: XCTestCase {
    private func commit(_ oid: String) -> BlameCommit {
        BlameCommit(oid: oid, abbreviatedOid: String(oid.prefix(7)), messageHeadline: "msg \(oid)", committedDate: Date(timeIntervalSince1970: 0), url: "https://github.com/o/r/commit/\(oid)", author: .init(name: "Name", user: .init(login: "login", avatarUrl: nil)))
    }

    private func range(_ start: Int, _ end: Int, _ oid: String, age: Int = 5) -> BlameRange {
        BlameRange(startingLine: start, endingLine: end, age: age, commit: commit(oid))
    }

    func testMapsLinesToRanges() {
        let map = BlameMap(id: "k", ranges: [range(4, 6, "c"), range(1, 3, "a")])
        XCTAssertEqual(map.lineCount, 6)
        XCTAssertEqual(map.range(atLine: 0)?.commit.oid, "a")
        XCTAssertEqual(map.range(atLine: 2)?.commit.oid, "a")
        XCTAssertEqual(map.range(atLine: 3)?.commit.oid, "c")
        XCTAssertEqual(map.range(atLine: 5)?.commit.oid, "c")
        XCTAssertNil(map.range(atLine: 6))
        XCTAssertNil(map.range(atLine: -1))
    }

    func testBlockStartsOncePerRange() {
        let map = BlameMap(id: "k", ranges: [range(1, 3, "a"), range(4, 4, "b"), range(5, 6, "a")])
        XCTAssertEqual((0..<6).filter(map.isBlockStart), [0, 3, 4])
    }

    func testAdjacentRangesOfSameCommitShareOneLabel() {
        let map = BlameMap(id: "k", ranges: [range(1, 2, "a"), range(3, 5, "a"), range(6, 6, "b")])
        XCTAssertEqual((0..<6).filter(map.isBlockStart), [0, 5])
    }

    func testGapsHaveNoRange() {
        let map = BlameMap(id: "k", ranges: [range(1, 1, "a"), range(4, 4, "b")])
        XCTAssertNil(map.range(atLine: 1))
        XCTAssertFalse(map.isBlockStart(1))
        XCTAssertTrue(map.isBlockStart(3))
    }

    func testIgnoresInvalidRanges() {
        let map = BlameMap(id: "k", ranges: [range(0, 2, "x"), range(3, 2, "y"), range(1, 1, "a")])
        XCTAssertEqual(map.range(atLine: 0)?.commit.oid, "a")
        XCTAssertNil(map.range(atLine: 1))
    }

    func testHeatIsStrongerForNewer() {
        XCTAssertEqual(BlameMap.heat(age: 1), 1, accuracy: 0.001)
        XCTAssertEqual(BlameMap.heat(age: 10), 0.1, accuracy: 0.001)
        XCTAssertGreaterThan(BlameMap.heat(age: 2), BlameMap.heat(age: 8))
        XCTAssertEqual(BlameMap.heat(age: 42), 0.1, accuracy: 0.001)
    }

    func testDecodesGraphQLResponse() throws {
        let json = """
        {"repository":{"object":{"blame":{"ranges":[{"startingLine":1,"endingLine":2,"age":3,"commit":{"oid":"abc","abbreviatedOid":"abc","messageHeadline":"Init","committedDate":"2026-01-02T03:04:05Z","url":"https://github.com/o/r/commit/abc","author":{"name":"Jo","user":null}}}]}}}}
        """
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        let r = try decoder.decode(BlameResponse.self, from: Data(json.utf8))
        let ranges = try XCTUnwrap(r.repository?.object?.blame?.ranges)
        XCTAssertEqual(ranges.count, 1)
        XCTAssertEqual(ranges[0].commit.authorLabel, "Jo")
    }

    func testAuthorLabelPrefersLogin() {
        XCTAssertEqual(commit("a").authorLabel, "login")
    }
}
