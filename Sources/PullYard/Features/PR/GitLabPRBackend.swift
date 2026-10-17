import Foundation

struct GitLabMRSnapshot {
    let project: String
    let mr: GitLabMR.Detail
    let approvals: GitLabMR.Approvals?
    let discussions: [GitLabMR.Discussion]
    let commits: [GitLabMR.Commit]
    let jobs: [GitLabMR.Job]
    let settings: GitLabMR.Project?
    let viewer: String
}

enum GitLabReviewMapper {
    static func repository(_ s: GitLabMRSnapshot) -> PRRepository {
        let squash = s.settings?.squashOption ?? "default_off"
        let prefersSquash = squash == "always" || squash == "default_on" || s.mr.squash == true || s.mr.squashOnMerge == true
        return PRRepository(
            viewerPermission: s.mr.user?.canMerge == true ? "WRITE" : "READ",
            mergeCommitAllowed: squash != "always",
            squashMergeAllowed: squash != "never",
            rebaseMergeAllowed: false,
            deleteBranchOnMerge: false,
            autoMergeAllowed: s.mr.headPipeline != nil,
            viewerDefaultMergeMethod: prefersSquash && squash != "never" ? "SQUASH" : "MERGE",
            pullRequest: detail(s)
        )
    }

    static func state(_ raw: String) -> String {
        switch raw {
        case "merged": return "MERGED"
        case "opened", "locked": return "OPEN"
        default: return "CLOSED"
        }
    }

    static func mergeStatus(_ mr: GitLabMR.Detail) -> (mergeable: String, status: String, blocker: String?) {
        let blocked = { (text: String?) in ("MERGEABLE", "BLOCKED", text) }
        switch mr.detailedMergeStatus ?? legacyStatus(mr.mergeStatus) {
        case "mergeable": return ("MERGEABLE", "CLEAN", nil)
        case "conflict", "broken_status": return ("CONFLICTING", "DIRTY", nil)
        case "need_rebase": return ("MERGEABLE", "BEHIND", nil)
        case "checking", "unchecked", "preparing", "approvals_syncing": return ("UNKNOWN", "UNKNOWN", nil)
        case "draft_status": return ("MERGEABLE", "DRAFT", nil)
        case "not_open": return ("UNKNOWN", "UNKNOWN", nil)
        case "not_approved", "requested_changes": return blocked(nil)
        case "ci_must_pass": return blocked(mr.headPipeline == nil ? "A pipeline must run and succeed" : "Pipeline must succeed")
        case "ci_still_running": return blocked("Pipeline is still running")
        case "discussions_not_resolved": return blocked("All threads must be resolved")
        case "blocked_status", "merge_request_blocked": return blocked("Blocked by another merge request")
        case "external_status_checks": return blocked("External status checks must pass")
        case "jira_association_missing": return blocked("Title or description must reference a Jira issue")
        case "security_policy_violations", "security_policy_pipeline_check": return blocked("Blocked by a security policy")
        case "locked_paths", "locked_lfs_files": return blocked("Changed files are locked")
        case "title_regex": return blocked("Title doesn't match the project's required pattern")
        case "commits_status": return blocked("Commits don't pass the project's push rules")
        case "merge_time": return blocked("Can't be merged before the scheduled time")
        case let other: return blocked(other.replacingOccurrences(of: "_", with: " ").capitalized)
        }
    }

    private static func legacyStatus(_ status: String?) -> String {
        switch status {
        case "can_be_merged": return "mergeable"
        case "cannot_be_merged": return "conflict"
        default: return "checking"
        }
    }

    static func reviewDecision(_ mr: GitLabMR.Detail, approvals: GitLabMR.Approvals?) -> String? {
        if mr.detailedMergeStatus == "requested_changes" { return "CHANGES_REQUESTED" }
        guard let approvals else { return nil }
        if (approvals.approvalsLeft ?? 0) > 0 { return "REVIEW_REQUIRED" }
        if !(approvals.approvedBy ?? []).isEmpty { return "APPROVED" }
        return nil
    }

    static func pulse(_ mr: GitLabMR.Detail, approvals: GitLabMR.Approvals?) -> String {
        let approvers = (approvals?.approvedBy ?? []).map(\.user.username).sorted().joined(separator: ",")
        return [
            String(mr.updatedAt.timeIntervalSince1970),
            mr.sha ?? "",
            mr.state,
            mr.headPipeline.map { "\($0.id):\($0.status)" } ?? "",
            mr.detailedMergeStatus ?? mr.mergeStatus ?? "",
            String(mr.userNotesCount ?? 0),
            String(mr.blockingDiscussionsResolved ?? true),
            approvers
        ].joined(separator: "|")
    }

