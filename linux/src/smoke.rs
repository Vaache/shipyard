use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;

use crate::api::models::*;
use crate::api::GitHubApi;
use crate::app::{self, App, IssueFilter, PrFilter, Section};
use crate::ui::pr::model::{Change, PrModel};

fn step(delay_ms: u64, name: &'static str, f: impl FnOnce() + 'static) {
    glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || {
        println!("smoke: {name}");
        f();
    });
}

const DIFF_ROUNDS: usize = 12;

fn diff_round(m: Rc<PrModel>, middle: String, i: usize, mut sync: Vec<f64>, mut frame: Vec<f64>) {
    if i == DIFF_ROUNDS {
        let half = DIFF_ROUNDS / 2;
        let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
        let max = |v: &[f64]| v.iter().cloned().fold(0.0, f64::max);
        for (label, range) in [("full rebuild", 0..half), ("splice", half..DIFF_ROUNDS)] {
            let (s, f) = (&sync[range.clone()], &frame[range]);
            println!("smoke: large diff {label}: sync avg {:.1} ms max {:.1} ms, to painted frame avg {:.1} ms max {:.1} ms", avg(s), max(s), avg(f), max(f));
        }
        return;
    }
    if i % 2 == 0 {
        m.collapsed.borrow_mut().insert(middle.clone());
    } else {
        m.collapsed.borrow_mut().remove(&middle);
    }
    let started = Instant::now();
    m.emit(if i < DIFF_ROUNDS / 2 { Change::Files } else { Change::Diff });
    sync.push(started.elapsed().as_secs_f64() * 1000.0);
    let Some(win) = app::main_window() else { return };
    crate::ui::widgets::after_next_paint(&win, move || {
        frame.push(started.elapsed().as_secs_f64() * 1000.0);
        glib::idle_add_local_once(move || diff_round(m, middle, i + 1, sync, frame));
    });
}

