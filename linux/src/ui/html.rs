use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use webkit6::prelude::*;

pub struct HtmlView {
    pub view: webkit6::WebView,
    html: RefCell<String>,
    restore_y: Cell<Option<f64>>,
    scroll_bottom: Cell<bool>,
    on_action: RefCell<Option<Rc<dyn Fn(String, String)>>>,
    on_link: RefCell<Option<Rc<dyn Fn(String)>>>,
}

impl HtmlView {
    pub fn new() -> Rc<Self> {
        let ucm = webkit6::UserContentManager::new();
        ucm.register_script_message_handler("app", None);
        let view = webkit6::WebView::builder().user_content_manager(&ucm).build();
        view.set_vexpand(true);
        view.set_hexpand(true);
        view.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, 0.0));
        if let Some(settings) = webkit6::prelude::WebViewExt::settings(&view) {
            settings.set_enable_developer_extras(crate::brand::env_var("DEBUG").is_some());
            settings.set_enable_back_forward_navigation_gestures(false);
        }
        let me = Rc::new(HtmlView {
            view,
            html: RefCell::new(String::new()),
            restore_y: Cell::new(None),
            scroll_bottom: Cell::new(false),
            on_action: RefCell::new(None),
            on_link: RefCell::new(None),
        });

        let weak = Rc::downgrade(&me);
        ucm.connect_script_message_received(Some("app"), move |_, value| {
            let Some(me) = weak.upgrade() else { return };
            let text = value.to_str().to_string();
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return };
            let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if !matches!(t.as_str(), "reply" | "resolve" | "unresolve" | "file") {
                return;
            }
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let f = me.on_action.borrow().clone();
            if let Some(f) = f {
                f(t, id);
            }
        });

        let weak = Rc::downgrade(&me);
        me.view.connect_decide_policy(move |_, decision, kind| {
            let Some(me) = weak.upgrade() else { return false };
            if kind == webkit6::PolicyDecisionType::NavigationAction || kind == webkit6::PolicyDecisionType::NewWindowAction {
                if let Some(nav) = decision.downcast_ref::<webkit6::NavigationPolicyDecision>() {
                    if let Some(action) = nav.navigation_action() {
                        let ty = action.navigation_type();
                        let uri = action.request().and_then(|r| r.uri()).map(|u| u.to_string()).unwrap_or_default();
                        if ty == webkit6::NavigationType::Reload || ty == webkit6::NavigationType::BackForward {
                            decision.ignore();
                            return true;
                        }
                        if ty == webkit6::NavigationType::LinkClicked || kind == webkit6::PolicyDecisionType::NewWindowAction {
                            decision.ignore();
                            let f = me.on_link.borrow().clone();
                            match f {
                                Some(f) => f(uri),
                                None => crate::util::open_url(&uri),
                            }
                            return true;
                        }
                    }
                }
            }
            false
        });

        let weak = Rc::downgrade(&me);
        me.view.connect_load_changed(move |view, event| {
            let Some(me) = weak.upgrade() else { return };
            if event != webkit6::LoadEvent::Finished {
                return;
            }
            if me.scroll_bottom.replace(false) {
                view.evaluate_javascript("window.scrollTo(0, document.body.scrollHeight)", None, None, None::<&gtk::gio::Cancellable>, |_| {});
            } else if let Some(y) = me.restore_y.take() {
                view.evaluate_javascript(&format!("window.scrollTo(0, {y})"), None, None, None::<&gtk::gio::Cancellable>, |_| {});
            }
        });
        me
    }

    pub fn on_action(&self, f: impl Fn(String, String) + 'static) {
        *self.on_action.borrow_mut() = Some(Rc::new(f));
    }

    pub fn on_link(&self, f: impl Fn(String) + 'static) {
        *self.on_link.borrow_mut() = Some(Rc::new(f));
    }

    pub fn set_html(self: &Rc<Self>, html: &str) {
        if *self.html.borrow() == html {
            return;
        }
        let first = self.html.borrow().is_empty();
        *self.html.borrow_mut() = html.to_string();
        if first {
            self.view.load_html(html, Some("https://github.com/"));
            return;
        }
        let weak = Rc::downgrade(self);
        self.view.evaluate_javascript("window.scrollY", None, None, None::<&gtk::gio::Cancellable>, move |res| {
            let Some(me) = weak.upgrade() else { return };
            if let Ok(v) = res {
                me.restore_y.set(Some(v.to_double()));
            }
            let html = me.html.borrow().clone();
            me.view.load_html(&html, Some("https://github.com/"));
        });
    }

    pub fn scroll_to_bottom_after_load(&self) {
        self.scroll_bottom.set(true);
    }
}