    static func checks(_ jobs: [GitLabMR.Job]) -> [CheckContext] {
        jobs.map { job in
            let status: String
            var conclusion: String?
            var note: String?
            switch job.status {
            case "running": status = "IN_PROGRESS"
            case "created", "pending", "waiting_for_resource", "preparing", "scheduled", "waiting_for_callback": status = "QUEUED"
            default: status = "COMPLETED"
            }
            switch job.status {
            case "success": conclusion = "SUCCESS"
            case "failed":
                conclusion = job.allowFailure == true ? "NEUTRAL" : "FAILURE"
                if job.allowFailure == true { note = "Allowed to fail" }
            case "canceled", "canceling": conclusion = "CANCELLED"
            case "skipped": conclusion = "SKIPPED"
            case "manual":
                conclusion = "SKIPPED"
                note = "Manual"
            default: conclusion = nil
            }
            return CheckContext(
                typename: "GitLabJob",
                databaseId: job.id,
                name: job.name,
                status: status,
                conclusion: conclusion,
                detailsUrl: job.webUrl,
                startedAt: job.startedAt,
                completedAt: job.finishedAt,
                checkSuite: job.stage.map { CheckSuite(workflowRun: WorkflowRun(databaseId: nil, workflow: WorkflowInfo(name: $0))) },
                context: nil,
                state: nil,
                targetUrl: nil,
                description: note
            )
        }
    }

    static func rollupState(_ pipeline: GitLabMR.Pipeline?) -> String? {
        switch pipeline?.status {
        case nil: return nil
        case "success": return "SUCCESS"
        case "failed": return "FAILURE"
        case "canceled", "skipped": return "ERROR"
        default: return "PENDING"
        }
    }

    static func detail(_ s: GitLabMRSnapshot) -> PullRequestDetail {
        let mr = s.mr
        let base = projectURL(mr.webUrl)
        let merge = mergeStatus(mr)
        let me = s.viewer.lowercased()
        let authorLogin = mr.author?.username
        let isAuthor = authorLogin?.lowercased() == me && !me.isEmpty
        let canMerge = mr.user?.canMerge == true
        let headSha = mr.diffRefs?.headSha ?? mr.sha
        let (timeline, threads) = conversation(s.discussions, headSha: headSha, base: base, viewer: me, moderator: canMerge || isAuthor)
        let commits = s.commits.reversed().map { c in
            CommitInfoNode(commit: CommitInfo(
                oid: c.id,
                abbreviatedOid: c.shortId,
                messageHeadline: c.title,
                committedDate: c.committedDate ?? c.authoredDate ?? mr.createdAt,
                url: c.webUrl ?? "\(base)/-/commit/\(c.id)",
                author: GitActor(name: c.authorName, avatarUrl: nil, user: nil),
                statusCheckRollup: nil
            ))
        }
        let contexts = checks(s.jobs)
        let rollup = rollupState(mr.headPipeline).map { StatusRollup(state: $0, contexts: Nodes(nodes: contexts, totalCount: contexts.count)) }
        let approvers = (s.approvals?.approvedBy ?? []).map(\.user)
        let approved = Set(approvers.map(\.username))
        let pendingReviewers = (mr.reviewers ?? []).filter { !approved.contains($0.username) }
        let changed = mr.changesCount.flatMap { Int($0.prefix { $0.isNumber }) } ?? 0
        let samePlace = mr.sourceProjectId == nil || mr.sourceProjectId == mr.targetProjectId
        let state = state(mr.state)
        return PullRequestDetail(
            id: "gitlab-mr-\(mr.id)",
            number: mr.iid,
            title: mr.title,
            url: mr.webUrl,
            state: state,
            isDraft: mr.isDraft,
            merged: state == "MERGED",
            mergeable: merge.mergeable,
            mergeStateStatus: merge.status,
            body: mr.description ?? "",
            bodyHTML: mr.descriptionHtml.map { MarkdownHTML.absolutizeHTML($0, base: base) } ?? MarkdownHTML.render(mr.description ?? "", base: base),
            createdAt: mr.createdAt,
            author: mr.author?.actor,
            headRefName: mr.sourceBranch,
            baseRefName: mr.targetBranch,
            headRefOid: headSha ?? "",
            baseRef: mr.diffRefs?.baseSha.map { BaseRef(target: BaseRef.Target(oid: $0)) },
            headRepository: samePlace ? RepoName(nameWithOwner: s.project, defaultBranchRef: nil) : nil,
            headRef: state == "OPEN" ? HeadRef(name: mr.sourceBranch) : nil,
            additions: 0,
            deletions: 0,
            changedFiles: changed,
            reviewDecision: reviewDecision(mr, approvals: s.approvals),
            viewerCanUpdate: canMerge || isAuthor,
            viewerDidAuthor: isAuthor,
            autoMergeRequest: mr.mergeWhenPipelineSucceeds == true ? AutoMergeRequest(enabledAt: nil, mergeMethod: nil) : nil,
            labels: Nodes(nodes: (mr.labels ?? []).map(\.info)),
            assignees: Nodes(nodes: (mr.assignees ?? []).map(\.actor)),
            reviewRequests: Nodes(nodes: pendingReviewers.map { ReviewRequest(requestedReviewer: Reviewer(typename: "User", login: $0.username, avatarUrl: $0.avatarUrl, name: $0.name)) }),
            latestOpinionatedReviews: Nodes(nodes: approvers.map { LatestReview(state: "APPROVED", submittedAt: nil, commit: nil, author: $0.actor) }),
            lastCommit: Nodes(nodes: [LastCommitNode(commit: LastCommit(oid: headSha ?? "", committedDate: commits.last?.commit.committedDate, statusCheckRollup: rollup))]),
            allCommits: Nodes(nodes: commits, totalCount: commits.count),
            files: nil,
            timelineItems: Nodes(nodes: timeline),
            reviewThreads: Nodes(nodes: threads),
            viewerCanApprove: s.approvals?.userCanApprove,
            viewerApproved: s.approvals?.userHasApproved,
            mergeBlocker: merge.blocker
        )
    }