pub fn run(_app: Rc<App>) {
    println!("smoke: window built ({} {})", crate::brand::APP_NAME, crate::brand::VERSION);
    let mut t = 200;
    let mut next = |gap: u64| {
        t += gap;
        t
    };
    step(next(0), "login screen", app::set_smoke_logged_out);
    step(next(400), "main view", || {
        let api = GitHubApi::new("smoke-test-invalid-token");
        app::set_smoke_logged_in(api, RestUser { login: "smoke".into(), name: Some("Smoke Test".into()), avatar_url: None });
    });
    step(next(200), "fixture pr", || {
        let r = ItemRef { repo: RepoRef::new("octocat", "Hello-World"), number: 1 };
        if let Some(api) = app::api() {
            let m = crate::ui::pr::model::PrModel::new(r, api);
            let resp: PrDetailResponse = serde_json::from_str(crate::fixtures::PR_DETAIL).expect("fixture");
            let files: Vec<PrFile> = serde_json::from_str(crate::fixtures::PR_FILES).expect("fixture");
            m.inject(resp, files);
            app::insert_pr_model(m);
        }
    });
    let sections = [
        Section::Prs(PrFilter::ReviewRequested),
        Section::Prs(PrFilter::Mine),
        Section::Prs(PrFilter::Assigned),
        Section::Prs(PrFilter::Mentioned),
        Section::Prs(PrFilter::Reviewed),
        Section::MyPrsByRepo,
        Section::Issues(IssueFilter::Assigned),
        Section::Issues(IssueFilter::Mine),
        Section::Issues(IssueFilter::Mentioned),
        Section::Notifications,
        Section::Repos,
        Section::Projects,
        Section::Claude,
        Section::Prs(PrFilter::Mine),
    ];
    for s in sections {
        step(next(250), "section", move || app::set_section(s));
    }
    let repo = RepoRef::new("octocat", "Hello-World");
    let routes = vec![
        Route::Pr(ItemRef { repo: repo.clone(), number: 1 }),
        Route::Issue(ItemRef { repo: repo.clone(), number: 2 }),
        Route::Repo(repo.clone()),
        Route::Project(ProjectRef { id: "PVT_x".into(), title: "Board".into(), url: "https://github.com/orgs/x/projects/1".into() }),
        Route::Commit(repo.clone(), "7fd1a60b01f91b314f59955a4e4d4e80d8edf11d".into()),
    ];
    for r in routes {
        step(next(400), "route", move || app::select(Some(r)));
    }
    step(next(400), "push route", move || app::open(Route::Pr(ItemRef { repo: RepoRef::new("octocat", "Spoon-Knife"), number: 3 }), true));
    step(next(300), "back", app::back);
    step(next(300), "pr files tab", || {
        if let Some(m) = app::pr_model(&ItemRef { repo: RepoRef::new("octocat", "Hello-World"), number: 1 }) {
            m.set_tab(crate::ui::pr::model::PrTab::Files);
            m.set_tab(crate::ui::pr::model::PrTab::Commits);
            m.set_tab(crate::ui::pr::model::PrTab::Checks);
            m.set_tab(crate::ui::pr::model::PrTab::Conversation);
        }
    });
    step(next(300), "fixture pr page", || app::select(Some(Route::Pr(ItemRef { repo: RepoRef::new("octocat", "Hello-World"), number: 1 }))));
    let pr = || app::pr_model(&ItemRef { repo: RepoRef::new("octocat", "Hello-World"), number: 1 }).expect("model");
    step(next(500), "files tab", move || pr().set_tab(crate::ui::pr::model::PrTab::Files));
    step(next(400), "inline composer and pending", move || {
        let m = pr();
        *m.composer.borrow_mut() = Some(crate::diff::DiffAnchor { path: "src/diff.rs".into(), line: 11, side: "RIGHT".into() });
        m.pending.borrow_mut().push(PendingComment { id: "p1".into(), path: "src/diff.rs".into(), line: 10, side: "RIGHT".into(), body: "Pending **note**".into() });
        m.emit(crate::ui::pr::model::Change::Diff);
        m.reveal_file("README.md");
    });
    let large = || ItemRef { repo: RepoRef::new("octocat", "Hello-World"), number: 4 };
    step(next(400), "large diff", move || {
        if let Some(api) = app::api() {
            let m = crate::ui::pr::model::PrModel::new(large(), api);
            let resp: PrDetailResponse = serde_json::from_str(crate::fixtures::PR_DETAIL).expect("fixture");
            m.inject(resp, crate::fixtures::large_diff(50));
            app::insert_pr_model(m.clone());
            app::select(Some(Route::Pr(large())));
            m.set_tab(crate::ui::pr::model::PrTab::Files);
        }
    });
    step(next(800), "large diff update", move || {
        let Some(m) = app::pr_model(&large()) else { return };
        let rows = m.flat_rows("").len();
        let changed: i64 = m.files.borrow().iter().map(|f| f.additions + f.deletions).sum();
        println!("smoke: large diff {changed} changed lines, {rows} rows");
        let middle = m.files.borrow().get(25).map(|f| f.id.clone()).unwrap_or_default();
        diff_round(m, middle, 0, vec![], vec![]);
    });
    step(next(3000), "fixture pr page again", || app::select(Some(Route::Pr(ItemRef { repo: RepoRef::new("octocat", "Hello-World"), number: 1 }))));
    step(next(400), "commits tab", move || pr().set_tab(crate::ui::pr::model::PrTab::Commits));
    step(next(300), "checks tab", move || pr().set_tab(crate::ui::pr::model::PrTab::Checks));
    step(next(300), "conversation tab", move || pr().set_tab(crate::ui::pr::model::PrTab::Conversation));
    step(next(300), "review sheet", move || crate::ui::pr::sheets::review(pr()));
    step(next(300), "merge sheet", move || crate::ui::pr::sheets::merge(pr()));
    step(next(300), "edit sheet", move || crate::ui::pr::sheets::edit(pr()));
    step(next(300), "reviewers sheet", move || crate::ui::pr::sheets::reviewers(pr()));
    step(next(300), "reply sheet", move || {
        let m = pr();
        let t = m.pr.borrow().as_ref().and_then(|p| p.review_threads.nodes.first().cloned());
        if let Some(t) = t {
            crate::ui::pr::sheets::reply(m.clone(), t);
        }
    });
    step(next(300), "fix with claude sheet", move || crate::ui::fix_claude::show(pr()));
    step(next(300), "job log sheet", move || {
        let m = pr();
        let c = m.pr.borrow().as_ref().map(|p| p.checks()[0].clone());
        if let Some(c) = c {
            crate::ui::pr::sheets::job_log(m.api.clone(), m.r.repo.clone(), c, Some(std::rc::Rc::new(|| {})));
        }
    });
    step(next(300), "fix ci sheet", move || crate::ui::fix_claude::show_ci(pr(), None));
    step(next(300), "open link dialog", crate::ui::main_view::open_link_dialog);
    step(next(300), "preferences", crate::ui::settings::show);
    step(next(300), "diagnostics", || {
        crate::diagnostics::set_enabled(true);
        crate::ui::diagnostics::show();
    });
    step(next(300), "update dialog", || crate::ui::update::show(false));
    step(next(300), "about", crate::ui::main_view::about);
    step(next(300), "theme toggle", || {
        crate::settings::update(|s| s.appearance = "dark".into());
        crate::theme::apply();
        crate::settings::update(|s| s.appearance = "auto".into());
        crate::theme::apply();
    });
    step(next(300), "claude section", || app::set_section(Section::Claude));
    step(next(300), "claude chat tab", || {
        let ws = app::get().claude.clone();
        let tab = ws.new_session("/tmp", None, Some("Smoke chat".into()), None, Some(crate::claude::TabKind::Chat));
        if let Some(chat) = &tab.chat {
            for line in crate::fixtures::CHAT_STREAM {
                chat.feed(line);
            }
        }
    });
    step(next(600), "claude terminal tab", || {
        let ws = app::get().claude.clone();
        ws.new_session("/tmp", None, Some("Smoke terminal".into()), None, Some(crate::claude::TabKind::Terminal));
    });
    step(next(600), "claude select chat", || {
        let ws = app::get().claude.clone();
        let first = ws.tabs.borrow().first().map(|t| t.id);
        if let Some(id) = first {
            ws.select(id);
        }
    });
    step(next(400), "deep link", || app::get().handle_url(&format!("{}://section/repos", crate::brand::URL_SCHEME)));
    step(next(1200), "done", || {
        println!("smoke: ok");
        app::finish_smoke(0);
    });
    glib::timeout_add_local_once(Duration::from_secs(60), || {
        eprintln!("smoke: timed out");
        app::finish_smoke(3);
    });
}
