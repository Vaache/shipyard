use crate::app::{PrFilter, Section};

pub fn build(section: Section) -> gtk::Widget {
    if crate::app::forge().is_some() {
        return forge(section);
    }
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

fn forge(section: Section) -> gtk::Widget {
    use super::forge_list::Kind;
    match section {
        Section::Prs(f) => super::forge_list::build(Kind::Pulls(f)),
        Section::MyPrsByRepo => super::forge_list::build(Kind::PullsByProject),
        Section::Issues(f) => super::forge_list::build(Kind::Issues(f)),
        Section::Notifications => super::forge_inbox::inbox(),
        Section::Repos => super::forge_inbox::repos(),
        Section::Claude => super::claude::column::build(),
        Section::Projects => super::forge_inbox::unavailable(),
    }
}
