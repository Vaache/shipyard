use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::menu::Menu;
use super::widgets::*;
use crate::app::{self, IssueFilter, PrFilter, Section};
use crate::{brand, updater};

struct Row {
    section: Section,
    list: gtk::ListBox,
    row: gtk::ListBoxRow,
    count: gtk::Label,
}

pub struct Sidebar {
    pub widget: gtk::Widget,
    rows: RefCell<Vec<Row>>,
    update_row: gtk::Button,
    update_label: gtk::Label,
    footer: gtk::Box,
}

fn sidebar_icon(icon_name: &str, color: &str) -> gtk::Box {
    let b = fixed_box(&["sy-sidebar-icon"], 22, 22);
    centered_in(&b, &colored_icon(icon_name, 13, color), 22, 22);
    b
}

impl Sidebar {
    pub fn new() -> Rc<Self> {
        let groups: Vec<(&str, Vec<(Section, String, &str, &str)>)> = vec![
            ("Inbox", vec![(Section::Notifications, "Notifications".into(), "sy-bell-symbolic", "amber")]),
            (
                "Pull requests",
                PrFilter::ALL
                    .iter()
                    .map(|f| (Section::Prs(*f), f.title().to_string(), f.icon(), f.tint()))
                    .chain(std::iter::once((Section::MyPrsByRepo, "My PRs by project".to_string(), "sy-folder-symbolic", "coral")))
                    .collect(),
            ),
            ("Issues", IssueFilter::ALL.iter().map(|f| (Section::Issues(*f), f.title().to_string(), f.icon(), f.tint())).collect()),
            (
                "Code",
                vec![(Section::Repos, "Repositories".into(), "sy-repo-symbolic", "teal"), (Section::Projects, "Projects".into(), "sy-project-symbolic", "indigo")],
            ),
            ("AI", vec![(Section::Claude, "Claude Code".into(), "sy-sparkles-symbolic", "coral")]),
        ];
        let column = vbox(0);
        let mut rows = vec![];
        for (title, items) in groups {
            column.append(&label_with(title, &["sy-section-title"]));
            let list = gtk::ListBox::new();
            list.add_css_class("sy-sidebar-list");
            list.set_selection_mode(gtk::SelectionMode::Single);
            for (section, title, icon_name, tint) in items {
                let b = hbox(8);
                b.append(&sidebar_icon(icon_name, tint));
                let l = ellipsized(&title, &["sy-text"]);
                l.set_hexpand(true);
                b.append(&l);
                let count = label_with("", &["sy-count", "sy-small"]);
                b.append(&count);
                let row = gtk::ListBoxRow::new();
                row.set_child(Some(&b));
                list.append(&row);
                rows.push(Row { section, list: list.clone(), row, count });
            }
            column.append(&list);
        }
        let scroll = scrolled(&column);

        let update_row = gtk::Button::new();
        update_row.add_css_class("flat");
        update_row.add_css_class("update-row");
        let ub = hbox(8);
        ub.append(&colored_icon("sy-download-symbolic", 14, "sky"));
        let update_label = label_with("", &["sy-text"]);
        update_label.set_hexpand(true);
        ub.append(&update_label);
        ub.append(&icon("go-next-symbolic", 12));
        update_row.set_child(Some(&ub));
        update_row.set_visible(false);
        update_row.set_tooltip_text(Some(&format!("A new version of {} is available", brand::APP_NAME)));
        update_row.connect_clicked(|_| updater::present());

        let footer = hbox(8);
        footer.add_css_class("user-footer");

        let bottom = vbox(0);
        bottom.append(&update_row);
        bottom.append(&footer);

        let tv = adw::ToolbarView::new();
        let hb = adw::HeaderBar::new();
        hb.set_title_widget(Some(&adw::WindowTitle::new(brand::APP_NAME, "")));
        let menu = Menu::new();
        menu.item("Open by Link…", || ui_activate("open-link"));
        menu.item("Refresh", || ui_activate("refresh"));
        let mut menu = menu;
        menu.section(None);
        menu.item("Preferences", || ui_activate("preferences"));
        menu.item("Check for Updates…", || ui_activate("check-updates"));
        menu.item("Buy Me a Pizza…", || ui_activate("donate"));
        menu.item(&format!("About {}", brand::APP_NAME), || ui_activate("about"));
        menu.section(None);
        menu.item("Quit", || ui_activate("quit"));
        let mb = menu.button("open-menu-symbolic", "Main menu");
        mb.set_primary(true);
        hb.pack_end(&mb);
        hb.pack_start(&super::inbox::button());
        tv.add_top_bar(&hb);
        tv.set_content(Some(&scroll));
        tv.add_bottom_bar(&bottom);
        tv.add_css_class("sy-sidebar-pane");

        let me = Rc::new(Sidebar { widget: tv.upcast(), rows: RefCell::new(rows), update_row, update_label, footer });
        for r in me.rows.borrow().iter() {
            let section = r.section;
            let row = r.row.clone();
            r.list.connect_row_activated(move |_, activated| {
                if activated == &row {
                    app::set_section(section);
                }
            });
        }
        me.sync_selection(app::section());
        let weak = Rc::downgrade(&me);
        app::get().section_changed.connect(move |s| match weak.upgrade() {
            Some(me) => {
                me.sync_selection(*s);
                true
            }
            None => false,
        });
        let weak = Rc::downgrade(&me);
        app::on_badges(move |_| match weak.upgrade() {
            Some(me) => {
                me.update_counts();
                true
            }
            None => false,
        });
        let weak = Rc::downgrade(&me);
        app::get().claude.changed.connect(move |_| match weak.upgrade() {
            Some(me) => {
                me.update_counts();
                true
            }
            None => false,
        });
        let weak = Rc::downgrade(&me);
        updater::on_change(move |_| match weak.upgrade() {
            Some(me) => {
                me.update_update_row();
                true
            }
            None => false,
        });
        let weak = Rc::downgrade(&me);
        app::get().phase_changed.connect(move |_| match weak.upgrade() {
            Some(me) => {
                me.build_footer();
                true
            }
            None => false,
        });
        me.update_counts();
        me.update_update_row();
        me.build_footer();
        keep_alive(&me.widget, me.clone());
        me
    }

