use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, glib};

pub struct Composer {
    pub widget: gtk::Overlay,
    pub view: gtk::TextView,
    placeholder: gtk::Label,
    submit: RefCell<Option<Rc<dyn Fn()>>>,
}

impl Composer {
    pub fn new(placeholder: &str, min_height: i32) -> Rc<Self> {
        let view = gtk::TextView::new();
        view.set_wrap_mode(gtk::WrapMode::WordChar);
        view.set_top_margin(6);
        view.set_bottom_margin(6);
        view.set_left_margin(8);
        view.set_right_margin(8);
        view.set_accepts_tab(false);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_child(Some(&view));
        scroll.set_min_content_height(min_height);
        scroll.set_propagate_natural_height(true);
        scroll.set_max_content_height(min_height.max(260));
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.add_css_class("composer-box");
        frame.append(&scroll);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&frame));
        let ph = gtk::Label::new(Some(placeholder));
        ph.add_css_class("sy-dim");
        ph.set_halign(gtk::Align::Start);
        ph.set_valign(gtk::Align::Start);
        ph.set_margin_start(13);
        ph.set_margin_top(10);
        ph.set_can_target(false);
        overlay.add_overlay(&ph);
        let me = Rc::new(Composer { widget: overlay, view, placeholder: ph, submit: RefCell::new(None) });
        let weak = Rc::downgrade(&me);
        me.view.buffer().connect_changed(move |b| {
            if let Some(me) = weak.upgrade() {
                me.placeholder.set_visible(b.char_count() == 0);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(&me);
        keys.connect_key_pressed(move |_, key, _, state| {
            let ctrl = state.contains(gdk::ModifierType::CONTROL_MASK);
            if ctrl && (key == gdk::Key::Return || key == gdk::Key::KP_Enter) {
                if let Some(me) = weak.upgrade() {
                    let f = me.submit.borrow().clone();
                    if let Some(f) = f {
                        f();
                    }
                }
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        me.view.add_controller(keys);
        me
    }

    pub fn on_submit(&self, f: impl Fn() + 'static) {
        *self.submit.borrow_mut() = Some(Rc::new(f));
    }

    pub fn text(&self) -> String {
        let b = self.view.buffer();
        b.text(&b.start_iter(), &b.end_iter(), false).to_string()
    }

    pub fn set_text(&self, s: &str) {
        self.view.buffer().set_text(s);
    }

    pub fn is_blank(&self) -> bool {
        self.text().trim().is_empty()
    }

    pub fn grab_focus(&self) {
        self.view.grab_focus();
    }

    pub fn on_changed(&self, f: impl Fn() + 'static) {
        self.view.buffer().connect_changed(move |_| f());
    }
}
