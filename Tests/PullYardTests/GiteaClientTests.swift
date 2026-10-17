import XCTest
@testable import PullYard

@MainActor
final class GiteaClientTests: XCTestCase {
    override func setUp() {
        StubProtocol.routes = [:]
        StubProtocol.queryRoutes = []
        StubProtocol.requests = []
        ForgeHTTP.protocolClasses = [StubProtocol.self]
    }

    override func tearDown() {
        ForgeHTTP.protocolClasses = []
    }

    private func client(host: String = "codeberg.org") -> GiteaClient {
        let account = GitAccount(id: "test", provider: .gitea, host: host, login: "me", email: nil, userID: nil, keychainKey: "test")
        return GiteaClient(account: account, credential: GitCredential(token: "tok"))
    }

    private var lastQuery: String { StubProtocol.requests.last?.url?.query ?? "" }

    func testCapabilitiesMatchGitHubSections() async {
        let caps = await client().capabilities()
        XCTAssertEqual(caps.pullFilters, PRFilter.allCases)
        XCTAssertEqual(caps.issueFilters, IssueFilter.allCases)
        XCTAssertEqual(caps.inboxTitle, "Notifications")
        XCTAssertTrue(caps.repositories)
    }

    func testEveryPullFilterUsesSearchFlag() async throws {
        StubProtocol.routes = ["/api/v1/repos/issues/search": (200, "[]")]
        let gitea = client()
        let expected: [PRFilter: String] = [
            .reviewRequested: "review_requested=true",
            .mine: "created=true",
            .assigned: "assigned=true",
            .mentioned: "mentioned=true",
            .reviewed: "reviewed=true"
        ]
        for (filter, flag) in expected {
            _ = try await gitea.pullRequests(filter, state: .open)
            XCTAssertTrue(lastQuery.contains(flag), "\(filter): \(lastQuery)")
            XCTAssertTrue(lastQuery.contains("type=pulls"), lastQuery)
            XCTAssertTrue(lastQuery.contains("state=open"), lastQuery)
            XCTAssertTrue(lastQuery.contains("sort=recentupdate"), lastQuery)
            XCTAssertTrue(lastQuery.contains("limit=50"), lastQuery)
        }
        XCTAssertEqual(StubProtocol.requests.count, expected.count)
    }

    func testPullRequestMapping() async throws {
        StubProtocol.routes = [
            "/api/v1/repos/issues/search": (200, """
            [{"number":3,"title":"Docs","html_url":"https://codeberg.org/me/site/pulls/3","state":"open",
              "created_at":"2026-09-01T10:00:00+02:00","updated_at":"2026-09-02T10:00:00Z",
              "user":{"id":1,"login":"max","full_name":"Max","avatar_url":"https://a/max.png"},
              "repository":{"id":9,"name":"site","owner":"me","full_name":"me/site"},"comments":4,
              "labels":[{"name":"docs","color":"#00aabb"}],"pull_request":{"merged":false,"draft":true}},
             {"number":2,"title":"Old","html_url":"https://codeberg.org/me/site/pulls/2","state":"closed",
              "created_at":"2026-08-01T10:00:00Z","updated_at":"2026-08-02T10:00:00Z",
              "repository":{"full_name":"me/site"},"pull_request":{"merged":false}}]
            """)
        ]
        let items = try await client().pullRequests(.reviewRequested, state: .all)
        XCTAssertEqual(items.map(\.number), [3, 2])
        let pr = try XCTUnwrap(items.first)
        XCTAssertTrue(pr.isPR)
        XCTAssertEqual(pr.prState, "OPEN")
        XCTAssertEqual(pr.isDraft, true)
        XCTAssertEqual(pr.author?.login, "max")
        XCTAssertEqual(pr.comments?.totalCount, 4)
        XCTAssertEqual(pr.labels?.nodes.first?.color, "00aabb")
        XCTAssertEqual(items.last?.prState, "CLOSED")
        XCTAssertTrue(lastQuery.contains("state=all"), lastQuery)
    }

    func testIssuesUseSearchWithIssueType() async throws {
        StubProtocol.routes = [
            "/api/v1/repos/issues/search": (200, """
            [{"number":11,"title":"Crash on start","html_url":"https://codeberg.org/org/app/issues/11","state":"closed",
              "created_at":"2026-10-01T09:00:00Z","updated_at":"2026-10-02T09:00:00Z",
              "user":{"id":2,"login":"ana","avatar_url":null},"repository":{"full_name":"org/app"},"comments":2,
              "labels":[{"name":"bug","color":"ee0701"}],"pull_request":null}]
            """)
        ]
        let gitea = client()
        let expected: [IssueFilter: String] = [.assigned: "assigned=true", .mine: "created=true", .mentioned: "mentioned=true"]
        for (filter, flag) in expected {
            let items = try await gitea.issues(filter, state: .closed)
            XCTAssertTrue(lastQuery.contains(flag), "\(filter): \(lastQuery)")
            XCTAssertTrue(lastQuery.contains("type=issues"), lastQuery)
            XCTAssertTrue(lastQuery.contains("state=closed"), lastQuery)
            let issue = try XCTUnwrap(items.first)
            XCTAssertFalse(issue.isPR)
            XCTAssertEqual(issue.issueState, "CLOSED")
            XCTAssertEqual(issue.repository.nameWithOwner, "org/app")
            XCTAssertEqual(issue.url, "https://codeberg.org/org/app/issues/11")
            XCTAssertEqual(issue.labels?.nodes.first?.name, "bug")
        }
    }

