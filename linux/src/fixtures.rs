use crate::api::models::PrFile;

pub const PR_DETAIL: &str = r##"{
  "viewer": { "login": "smoke" },
  "repository": {
    "viewerPermission": "WRITE", "mergeCommitAllowed": true, "squashMergeAllowed": true, "rebaseMergeAllowed": false,
    "deleteBranchOnMerge": false, "autoMergeAllowed": true, "viewerDefaultMergeMethod": "SQUASH",
    "pullRequest": {
      "id": "PR_1", "number": 1, "title": "Add a faster diff viewer", "url": "https://github.com/octocat/Hello-World/pull/1",
      "state": "OPEN", "isDraft": false, "merged": false, "mergeable": "MERGEABLE", "mergeStateStatus": "BLOCKED",
      "body": "Speeds up **large** diffs.\n\n--- FIX PROMPT ---\nRename `foo` to `bar`.\n--- END FIX PROMPT ---",
      "bodyHTML": "<p>Speeds up <strong>large</strong> diffs.</p><pre><code>let x = 1;</code></pre>",
      "createdAt": "2026-10-01T10:00:00Z",
      "author": { "login": "octocat", "avatarUrl": null },
      "headRefName": "feature/diff", "baseRefName": "main", "headRefOid": "bbb",
      "headRepository": { "nameWithOwner": "octocat/Hello-World" }, "headRef": { "name": "feature/diff" },
      "additions": 12, "deletions": 3, "changedFiles": 2, "reviewDecision": "CHANGES_REQUESTED",
      "viewerCanUpdate": true, "viewerDidAuthor": true, "autoMergeRequest": null,
      "labels": { "nodes": [ { "name": "perf", "color": "0e8a16" }, { "name": "ui", "color": "fbca04" } ] },
      "assignees": { "nodes": [ { "login": "octocat", "avatarUrl": null } ] },
      "reviewRequests": { "nodes": [ { "requestedReviewer": { "__typename": "User", "login": "hubot", "avatarUrl": null } } ] },
      "latestOpinionatedReviews": { "nodes": [ { "state": "CHANGES_REQUESTED", "submittedAt": "2026-10-02T10:00:00Z", "commit": { "oid": "aaa" }, "author": { "login": "monalisa", "avatarUrl": null } } ] },
      "lastCommit": { "nodes": [ { "commit": { "oid": "bbb", "committedDate": "2026-10-03T10:00:00Z", "statusCheckRollup": { "state": "FAILURE", "contexts": { "nodes": [
        { "__typename": "CheckRun", "databaseId": 11, "name": "build", "status": "COMPLETED", "conclusion": "FAILURE", "detailsUrl": "https://example.com/1", "startedAt": "2026-10-03T10:00:00Z", "completedAt": "2026-10-03T10:02:05Z", "checkSuite": { "workflowRun": { "databaseId": 77, "workflow": { "name": "CI" } } } },
        { "__typename": "CheckRun", "databaseId": 12, "name": "lint", "status": "IN_PROGRESS", "conclusion": null, "detailsUrl": null, "startedAt": null, "completedAt": null, "checkSuite": null },
        { "__typename": "StatusContext", "context": "deploy", "state": "SUCCESS", "targetUrl": "https://example.com/2", "description": "Deployed" }
      ] } } } } ] },
      "allCommits": { "totalCount": 2, "nodes": [
        { "commit": { "oid": "aaa", "abbreviatedOid": "aaa1234", "messageHeadline": "First pass", "committedDate": "2026-10-01T11:00:00Z", "url": "https://github.com/octocat/Hello-World/commit/aaa", "author": { "name": "Octo", "avatarUrl": null, "user": { "login": "octocat" } }, "statusCheckRollup": { "state": "SUCCESS" } } },
        { "commit": { "oid": "bbb", "abbreviatedOid": "bbb1234", "messageHeadline": "Address review", "committedDate": "2026-10-03T10:00:00Z", "url": "https://github.com/octocat/Hello-World/commit/bbb", "author": { "name": "Octo", "avatarUrl": null, "user": null }, "statusCheckRollup": { "state": "FAILURE" } } }
      ] },
      "files": { "nodes": [ { "path": "src/diff.rs", "viewerViewedState": "UNVIEWED" }, { "path": "README.md", "viewerViewedState": "VIEWED" } ] },
      "timelineItems": { "totalCount": 6, "nodes": [
        { "__typename": "IssueComment", "id": "IC_1", "author": { "login": "hubot", "avatarUrl": null }, "body": "Looks good", "bodyHTML": "<p>Looks <em>good</em></p>", "createdAt": "2026-10-01T12:00:00Z", "url": "https://github.com/octocat/Hello-World/pull/1#c1" },
        { "__typename": "PullRequestReview", "id": "R_1", "author": { "login": "monalisa", "avatarUrl": null }, "state": "CHANGES_REQUESTED", "body": "Please rename things", "bodyHTML": "<p>Please rename things</p>", "createdAt": "2026-10-02T10:00:00Z", "url": "https://github.com/octocat/Hello-World/pull/1#r1" },
        { "__typename": "PullRequestCommit", "commit": { "oid": "bbb", "abbreviatedOid": "bbb1234", "messageHeadline": "Address review", "committedDate": "2026-10-03T10:00:00Z", "author": { "name": "Octo", "avatarUrl": null, "user": { "login": "octocat" } } } },
        { "__typename": "LabeledEvent", "actor": { "login": "octocat", "avatarUrl": null }, "createdAt": "2026-10-03T11:00:00Z", "label": { "name": "perf", "color": "0e8a16" } },
        { "__typename": "RenamedTitleEvent", "actor": { "login": "octocat", "avatarUrl": null }, "createdAt": "2026-10-03T12:00:00Z", "previousTitle": "WIP", "currentTitle": "Add a faster diff viewer" },
        { "__typename": "ReviewRequestedEvent", "actor": { "login": "octocat", "avatarUrl": null }, "createdAt": "2026-10-03T13:00:00Z", "requestedReviewer": { "__typename": "User", "login": "hubot", "avatarUrl": null } }
      ] },
      "reviewThreads": { "nodes": [
        { "id": "T_1", "isResolved": false, "isOutdated": false, "path": "src/diff.rs", "line": 11, "originalLine": 11, "diffSide": "RIGHT", "viewerCanResolve": true, "viewerCanUnresolve": false, "viewerCanReply": true,
          "comments": { "nodes": [ { "id": "C_1", "author": { "login": "monalisa", "avatarUrl": null }, "body": "Rename `foo` here", "bodyHTML": "<p>Rename <code>foo</code> here</p>", "createdAt": "2026-10-02T10:00:00Z", "diffHunk": "@@ -10,3 +10,4 @@\n a\n+foo", "pullRequestReview": { "id": "R_1" } } ] } },
        { "id": "T_2", "isResolved": true, "isOutdated": true, "path": "README.md", "line": null, "originalLine": 3, "diffSide": "RIGHT", "viewerCanResolve": false, "viewerCanUnresolve": true, "viewerCanReply": true,
          "comments": { "nodes": [ { "id": "C_2", "author": { "login": "hubot", "avatarUrl": null }, "body": "Typo", "bodyHTML": "<p>Typo</p>", "createdAt": "2026-10-02T11:00:00Z", "diffHunk": null, "pullRequestReview": null } ] } }
      ] }
    }
  }
}"##;

