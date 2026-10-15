use std::rc::Rc;
use std::time::Duration;

use gtk::glib;

use crate::api::models::*;
use crate::api::GitHubApi;
use crate::app::{self, App, IssueFilter, PrFilter, Section};

fn step(delay_ms: u64, name: &'static str, f: impl FnOnce() + 'static) {
    glib::timeout_add_local_once(Duration::from_millis(delay_ms), move || {
        println!("smoke: {name}");
        f();
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
    step(next(300), "open link dialog", crate::ui::main_view::open_link_dialog);
    step(next(300), "preferences", crate::ui::settings::show);
    step(next(300), "update dialog", || crate::ui::update::show(false));
    step(next(300), "about", crate::ui::main_view::about);
    step(next(300), "theme toggle", || {
        crate::settings::update(|s| s.appearance = "dark".into());
        crate::theme::apply();
        crate::settings::update(|s| s.appearance = "auto".into());
        crate::theme::apply();
    });
    step(next(300), "claude section", || app::set_section(Section::Claude));
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