    static func projectURL(_ webUrl: String) -> String {
        webUrl.components(separatedBy: "/-/").first ?? webUrl
    }

    static func conversation(_ discussions: [GitLabMR.Discussion], headSha: String?, base: String, viewer: String, moderator: Bool) -> ([TimelineItem], [ReviewThread]) {
        var timeline: [TimelineItem] = []
        var threads: [ReviewThread] = []
        let ordered = discussions.sorted { ($0.notes.first?.createdAt ?? .distantPast) < ($1.notes.first?.createdAt ?? .distantPast) }
        for d in ordered {
            guard let first = d.notes.first else { continue }
            if first.system {
                for n in d.notes {
                    timeline.append(item("SystemNote", id: "gl-n-\(n.id)", author: nil, actor: n.author?.actor, body: n.body, html: nil, date: n.createdAt))
                }
                continue
            }
            let notes = d.notes.filter { !$0.system }
            if d.individualNote, first.position == nil {
                timeline.append(item("IssueComment", id: "gl-n-\(first.id)", author: first.author?.actor, actor: nil, body: first.body, html: MarkdownHTML.render(first.body, base: base), date: first.createdAt))
                continue
            }
            let review = "gl-d-\(d.id)"
            let resolvable = notes.contains { $0.resolvable == true }
            let resolved = resolvable && notes.filter { $0.resolvable == true }.allSatisfy { $0.resolved == true }
            let starter = first.author?.username.lowercased() == viewer
            let canResolve = resolvable && (moderator || starter)
            let position = first.position
            let outdated = position != nil && headSha != nil && position?.headSha != nil && position?.headSha != headSha
            let line = position?.newLine ?? position?.oldLine
            threads.append(ReviewThread(
                id: d.id,
                isResolved: resolved,
                isOutdated: outdated,
                path: position.map { $0.newPath ?? $0.oldPath ?? "" } ?? "",
                line: outdated ? nil : line,
                originalLine: line,
                diffSide: position?.newLine == nil && position?.oldLine != nil ? "LEFT" : "RIGHT",
                viewerCanResolve: canResolve && !resolved,
                viewerCanUnresolve: canResolve && resolved,
                viewerCanReply: true,
                comments: Nodes(nodes: notes.map { n in
                    ThreadComment(id: String(n.id), author: n.author?.actor, body: n.body, bodyHTML: MarkdownHTML.render(n.body, base: base), createdAt: n.createdAt, diffHunk: nil, pullRequestReview: IDOnly(id: review))
                })
            ))
            timeline.append(item("PullRequestReview", id: review, author: first.author?.actor, actor: nil, body: nil, html: nil, date: first.createdAt, state: "DISCUSSION"))
        }
        return (timeline, threads)
    }

