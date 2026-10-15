use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::api::models::*;
use crate::app::{self, App, IssueFilter, PrFilter, Section};
use crate::ui::pr::model::PrTab;

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

pub fn run(_app: Rc<App>) {
    let r = item_ref("TOUR_PR", 753);
    let issue = item_ref("TOUR_ISSUE", 757);
    let repo = r.repo.clone();
    let mut p = Plan { at: 0, steps: vec![] };
    p.wait(9000).shot("01-review-requests");
    p.act(|| app::set_section(Section::Prs(PrFilter::Mine))).wait(4000).shot("02-created-by-me");
    let rr = r.clone();
    p.act(move || app::select(Some(Route::Pr(rr)))).wait(9000).shot("03-pr-conversation");
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
    p.wait(800).act(|| app::set_section(Section::Issues(IssueFilter::Mine))).wait(5000).shot("10-issues");
    p.act(move || app::select(Some(Route::Issue(issue)))).wait(7000).shot("11-issue");
    p.act(|| app::set_section(Section::Notifications)).wait(5000).shot("12-notifications");
    p.act(|| app::set_section(Section::Repos)).wait(5000).shot("13-repos");
    let rp = repo.clone();
    p.act(move || app::select(Some(Route::Repo(rp)))).wait(8000).shot("14-repo");
    p.act(|| app::set_section(Section::Projects)).wait(5000).shot("15-projects");
    p.act(|| app::set_section(Section::Claude)).wait(4000).shot("16-claude");
    p.act(crate::ui::settings::show).wait(2500).shot("17-preferences").act(close_dialog);
    p.wait(1000).act(|| {
        println!("tour: done");
        app::finish_smoke(0);
    });
    for (at, f) in p.steps.drain(..) {
        glib::timeout_add_local_once(Duration::from_millis(at), f);
    }
    glib::timeout_add_local_once(Duration::from_secs(240), || {
        eprintln!("tour: timed out");
        app::finish_smoke(3);
    });
}
