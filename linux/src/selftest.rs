use std::future::Future;

use crate::api::models::{ItemRef, Route};
use crate::api::{ApiError, GitHubApi};
use crate::brand;
use crate::conversation_html::{self, Look};
use crate::diff::DiffFile;
use crate::fix_prompt::{self, FixOptions};
use crate::util;

async fn step<F: Future<Output = Result<String, ApiError>>>(failures: &mut i32, name: &str, op: F) {
    match op.await {
        Ok(s) => println!("OK   {name}: {s}"),
        Err(e) => {
            *failures += 1;
            println!("FAIL {name}: {e}");
        }
    }
}

pub fn run() -> i32 {
    let Some(token) = brand::env_var("TOKEN") else {
        println!("{} not set", brand::env_key("TOKEN"));
        return 2;
    };
    let api = GitHubApi::new(&token);
    let failures = crate::rt::block_on(async move {
        let mut failures = 0;
        let mut sample_pr: Option<ItemRef> = None;
        let mut sample_issue: Option<ItemRef> = None;
        step(&mut failures, "user", async {
            let (u, s) = api.current_user().await?;
            Ok(format!("{} scopes={:?}", u.login, s))
        })
        .await;
        step(&mut failures, "badges", async {
            let b = api.badges().await?;
            Ok(format!("rr={} mine={}", b.rr.issue_count, b.mine.issue_count))
        })
        .await;
        for q in ["is:pr author:@me is:closed archived:false sort:updated-desc", "is:pr review-requested:@me archived:false sort:updated-desc", "is:pr author:@me sort:updated-desc"] {
            let mut found = None;
            step(&mut failures, &format!("search {q}"), async {
                let r = api.search(q, None).await?;
                found = r.nodes.first().map(|n| n.item_ref());
                Ok(format!("count={} first={}", r.issue_count, r.nodes.first().map(|n| n.item_ref().title()).unwrap_or_else(|| "-".into())))
            })
            .await;
            if found.is_some() {
                sample_pr = found;
            }
        }
        step(&mut failures, "search issues", async {
            let r = api.search("is:issue involves:@me sort:updated-desc", None).await?;
            sample_issue = r.nodes.first().map(|n| n.item_ref());
            Ok(format!("count={}", r.issue_count))
        })
        .await;
        if let Some(Route::Pr(r)) = brand::env_var("PR").and_then(|p| util::parse_github_url(&p, true)) {
            sample_pr = Some(r);
        }
        if let Some(pr) = sample_pr.clone() {
            step(&mut failures, &format!("pr {}", pr.title()), async {
                let r = api.pull_request(&pr).await?;
                let Some(p) = r.repository.and_then(|r| r.pull_request) else { return Ok("not a PR".into()) };
                let html = conversation_html::render_pr(&p, Look { dark: false, brand: true });
                let fix = fix_prompt::latest(&p);
                let unresolved: Vec<_> = p.review_threads.nodes.iter().filter(|t| !t.is_resolved).collect();
                let composed = fix_prompt::build(&p, &pr.repo, &unresolved, &fix_prompt::general_feedback(&p), &FixOptions::default());
                if brand::env_var("SHOWFIX").is_some() {
                    println!("----- FOUND -----\n{}", fix.as_ref().map(|f| f.text.chars().take(300).collect::<String>()).unwrap_or_else(|| "none".into()));
                    println!("----- COMPOSED -----\n{}", composed.chars().take(1200).collect::<String>());
                }
                Ok(format!(
                    "state={} timeline={} threads={} unresolved={} html={} fixPrompt={} composed={}ch",
                    p.state,
                    p.timeline_items.nodes.len(),
                    p.review_threads.nodes.len(),
                    unresolved.len(),
                    html.len(),
                    fix.map(|f| format!("@{} {}ch", f.author, f.text.len())).unwrap_or_else(|| "none".into()),
                    composed.len()
                ))
            })
            .await;
            step(&mut failures, "pr light", async {
                let full = api.pull_request(&pr).await?.repository.and_then(|r| r.pull_request).map(|p| p.light);
                let light = api.pull_request_light(&pr).await?;
                let known = light.as_ref().is_some_and(|l| l.is_known());
                Ok(format!("known={known} matchesFull={}", light.is_some() && light == full))
            })
            .await;
            step(&mut failures, "files", async {
                let f = api.pull_request_files(&pr).await?;
                let lines: usize = f.iter().map(|x| DiffFile::new(x).lines.len()).sum();
                Ok(format!("files={} lines={lines}", f.len()))
            })
            .await;
            step(&mut failures, &format!("repo PRs {}", pr.repo.full()), async {
                let open = api.repo_pull_requests(&pr.repo, None, &["OPEN"], 30).await?;
                let merged = api.repo_pull_requests(&pr.repo, None, &["MERGED"], 30).await?;
                let branch = brand::env_var("BRANCH").unwrap_or_else(|| "main".into());
                let by_branch = api.repo_pull_requests(&pr.repo, Some(branch.clone()), &["OPEN", "MERGED", "CLOSED"], 5).await?;
                Ok(format!("open={} merged={} branch({branch})={}", open.len(), merged.len(), by_branch.len()))
            })
            .await;
            step(&mut failures, "stack", async {
                let s = api.pull_request_stack(&pr).await?;
                Ok(format!("entries={}", s.len()))
            })
            .await;
        }
        if let Some(issue) = sample_issue {
            step(&mut failures, &format!("issue {}", issue.title()), async {
                let r = api.issue(&issue).await?;
                let node = r.repository.and_then(|r| r.issue_or_pull_request);
                Ok(format!(
                    "type={} timeline={}",
                    node.as_ref().map(|n| n.typename.clone()).unwrap_or_else(|| "-".into()),
                    node.and_then(|n| n.issue).map(|i| i.timeline_items.nodes.len() as i64).unwrap_or(-1)
                ))
            })
            .await;
        }
        step(&mut failures, "repos", async {
            let r = api.repos(None).await?;
            Ok(format!("total={} first={}", r.total_count, r.nodes.first().map(|n| n.name_with_owner.clone()).unwrap_or_else(|| "-".into())))
        })
        .await;
        step(&mut failures, "projects", async {
            let p = api.projects().await?;
            if let Some(first) = p.first() {
                let (d, items) = api.project(&first.id).await?;
                return Ok(format!("count={} first={} fields={} items={}", p.len(), d.title, d.fields.nodes.len(), items.len()));
            }
            Ok("count=0".into())
        })
        .await;
        step(&mut failures, "notifications", async {
            let n = api.notifications(false, true).await?;
            Ok(format!("unread={}", n.len()))
        })
        .await;
        step(&mut failures, "latest release", async {
            let url = brand::latest_release_api();
            let mut probe = crate::diagnostics::begin_rest("GET", &url);
            let (status, bytes) = crate::api::probed_get(&mut probe, crate::api::http_client().get(&url)).await?;
            if status.as_u16() == 404 {
                return Ok("none".into());
            }
            let r = crate::updater::parse_release(&bytes)?;
            Ok(format!("{} linux-asset={}", r.version, r.asset_url.is_some()))
        })
        .await;
        failures
    });
    if failures == 0 {
        0
    } else {
        1
    }
}