    private static func item(_ type: String, id: String, author: Actor?, actor: Actor?, body: String?, html: String?, date: Date, state: String? = nil) -> TimelineItem {
        TimelineItem(typename: type, id: id, author: author, actor: actor, body: body, bodyHTML: html, createdAt: date, url: nil, state: state, label: nil, source: nil, previousTitle: nil, currentTitle: nil, commit: nil, mergeRefName: nil, requestedReviewer: nil)
    }

    static func file(_ d: GitLabMR.Diff) -> PRFile {
        let patch = d.diff.flatMap { $0.isEmpty || d.tooLarge == true ? nil : $0 }
        var additions = 0
        var deletions = 0
        patch?.enumerateLines { line, _ in
            if line.hasPrefix("+") { additions += 1 } else if line.hasPrefix("-") { deletions += 1 }
        }
        let status = d.newFile ? "added" : d.deletedFile ? "removed" : d.renamedFile ? "renamed" : "modified"
        return PRFile(filename: d.newPath, status: status, additions: additions, deletions: deletions, patch: patch, previousFilename: d.renamedFile ? d.oldPath : nil)
    }

    static func position(_ comment: PendingComment, files: [DiffFile], refs: GitLabMR.DiffRefs?) -> [String: Any]? {
        guard let refs, let baseSha = refs.baseSha, let headSha = refs.headSha else { return nil }
        let file = files.first { $0.id == comment.path }
        var p: [String: Any] = [
            "position_type": "text",
            "base_sha": baseSha,
            "start_sha": refs.startSha ?? baseSha,
            "head_sha": headSha,
            "new_path": comment.path,
            "old_path": file?.previousFilename ?? comment.path
        ]
        let left = comment.side == "LEFT"
        let line = file?.lines.first { l in
            left ? (l.oldLine == comment.line && l.kind != .add) : (l.newLine == comment.line && l.kind != .del)
        }
        if left {
            p["old_line"] = comment.line
        } else {
            p["new_line"] = comment.line
        }
        if line?.kind == .context {
            if let o = line?.oldLine { p["old_line"] = o }
            if let n = line?.newLine { p["new_line"] = n }
        }
        return p
    }
}

@MainActor
final class GitLabPRBackend: PRBackend {
    let client: GitLabClient
    let features = PRFeatures.gitlab
    private var settings: [String: GitLabMR.Project] = [:]
    private var refs: [ItemRef: GitLabMR.DiffRefs] = [:]

    init(client: GitLabClient) {
        self.client = client
    }

    private func projectSettings(_ project: String) async -> GitLabMR.Project? {
        if let hit = settings[project] { return hit }
        let fresh = try? await client.project(project)
        settings[project] = fresh
        return fresh
    }

    func load(_ ref: ItemRef) async throws -> PRLoad {
        let project = ref.repo.full
        async let mrTask = client.mergeRequest(project, iid: ref.number)
        async let approvalsTask = try? client.approvals(project, iid: ref.number)
        async let discussionsTask = client.discussions(project, iid: ref.number)
        async let commitsTask = client.commits(project, iid: ref.number)
        async let settingsTask = projectSettings(project)
        async let viewerTask = try? client.username()
        let mr = try await mrTask
        var jobs: [GitLabMR.Job] = []
        if let pipeline = mr.headPipeline {
            let owner = pipeline.projectId.map(String.init) ?? project
            jobs = (try? await client.jobs(owner, pipeline: pipeline.id)) ?? []
        }
        let approvals = await approvalsTask
        let snapshot = GitLabMRSnapshot(
            project: project,
            mr: mr,
            approvals: approvals,
            discussions: try await discussionsTask,
            commits: try await commitsTask,
            jobs: jobs,
            settings: await settingsTask,
            viewer: await viewerTask ?? ""
        )
        refs[ref] = mr.diffRefs
        return PRLoad(viewer: snapshot.viewer, repository: GitLabReviewMapper.repository(snapshot), pulse: .fingerprint(GitLabReviewMapper.pulse(mr, approvals: approvals)))
    }

