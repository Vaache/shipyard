use std::collections::{HashMap, HashSet};

use crate::api::models::*;
use crate::util::{self, esc};

#[derive(Clone, Copy)]
pub struct Look {
    pub dark: bool,
    pub brand: bool,
}

impl Look {
    pub fn current() -> Self {
        Look { dark: crate::theme::is_dark(), brand: crate::theme::is_brand() }
    }
}

fn avatar(url: Option<&str>, size: i32) -> String {
    match url {
        None => "<div class=\"avatar ph\"></div>".into(),
        Some(url) => {
            let sep = if url.contains('?') { "&" } else { "?" };
            format!("<img class=\"avatar\" src=\"{}{sep}s={}\" width=\"{size}\" height=\"{size}\">", esc(url), size * 2)
        }
    }
}

fn time(d: Option<Date>) -> String {
    match d {
        None => String::new(),
        Some(d) => format!("<span title=\"{}\">{}</span>", esc(&util::full_date(d)), esc(&util::rel(Some(d)))),
    }
}

fn body(html: Option<&str>) -> String {
    let h = html.unwrap_or("").trim();
    if h.is_empty() {
        "<div class=\"md\"><p class=\"empty\">No description provided.</p></div>".into()
    } else {
        format!("<div class=\"md\">{h}</div>")
    }
}

fn comment_box(author: Option<&Actor>, date: Option<Date>, html: Option<&str>, note: &str, highlight: bool, link: Option<&str>) -> String {
    let login = esc(author.map(|a| a.login.as_str()).unwrap_or("ghost"));
    let link_html = link.map(|l| format!(" · <a href=\"{}\">link</a>", esc(l))).unwrap_or_default();
    format!(
        "<div class=\"item\">{}<div class=\"box{}\">\n<div class=\"head\"><b>{login}</b><span>{note}</span><span>·</span>{}{link_html}</div>\n{}</div></div>",
        avatar(author.and_then(|a| a.avatar_url.as_deref()), 32),
        if highlight { " mine" } else { "" },
        time(date),
        body(html)
    )
}

fn event(icon: &str, cls: &str, actor: Option<&Actor>, text: &str, date: Option<Date>) -> String {
    let who = actor.map(|a| format!("{}<b>{}</b>", avatar(a.avatar_url.as_deref(), 18), esc(&a.login))).unwrap_or_default();
    format!("<div class=\"event\"><span class=\"ic {cls}\">{icon}</span>{who}<span>{text}</span><span class=\"t\">{}</span></div>", time(date))
}

fn label(l: &LabelInfo) -> String {
    let c = esc(&util::clean_hex(&l.color));
    format!("<span class=\"label\" style=\"background:#{c}38;border-color:#{c}\">{}</span>", esc(&l.name))
}

fn hunk(diff_hunk: Option<&str>) -> String {
    let Some(diff_hunk) = diff_hunk else { return String::new() };
    let mut lines: Vec<&str> = diff_hunk.split('\n').collect();
    if lines.first().map(|l| l.starts_with("@@")).unwrap_or(false) && lines.len() > 1 {
        lines.remove(0);
    }
    let start = lines.len().saturating_sub(6);
    let rows: String = lines[start..]
        .iter()
        .map(|l| {
            let cls = if l.starts_with('+') { "a" } else if l.starts_with('-') { "d" } else { "" };
            format!("<div class=\"{cls}\">{}</div>", esc(if l.is_empty() { " " } else { l }))
        })
        .collect();
    format!("<div class=\"hunk\">{rows}</div>")
}

