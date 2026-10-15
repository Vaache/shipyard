use crate::app::{PrFilter, Section};

pub fn build(section: Section) -> gtk::Widget {
    match section {
        Section::Notifications => super::notifications::build(),
        Section::Prs(f) => super::lists::SearchList::build(f.title(), "pr", f.query(), false),
        Section::MyPrsByRepo => super::lists::SearchList::build("My PRs by project", "pr", PrFilter::Mine.query(), true),
        Section::Issues(f) => super::lists::SearchList::build(&format!("Issues: {}", f.title()), "issue", f.query(), false),
        Section::Repos => super::repos::list(),
        Section::Projects => super::projects::list(),
        Section::Claude => super::claude::column::build(),
    }
}
