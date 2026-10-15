use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::widgets::*;

pub struct TabItem {
    pub id: &'static str,
    pub title: String,
    pub icon: &'static str,
}

pub struct TabStrip {
    pub widget: gtk::Box,
    buttons: Vec<(&'static str, gtk::Button, gtk::Label)>,
    active: RefCell<&'static str>,
    listeners: RefCell<Vec<Rc<dyn Fn(&'static str)>>>,
}

impl TabStrip {
    pub fn new(items: Vec<TabItem>) -> Rc<Self> {
        let widget = hbox(2);
        widget.add_css_class("tab-strip");
        let mut buttons = vec![];
        for item in &items {
            let b = gtk::Button::new();
            let content = hbox(6);
            content.append(&icon(item.icon, 14));
            content.append(&gtk::Label::new(Some(&item.title)));
            let badge = gtk::Label::new(None);
            badge.add_css_class("tab-badge");
            badge.set_visible(false);
            content.append(&badge);
            b.set_child(Some(&content));
            widget.append(&b);
            buttons.push((item.id, b, badge));
        }
        let first = items.first().map(|i| i.id).unwrap_or("");
        let me = Rc::new(TabStrip { widget, buttons, active: RefCell::new(first), listeners: RefCell::new(vec![]) });
        for (id, b, _) in &me.buttons {
            let weak = Rc::downgrade(&me);
            let id = *id;
            b.connect_clicked(move |_| {
                if let Some(me) = weak.upgrade() {
                    me.select(id);
                }
            });
        }
        me.refresh_classes();
        me
    }

    fn refresh_classes(&self) {
        let active = *self.active.borrow();
        for (id, b, _) in &self.buttons {
            if *id == active {
                b.add_css_class("active");
            } else {
                b.remove_css_class("active");
            }
        }
    }

    pub fn active(&self) -> &'static str {
        *self.active.borrow()
    }

    pub fn select(&self, id: &'static str) {
        if !self.buttons.iter().any(|(i, _, _)| *i == id) {
            return;
        }
        *self.active.borrow_mut() = id;
        self.refresh_classes();
        let listeners = self.listeners.borrow().clone();
        for l in listeners {
            l(id);
        }
    }

    pub fn on_change(&self, f: impl Fn(&'static str) + 'static) {
        self.listeners.borrow_mut().push(Rc::new(f));
    }

    pub fn set_badge(&self, id: &str, text: Option<&str>, color: Option<&str>) {
        for (i, _, badge) in &self.buttons {
            if *i == id {
                for c in crate::theme::ACCENTS {
                    badge.remove_css_class(&format!("fg-{c}"));
                }
                match text {
                    Some(t) => {
                        badge.set_text(t);
                        badge.set_visible(true);
                        if let Some(c) = color {
                            badge.add_css_class(&format!("fg-{c}"));
                        }
                    }
                    None => badge.set_visible(false),
                }
            }
        }
    }
}