pub fn thread(t: &ReviewThread) -> String {
    let line = t.line.or(t.original_line);
    let mut tags = String::new();
    if t.is_outdated {
        tags.push_str("<span class=\"pill\">outdated</span>");
    }
    if t.is_resolved {
        tags.push_str("<span class=\"pill ok\">resolved</span>");
    }
    let comments: String = t
        .comments
        .nodes
        .iter()
        .map(|c| {
            format!(
                "<div class=\"tc\">{}<div class=\"tcb\"><div class=\"meta\"><b>{}</b> · {}</div><div class=\"md\">{}</div></div></div>",
                avatar(c.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 20),
                esc(c.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost")),
                time(Some(c.created_at)),
                c.body_html
            )
        })
        .collect();
    let id = esc(&t.id);
    let mut actions = String::new();
    if t.viewer_can_reply {
        actions.push_str(&format!("<button onclick=\"post('reply','{id}')\">Reply</button>"));
    }
    if t.is_resolved && t.viewer_can_unresolve {
        actions.push_str(&format!("<button onclick=\"post('unresolve','{id}')\">Unresolve</button>"));
    }
    if !t.is_resolved && t.viewer_can_resolve {
        actions.push_str(&format!("<button onclick=\"post('resolve','{id}')\">Resolve</button>"));
    }
    actions.push_str(&format!("<button class=\"ghost\" onclick=\"post('file','{}')\">View in files</button>", esc(&t.path.replace('\\', "\\\\").replace('\'', "\\'"))));
    format!(
        "<details class=\"thread\"{}><summary class=\"th\">{}{}{tags}</summary>\n{}{comments}<div class=\"actions\">{actions}</div></details>",
        if t.is_resolved { "" } else { " open" },
        esc(&t.path),
        line.map(|l| format!("<span class=\"ln\">:{l}</span>")).unwrap_or_default(),
        hunk(t.comments.nodes.first().and_then(|c| c.diff_hunk.as_deref()))
    )
}

pub fn render_pr(pr: &PullRequestDetail, look: Look) -> String {
    let author_login = pr.author.as_ref().map(|a| a.login.clone());
    let mut out = comment_box(pr.author.as_ref(), Some(pr.created_at), Some(&pr.body_html), "description", true, None);
    let mut by_review: HashMap<String, Vec<&ReviewThread>> = HashMap::new();
    for t in &pr.review_threads.nodes {
        if let Some(rid) = t.comments.nodes.first().and_then(|c| c.pull_request_review.as_ref()).map(|r| r.id.clone()) {
            by_review.entry(rid).or_default().push(t);
        }
    }
    let mut rendered: HashSet<String> = HashSet::new();
    let total = pr.timeline_items.total_count.unwrap_or(0);
    if total > pr.timeline_items.nodes.len() as i64 {
        out.push_str(&format!(
            "<div class=\"event\"><span class=\"ic\">…</span><span>Showing the latest {} of {} events — <a href=\"{}\">full history</a></span></div>",
            pr.timeline_items.nodes.len(),
            total,
            esc(&pr.url)
        ));
    }
    for item in &pr.timeline_items.nodes {
        if item.typename == "PullRequestReview" {
            let threads = by_review.get(item.id.as_deref().unwrap_or("")).cloned().unwrap_or_default();
            for t in &threads {
                rendered.insert(t.id.clone());
            }
            out.push_str(&review(item, &threads, item.author.as_ref().map(|a| a.login.clone()) == author_login));
        } else {
            out.push_str(&common(item, author_login.as_deref(), &pr.base_ref_name));
        }
    }
    let rest: Vec<&ReviewThread> = pr.review_threads.nodes.iter().filter(|t| !rendered.contains(&t.id)).collect();
    if !rest.is_empty() {
        out.push_str("<h3 class=\"sec\">Review threads</h3>");
        for t in rest {
            out.push_str(&thread(t));
        }
    }
    page(&out, look)
}

pub fn render_issue(issue: &IssueDetail, look: Look) -> String {
    let mut out = comment_box(issue.author.as_ref(), Some(issue.created_at), Some(&issue.body_html), "description", true, None);
    let author = issue.author.as_ref().map(|a| a.login.clone());
    for item in &issue.timeline_items.nodes {
        out.push_str(&common(item, author.as_deref(), ""));
    }
    page(&out, look)
}

fn review(item: &TimelineItem, threads: &[&ReviewThread], highlight: bool) -> String {
    let has_body = !item.body_html.as_deref().unwrap_or("").trim().is_empty();
    let (icon, cls, text) = match item.state.as_deref() {
        Some("APPROVED") => ("✓", "ok", "approved"),
        Some("CHANGES_REQUESTED") => ("±", "bad", "requested changes"),
        Some("DISMISSED") => ("✕", "", "review dismissed"),
        Some("PENDING") => ("…", "", "pending review"),
        _ => ("💬", "", "reviewed"),
    };
    if !has_body && threads.is_empty() {
        if item.state.as_deref() == Some("COMMENTED") {
            return String::new();
        }
        return event(icon, cls, item.author.as_ref(), text, item.created_at);
    }
    let login = esc(item.author.as_ref().map(|a| a.login.as_str()).unwrap_or("ghost"));
    let body_html = if has_body { format!("<div class=\"md\">{}</div>", item.body_html.as_deref().unwrap_or("")) } else { String::new() };
    let th: String = threads.iter().map(|t| thread(t)).collect();
    format!(
        "<div class=\"item\">{}<div class=\"box{}\">\n<div class=\"head\"><b>{login}</b><span class=\"rv {cls}\">{icon} {text}</span><span>·</span>{}</div>\n{body_html}{th}</div></div>",
        avatar(item.author.as_ref().and_then(|a| a.avatar_url.as_deref()), 32),
        if highlight { " mine" } else { "" },
        time(item.created_at)
    )
}

fn common(item: &TimelineItem, author_login: Option<&str>, base: &str) -> String {
    match item.typename.as_str() {
        "IssueComment" => comment_box(
            item.author.as_ref(),
            item.created_at,
            item.body_html.as_deref(),
            "commented",
            item.author.as_ref().map(|a| a.login.as_str()) == author_login,
            None,
        ),
        "PullRequestCommit" => {
            let Some(c) = &item.commit else { return String::new() };
            let who = c.author.as_ref().and_then(|a| a.user.as_ref().map(|u| u.login.clone()).or_else(|| a.name.clone())).unwrap_or_default();
            let actor = Actor { login: who, avatar_url: c.author.as_ref().and_then(|a| a.avatar_url.clone()) };
            event(
                "●",
                "",
                Some(&actor),
                &format!("commit <code>{}</code> {}", esc(c.abbreviated_oid.as_deref().unwrap_or("")), esc(c.message_headline.as_deref().unwrap_or(""))),
                c.committed_date,
            )
        }
        "MergedEvent" => {
            let oid = item.commit.as_ref().and_then(|c| c.abbreviated_oid.as_ref()).map(|o| format!(" (<code>{}</code>)", esc(o))).unwrap_or_default();
            event("⇢", "merged", item.actor.as_ref(), &format!("merged{oid} into <code>{}</code>", esc(item.merge_ref_name.as_deref().unwrap_or(base))), item.created_at)
        }
        "ClosedEvent" => event("✕", "bad", item.actor.as_ref(), "closed", item.created_at),
        "ReopenedEvent" => event("↺", "ok", item.actor.as_ref(), "reopened", item.created_at),
        "HeadRefForcePushedEvent" => event("⤒", "", item.actor.as_ref(), "force-pushed the branch", item.created_at),
        "HeadRefDeletedEvent" => event("⌫", "", item.actor.as_ref(), "deleted the branch", item.created_at),
        "ReadyForReviewEvent" => event("◉", "ok", item.actor.as_ref(), "marked as ready for review", item.created_at),
        "ConvertToDraftEvent" => event("◌", "", item.actor.as_ref(), "converted to draft", item.created_at),
        "ReviewRequestedEvent" => event(
            "👁",
            "",
            item.actor.as_ref(),
            &format!("requested review from → <b>{}</b>", esc(&item.requested_reviewer.as_ref().map(|r| r.display()).unwrap_or_else(|| "?".into()))),
            item.created_at,
        ),
        "LabeledEvent" => match &item.label {
            Some(l) => event("🏷", "", item.actor.as_ref(), &format!("label {}", label(l)), item.created_at),
            None => String::new(),
        },
        "RenamedTitleEvent" => event(
            "✎",
            "",
            item.actor.as_ref(),
            &format!("title: <s>{}</s> → {}", esc(item.previous_title.as_deref().unwrap_or("")), esc(item.current_title.as_deref().unwrap_or(""))),
            item.created_at,
        ),
        "CrossReferencedEvent" => {
            let Some(s) = &item.source else { return String::new() };
            let Some(url) = &s.url else { return String::new() };
            let name = format!("{}#{}", s.repository.as_ref().map(|r| r.name_with_owner.as_str()).unwrap_or(""), s.number.unwrap_or(0));
            event("↗", "", item.actor.as_ref(), &format!("mentioned in <a href=\"{}\">{} {}</a>", esc(url), esc(&name), esc(s.title.as_deref().unwrap_or(""))), item.created_at)
        }
        _ => String::new(),
    }
}

pub const POST_SCRIPT: &str = "function post(t,id){try{window.webkit.messageHandlers.app.postMessage(JSON.stringify({type:t,id:id}))}catch(e){}}";

pub fn page(content: &str, look: Look) -> String {
    format!(
        "<!doctype html><html class=\"{}\"><head><meta charset=\"utf-8\"><style>{}</style>\n<script>{POST_SCRIPT}</script>\n</head><body><div class=\"tl\">{content}</div></body></html>",
        if look.dark { "dark" } else { "light" },
        css(look)
    )
}

pub fn markdown_page(inner: &str, look: Look) -> String {
    format!(
        "<!doctype html><html class=\"{}\"><head><meta charset=\"utf-8\"><style>{} .md{{padding:8px 4px}} .tl{{max-width:900px}} .md a.anchor,.md .anchor,.md .octicon-link{{display:none}}</style></head>\n<body><div class=\"tl\"><div class=\"md\">{inner}</div></div></body></html>",
        if look.dark { "dark" } else { "light" },
        css(look)
    )
}

pub fn css(look: Look) -> String {
    if look.brand {
        format!("{BRAND_VARS}{BASE_CSS}{BRAND_OVERRIDES}")
    } else {
        format!("{SYSTEM_VARS}{BASE_CSS}")
    }
}

const SYSTEM_VARS: &str = ":root{--fg:#1f2328;--muted:#59636e;--border:#d1d9e0;--sub:#f6f8fa;--card:transparent;--link:#0969da;--green:#1a7f37;--red:#d1242f;--purple:#8250df;--add:#dafbe1;--del:#ffebe9;--mine:#ddf4ff;--code:#fff;color-scheme:light}
:root.dark{--fg:#e6edf3;--muted:#9198a1;--border:#3d444d;--sub:#151b23;--link:#4493f8;--green:#3fb950;--red:#f85149;--purple:#ab7df8;--add:rgba(46,160,67,.18);--del:rgba(248,81,73,.18);--mine:rgba(56,139,253,.12);--code:transparent;color-scheme:dark}
";

const BRAND_VARS: &str = ":root{--fg:#1a2333;--muted:#5f6b7e;--border:rgba(26,35,51,.09);--sub:#f3f1ec;--card:#ffffff;--link:#1f7fb8;--green:#2e8a57;--red:#c2412f;--purple:#6b55d6;--add:rgba(46,138,87,.10);--del:rgba(194,65,47,.09);--mine:rgba(31,127,184,.07);--code:#f7f5f0;color-scheme:light}
:root.dark{--fg:#e6ebf2;--muted:#8693a8;--border:rgba(160,174,196,.10);--sub:#111a2c;--card:#152036;--link:#5bb8ec;--green:#5dbe8a;--red:#e8746a;--purple:#9d8cf0;--add:rgba(93,190,138,.10);--del:rgba(232,116,106,.10);--mine:rgba(91,184,236,.07);--code:#0a101c;color-scheme:dark}
";

const BRAND_OVERRIDES: &str = ".box,.thread,.hunk,.event .ic,.pill,button,.md pre{border:0}
.head{border-bottom:0}
.pill{background:var(--sub)}
button:hover{background:var(--border)}
.tc{border-bottom:1px solid var(--border)}
";

const BASE_CSS: &str = "*{box-sizing:border-box}
body{font:13px/1.5 system-ui,Cantarell,\"Noto Sans\",\"Helvetica Neue\",sans-serif;color:var(--fg);background:transparent;margin:0;padding:16px 20px 32px}
a{color:var(--link);text-decoration:none}a:hover{text-decoration:underline}
.tl{max-width:1000px;margin:0 auto}
.item{display:flex;gap:12px;margin:0 0 16px;align-items:flex-start}
.avatar{border-radius:50%;flex:none}.avatar.ph{width:32px;height:32px;background:var(--border)}
.box{flex:1;min-width:0;border:1px solid var(--border);border-radius:10px;overflow:hidden;background:var(--card)}
.head{padding:7px 12px;background:var(--sub);border-bottom:1px solid var(--border);color:var(--muted);display:flex;gap:6px;flex-wrap:wrap;align-items:center}
.box.mine .head{background:var(--mine)}
.head b{color:var(--fg)}
.rv.ok{color:var(--green);font-weight:600}.rv.bad{color:var(--red);font-weight:600}
.md{padding:10px 14px;overflow-x:auto;word-wrap:break-word}
.md>:first-child{margin-top:0}.md>:last-child{margin-bottom:0}
.md p,.md ul,.md ol,.md blockquote,.md pre,.md table,.md details{margin:0 0 10px}
.md img{max-width:100%;border-radius:4px}
.md pre{background:var(--code);border:1px solid var(--border);padding:10px 12px;border-radius:8px;overflow:auto;font-size:12px;line-height:1.45}
code,pre,kbd{font-family:ui-monospace,\"JetBrains Mono\",\"DejaVu Sans Mono\",\"Source Code Pro\",monospace;font-size:12px}
.md :not(pre)>code,.event code{background:rgba(129,139,152,.16);padding:.1em .35em;border-radius:5px}
.md blockquote{padding:0 1em;color:var(--muted);border-left:.25em solid var(--border)}
.md table{border-collapse:collapse;display:block;overflow:auto;max-width:100%}.md td,.md th{border:1px solid var(--border);padding:4px 10px}
.md h1,.md h2{border-bottom:1px solid var(--border);padding-bottom:.25em}.md h1{font-size:1.5em}.md h2{font-size:1.3em}.md h3{font-size:1.12em}
.md h1,.md h2,.md h3,.md h4{margin:14px 0 8px}
.md hr{border:0;border-top:1px solid var(--border)}
.md ul.contains-task-list{list-style:none;padding-left:1.1em}
.md summary{cursor:pointer}
.md .user-mention,.md .team-mention{font-weight:600;color:var(--fg)}
.md .pl-k,.md .pl-s1 .pl-k{color:#cf222e}.md .pl-s,.md .pl-pds{color:#0a3069}.md .pl-c{color:#6e7781}.md .pl-en,.md .pl-e{color:#8250df}.md .pl-c1,.md .pl-v{color:#0550ae}.md .pl-smi{color:inherit}
.dark .md .pl-k{color:#ff7b72}.dark .md .pl-s,.dark .md .pl-pds{color:#a5d6ff}.dark .md .pl-c{color:#8b949e}.dark .md .pl-en,.dark .md .pl-e{color:#d2a8ff}.dark .md .pl-c1,.dark .md .pl-v{color:#79c0ff}
.md .blob-num,.md .blob-code{font:12px ui-monospace,monospace}
.empty{color:var(--muted);font-style:italic}
.event{display:flex;align-items:center;gap:7px;margin:0 0 12px 44px;color:var(--muted);font-size:12px;flex-wrap:wrap}
.event b{color:var(--fg)}
.event .ic{width:22px;height:22px;border-radius:50%;display:inline-flex;align-items:center;justify-content:center;background:var(--sub);border:1px solid var(--border);font-size:11px;flex:none;color:var(--fg)}
.event .ic.ok{background:var(--green);color:#fff;border-color:transparent}.event .ic.bad{background:var(--red);color:#fff;border-color:transparent}.event .ic.merged{background:var(--purple);color:#fff;border-color:transparent}
.event .t{margin-left:2px}
.label{display:inline-block;padding:0 7px;border-radius:10px;border:1px solid;font-size:11px;font-weight:500;color:var(--fg)}
.thread{margin:10px 12px;border:1px solid var(--border);border-radius:9px;overflow:hidden;background:var(--sub)}
.thread>summary{padding:6px 10px;font:12px ui-monospace,monospace;cursor:pointer;display:flex;gap:8px;align-items:center;list-style:none}
.thread>summary::-webkit-details-marker{display:none}
.thread .ln{color:var(--muted);margin-left:-6px}
.pill{font:11px system-ui,sans-serif;padding:0 7px;border-radius:10px;border:1px solid var(--border);color:var(--muted)}.pill.ok{color:var(--green);border-color:var(--green)}
.hunk{font:11.5px/1.5 ui-monospace,monospace;overflow:auto;border-top:1px solid var(--border);border-bottom:1px solid var(--border);background:var(--code)}
.hunk div{padding:0 10px;white-space:pre}.hunk .a{background:var(--add)}.hunk .d{background:var(--del)}
.tc{display:flex;gap:8px;padding:8px 10px;border-bottom:1px solid var(--border)}
.tc .avatar{margin-top:2px}.tcb{min-width:0;flex:1}.tc .meta{color:var(--muted);font-size:12px}.tc .meta b{color:var(--fg)}
.tc .md{padding:2px 0 0}
.actions{padding:6px 10px;display:flex;gap:6px}
button{font:12px system-ui,sans-serif;padding:3px 10px;border-radius:7px;border:1px solid var(--border);background:var(--sub);color:var(--fg);cursor:pointer}
button:hover{border-color:var(--muted)}button.ghost{background:transparent;color:var(--muted)}
h3.sec{margin:24px 0 8px 44px;font-size:13px;color:var(--muted)}
";
