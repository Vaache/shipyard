import XCTest
@testable import PullYard

final class StubProtocol: URLProtocol {
    nonisolated(unsafe) static var routes: [String: (Int, String)] = [:]
    nonisolated(unsafe) static var requests: [URLRequest] = []

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        Self.requests.append(request)
        let path = request.url?.path ?? ""
        let (status, body) = Self.routes.first { path.hasSuffix($0.key) }?.value ?? (404, #"{"message":"not found"}"#)
        let response = HTTPURLResponse(url: request.url!, statusCode: status, httpVersion: nil, headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: Data(body.utf8))
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

@MainActor
final class ForgeClientTests: XCTestCase {
    override func setUp() {
        StubProtocol.routes = [:]
        StubProtocol.requests = []
        ForgeHTTP.protocolClasses = [StubProtocol.self]
    }

    override func tearDown() {
        ForgeHTTP.protocolClasses = []
    }

    private func account(_ provider: GitProvider, host: String, login: String = "", email: String? = nil, userID: String? = nil) -> GitAccount {
        GitAccount(id: "test", provider: provider, host: host, login: login, email: email, userID: userID, keychainKey: "test")
    }

    func testGitLabMapsMergeRequests() async throws {
        StubProtocol.routes = [
            "/api/v4/user": (200, #"{"id":7,"username":"sofia","name":"Sofia","avatar_url":"https://a/s.png"}"#),
            "/api/v4/merge_requests": (200, """
            [{"iid":12,"title":"Fix login","web_url":"https://gitlab.example.com/group/sub/app/-/merge_requests/12","state":"merged","draft":false,
              "created_at":"2026-10-01T10:00:00.123Z","updated_at":"2026-10-02T11:30:00.456Z","target_branch":"main",
              "author":{"username":"max","avatar_url":null},"references":{"full":"group/sub/app!12"},"user_notes_count":3,
              "labels":[{"name":"bug","color":"#FF0000"}]}]
            """)
        ]
        let client = GitLabClient(account: account(.gitlab, host: "gitlab.example.com"), credential: GitCredential(token: "glpat-x"))
        let user = try await client.currentUser()
        XCTAssertEqual(user.login, "sofia")
        let items = try await client.pullRequests(.reviewRequested, state: .open)
        XCTAssertEqual(items.count, 1)
        let mr = try XCTUnwrap(items.first)
        XCTAssertEqual(mr.number, 12)
        XCTAssertEqual(mr.prState, "MERGED")
        XCTAssertEqual(mr.repository.nameWithOwner, "group/sub/app")
        XCTAssertEqual(mr.baseRefName, "main")
        XCTAssertEqual(mr.labels?.nodes.first?.color, "FF0000")
        XCTAssertEqual(mr.comments?.totalCount, 3)
        let query = StubProtocol.requests.last?.url?.query ?? ""
        XCTAssertTrue(query.contains("reviewer_username=sofia"), query)
        XCTAssertTrue(query.contains("state=opened"), query)
        XCTAssertEqual(StubProtocol.requests.last?.value(forHTTPHeaderField: "Authorization"), "Bearer glpat-x")
    }

    func testGitLabUnsupportedFilterReturnsEmpty() async throws {
        let client = GitLabClient(account: account(.gitlab, host: "gitlab.com", login: "sofia"), credential: GitCredential(token: "t"))
        let items = try await client.pullRequests(.mentioned, state: .open)
        XCTAssertTrue(items.isEmpty)
        XCTAssertTrue(StubProtocol.requests.isEmpty)
    }

    func testBitbucketMineAcrossWorkspaces() async throws {
        StubProtocol.routes = [
            "/2.0/user/workspaces": (200, #"{"values":[{"workspace":{"slug":"acme"}},{"workspace":{"slug":"side"}}]}"#),
            "/pullrequests/{u-1}": (200, """
            {"values":[{"id":5,"title":"Add cache","state":"OPEN","draft":true,
              "created_on":"2026-10-01T10:00:00.123456+00:00","updated_on":"2026-10-03T09:00:00.000001+00:00",
              "author":{"uuid":"{u-1}","display_name":"Sofia","nickname":"sofia","links":{"avatar":{"href":"https://a/s.png"}}},
              "destination":{"branch":{"name":"main"},"repository":{"full_name":"acme/api"}},
              "links":{"html":{"href":"https://bitbucket.org/acme/api/pull-requests/5"}},"comment_count":2,
              "participants":[{"approved":true,"state":"approved"}]}]}
            """)
        ]
        let client = BitbucketClient(account: account(.bitbucket, host: "bitbucket.org", login: "sofia", email: "s@example.com", userID: "{u-1}"), credential: GitCredential(token: "tok"))
        let items = try await client.pullRequests(.mine, state: .open)
        XCTAssertEqual(items.count, 1)
        let pr = try XCTUnwrap(items.first)
        XCTAssertEqual(pr.repository.nameWithOwner, "acme/api")
        XCTAssertEqual(pr.isDraft, true)
        XCTAssertEqual(pr.reviewDecision, "APPROVED")
        XCTAssertEqual(pr.url, "https://bitbucket.org/acme/api/pull-requests/5")
        let auth = StubProtocol.requests.first?.value(forHTTPHeaderField: "Authorization")
        XCTAssertEqual(auth, "Basic " + Data("s@example.com:tok".utf8).base64EncodedString())
        let prRequests = StubProtocol.requests.filter { $0.url?.path.contains("/pullrequests/") == true }
        XCTAssertEqual(prRequests.count, 2)
        XCTAssertTrue(prRequests[0].url?.absoluteString.contains("/workspaces/acme/pullrequests/%7Bu-1%7D") == true, prRequests[0].url?.absoluteString ?? "")
    }

    func testBitbucketCachesResults() async throws {
        StubProtocol.routes = [
            "/2.0/user/workspaces": (200, #"{"values":[{"workspace":{"slug":"acme"}}]}"#),
            "/pullrequests/{u-1}": (200, #"{"values":[]}"#)
        ]
        let client = BitbucketClient(account: account(.bitbucket, host: "bitbucket.org", login: "sofia", email: "s@example.com", userID: "{u-1}"), credential: GitCredential(token: "tok"))
        _ = try await client.pullRequests(.mine, state: .open)
        let count = StubProtocol.requests.count
        _ = try await client.pullRequests(.mine, state: .open)
        XCTAssertEqual(StubProtocol.requests.count, count)
    }

    func testGiteaMapsMergedPull() async throws {
        StubProtocol.routes = [
            "/api/v1/repos/issues/search": (200, """
            [{"number":3,"title":"Docs","html_url":"https://codeberg.org/me/site/pulls/3","state":"closed",
              "created_at":"2026-09-01T10:00:00+02:00","updated_at":"2026-09-02T10:00:00Z",
              "user":{"id":1,"login":"me","full_name":"","avatar_url":"https://a/me.png"},
              "repository":{"full_name":"me/site"},"comments":0,"labels":[],"pull_request":{"merged":true,"draft":false}}]
            """)
        ]
        let client = GiteaClient(account: account(.gitea, host: "codeberg.org", login: "me"), credential: GitCredential(token: "tok"))
        let items = try await client.pullRequests(.mine, state: .closed)
        XCTAssertEqual(items.first?.prState, "MERGED")
        XCTAssertEqual(items.first?.repository.nameWithOwner, "me/site")
        let query = StubProtocol.requests.last?.url?.query ?? ""
        XCTAssertTrue(query.contains("created=true") && query.contains("state=closed") && query.contains("type=pulls"), query)
        XCTAssertEqual(StubProtocol.requests.last?.value(forHTTPHeaderField: "Authorization"), "token tok")
    }

    func testUnauthorizedIsReported() async {
        StubProtocol.routes = ["/api/v1/user": (401, #"{"message":"bad token"}"#)]
        let client = GiteaClient(account: account(.gitea, host: "codeberg.org"), credential: GitCredential(token: "x"))
        do {
            _ = try await client.currentUser()
            XCTFail("expected 401")
        } catch {
            XCTAssertEqual((error as? APIError)?.status, 401)
        }
    }
}

final class GitAccountTests: XCTestCase {
    func testNormalizesHost() {
        XCTAssertEqual(GitProvider.normalizeHost(" https://GitLab.Example.com/ "), "gitlab.example.com")
        XCTAssertEqual(GitProvider.normalizeHost("http://git.local//"), "git.local")
    }

    func testCredentialRoundTrip() {
        XCTAssertEqual(GitCredential(token: "plain").encoded, "plain")
        XCTAssertEqual(GitCredential.decode("ghp_legacy").token, "ghp_legacy")
        let full = GitCredential(token: "a", refreshToken: "r", expiresAt: Date(timeIntervalSince1970: 100), clientID: "c")
        let back = GitCredential.decode(full.encoded)
        XCTAssertEqual(back.token, "a")
        XCTAssertEqual(back.refreshToken, "r")
        XCTAssertEqual(back.clientID, "c")
    }

    func testParsesForgeDates() {
        let expected = Date(timeIntervalSince1970: 1_790_850_600)
        XCTAssertEqual(ForgeHTTP.parseDate("2026-10-01T10:30:00Z"), expected)
        XCTAssertEqual(ForgeHTTP.parseDate("2026-10-01T10:30:00.123456+00:00"), expected)
        XCTAssertEqual(ForgeHTTP.parseDate("2026-10-01T12:30:00.5+02:00"), expected)
        XCTAssertNil(ForgeHTTP.parseDate("yesterday"))
    }

    func testGitLabTerminology() {
        XCTAssertEqual(GitProvider.gitlab.requestsTitle, "Merge requests")
        XCTAssertEqual(GitProvider.gitlab.requestShort, "MR")
        XCTAssertEqual(GitProvider.gitlab.numberPrefix, "!")
        XCTAssertEqual(GitProvider.bitbucket.requestShort, "PR")
    }

    func testCacheKeysDifferPerHost() {
        let gh = GitAccount(id: "1", provider: .github, host: "github.com", login: "sofia", keychainKey: "k")
        let ghe = GitAccount(id: "2", provider: .githubEnterprise, host: "git.corp", login: "sofia", keychainKey: "k2")
        XCTAssertEqual(gh.cacheKey, "sofia")
        XCTAssertNotEqual(gh.cacheKey, ghe.cacheKey)
    }
}
