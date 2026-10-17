import XCTest
@testable import PullYard

@MainActor
final class BitbucketClientTests: XCTestCase {
    override func setUp() {
        StubProtocol.routes = [:]
        StubProtocol.queryRoutes = []
        StubProtocol.requests = []
        ForgeHTTP.protocolClasses = [StubProtocol.self]
    }

    override func tearDown() {
        ForgeHTTP.protocolClasses = []
    }

    private func client() -> BitbucketClient {
        let account = GitAccount(id: "test", provider: .bitbucket, host: "bitbucket.org", login: "sofia", email: "s@example.com", userID: "{u-1}", keychainKey: "test")
        return BitbucketClient(account: account, credential: GitCredential(token: "tok"))
    }

    private func pull(_ id: Int, author: String, reviewers: [String] = [], participants: [String] = [], updated: String = "2026-10-03T09:00:00+00:00") -> String {
        """
        {"id":\(id),"title":"PR \(id)","state":"OPEN","created_on":"2026-10-01T10:00:00+00:00","updated_on":"\(updated)",
         "author":{"uuid":"\(author)","nickname":"n\(id)"},
         "destination":{"branch":{"name":"main"},"repository":{"full_name":"acme/api"}},
         "reviewers":[\(reviewers.map { #"{"uuid":"\#($0)"}"# }.joined(separator: ","))],
         "participants":[\(participants.joined(separator: ","))]}
        """
    }

    private let workspaces = #"{"values":[{"workspace":{"slug":"acme"}}]}"#
    private let repos = """
    {"values":[{"full_name":"acme/api","is_private":true,"updated_on":"2026-10-04T10:00:00+00:00","links":{"html":{"href":"https://bitbucket.org/acme/api"}}}]}
    """

    private func requests(containing s: String) -> [URLRequest] {
        StubProtocol.requests.filter { $0.url?.absoluteString.contains(s) == true }
    }

    func testCapabilities() async {
        let caps = await client().capabilities()
        XCTAssertEqual(caps.pullFilters, [.reviewRequested, .mine, .reviewed])
        XCTAssertTrue(caps.repositories)
        XCTAssertTrue(caps.issueFilters.isEmpty)
        XCTAssertNil(caps.inboxTitle)
    }

