import XCTest
@testable import PullYard

@MainActor
final class GitLabReviewTests: XCTestCase {
    private let ref = ItemRef(repo: RepoRef(owner: "group", name: "sub/app"), number: 7)

    override func setUp() {
        StubProtocol.routes = [:]
        StubProtocol.queryRoutes = []
        StubProtocol.requests = []
        ForgeHTTP.protocolClasses = [StubProtocol.self]
    }

    override func tearDown() {
        ForgeHTTP.protocolClasses = []
    }

    private func backend(login: String = "sofia") -> GitLabPRBackend {
        let account = GitAccount(id: "test", provider: .gitlab, host: "gitlab.example.com", login: login, email: nil, userID: nil, keychainKey: "test")
        return GitLabPRBackend(client: GitLabClient(account: account, credential: GitCredential(token: "glpat-x")))
    }

    private func requests(_ method: String, _ suffix: String) -> [URLRequest] {
        StubProtocol.requests.filter { $0.httpMethod == method && ($0.url?.path ?? "").hasSuffix(suffix) }
    }

    private func json(_ request: URLRequest?) -> [String: Any] {
        guard let stream = request?.httpBodyStream else { return [:] }
        stream.open()
        defer { stream.close() }
        var data = Data()
        var buffer = [UInt8](repeating: 0, count: 4096)
        while stream.hasBytesAvailable {
            let n = stream.read(&buffer, maxLength: buffer.count)
            if n <= 0 { break }
            data.append(buffer, count: n)
        }
        return (try? JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
    }

    nonisolated private static let detail = """
    {"id":901,"iid":7,"project_id":42,"source_project_id":42,"target_project_id":42,"title":"Add login","description":"Adds **login**",
     "description_html":"<p>Adds <strong>login</strong> <img data-src=\\"/group/sub/app/uploads/a.png\\" src=\\"data:image/gif;base64,R0lG\\"> <a href=\\"/group/sub/app/-/issues/3\\">#3</a></p>",
     "state":"opened","draft":false,"web_url":"https://gitlab.example.com/group/sub/app/-/merge_requests/7",
     "created_at":"2026-10-01T10:00:00.000Z","updated_at":"2026-10-03T12:00:00.000Z",
     "author":{"id":1,"username":"max","name":"Max","avatar_url":"https://a/max.png"},
     "assignees":[{"id":1,"username":"max","avatar_url":null}],
     "reviewers":[{"id":2,"username":"sofia","avatar_url":null},{"id":3,"username":"lea","avatar_url":null}],
     "source_branch":"feature/login","target_branch":"main","sha":"head2",
     "diff_refs":{"base_sha":"base1","head_sha":"head2","start_sha":"start1"},
     "detailed_merge_status":"discussions_not_resolved","has_conflicts":false,"blocking_discussions_resolved":false,
     "labels":["backend"],"head_pipeline":{"id":55,"project_id":42,"status":"failed","web_url":"https://gitlab.example.com/p/55"},
     "changes_count":"3","user_notes_count":4,"squash":false,"merge_when_pipeline_succeeds":false,
     "user":{"can_merge":true},"references":{"full":"group/sub/app!7"}}
    """

    nonisolated private static let discussions = """
    [
     {"id":"d3","individual_note":false,"notes":[
       {"id":31,"type":"DiffNote","body":"Old remark","author":{"username":"lea"},"created_at":"2026-10-02T09:00:00Z","system":false,"resolvable":true,"resolved":true,
        "position":{"base_sha":"base0","start_sha":"base0","head_sha":"head1","old_path":"a.swift","new_path":"a.swift","old_line":null,"new_line":4,"position_type":"text"}}]},
     {"id":"d1","individual_note":true,"notes":[
       {"id":11,"type":null,"body":"approved this merge request","author":{"username":"lea"},"created_at":"2026-10-01T11:00:00Z","system":true}]},
     {"id":"d2","individual_note":true,"notes":[
       {"id":21,"type":null,"body":"Looks **good**","author":{"username":"lea","avatar_url":"https://a/lea.png"},"created_at":"2026-10-01T12:00:00Z","system":false}]},
     {"id":"d4","individual_note":false,"notes":[
       {"id":41,"type":"DiffNote","body":"Rename this","author":{"username":"sofia"},"created_at":"2026-10-02T10:00:00Z","system":false,"resolvable":true,"resolved":false,
        "position":{"base_sha":"base1","start_sha":"start1","head_sha":"head2","old_path":"b.swift","new_path":"b.swift","old_line":9,"new_line":null,"position_type":"text"}},
       {"id":42,"type":"DiffNote","body":"Done","author":{"username":"max"},"created_at":"2026-10-02T11:00:00Z","system":false,"resolvable":true,"resolved":false}]},
     {"id":"d5","individual_note":false,"notes":[
       {"id":51,"type":"DiscussionNote","body":"General question","author":{"username":"lea"},"created_at":"2026-10-02T12:00:00Z","system":false,"resolvable":true,"resolved":false}]}
    ]
    """

    private func stubMergeRequest(detail: String = GitLabReviewTests.detail) {
        StubProtocol.routes = [
            "/api/v4/user": (200, #"{"id":2,"username":"sofia","name":"Sofia"}"#),
            "/merge_requests/7": (200, detail),
            "/merge_requests/7/approvals": (200, #"{"approved":false,"approvals_required":2,"approvals_left":1,"approved_by":[{"user":{"id":3,"username":"lea","avatar_url":"https://a/lea.png"}}],"user_has_approved":false,"user_can_approve":true}"#),
            "/merge_requests/7/discussions": (200, Self.discussions),
            "/merge_requests/7/commits": (200, #"[{"id":"c2","short_id":"c2","title":"Second","author_name":"Max","committed_date":"2026-10-02T08:00:00Z"},{"id":"c1","short_id":"c1","title":"First","author_name":"Max","committed_date":"2026-10-01T08:00:00Z"}]"#),
            "/pipelines/55/jobs": (200, #"[{"id":1,"name":"build","stage":"build","status":"success","started_at":"2026-10-02T08:00:00Z","finished_at":"2026-10-02T08:01:30Z"},{"id":2,"name":"lint","stage":"test","status":"failed","allow_failure":true},{"id":3,"name":"unit","stage":"test","status":"failed"},{"id":4,"name":"deploy","stage":"deploy","status":"manual"},{"id":5,"name":"e2e","stage":"test","status":"running"}]"#),
            "/projects/group/sub/app": (200, #"{"id":42,"path_with_namespace":"group/sub/app","merge_method":"merge","squash_option":"default_on"}"#),
            "/merge_requests/7/notes": (201, "{}"),
            "/merge_requests/7/discussions/d4": (200, "{}"),
            "/merge_requests/7/discussions/d4/notes": (201, "{}"),
            "/merge_requests/7/approve": (201, "{}"),
            "/merge_requests/7/unapprove": (201, "{}"),
            "/merge_requests/7/merge": (200, "{}"),
            "/merge_requests/7/cancel_merge_when_pipeline_succeeds": (201, "{}"),
            "/merge_requests/7/rebase": (202, "{}")
        ]
    }

    func testLoadMapsMergeRequest() async throws {
        stubMergeRequest()
        let load = try await backend().load(ref)
        let repo = try XCTUnwrap(load.repository)
        let pr = try XCTUnwrap(repo.pullRequest)

        XCTAssertEqual(load.viewer, "sofia")
        XCTAssertTrue(repo.canWrite)
        XCTAssertTrue(repo.squashMergeAllowed)
        XCTAssertFalse(repo.rebaseMergeAllowed)
        XCTAssertEqual(repo.viewerDefaultMergeMethod, "SQUASH")
        XCTAssertEqual(pr.number, 7)
        XCTAssertEqual(pr.state, "OPEN")
        XCTAssertEqual(pr.headRefName, "feature/login")
        XCTAssertEqual(pr.baseRefName, "main")
        XCTAssertEqual(pr.headRefOid, "head2")
        XCTAssertEqual(pr.changedFiles, 3)
        XCTAssertEqual(pr.mergeStateStatus, "BLOCKED")
        XCTAssertEqual(pr.mergeBlocker, "All threads must be resolved")
        XCTAssertEqual(pr.reviewDecision, "REVIEW_REQUIRED")
        XCTAssertFalse(pr.viewerDidAuthor)
        XCTAssertEqual(pr.viewerCanApprove, true)
        XCTAssertEqual(pr.viewerApproved, false)
        XCTAssertEqual(pr.labels.nodes.map(\.name), ["backend"])
        XCTAssertEqual(pr.latestOpinionatedReviews.nodes.map { $0.author?.login }, ["lea"])
        XCTAssertEqual(pr.reviewRequests.nodes.compactMap { $0.requestedReviewer?.login }, ["sofia"])
        XCTAssertTrue(pr.bodyHTML.contains(#"src="https://gitlab.example.com/group/sub/app/uploads/a.png""#), pr.bodyHTML)
        XCTAssertTrue(pr.bodyHTML.contains(#"href="https://gitlab.example.com/group/sub/app/-/issues/3""#), pr.bodyHTML)
        XCTAssertFalse(pr.bodyHTML.contains("data:image"), pr.bodyHTML)

        XCTAssertEqual(pr.allCommits.nodes.map(\.commit.oid), ["c1", "c2"])

        let checks = Dictionary(uniqueKeysWithValues: pr.checks.map { ($0.title, $0.outcome) })
        XCTAssertEqual(checks["build"], .success)
        XCTAssertEqual(checks["lint"], .neutral)
        XCTAssertEqual(checks["unit"], .failure)
        XCTAssertEqual(checks["deploy"], .skipped)
        XCTAssertEqual(checks["e2e"], .pending)
        XCTAssertEqual(pr.checks.first?.workflowName, "build")
        XCTAssertEqual(pr.checks.first?.duration, "1m 30s")
        XCTAssertEqual(pr.rollup?.state, "FAILURE")

        let threads = Dictionary(uniqueKeysWithValues: pr.reviewThreads.nodes.map { ($0.id, $0) })
        XCTAssertEqual(threads.count, 3)
        let current = try XCTUnwrap(threads["d4"])
        XCTAssertEqual(current.path, "b.swift")
        XCTAssertEqual(current.line, 9)
        XCTAssertEqual(current.diffSide, "LEFT")
        XCTAssertFalse(current.isOutdated)
        XCTAssertFalse(current.isResolved)
        XCTAssertTrue(current.viewerCanResolve)
        XCTAssertEqual(current.comments.nodes.map(\.body), ["Rename this", "Done"])
        let old = try XCTUnwrap(threads["d3"])
        XCTAssertTrue(old.isOutdated)
        XCTAssertTrue(old.isResolved)
        XCTAssertNil(old.line)
        XCTAssertEqual(old.originalLine, 4)
        XCTAssertTrue(old.viewerCanUnresolve)
        let general = try XCTUnwrap(threads["d5"])
        XCTAssertEqual(general.path, "")
        XCTAssertNil(general.line)

        XCTAssertEqual(pr.timelineItems.nodes.map(\.typename), ["SystemNote", "IssueComment", "PullRequestReview", "PullRequestReview", "PullRequestReview"])
        let comment = try XCTUnwrap(pr.timelineItems.nodes.first { $0.typename == "IssueComment" })
        XCTAssertEqual(comment.bodyHTML, "<p>Looks <strong>good</strong></p>")

        let detailRequest = try XCTUnwrap(requests("GET", "/merge_requests/7").first)
        XCTAssertTrue(detailRequest.url?.absoluteString.contains("/api/v4/projects/group%2Fsub%2Fapp/merge_requests/7?render_html=true") == true, detailRequest.url?.absoluteString ?? "")

        let html = ConversationHTML.render(pr: pr)
        XCTAssertTrue(html.contains("started a thread"))
        XCTAssertTrue(html.contains("approved this merge request"))
        XCTAssertFalse(html.contains("post('file','')"))
    }

    func testPulseTracksApprovals() async throws {
        stubMergeRequest()
        let b = backend()
        let load = try await b.load(ref)
        let same = try await b.pulse(ref)
        XCTAssertEqual(load.pulse, same)
        StubProtocol.routes["/merge_requests/7/approvals"] = (200, #"{"approved_by":[{"user":{"username":"lea"}},{"user":{"username":"sofia"}}],"user_has_approved":true}"#)
        let changed = try await b.pulse(ref)
        XCTAssertNotEqual(load.pulse, changed)
        XCTAssertTrue(requests("GET", "/merge_requests/7").last?.url?.query?.contains("render_html") != true)
    }

    func testFilesMapDiffsAndFallBackToChanges() async throws {
        stubMergeRequest()
        let b = backend()
        let loaded = try await b.load(ref)
        let pr = try XCTUnwrap(loaded.repository?.pullRequest)
        StubProtocol.routes["/merge_requests/7/diffs"] = (200, #"""
        [{"old_path":"a.swift","new_path":"a.swift","diff":"@@ -1,2 +1,3 @@\n line\n-old\n+new\n+more\n","new_file":false,"renamed_file":false,"deleted_file":false},
         {"old_path":"old.swift","new_path":"new.swift","diff":"","new_file":false,"renamed_file":true,"deleted_file":false},
         {"old_path":"big.json","new_path":"big.json","diff":"@@ -1 +1 @@\n-a\n+b\n","new_file":true,"renamed_file":false,"deleted_file":false,"too_large":true}]
        """#)
        let files = try await b.files(ref, pr: pr)
        XCTAssertEqual(files.map(\.filename), ["a.swift", "new.swift", "big.json"])
        XCTAssertEqual(files[0].status, "modified")
        XCTAssertEqual(files[0].additions, 2)
        XCTAssertEqual(files[0].deletions, 1)
        XCTAssertEqual(DiffFile(files[0]).lines.count, 5)
        XCTAssertEqual(files[1].status, "renamed")
        XCTAssertEqual(files[1].previousFilename, "old.swift")
        XCTAssertNil(files[1].patch)
        XCTAssertEqual(files[2].status, "added")
        XCTAssertNil(files[2].patch)

        StubProtocol.routes["/merge_requests/7/diffs"] = nil
        StubProtocol.routes["/merge_requests/7/changes"] = (200, #"{"changes":[{"old_path":"x","new_path":"x","diff":"@@ -0,0 +1 @@\n+x\n","new_file":true,"renamed_file":false,"deleted_file":true}]}"#)
        let legacy = try await b.files(ref, pr: pr)
        XCTAssertEqual(legacy.map(\.status), ["added"])
    }

    func testSubmitReviewPostsPositionedThreadsThenApproves() async throws {
        stubMergeRequest()
        let b = backend()
        let loaded = try await b.load(ref)
        let pr = try XCTUnwrap(loaded.repository?.pullRequest)
        let file = DiffFile(PRFile(filename: "a.swift", status: "renamed", additions: 1, deletions: 1, patch: "@@ -10,3 +12,3 @@\n keep\n-old\n+new\n", previousFilename: "z.swift"))
        let context = PendingComment(path: "a.swift", line: 12, side: "RIGHT", body: "On context")
        let removed = PendingComment(path: "a.swift", line: 11, side: "LEFT", body: "On removed")
        StubProtocol.requests = []
        try await b.submitReview(ref, pr: pr, event: "APPROVE", body: " Summary ", comments: [context, removed], files: [file])

        let threads = requests("POST", "/merge_requests/7/discussions").map(json)
        XCTAssertEqual(threads.count, 2)
        let first = try XCTUnwrap(threads.first?["position"] as? [String: Any])
        XCTAssertEqual(threads.first?["body"] as? String, "On context")
        XCTAssertEqual(first["position_type"] as? String, "text")
        XCTAssertEqual(first["base_sha"] as? String, "base1")
        XCTAssertEqual(first["start_sha"] as? String, "start1")
        XCTAssertEqual(first["head_sha"] as? String, "head2")
        XCTAssertEqual(first["new_path"] as? String, "a.swift")
        XCTAssertEqual(first["old_path"] as? String, "z.swift")
        XCTAssertEqual(first["new_line"] as? Int, 12)
        XCTAssertEqual(first["old_line"] as? Int, 10)
        let second = try XCTUnwrap(threads.last?["position"] as? [String: Any])
        XCTAssertEqual(second["old_line"] as? Int, 11)
        XCTAssertNil(second["new_line"])

        XCTAssertEqual(json(requests("POST", "/merge_requests/7/notes").first)["body"] as? String, "Summary")
        XCTAssertEqual(json(requests("POST", "/merge_requests/7/approve").first)["sha"] as? String, "head2")
        let order = StubProtocol.requests.map { $0.url?.lastPathComponent ?? "" }
        XCTAssertEqual(order, ["discussions", "discussions", "notes", "approve"])
    }

    func testThreadActionsMergeAndStateChanges() async throws {
        stubMergeRequest()
        let b = backend()
        let loaded = try await b.load(ref)
        let pr = try XCTUnwrap(loaded.repository?.pullRequest)
        StubProtocol.requests = []

        try await b.reply(ref, thread: "d4", body: "Fixed")
        XCTAssertEqual(json(requests("POST", "/merge_requests/7/discussions/d4/notes").first)["body"] as? String, "Fixed")

        try await b.setResolved(ref, thread: "d4", resolved: true)
        XCTAssertEqual(json(requests("PUT", "/merge_requests/7/discussions/d4").first)["resolved"] as? Bool, true)

        try await b.comment(ref, pr: pr, body: "Hello")
        XCTAssertEqual(json(requests("POST", "/merge_requests/7/notes").first)["body"] as? String, "Hello")

        try await b.revokeApproval(ref, pr: pr)
        XCTAssertEqual(requests("POST", "/merge_requests/7/unapprove").count, 1)

        try await b.merge(ref, pr: pr, method: "SQUASH", title: "Add login", body: "Details", deleteBranch: true)
        let merge = json(requests("PUT", "/merge_requests/7/merge").first)
        XCTAssertEqual(merge["squash"] as? Bool, true)
        XCTAssertEqual(merge["should_remove_source_branch"] as? Bool, true)
        XCTAssertEqual(merge["sha"] as? String, "head2")
        XCTAssertEqual(merge["squash_commit_message"] as? String, "Add login\n\nDetails")
        XCTAssertNil(merge["merge_commit_message"])
        XCTAssertNil(merge["auto_merge"])

        try await b.enableAutoMerge(ref, pr: pr, method: "MERGE")
        let auto = json(requests("PUT", "/merge_requests/7/merge").last)
        XCTAssertEqual(auto["auto_merge"] as? Bool, true)
        XCTAssertEqual(auto["merge_when_pipeline_succeeds"] as? Bool, true)
        XCTAssertEqual(auto["squash"] as? Bool, false)

        try await b.perform(.disableAutoMerge, ref: ref, pr: pr)
        XCTAssertEqual(requests("POST", "/merge_requests/7/cancel_merge_when_pipeline_succeeds").count, 1)
        try await b.perform(.updateBranch, ref: ref, pr: pr)
        XCTAssertEqual(requests("PUT", "/merge_requests/7/rebase").count, 1)
        try await b.perform(.close, ref: ref, pr: pr)
        XCTAssertEqual(json(requests("PUT", "/merge_requests/7").last)["state_event"] as? String, "close")
        try await b.perform(.convertToDraft, ref: ref, pr: pr)
        XCTAssertEqual(json(requests("PUT", "/merge_requests/7").last)["title"] as? String, "Draft: Add login")
        XCTAssertEqual(GitLabPRBackend.undrafted("Draft: Add login"), "Add login")
        XCTAssertEqual(GitLabPRBackend.undrafted("[WIP] Add login"), "Add login")
    }

    func testMergeErrorsAreReadable() async throws {
        stubMergeRequest()
        let b = backend()
        let loaded = try await b.load(ref)
        let pr = try XCTUnwrap(loaded.repository?.pullRequest)
        StubProtocol.routes["/merge_requests/7/merge"] = (405, #"{"message":"405 Method Not Allowed"}"#)
        StubProtocol.routes["/merge_requests/7/discussions/d4"] = (400, #"{"message":{"base":["Discussion is not resolvable"]}}"#)
        do {
            try await b.merge(ref, pr: pr, method: "MERGE", title: nil, body: nil, deleteBranch: false)
            XCTFail("expected error")
        } catch {
            XCTAssertEqual(error.localizedDescription, "GitLab 405: 405 Method Not Allowed")
        }
        do {
            try await b.setResolved(ref, thread: "d4", resolved: true)
            XCTFail("expected error")
        } catch {
            XCTAssertEqual(error.localizedDescription, "GitLab 400: Discussion is not resolvable")
        }
    }

    func testCommitDetail() async throws {
        StubProtocol.routes = [
            "/repository/commits/abc": (200, #"{"id":"abc","short_id":"abc","title":"Fix","message":"Fix\n\nBody","author_name":"Max","authored_date":"2026-10-01T08:00:00Z","web_url":"https://gitlab.example.com/g/p/-/commit/abc","stats":{"additions":1,"deletions":0}}"#),
            "/repository/commits/abc/diff": (200, #"[{"old_path":"a","new_path":"a","diff":"@@ -0,0 +1 @@\n+x\n","new_file":true,"renamed_file":false,"deleted_file":false}]"#)
        ]
        let account = GitAccount(id: "test", provider: .gitlab, host: "gitlab.example.com", login: "sofia", email: nil, userID: nil, keychainKey: "test")
        let c = try await GitLabClient(account: account, credential: GitCredential(token: "t")).commit("g/p", sha: "abc")
        XCTAssertEqual(c.commit.message, "Fix\n\nBody")
        XCTAssertEqual(c.commit.author?.name, "Max")
        XCTAssertEqual(c.stats?.additions, 1)
        XCTAssertEqual(c.files?.first?.status, "added")
        XCTAssertEqual(c.htmlUrl, "https://gitlab.example.com/g/p/-/commit/abc")
    }

    func testMergeStatusMapping() {
        func status(_ detailed: String?, legacy: String? = nil, pipeline: Bool = true) -> (String, String, String?) {
            let json = """
            {"id":1,"iid":1,"title":"t","state":"opened","web_url":"https://g/x/-/merge_requests/1","created_at":"2026-10-01T10:00:00Z","updated_at":"2026-10-01T10:00:00Z",
             "source_branch":"a","target_branch":"b"\(detailed.map { #","detailed_merge_status":"\#($0)""# } ?? "")\(legacy.map { #","merge_status":"\#($0)""# } ?? "")\(pipeline ? #","head_pipeline":{"id":1,"status":"running"}"# : "")}
            """
            let decoder = JSONDecoder()
            decoder.keyDecodingStrategy = .convertFromSnakeCase
            decoder.dateDecodingStrategy = .iso8601
            let mr = try! decoder.decode(GitLabMR.Detail.self, from: Data(json.utf8))
            let s = GitLabReviewMapper.mergeStatus(mr)
            return (s.mergeable, s.status, s.blocker)
        }
        XCTAssertEqual(status("mergeable").1, "CLEAN")
        XCTAssertEqual(status("conflict").0, "CONFLICTING")
        XCTAssertEqual(status("need_rebase").1, "BEHIND")
        XCTAssertEqual(status("checking").1, "UNKNOWN")
        XCTAssertEqual(status("ci_still_running").2, "Pipeline is still running")
        XCTAssertEqual(status("ci_must_pass", pipeline: false).2, "A pipeline must run and succeed")
        XCTAssertNil(status("not_approved").2)
        XCTAssertEqual(status("some_new_check").2, "Some New Check")
        XCTAssertEqual(status(nil, legacy: "can_be_merged").1, "CLEAN")
        XCTAssertEqual(status(nil, legacy: "cannot_be_merged").1, "DIRTY")
    }

    func testMarkdownHTML() {
        let html = MarkdownHTML.render("# Title\n\nSee [docs](/help) and `code` ![shot](/uploads/x.png)\n\n- [x] done\n- todo\n\n```swift\nlet a = 1 < 2\n```", base: "https://gitlab.example.com/g/p")
        XCTAssertTrue(html.contains("<h1>Title</h1>"), html)
        XCTAssertTrue(html.contains(#"<a href="https://gitlab.example.com/help">docs</a>"#), html)
        XCTAssertTrue(html.contains("<code>code</code>"), html)
        XCTAssertTrue(html.contains(#"<img src="https://gitlab.example.com/g/p/uploads/x.png" alt="shot">"#), html)
        XCTAssertTrue(html.contains(#"<li><input type="checkbox" checked disabled> done</li>"#), html)
        XCTAssertTrue(html.contains("let a = 1 &lt; 2"), html)
        XCTAssertEqual(MarkdownHTML.inline("<script>"), "&lt;script&gt;")
    }
}
