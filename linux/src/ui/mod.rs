pub mod claude;
pub mod code_view;
pub mod commit;
pub mod composer;
pub mod dialogs;
pub mod fix_claude;
pub mod html;
pub mod inbox;
pub mod issue;
pub mod item_row;
pub mod lists;
pub mod login;
pub mod main_view;
pub mod markdown;
pub mod menu;
pub mod notifications;
pub mod pr;
pub mod projects;
pub mod repos;
pub mod sections;
pub mod settings;
pub mod sidebar;
pub mod splash;
pub mod tabstrip;
pub mod update;
pub mod widgets;

use crate::api::models::Route;

pub fn route_page(route: &Route) -> adw::NavigationPage {
    match route {
        Route::Pr(r) => pr::page(r),
        Route::Issue(r) => issue::page(r),
        Route::Repo(r) => repos::detail_page(r),
        Route::Project(p) => projects::board_page(p),
        Route::Commit(r, sha) => commit::page(r, sha),
    }
}