    func testReviewRequestedAndReviewedShareOneFetch() async throws {
        let pulls = [
            pull(1, author: "{u-2}", reviewers: ["{u-1}"], participants: [#"{"user":{"uuid":"{u-1}"},"role":"REVIEWER","approved":false,"state":null}"#]),
            pull(2, author: "{u-2}", reviewers: ["{U-1}"], participants: [#"{"user":{"uuid":"{u-1}"},"role":"REVIEWER","approved":true,"state":"approved"}"#]),
            pull(3, author: "{u-3}", participants: [#"{"user":{"uuid":"{u-1}"},"role":"PARTICIPANT","approved":false,"state":"changes_requested"}"#]),
            pull(4, author: "{u-3}", participants: [#"{"user":{"uuid":"{u-1}"},"role":"PARTICIPANT","approved":false,"state":null}"#]),
            pull(5, author: "{u-1}", participants: [#"{"user":{"uuid":"{u-1}"},"role":"PARTICIPANT","approved":true,"state":"approved"}"#]),
            pull(6, author: "{u-2}", reviewers: ["{u-1}"])
        ]
        StubProtocol.routes = [
            "/2.0/user/workspaces": (200, workspaces),
            "/repositories/acme": (200, repos),
            "/acme/api/pullrequests": (200, #"{"values":[\#(pulls.joined(separator: ","))]}"#)
        ]
        let bb = client()
        let requested = try await bb.pullRequests(.reviewRequested, state: .open)
        XCTAssertEqual(Set(requested.map(\.number)), [1, 6])
        let count = StubProtocol.requests.count
        let reviewed = try await bb.pullRequests(.reviewed, state: .open)
        XCTAssertEqual(Set(reviewed.map(\.number)), [2, 3])
        XCTAssertEqual(reviewed.first { $0.number == 3 }?.reviewDecision, "CHANGES_REQUESTED")
        XCTAssertEqual(StubProtocol.requests.count, count)
        let prRequest = try XCTUnwrap(requests(containing: "/pullrequests").first?.url)
        let query = try XCTUnwrap(URLComponents(url: prRequest, resolvingAgainstBaseURL: false)?.queryItems)
        XCTAssertEqual(query.first { $0.name == "q" }?.value, #"reviewers.uuid="{u-1}" OR participants.user.uuid="{u-1}""#)
        XCTAssertEqual(query.first { $0.name == "fields" }?.value, "+values.participants,+values.reviewers")
    }

    func testFallsBackToReviewerQueryWhenParticipantFilterRejected() async throws {
        StubProtocol.queryRoutes = [("participants.user.uuid", 400, #"{"type":"error","error":{"message":"Invalid field name"}}"#)]
        StubProtocol.routes = [
            "/2.0/user/workspaces": (200, workspaces),
            "/repositories/acme": (200, repos),
            "/acme/api/pullrequests": (200, #"{"values":[\#(pull(1, author: "{u-2}", reviewers: ["{u-1}"]))]}"#)
        ]
        let bb = client()
        let requested = try await bb.pullRequests(.reviewRequested, state: .open)
        XCTAssertEqual(requested.map(\.number), [1])
        XCTAssertEqual(requests(containing: "participants.user.uuid").count, 1)
        _ = try await bb.pullRequests(.reviewRequested, state: .closed)
        XCTAssertEqual(requests(containing: "participants.user.uuid").count, 1)
        XCTAssertEqual(requests(containing: "/pullrequests").count, 3)
    }

    func testRepositoriesAcrossWorkspacesNewestFirst() async throws {
        StubProtocol.routes = [
            "/2.0/user/workspaces": (200, #"{"values":[{"workspace":{"slug":"acme"}},{"workspace":{"slug":"side"}}]}"#),
            "/repositories/acme": (200, """
            {"values":[{"full_name":"acme/old","description":"Legacy","is_private":false,"updated_on":"2026-09-01T10:00:00.123456+00:00",
              "mainbranch":{"name":"master"},"links":{"html":{"href":"https://bitbucket.org/acme/old"}}}]}
            """),
            "/repositories/side": (200, """
            {"values":[{"full_name":"side/new","description":"","is_private":true,"updated_on":"2026-10-05T10:00:00+00:00","mainbranch":{"name":"main"}}]}
            """),
            "/pullrequests": (200, #"{"values":[]}"#)
        ]
        let bb = client()
        let list = try await bb.repositories()
        XCTAssertEqual(list.map(\.fullName), ["side/new", "acme/old"])
        XCTAssertEqual(list[0].url, "https://bitbucket.org/side/new")
        XCTAssertEqual(list[0].defaultBranch, "main")
        XCTAssertEqual(list[1].isPrivate, false)
        XCTAssertEqual(list[1].description, "Legacy")
        let query = requests(containing: "/repositories/acme").first?.url?.query ?? ""
        XCTAssertTrue(query.contains("role=member") && query.contains("sort=-updated_on"), query)
        _ = try await bb.repositories()
        _ = try await bb.pullRequests(.reviewed, state: .open)
        XCTAssertEqual(requests(containing: "/repositories/acme?").count, 1)
        XCTAssertEqual(requests(containing: "/user/workspaces").count, 1)
        let prPaths = requests(containing: "/pullrequests").compactMap(\.url?.path)
        XCTAssertEqual(prPaths, ["/2.0/repositories/side/new/pullrequests", "/2.0/repositories/acme/old/pullrequests"])
    }

    func testReviewFetchCapsRepositories() async throws {
        let many = (0..<30).map { i in
            #"{"full_name":"acme/r\#(i)","updated_on":"2026-10-\#(String(format: "%02d", i + 1))T10:00:00+00:00"}"#
        }
        StubProtocol.routes = [
            "/2.0/user/workspaces": (200, workspaces),
            "/repositories/acme": (200, #"{"values":[\#(many.joined(separator: ","))]}"#),
            "/pullrequests": (200, #"{"values":[]}"#)
        ]
        let bb = client()
        _ = try await bb.pullRequests(.reviewRequested, state: .open)
        let prRequests = requests(containing: "/pullrequests")
        XCTAssertEqual(prRequests.count, BitbucketClient.maxRepos)
        XCTAssertEqual(prRequests.first?.url?.path, "/2.0/repositories/acme/r29/pullrequests")
    }

    func testReviewRequestedReportsUnauthorized() async {
        StubProtocol.routes = [
            "/2.0/user/workspaces": (200, workspaces),
            "/repositories/acme": (200, repos),
            "/acme/api/pullrequests": (401, #"{"error":{"message":"Unauthorized"}}"#)
        ]
        do {
            _ = try await client().pullRequests(.reviewed, state: .open)
            XCTFail("expected 401")
        } catch {
            XCTAssertEqual((error as? APIError)?.status, 401)
        }
    }
}
