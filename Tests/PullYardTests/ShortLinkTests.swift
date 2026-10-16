import XCTest
@testable import PullYard

final class ShortLinkTests: XCTestCase {
    func testFormatsItemRef() {
        let ref = ItemRef(repo: RepoRef(owner: "pullyard", name: "pullyard"), number: 123)
        XCTAssertEqual(ShortLink.make(ref), "pullyard/pullyard#123")
    }

    func testPullRequestURL() {
        XCTAssertEqual(ShortLink.make(url: "https://github.com/Owner/Repo.Name/pull/42"), "Owner/Repo.Name#42")
    }

    func testIssueURL() {
        XCTAssertEqual(ShortLink.make(url: "https://github.com/owner/repo/issues/7"), "owner/repo#7")
    }

    func testPullRequestSubpageURL() {
        XCTAssertEqual(ShortLink.make(url: "https://github.com/owner/repo/pull/9/files"), "owner/repo#9")
    }

    func testAPIURL() {
        XCTAssertEqual(ShortLink.make(url: "https://api.github.com/repos/owner/repo/pulls/15"), "owner/repo#15")
    }

    func testNonItemURL() {
        XCTAssertNil(ShortLink.make(url: "https://github.com/owner/repo"))
        XCTAssertNil(ShortLink.make(url: "https://github.com/owner/repo/commit/abc123"))
    }

    func testRoute() {
        let ref = ItemRef(repo: RepoRef(owner: "a", name: "b"), number: 1)
        XCTAssertEqual(ShortLink.ref(route: .pr(ref)), ref)
        XCTAssertEqual(ShortLink.ref(route: .issue(ref)), ref)
        XCTAssertNil(ShortLink.ref(route: .repo(ref.repo)))
        XCTAssertNil(ShortLink.ref(route: nil))
    }

    func testRoundTripsThroughShortParser() {
        let ref = ItemRef(repo: RepoRef(owner: "owner", name: "repo"), number: 99)
        XCTAssertEqual(GitHubURL.parse(ShortLink.make(ref)), .pr(ref))
    }
}
