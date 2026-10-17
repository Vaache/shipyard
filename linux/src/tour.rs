use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::api::models::*;
use crate::app::{self, App, IssueFilter, PrFilter, Section};
use crate::ci_fix::{self, Details, Failure};
use crate::ui::pr::model::{Change, PrModel, PrTab};

thread_local! {
    static DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub fn set_dir(dir: PathBuf) {
    DIR.with(|d| *d.borrow_mut() = Some(dir));
}

pub fn active() -> bool {
    DIR.with(|d| d.borrow().is_some())
}

fn dir() -> Option<PathBuf> {
    DIR.with(|d| d.borrow().clone())
}

pub fn capture(name: &str) {
    let Some(dir) = dir() else { return };
    let Some(window) = app::main_window() else { return };
    let (w, h) = (window.width(), window.height());
    if w <= 0 || h <= 0 {
        return;
    }
    let paintable = gtk::WidgetPaintable::new(Some(&window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, w as f64, h as f64);
    let Some(node) = snapshot.to_node() else {
        eprintln!("tour: {name}: nothing to draw");
        return;
    };
    let Some(renderer) = window.renderer() else { return };
    let texture = renderer.render_texture(&node, Some(&gtk::graphene::Rect::new(0.0, 0.0, w as f32, h as f32)));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(format!("{name}.png"));
    match texture.save_to_png(&path) {
        Ok(()) => println!("tour: {}", path.display()),
        Err(e) => eprintln!("tour: {name}: {e}"),
    }
    if crate::brand::env_var("TOUR_X11").is_some() {
        let x11 = dir.join(format!("{name}-x11.png"));
        let _ = std::process::Command::new("import").arg("-window").arg("root").arg(&x11).status();
    }
}

fn close_dialog() {
    if let Some(w) = app::main_window().and_then(|w| w.downcast::<adw::ApplicationWindow>().ok()) {
        if let Some(d) = w.visible_dialog() {
            d.force_close();
        }
    }
}

fn item_ref(var: &str, number: u32) -> ItemRef {
    let spec = crate::brand::env_var(var).unwrap_or_else(|| format!("{}#{number}", crate::demo::repo().unwrap_or_else(|| "migueldeicaza/SwiftTerm".into())));
    let (repo, number) = spec.split_once('#').unwrap_or((spec.as_str(), "1"));
    let (owner, name) = repo.split_once('/').unwrap_or(("octocat", "Hello-World"));
    ItemRef { repo: RepoRef::new(owner, name), number: number.parse().unwrap_or(1) }
}

struct Plan {
    at: u64,
    steps: Vec<(u64, Box<dyn FnOnce()>)>,
}

impl Plan {
    fn wait(&mut self, ms: u64) -> &mut Self {
        self.at += ms;
        self
    }

    fn act(&mut self, f: impl FnOnce() + 'static) -> &mut Self {
        self.steps.push((self.at, Box::new(f)));
        self
    }

    fn shot(&mut self, name: &'static str) -> &mut Self {
        self.act(move || capture(name))
    }
}

fn inject_failing_checks(m: &PrModel) {
    let Ok(checks) = serde_json::from_str::<Vec<CheckContext>>(crate::fixtures::TOUR_FAILING_CHECKS) else { return };
    {
        let mut pr = m.pr.borrow_mut();
        let Some(pr) = pr.as_mut() else { return };
        let rollup = StatusRollup { state: "FAILURE".into(), contexts: Some(Nodes { nodes: checks, total_count: None }) };
        match pr.last_commit.nodes.first_mut() {
            Some(n) => n.commit.status_check_rollup = Some(rollup),
            None => pr.last_commit.nodes.push(LastCommitNode { commit: LastCommit { oid: pr.head_ref_oid.clone(), committed_date: None, status_check_rollup: Some(rollup) } }),
        }
    }
    m.emit(Change::Pr);
}

fn tour_failures(m: &PrModel) -> Vec<Failure> {
    let Some(pr) = m.pr.borrow().clone() else { return vec![] };
    ci_fix::failing(&pr)
        .iter()
        .map(|c| {
            let mut f = Failure::from_check(c);
            f.details = Details::Log(ci_fix::excerpt(crate::fixtures::CI_LOG));
            f.annotations = vec![CheckAnnotation {
                path: "Tests/SwiftTermTests/SearchTests.swift".into(),
                start_line: Some(42),
                end_line: Some(42),
                annotation_level: Some("failure".into()),
                title: Some("SearchTests.testStaleResultsAreDropped".into()),
                message: "XCTAssertEqual failed: (\"2\") is not equal to (\"1\")".into(),
            }];
            f
        })
        .collect()
}

pub fn run(_app: Rc<App>) {
    let r = item_ref("TOUR_PR", 753);
    let issue = item_ref("TOUR_ISSUE", 757);
    let repo = r.repo.clone();
    let mut p = Plan { at: 0, steps: vec![] };
    p.wait(9000).shot("01-review-requests");
    p.act(|| app::set_section(Section::Prs(PrFilter::Mine))).wait(4000).shot("02-created-by-me");
    let rr = r.clone();
    p.act(|| app::set_section(Section::Prs(PrFilter::ReviewRequested))).wait(3000);
    p.act(move || app::select(Some(Route::Pr(rr)))).wait(9000).shot("03-pr-conversation");
    p.act(|| {
        if let Some(r) = app::visible_route() {
            crate::ui::item_row::copy_short_link(&r);
        }
    })
    .wait(600)
    .shot("03-pr-short-link")
    .wait(2000);
    let model = move || app::pr_model(&r);
    let m = model.clone();
    p.act(move || {
        if let Some(m) = m() {
            m.set_tab(PrTab::Files);
        }
    })
    .wait(7000)
    .shot("04-pr-files");
    let m = model.clone();
    p.act(move || {
        let Some(m) = m() else { return };
        let Some(oid) = m.pr.borrow().as_ref().map(|p| p.head_ref_oid.clone()) else { return };
        let file = m.files.borrow().iter().filter(|f| f.status != "removed").max_by_key(|f| f.additions + f.deletions).map(|f| f.id.clone());
        if let Some(path) = file {
            crate::ui::repos::open_blame(m.r.repo.clone(), oid, path);
        }
    })
    .wait(8000)
    .shot("04b-pr-diff-blame")
    .act(app::back)
    .wait(1500);
    let m = model.clone();
    p.act(move || {
        if let Some(m) = m() {
            m.set_tab(PrTab::Commits);
        }
    })
    .wait(4000)
    .shot("05-pr-commits");
    let m = model.clone();
    p.act(move || {
        if let Some(m) = m() {
            m.set_tab(PrTab::Checks);
        }
    })
    .wait(4000)
    .shot("06-pr-checks");
    let m = model.clone();
    p.act(move || {
        if let Some(m) = m() {
            m.set_tab(PrTab::Conversation);
            crate::ui::pr::sheets::review(m);
        }
    })
    .wait(2500)
    .shot("07-review-sheet")
    .act(close_dialog);
    let m = model.clone();
    p.wait(800)
        .act(move || {
            if let Some(m) = m() {
                crate::ui::pr::sheets::merge(m);
            }
        })
        .wait(2500)
        .shot("08-merge-sheet")
        .act(close_dialog);
    let m = model.clone();
    p.wait(800)
        .act(move || {
            if let Some(m) = m() {
                crate::ui::fix_claude::show(m);
            }
        })
        .wait(2500)
        .shot("09-fix-with-claude")
        .act(close_dialog);
    let m = model.clone();
    p.wait(800)
        .act(move || {
            if let Some(m) = m() {
                inject_failing_checks(&m);
            }
        })
        .wait(1500)
        .shot("09b-ci-failing-banner");
    let m = model.clone();
    p.act(move || {
        if let Some(m) = m() {
            m.set_tab(PrTab::Checks);
        }
    })
    .wait(2000)
    .shot("09c-ci-checks");
    let m = model.clone();
    p.act(move || {
        if let Some(m) = m() {
            let failures = tour_failures(&m);
            crate::ui::fix_claude::show_ci_prefetched(m, failures);
        }
    })
    .wait(2500)
    .shot("09d-fix-ci-with-claude")
    .act(close_dialog);
    let m = model.clone();
    p.wait(800).act(move || {
        if let Some(m) = m() {
            m.set_tab(PrTab::Conversation);
        }
    });
    p.wait(800).act(|| app::set_section(Section::Issues(IssueFilter::Mine))).wait(5000).shot("10-issues");
    p.act(move || app::select(Some(Route::Issue(issue)))).wait(7000).shot("11-issue");
    p.act(|| app::set_section(Section::Notifications)).wait(5000).shot("12-notifications");
    p.act(|| app::set_section(Section::Repos)).wait(5000).shot("13-repos");
    let rp = repo.clone();
    p.act(move || app::select(Some(Route::Repo(rp)))).wait(8000).shot("14-repo");
    p.act(|| crate::ui::repos::show_blame(crate::brand::env_var("TOUR_BLAME_PATH").unwrap_or_else(|| "Package.swift".into()), None)).wait(7000).shot("14-repo-blame");
    p.act(|| app::set_section(Section::Projects)).wait(5000).shot("15-projects");
    p.act(|| app::set_section(Section::Claude)).wait(4000).shot("16-claude");
    p.act(|| {
        let ws = app::get().claude.clone();
        let folder = crate::demo::folder().unwrap_or_else(|| "/tmp".into());
        let prompt = crate::brand::env_var("TOUR_PROMPT");
        let tab = ws.new_session(&folder, prompt, Some("Cancel stale searches".into()), None, Some(crate::claude::TabKind::Chat));
        let script = crate::brand::env_var("TOUR_CHAT").and_then(|p| std::fs::read_to_string(p).ok());
        let chat = tab.chat.clone();
        glib::timeout_add_local_once(Duration::from_millis(1500), move || {
            let Some(chat) = chat else { return };
            match &script {
                Some(text) => text.lines().for_each(|line| chat.feed(line)),
                None => crate::fixtures::CHAT_STREAM.iter().for_each(|line| chat.feed(line)),
            }
        });
    })
    .wait(5000)
    .shot("16-claude-chat");
    p.act(crate::ui::settings::show).wait(2500).shot("17-preferences").act(close_dialog);
    p.act(app::set_smoke_logged_out).wait(2000).shot("18-login");
    p.act(|| crate::ui::login::select_provider(crate::git_accounts::GitProvider::Gitlab)).wait(800).shot("18b-login-gitlab");
    p.act(|| crate::ui::login::select_provider(crate::git_accounts::GitProvider::Bitbucket)).wait(800).shot("18c-login-bitbucket");
    p.wait(1000).act(|| {
        println!("tour: done");
        app::finish_smoke(0);
    });
    for (at, f) in p.steps.drain(..) {
        glib::timeout_add_local_once(Duration::from_millis(at), f);
    }
    glib::timeout_add_local_once(Duration::from_secs(260), || {
        eprintln!("tour: timed out");
        app::finish_smoke(3);
    });
}
