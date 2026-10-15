use crate::api::models::*;

#[derive(Clone, Debug)]
pub struct FixOptions {
    pub include_general: bool,
    pub commit: bool,
    pub push: bool,
    pub reply_and_resolve: bool,
}

impl Default for FixOptions {
    fn default() -> Self {
        FixOptions { include_general: true, commit: true, push: false, reply_and_resolve: false }
    }
}

#[derive(Clone, Debug)]
pub struct GeneralFeedback {
    pub id: String,
    pub author: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct FoundFixPrompt {
    pub text: String,
    pub author: String,
    pub date: Option<Date>,
    pub url: Option<String>,
}

pub fn general_feedback(pr: &PullRequestDetail) -> Vec<GeneralFeedback> {
    pr.timeline_items
        .nodes
        .iter()
        .filter_map(|item| {
            if item.typename != "PullRequestReview" || item.state.as_deref() != Some("CHANGES_REQUESTED") {
                return None;
            }
            let body = item.body.as_deref()?.trim().to_string();
            if body.is_empty() {
                return None;
            }
            Some(GeneralFeedback {
                id: item.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                author: item.author.as_ref().map(|a| a.login.clone()).unwrap_or_else(|| "ghost".into()),
                body,
            })
        })
        .collect()
}

fn quote(body: &str) -> String {
    body.split('\n').map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n")
}

pub fn build(pr: &PullRequestDetail, repo: &RepoRef, threads: &[&ReviewThread], general: &[GeneralFeedback], options: &FixOptions) -> String {
    let mut out = format!(
        "Address the code review feedback on pull request #{} \"{}\" in {}.\nPR: {}\nBranch: {} → {}\n\nSteps:\n1. Make sure the working tree is on the PR branch: run `gh pr checkout {}` and `git pull`. If there are uncommitted local changes, stop and ask me first.\n2. Go through every item below. Make the change the reviewer asked for. If a comment is unclear, already handled, or you disagree with it, do not change code for it — explain why in your summary.\n3. Keep changes minimal and in the style of the surrounding code. Run the project's analyzer/linter and the tests related to the touched code.\n",
        pr.number,
        pr.title,
        repo.full(),
        pr.url,
        pr.head_ref_name,
        pr.base_ref_name,
        pr.number
    );
    let mut step = 4;
    if options.commit {
        out.push_str(&format!("{step}. Commit the fixes with a clear message.\n"));
        step += 1;
    }
    if options.push {
        out.push_str(&format!("{step}. Push the branch (`git push`).\n"));
    } else {
        out.push_str(&format!("{step}. Do not push — I will review the changes first.\n"));
    }
    step += 1;
    if options.reply_and_resolve {
        out.push_str(&format!(
            "{step}. After pushing, for each fixed thread reply with a short note of what changed and resolve it:\n   gh api graphql -f query='mutation($id:ID!,$b:String!){{addPullRequestReviewThreadReply(input:{{pullRequestReviewThreadId:$id,body:$b}}){{clientMutationId}}}}' -f id=THREAD_ID -f b='Fixed: …'\n   gh api graphql -f query='mutation($id:ID!){{resolveReviewThread(input:{{threadId:$id}}){{clientMutationId}}}}' -f id=THREAD_ID\n   Leave threads you did not fix unresolved.\n"
        ));
        step += 1;
    }
    out.push_str(&format!("{step}. Finish with a short list: each item → fixed / skipped (and why).\n"));

    if !threads.is_empty() {
        out.push_str(&format!("\n## Review comments in code ({})\n", threads.len()));
        for (i, th) in threads.iter().enumerate() {
            let line = th.line.or(th.original_line);
            out.push_str(&format!("\n### {}. `{}{}`", i + 1, th.path, line.map(|l| format!(":{l}")).unwrap_or_default()));
            if th.is_outdated {
                out.push_str(" (outdated — code may have moved)");
            }
            out.push_str(&format!("\nThread ID: {}\n", th.id));
            if let Some(hunk) = th.comments.nodes.first().and_then(|c| c.diff_hunk.as_deref()) {
                let lines: Vec<&str> = hunk.split('\n').filter(|l| !l.starts_with("@@")).collect();
                let start = lines.len().saturating_sub(8);
                out.push_str(&format!("```diff\n{}\n```\n", lines[start..].join("\n")));
            }
            for c in &th.comments.nodes {
                let body = c.body.trim();
                out.push_str(&format!("**@{}**:\n{}\n", c.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost"), quote(body)));
            }
        }
    }
    if options.include_general && !general.is_empty() {
        out.push_str("\n## General review feedback (changes requested)\n");
        for g in general {
            out.push_str(&format!("\n**@{}**:\n{}\n", g.author, quote(&g.body)));
        }
    }
    out
}

fn find_ci(hay: &str, needle: &str) -> Option<(usize, usize)> {
    let lower = hay.to_lowercase();
    let n = needle.to_lowercase();
    if lower.len() != hay.len() {
        return hay.find(needle).map(|i| (i, i + needle.len()));
    }
    lower.find(&n).map(|i| (i, i + n.len()))
}

pub fn extract(body: &str) -> Option<String> {
    if let Some((_, start)) = find_ci(body, "--- FIX PROMPT ---") {
        let mut rest = &body[start..];
        if let Some((end, _)) = find_ci(rest, "--- END FIX PROMPT ---") {
            rest = &rest[..end];
        } else if let Some(fence) = rest.find("```") {
            rest = &rest[..fence];
        }
        let text = rest.trim();
        return if text.is_empty() { None } else { Some(text.to_string()) };
    }
    for marker in ["fix prompt", "prompt for ai agent"] {
        let Some((_, after_marker)) = find_ci(body, marker) else { continue };
        let after = &body[after_marker..];
        let Some(open) = after.find("```") else { continue };
        let after_open = &after[open + 3..];
        let Some(nl) = after_open.find('\n') else { continue };
        let content = &after_open[nl + 1..];
        let Some(close) = content.find("```") else { continue };
        let text = content[..close].trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

pub fn latest(pr: &PullRequestDetail) -> Option<FoundFixPrompt> {
    let mut best: Option<FoundFixPrompt> = None;
    let mut consider = |body: Option<&str>, author: Option<&str>, date: Option<Date>, url: Option<&str>| {
        let Some(text) = body.and_then(extract) else { return };
        if let (Some(b), Some(d)) = (best.as_ref().and_then(|b| b.date), date) {
            if d < b {
                return;
            }
        }
        best = Some(FoundFixPrompt { text, author: author.unwrap_or("ghost").to_string(), date, url: url.map(|s| s.to_string()) });
    };
    consider(Some(&pr.body), pr.author.as_ref().map(|a| a.login.as_str()), Some(pr.created_at), Some(&pr.url));
    for item in &pr.timeline_items.nodes {
        if item.typename == "IssueComment" || item.typename == "PullRequestReview" {
            consider(item.body.as_deref(), item.author.as_ref().map(|a| a.login.as_str()), item.created_at, item.url.as_deref());
        }
    }
    best
}

pub fn wrap(found: &FoundFixPrompt, pr: &PullRequestDetail, options: &FixOptions) -> String {
    let mut out = format!(
        "Before starting: make sure the working tree is on the PR branch — run `gh pr checkout {}` and `git pull`. If there are uncommitted local changes, stop and ask me first.\n\n{}\n",
        pr.number, found.text
    );
    let mut extra = vec![];
    if options.commit {
        extra.push("Commit the fixes with a clear message.".to_string());
    }
    extra.push(if options.push { "Push the branch when done (`git push`).".to_string() } else { "Do not push — I will review the changes first.".to_string() });
    if options.reply_and_resolve {
        extra.push("After pushing, reply on each review thread you fixed with a short note and resolve it via `gh api graphql` (addPullRequestReviewThreadReply / resolveReviewThread). Leave unfixed threads unresolved.".to_string());
    }
    out.push_str(&format!("\nAdditional instructions:\n{}\n", extra.iter().map(|e| format!("- {e}")).collect::<Vec<_>>().join("\n")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_markers() {
        assert_eq!(extract("hello --- FIX PROMPT ---\nDo it\n--- END FIX PROMPT --- bye").as_deref(), Some("Do it"));
        assert_eq!(extract("### Fix prompt\n```text\nRename x\n```").as_deref(), Some("Rename x"));
        assert_eq!(extract("nothing here"), None);
    }
}