pub const PR_FILES: &str = r##"[
  { "filename": "src/diff.rs", "status": "modified", "additions": 10, "deletions": 2, "patch": "@@ -10,3 +10,4 @@ fn parse\n a\n-b\n+foo\n+\tbar\n c\n\\ No newline at end of file" },
  { "filename": "README.md", "status": "renamed", "previous_filename": "README.txt", "additions": 2, "deletions": 1, "patch": "@@ -1,2 +1,3 @@\n # Title\n-old\n+new\n+more" },
  { "filename": "logo.png", "status": "added", "additions": 0, "deletions": 0 }
]"##;

pub const CHAT_STREAM: &[&str] = &[
    r##"{"type":"system","subtype":"init","session_id":"sess-1","model":"claude-test","permissionMode":"default"}"##,
    r##"{"type":"control_response","response":{"request_id":"gl-1-initialize","subtype":"success","response":{"models":[{"value":"default","displayName":"Default","description":"Recommended","supportedEffortLevels":["low","high"]}],"commands":[{"name":"review","description":"Review code"}],"account":{"subscriptionType":"Claude Max"}}}}"##,
    r##"{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking"}}}"##,
    r##"{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Considering the diff."}}}"##,
    r##"{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_start","index":1,"content_block":{"type":"text"}}}"##,
    r##"{"type":"stream_event","api_message_id":"m1","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"# Plan\n\n- read **files**\n- run `tests`\n\n```rust\nfn main() {}\n```\n| a | b |\n|---|---|\n| 1 | 2 |"}}}"##,
    r##"{"type":"assistant","message":{"id":"m1","model":"claude-test","content":[{"type":"text","text":"# Plan\n\n- read **files**\n- run `tests`"},{"type":"tool_use","id":"tool-1","name":"Edit","input":{"file_path":"/tmp/x.rs","old_string":"a\nb","new_string":"a\nc"}}]}}"##,
    r##"{"type":"control_request","request_id":"req-1","request":{"subtype":"can_use_tool","tool_name":"Edit","input":{"file_path":"/tmp/x.rs"},"tool_use_id":"tool-1","permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Edit","ruleContent":"/tmp/**"}]}]}}"##,
    r##"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tool-1","content":"ok","is_error":false}]},"tool_use_result":{"structuredPatch":[{"oldStart":1,"newStart":1,"lines":[" a","-b","+c"]}]}}"##,
    r##"{"type":"assistant","message":{"id":"m2","content":[{"type":"tool_use","id":"tool-2","name":"Bash","input":{"command":"cargo test","description":"Run tests"}},{"type":"tool_use","id":"tool-3","name":"TodoWrite","input":{"todos":[{"content":"Fix","status":"completed"},{"content":"Test","status":"in_progress"}]}}]}}"##,
    r##"{"type":"control_request","request_id":"req-2","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{"questions":[{"question":"Which?","options":[{"label":"A","description":"first"},{"label":"B"}]}]},"tool_use_id":"tool-4"}}"##,
    r##"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","unifiedWindows":{"five_hour":{"utilization":0.4,"resetsAt":1900000000},"seven_day":{"utilization":0.1}}}}"##,
    r##"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.01}"##,
    r##"{"type":"control_response","response":{"request_id":"gl-2-get_context_usage","subtype":"success","response":{"totalTokens":12000,"maxTokens":200000,"percentage":6,"categories":[{"name":"Messages","tokens":9000,"color":"blue","kind":"used"}]}}}"##,
];

pub fn large_diff(files: usize) -> Vec<PrFile> {
    (0..files)
        .map(|f| {
            let mut patch = String::from("@@ -1,40 +1,80 @@ fn generated");
            for i in 0..100 {
                patch.push('\n');
                patch.push_str(&match i % 5 {
                    0 => format!(" let context_{i} = {f};"),
                    1 => format!("-    let old_{i} = compute({i}, {f});"),
                    _ => format!("+    let new_{i} = compute_faster({i}, {f}).unwrap_or_default();"),
                });
            }
            PrFile { filename: format!("src/generated/module_{f}.rs"), status: "modified".into(), additions: 60, deletions: 20, patch: Some(patch), previous_filename: None }
        })
        .collect()
}
