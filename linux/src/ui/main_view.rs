use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;

use super::dialogs::Sheet;
use super::widgets::*;
use crate::api::models::Route;
use crate::app::{self, Section};
use crate::{brand, util};

pub struct MainView {
    pub widget: gtk::Widget,
    content: adw::Bin,
    detail: gtk::Stack,
    nav: adw::NavigationView,
    cache: RefCell<HashMap<String, gtk::Widget>>,
    claude_view: RefCell<Option<gtk::Widget>>,
    routes: RefCell<Vec<(gtk::glib::WeakRef<adw::NavigationPage>, Route)>>,
}

fn empty_page() -> adw::NavigationPage {
    let b = vbox(12);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.set_vexpand(true);
    let tile = icon_tile("sy-pr-symbolic", "violet", 56);
    tile.set_opacity(0.9);
    b.append(&tile);
    let t = label_with("Select a pull request, issue or project", &["sy-muted"]);
    t.set_xalign(0.5);
    b.append(&t);
    let s = label_with("Ctrl+L — open by link", &["sy-dim", "sy-small"]);
    s.set_xalign(0.5);
    b.append(&s);
    let (tv, _, _) = toolbar_page("", "", &b);
    tv.add_css_class("sy-panel-pane");
    let page = adw::NavigationPage::new(&tv, brand::APP_NAME);
    page.set_tag(Some("empty"));
    page
}

impl MainView {
    pub fn new() -> Rc<Self> {
        let split = adw::OverlaySplitView::new();
        split.set_min_sidebar_width(200.0);
        split.set_max_sidebar_width(300.0);
        split.set_sidebar_width_fraction(0.18);
        let sidebar = super::sidebar::Sidebar::new();
        split.set_sidebar(Some(&sidebar.widget));

        let content = adw::Bin::new();
        content.add_css_class("sy-list-pane");
        content.set_size_request(300, -1);
        let nav = adw::NavigationView::new();
        nav.add(&empty_page());
        let detail = gtk::Stack::new();
        detail.set_transition_type(gtk::StackTransitionType::Crossfade);
        detail.add_named(&nav, Some("nav"));
        detail.set_hexpand(true);

        let paned = gtk::Paned::new(gtk::Orientation::Horizontal);
        paned.set_start_child(Some(&content));
        paned.set_end_child(Some(&detail));
        paned.set_resize_start_child(false);
        paned.set_shrink_start_child(false);
        paned.set_shrink_end_child(false);
        paned.set_position(380);
        paned.set_wide_handle(false);
        split.set_content(Some(&paned));

        let me = Rc::new(MainView { widget: split.upcast(), content, detail, nav, cache: RefCell::new(HashMap::new()), claude_view: RefCell::new(None), routes: RefCell::new(vec![]) });
        me.nav.connect_visible_page_notify(|_| {
            gtk::glib::idle_add_local_once(app::sync_actions);
        });
        me.show_section(app::section());
        let weak = Rc::downgrade(&me);
        app::get().section_changed.connect(move |s| match weak.upgrade() {
            Some(me) => {
                me.show_section(*s);
                gtk::glib::idle_add_local_once(app::sync_actions);
                true
            }
            None => false,
        });
        let route = app::get().state.borrow().route.clone();
        if route.is_some() {
            me.show_route(route, false);
        }
        me
    }

    fn show_section(self: &Rc<Self>, s: Section) {
        let key = s.key();
        let cached = self.cache.borrow().get(&key).cloned();
        let w = match cached {
            Some(w) => w,
            None => {
                let w = super::sections::build(s);
                self.cache.borrow_mut().insert(key, w.clone());
                w
            }
        };
        self.content.set_child(Some(&w));
        if s == Section::Claude {
            let existing = self.claude_view.borrow().clone();
            let cv = match existing {
                Some(v) => v,
                None => {
                    let v = super::claude::workspace_view::build();
                    self.detail.add_named(&v, Some("claude"));
                    *self.claude_view.borrow_mut() = Some(v.clone());
                    v
                }
            };
            self.detail.set_visible_child(&cv);
        } else {
            self.detail.set_visible_child_name("nav");
        }
    }

    pub fn show_route(&self, route: Option<Route>, push: bool) {
        match route {
            None => {
                self.nav.replace(&[empty_page()]);
            }
            Some(r) => {
                let page = super::route_page(&r);
                {
                    let mut routes = self.routes.borrow_mut();
                    routes.retain(|(w, _)| w.upgrade().is_some_and(|p| p.parent().is_some()));
                    routes.push((page.downgrade(), r));
                }
                if push {
                    self.nav.push(&page);
                } else {
                    self.nav.replace(&[page]);
                }
            }
        }
    }

    pub fn visible_route(&self) -> Option<Route> {
        let page = self.nav.visible_page()?;
        self.routes.borrow().iter().rev().find(|(w, _)| w.upgrade().as_ref() == Some(&page)).map(|(_, r)| r.clone())
    }

    pub fn back(&self) {
        self.nav.pop();
    }
}

pub fn open_link_dialog() {
    let sheet = Sheet::new("Open by link", 520, -1);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("https://github.com/org/repo/pull/123 or org/repo#123"));
    entry.set_hexpand(true);
    let invalid = label_with("Couldn't recognize the link", &["fg-red", "sy-small"]);
    invalid.set_visible(false);
    sheet.body.append(&entry);
    sheet.body.append(&invalid);
    sheet.add_footer_spacer();
    sheet.cancel_button();
    let open = suggested("Open");
    sheet.footer.append(&open);
    let d = sheet.dialog.clone();
    let e = entry.clone();
    let inv = invalid.clone();
    let go = Rc::new(move || match util::parse_github_url(&e.text(), true) {
        Some(r) => {
            d.close();
            app::open(r, false);
        }
        None => inv.set_visible(true),
    });
    let g = go.clone();
    open.connect_clicked(move |_| g());
    let g = go.clone();
    entry.connect_activate(move |_| g());
    if let Some(display) = gtk::gdk::Display::default() {
        let e = entry.clone();
        display.clipboard().read_text_async(None::<&gtk::gio::Cancellable>, move |res| {
            if let Ok(Some(t)) = res {
                if util::parse_github_url(&t, true).is_some() {
                    e.set_text(t.trim());
                }
            }
        });
    }
    sheet.present();
    entry.grab_focus();
}

pub fn about() {
    let d = adw::AboutDialog::builder()
        .application_name(brand::APP_NAME)
        .application_icon(brand::APP_ID)
        .version(brand::VERSION)
        .comments(brand::TAGLINE)
        .website(brand::repo_url())
        .issue_url(format!("{}/issues", brand::repo_url()))
        .license_type(gtk::License::MitX11)
        .developer_name(env!("BRAND_DEVELOPER_NAME"))
        .build();
    d.add_acknowledgement_section(Some("Built with"), &["GTK https://gtk.org", "libadwaita https://gnome.pages.gitlab.gnome.org/libadwaita/", "VTE https://gitlab.gnome.org/GNOME/vte", "WebKitGTK https://webkitgtk.org"]);
    d.add_legal_section(
        "Trademarks",
        None,
        gtk::License::Custom,
        Some(&format!("{} is an independent project and is not affiliated with or endorsed by GitHub, Inc. or Anthropic. GitHub and Claude are trademarks of their respective owners.", brand::APP_NAME)),
    );
    let w = app::main_window();
    d.present(w.as_ref());
}