    fn sync_selection(&self, s: Section) {
        for r in self.rows.borrow().iter() {
            if r.section == s {
                r.list.select_row(Some(&r.row));
            } else if r.list.selected_row().as_ref() == Some(&r.row) {
                r.list.unselect_row(&r.row);
            }
        }
    }

    fn update_counts(&self) {
        let a = app::get();
        let (rr, mine, unread) = {
            let s = a.state.borrow();
            (s.review_request_count, s.my_pr_count, s.unread_count)
        };
        let attention = a.claude.attention_count();
        let running = a.claude.running_count();
        for r in self.rows.borrow().iter() {
            let n: i64 = match r.section {
                Section::Notifications => unread as i64,
                Section::Prs(PrFilter::ReviewRequested) => rr,
                Section::Prs(PrFilter::Mine) => mine,
                Section::Claude => {
                    if attention > 0 {
                        attention as i64
                    } else {
                        running as i64
                    }
                }
                _ => 0,
            };
            r.count.set_text(&if n > 0 { n.to_string() } else { String::new() });
        }
    }

    fn update_update_row(&self) {
        match updater::available_version() {
            Some(v) => {
                self.update_label.set_text(&format!("Update to {v}"));
                self.update_row.set_visible(true);
            }
            None => self.update_row.set_visible(false),
        }
    }

    fn build_footer(&self) {
        clear(&self.footer);
        let user = app::get().state.borrow().user.clone();
        let Some(user) = user else { return };
        let display = user.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| user.login.clone());
        self.footer.append(&avatar(user.avatar_url.as_deref(), 24, Some(&display)));
        let texts = vbox(0);
        texts.set_hexpand(true);
        texts.append(&ellipsized(&display, &["sy-text", "sy-bold"]));
        texts.append(&label_with(&format!("@{}", user.login), &["sy-muted", "sy-small"]));
        self.footer.append(&texts);
        let settings = icon_button("emblem-system-symbolic", "Preferences");
        settings.connect_clicked(|_| ui_activate("preferences"));
        self.footer.append(&settings);
    }
}

pub fn ui_activate(name: &str) {
    let a = app::get();
    gtk::prelude::ActionGroupExt::activate_action(&a.gtk_app, name, None);
}
