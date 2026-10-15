use adw::prelude::*;

use super::widgets::*;
use crate::brand;

pub fn app_icon(size: i32) -> gtk::Image {
    let i = gtk::Image::from_icon_name(brand::APP_ID);
    i.set_pixel_size(size);
    i
}

pub fn build() -> gtk::Widget {
    let b = vbox(20);
    b.set_valign(gtk::Align::Center);
    b.set_halign(gtk::Align::Center);
    b.set_vexpand(true);
    b.append(&app_icon(136));
    let texts = vbox(6);
    let title = label_with(brand::APP_NAME, &["splash-title", "sy-text"]);
    title.set_xalign(0.5);
    let sub = label_with(brand::TAGLINE, &["sy-muted"]);
    sub.set_xalign(0.5);
    texts.append(&title);
    texts.append(&sub);
    b.append(&texts);
    let s = spinner(16);
    s.set_halign(gtk::Align::Center);
    s.set_opacity(0.7);
    b.append(&s);
    let w = vbox(0);
    w.add_css_class("sy-panel-pane");
    w.append(&b);
    w.upcast()
}
