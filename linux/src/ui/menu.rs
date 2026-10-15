use std::cell::Cell;

use adw::prelude::*;
use gtk::{gio, glib};

thread_local! {
    static COUNTER: Cell<u64> = const { Cell::new(0) };
}

fn next_prefix() -> String {
    COUNTER.with(|c| {
        let v = c.get() + 1;
        c.set(v);
        format!("m{v}")
    })
}

pub struct Menu {
    pub model: gio::Menu,
    pub group: gio::SimpleActionGroup,
    prefix: String,
    count: Cell<u32>,
    current: gio::Menu,
}

impl Default for Menu {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu {
    pub fn new() -> Self {
        let model = gio::Menu::new();
        let current = gio::Menu::new();
        model.append_section(None, &current);
        Menu { model, group: gio::SimpleActionGroup::new(), prefix: next_prefix(), count: Cell::new(0), current }
    }

    pub fn ctx() -> Self {
        let model = gio::Menu::new();
        let current = gio::Menu::new();
        model.append_section(None, &current);
        Menu { model, group: gio::SimpleActionGroup::new(), prefix: "ctx".into(), count: Cell::new(0), current }
    }

    fn action_name(&self) -> String {
        let n = self.count.get() + 1;
        self.count.set(n);
        format!("a{n}")
    }

    pub fn item(&self, label: &str, f: impl Fn() + 'static) -> &Self {
        let name = self.action_name();
        let action = gio::SimpleAction::new(&name, None);
        action.connect_activate(move |_, _| f());
        self.group.add_action(&action);
        self.current.append(Some(label), Some(&format!("{}.{name}", self.prefix)));
        self
    }

    pub fn disabled(&self, label: &str) -> &Self {
        let name = self.action_name();
        let action = gio::SimpleAction::new(&name, None);
        action.set_enabled(false);
        self.group.add_action(&action);
        self.current.append(Some(label), Some(&format!("{}.{name}", self.prefix)));
        self
    }

    pub fn section(&mut self, title: Option<&str>) -> &mut Self {
        let s = gio::Menu::new();
        self.model.append_section(title, &s);
        self.current = s;
        self
    }

    pub fn submenu(&self, label: &str, build: impl FnOnce(&Menu)) -> &Self {
        let sub = Menu { model: gio::Menu::new(), group: self.group.clone(), prefix: self.prefix.clone(), count: Cell::new(self.count.get() + 1000), current: gio::Menu::new() };
        sub.model.append_section(None, &sub.current);
        build(&sub);
        self.count.set(sub.count.get());
        self.current.append_submenu(Some(label), &sub.model);
        self
    }

    pub fn attach<W: IsA<gtk::Widget>>(&self, widget: &W) {
        widget.insert_action_group(&self.prefix, Some(&self.group));
    }

    pub fn button(&self, icon_name: &str, tooltip: &str) -> gtk::MenuButton {
        let b = gtk::MenuButton::new();
        b.set_icon_name(icon_name);
        b.set_tooltip_text(Some(tooltip));
        b.set_menu_model(Some(&self.model));
        self.attach(&b);
        b
    }

    pub fn apply_to(&self, b: &gtk::MenuButton) {
        b.set_menu_model(Some(&self.model));
        self.attach(b);
    }
}

pub fn context_menu<W: IsA<gtk::Widget>>(widget: &W, build: impl Fn() -> Menu + 'static) {
    let gesture = gtk::GestureClick::new();
    gesture.set_button(3);
    let w = widget.clone().upcast::<gtk::Widget>();
    gesture.connect_pressed(move |g, _, x, y| {
        g.set_state(gtk::EventSequenceState::Claimed);
        let menu = build();
        let popover = gtk::PopoverMenu::from_model(Some(&menu.model));
        popover.set_parent(&w);
        menu.attach(&w);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.connect_closed(|p| {
            let p = p.clone();
            glib::idle_add_local_once(move || p.unparent());
        });
        popover.popup();
    });
    widget.add_controller(gesture);
}

pub fn dropdown_button(label: &str, icon_name: Option<&str>, menu: &Menu) -> gtk::MenuButton {
    let b = gtk::MenuButton::new();
    if let Some(i) = icon_name {
        let content = adw::ButtonContent::new();
        content.set_icon_name(i);
        content.set_label(label);
        b.set_child(Some(&content));
    } else {
        b.set_label(label);
    }
    menu.apply_to(&b);
    b
}