    func pulse(_ ref: ItemRef) async throws -> PRPulseToken? {
        async let mr = client.mergeRequest(ref.repo.full, iid: ref.number, html: false)
        async let approvals = try? client.approvals(ref.repo.full, iid: ref.number)
        return .fingerprint(GitLabReviewMapper.pulse(try await mr, approvals: await approvals))
    }

    func files(_ ref: ItemRef, pr: PullRequestDetail) async throws -> [PRFile] {
        try await client.diffs(ref.repo.full, iid: ref.number).map(GitLabReviewMapper.file)
    }

    func comment(_ ref: ItemRef, pr: PullRequestDetail, body: String) async throws {
        try await client.addNote(ref.repo.full, iid: ref.number, body: body)
    }

    func submitReview(_ ref: ItemRef, pr: PullRequestDetail, event: String, body: String, comments: [PendingComment], files: [DiffFile]) async throws {
        let project = ref.repo.full
        let diffRefs = refs[ref]
        for c in comments {
            guard let position = GitLabReviewMapper.position(c, files: files, refs: diffRefs) else {
                throw APIError(message: "Reload the merge request and try again: its diff version is unknown")
            }
            try await client.addDiscussion(project, iid: ref.number, body: c.body, position: position)
        }
        let text = body.trimmingCharacters(in: .whitespacesAndNewlines)
        if !text.isEmpty {
            try await client.addNote(project, iid: ref.number, body: text)
        }
        if event == "APPROVE" {
            try await client.approve(project, iid: ref.number, sha: pr.headRefOid.isEmpty ? nil : pr.headRefOid)
        }
    }

    func reply(_ ref: ItemRef, thread: String, body: String) async throws {
        try await client.reply(ref.repo.full, iid: ref.number, discussion: thread, body: body)
    }

    func setResolved(_ ref: ItemRef, thread: String, resolved: Bool) async throws {
        try await client.resolve(ref.repo.full, iid: ref.number, discussion: thread, resolved: resolved)
    }

    func merge(_ ref: ItemRef, pr: PullRequestDetail, method: String, title: String?, body: String?, deleteBranch: Bool) async throws {
        try await client.merge(ref.repo.full, iid: ref.number, squash: method == "SQUASH", removeBranch: deleteBranch, message: Self.message(title: title, body: body, pr: pr), sha: pr.headRefOid.isEmpty ? nil : pr.headRefOid, auto: false)
    }

    func enableAutoMerge(_ ref: ItemRef, pr: PullRequestDetail, method: String) async throws {
        try await client.merge(ref.repo.full, iid: ref.number, squash: method == "SQUASH", removeBranch: false, message: nil, sha: pr.headRefOid.isEmpty ? nil : pr.headRefOid, auto: true)
    }

    private static func message(title: String?, body: String?, pr: PullRequestDetail) -> String? {
        guard title != nil || body != nil else { return nil }
        return [title ?? pr.title, body].compactMap { $0 }.joined(separator: "\n\n")
    }

    func perform(_ action: PRAction, ref: ItemRef, pr: PullRequestDetail) async throws {
        let project = ref.repo.full
        switch action {
        case .close: try await client.update(project, iid: ref.number, fields: ["state_event": "close"])
        case .reopen: try await client.update(project, iid: ref.number, fields: ["state_event": "reopen"])
        case .readyForReview: try await client.update(project, iid: ref.number, fields: ["title": Self.undrafted(pr.title)])
        case .convertToDraft: try await client.update(project, iid: ref.number, fields: ["title": "Draft: " + Self.undrafted(pr.title)])
        case .updateBranch: try await client.rebase(project, iid: ref.number)
        case .disableAutoMerge: try await client.cancelAutoMerge(project, iid: ref.number)
        }
    }

    static func undrafted(_ title: String) -> String {
        let pattern = #"^\s*(\[draft\]|\(draft\)|draft:|draft\s*-|wip:|\[wip\])\s*"#
        return title.replacingOccurrences(of: pattern, with: "", options: [.regularExpression, .caseInsensitive])
    }

    func edit(_ ref: ItemRef, pr: PullRequestDetail, title: String, body: String) async throws {
        try await client.update(ref.repo.full, iid: ref.number, fields: ["title": title, "description": body])
    }

    func revokeApproval(_ ref: ItemRef, pr: PullRequestDetail) async throws {
        try await client.unapprove(ref.repo.full, iid: ref.number)
    }
}