    func testNotificationsInbox() async throws {
        StubProtocol.routes = [
            "/api/v1/notifications": (200, """
            [{"id":5,"unread":true,"pinned":false,"updated_at":"2026-10-03T10:00:00Z",
              "subject":{"title":"Fix login","type":"Pull","state":"merged","url":"https://codeberg.org/api/v1/repos/org/app/pulls/7","html_url":"","latest_comment_html_url":""},
              "repository":{"full_name":"org/app","html_url":"https://codeberg.org/org/app"}},
             {"id":6,"unread":false,"pinned":true,"updated_at":"2026-10-04T10:00:00Z",
              "subject":{"title":"Crash","type":"Issue","state":"open","url":"https://codeberg.org/api/v1/repos/org/app/issues/8","html_url":"https://codeberg.org/org/app/issues/8","latest_comment_html_url":"https://codeberg.org/org/app/issues/8#issuecomment-42"},
              "repository":{"full_name":"org/app"}},
             {"id":7,"unread":true,"updated_at":"2026-10-01T10:00:00Z",
              "subject":{"title":"Bump","type":"Commit","url":"https://codeberg.org/api/v1/repos/org/app/git/commits/abc123"},
              "repository":{"full_name":"org/app"}}]
            """)
        ]
        let inbox = try await client().notifications()
        XCTAssertEqual(inbox.map(\.id), ["6", "5", "7"])
        let pinned = inbox[0]
        XCTAssertEqual(pinned.reason, "Pinned")
        XCTAssertEqual(pinned.kind, "Issue")
        XCTAssertFalse(pinned.unread)
        XCTAssertEqual(pinned.url, "https://codeberg.org/org/app/issues/8#issuecomment-42")
        let pull = inbox[1]
        XCTAssertEqual(pull.title, "Fix login")
        XCTAssertEqual(pull.kind, "PR")
        XCTAssertEqual(pull.reason, "Pull request merged")
        XCTAssertEqual(pull.repo, "org/app")
        XCTAssertEqual(pull.url, "https://codeberg.org/org/app/pulls/7")
        XCTAssertTrue(pull.unread)
        XCTAssertEqual(inbox[2].url, "https://codeberg.org/org/app/commit/abc123")
        XCTAssertEqual(inbox[2].reason, "Commit")
        XCTAssertTrue(lastQuery.contains("limit=50"), lastQuery)
        XCTAssertEqual(StubProtocol.requests.count, 1)
    }

    func testMarkDoneMarksThreadRead() async throws {
        StubProtocol.routes = ["/api/v1/notifications/threads/5": (205, "")]
        let note = ForgeNotification(id: "5", title: "x", repo: "org/app", kind: "PR", reason: "Pull request", url: "https://codeberg.org/org/app/pulls/7", updatedAt: Date(), author: nil, unread: true)
        try await client().markDone(note)
        let request = try XCTUnwrap(StubProtocol.requests.last)
        XCTAssertEqual(request.httpMethod, "PATCH")
        XCTAssertEqual(request.url?.path, "/api/v1/notifications/threads/5")
        XCTAssertTrue(request.url?.query?.contains("to-status=read") == true)
        XCTAssertEqual(request.value(forHTTPHeaderField: "Authorization"), "token tok")
    }

    func testRepositoriesSortedByUpdate() async throws {
        StubProtocol.routes = [
            "/api/v1/user/repos": (200, """
            [{"full_name":"me/old","description":"","html_url":"https://codeberg.org/me/old","private":false,"archived":false,
              "updated_at":"2026-01-01T10:00:00Z","default_branch":"main","stars_count":1},
             {"full_name":"me/new","description":"Fresh","html_url":"https://codeberg.org/me/new","private":true,"archived":false,
              "updated_at":"2026-10-01T10:00:00Z","default_branch":"dev","stars_count":5},
             {"full_name":"me/gone","html_url":"https://codeberg.org/me/gone","archived":true,"updated_at":"2026-10-05T10:00:00Z"}]
            """)
        ]
        let repos = try await client().repositories()
        XCTAssertEqual(repos.map(\.fullName), ["me/new", "me/old"])
        let fresh = repos[0]
        XCTAssertEqual(fresh.description, "Fresh")
        XCTAssertTrue(fresh.isPrivate)
        XCTAssertEqual(fresh.defaultBranch, "dev")
        XCTAssertEqual(fresh.stars, 5)
        XCTAssertEqual(fresh.url, "https://codeberg.org/me/new")
        XCTAssertNil(repos[1].description)
        XCTAssertEqual(StubProtocol.requests.count, 1)
        XCTAssertTrue(lastQuery.contains("order_by=recentupdate") && lastQuery.contains("limit=50") && lastQuery.contains("page=1"), lastQuery)
    }

    func testRepositoriesFollowFullPages() async throws {
        let repo = { (n: Int) in #"{"full_name":"me/r\#(n)","html_url":"https://codeberg.org/me/r\#(n)","updated_at":"2026-10-01T10:00:00Z"}"# }
        StubProtocol.queryRoutes = [
            ("page=1", 200, "[" + (0..<50).map(repo).joined(separator: ",") + "]"),
            ("page=2", 200, "[" + repo(50) + "]")
        ]
        let repos = try await client().repositories()
        XCTAssertEqual(repos.count, 51)
        XCTAssertEqual(StubProtocol.requests.count, 2)
    }
}
