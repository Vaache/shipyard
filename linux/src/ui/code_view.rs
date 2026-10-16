use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;
use sourceview5::prelude::*;

use crate::api::models::BlameRange;
use crate::blame::{self, Blame};

const BLAME_AUTHOR_CHARS: usize = 14;
const BLAME_DATE_CHARS: usize = 12;

type BlameHandler = Rc<RefCell<Option<Box<dyn Fn(&BlameRange)>>>>;

pub struct CodeView {
    pub widget: gtk::ScrolledWindow,
    pub view: sourceview5::View,
    buffer: sourceview5::Buffer,
    blame: Rc<RefCell<Option<Rc<Blame>>>>,
    blame_renderer: RefCell<Option<sourceview5::GutterRendererText>>,
    blame_inserted: Cell<bool>,
    blame_handler: BlameHandler,
}

fn scheme_id() -> &'static str {
    if crate::theme::is_dark() {
        "Adwaita-dark"
    } else {
        "Adwaita"
    }
}

fn heat_color() -> &'static str {
    crate::theme::token_hex("accent").or_else(|| crate::theme::token_hex("sky")).unwrap_or("#1c71d8")
}

fn blame_markup(b: &Blame, line: u32, label: bool) -> String {
    let Some(r) = b.range_at(line) else {
        return " ".repeat(BLAME_AUTHOR_CHARS + BLAME_DATE_CHARS + 3);
    };
    let stripe = format!("<span foreground=\"{}\" fgalpha=\"{}%\">▌</span> ", heat_color(), (blame::heat(r.age) * 100.0).round() as i64);
    if !label {
        return format!("{stripe}{}", " ".repeat(BLAME_AUTHOR_CHARS + BLAME_DATE_CHARS + 1));
    }
    let author = glib::markup_escape_text(&blame::fit(&r.commit.author_name(), BLAME_AUTHOR_CHARS));
    let date = glib::markup_escape_text(&blame::fit(&crate::util::rel(r.commit.committed_date), BLAME_DATE_CHARS));
    format!("{stripe}{author} <span fgalpha=\"60%\">{date}</span>")
}

fn blame_tooltip(r: &BlameRange) -> String {
    let date = r.commit.committed_date.map(crate::util::full_date).unwrap_or_default();
    let mut text = format!("{}\n{} · {}", r.commit.message_headline, r.commit.abbreviated_oid, r.commit.author_name());
    if !date.is_empty() {
        text.push_str(&format!(" · {date}"));
    }
    text.push_str("\nClick to open the commit");
    text
}

fn line_under(renderer: &sourceview5::GutterRendererText, y: i32) -> u32 {
    let view = renderer.view();
    let (_, by) = view.window_to_buffer_coords(gtk::TextWindowType::Left, 0, y);
    let (iter, _) = view.line_at_y(by);
    iter.line().max(0) as u32
}

impl CodeView {
    pub fn new() -> CodeView {
        let buffer = sourceview5::Buffer::new(None);
        buffer.set_highlight_syntax(true);
        let view = sourceview5::View::with_buffer(&buffer);
        view.set_editable(false);
        view.set_monospace(true);
        view.set_show_line_numbers(true);
        view.set_highlight_current_line(false);
        view.set_tab_width(4);
        view.set_top_margin(8);
        view.set_bottom_margin(8);
        view.set_left_margin(6);
        view.add_css_class("code-view");
        let widget = gtk::ScrolledWindow::new();
        widget.set_child(Some(&view));
        widget.set_vexpand(true);
        widget.set_hexpand(true);
        let me = CodeView {
            widget,
            view,
            buffer,
            blame: Rc::new(RefCell::new(None)),
            blame_renderer: RefCell::new(None),
            blame_inserted: Cell::new(false),
            blame_handler: Rc::new(RefCell::new(None)),
        };
        me.apply_scheme();
        me
    }

    pub fn apply_scheme(&self) {
        if let Some(s) = sourceview5::StyleSchemeManager::default().scheme(scheme_id()) {
            self.buffer.set_style_scheme(Some(&s));
        }
        if let Some(r) = self.blame_renderer.borrow().as_ref() {
            r.queue_draw();
        }
    }

    pub fn set_text(&self, text: &str, filename: &str) {
        let lang = sourceview5::LanguageManager::default().guess_language(Some(filename), None);
        self.buffer.set_language(lang.as_ref());
        let shown = if text.len() > 2_000_000 { &text[..text.char_indices().take_while(|(i, _)| *i < 2_000_000).last().map(|(i, _)| i).unwrap_or(0)] } else { text };
        self.buffer.set_text(shown);
        self.buffer.place_cursor(&self.buffer.start_iter());
        self.apply_scheme();
    }

    pub fn set_wrap(&self, wrap: bool) {
        self.view.set_wrap_mode(if wrap { gtk::WrapMode::WordChar } else { gtk::WrapMode::None });
    }

    pub fn on_blame_activate(&self, f: impl Fn(&BlameRange) + 'static) {
        *self.blame_handler.borrow_mut() = Some(Box::new(f));
    }

    pub fn set_blame(&self, blame: Option<Rc<Blame>>) {
        let same = match (self.blame.borrow().as_ref(), blame.as_ref()) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        let show = blame.is_some();
        *self.blame.borrow_mut() = blame;
        if show {
            let r = self.blame_renderer();
            if !self.blame_inserted.get() {
                sourceview5::prelude::ViewExt::gutter(&self.view, gtk::TextWindowType::Left).insert(&r, -40);
                self.blame_inserted.set(true);
                self.size_blame(&r);
            }
            if !same {
                r.queue_draw();
            }
        } else if self.blame_inserted.get() {
            if let Some(r) = self.blame_renderer.borrow().as_ref() {
                sourceview5::prelude::ViewExt::gutter(&self.view, gtk::TextWindowType::Left).remove(r);
            }
            self.blame_inserted.set(false);
        }
    }

    fn size_blame(&self, r: &sourceview5::GutterRendererText) {
        let sample = format!("▌ {} <span>{}</span>", "M".repeat(BLAME_AUTHOR_CHARS), "M".repeat(BLAME_DATE_CHARS));
        let (w, _) = r.measure_markup(&sample);
        r.set_width_request(w + r.xpad() * 2 + 2);
    }

    fn blame_renderer(&self) -> sourceview5::GutterRendererText {
        if let Some(r) = self.blame_renderer.borrow().as_ref() {
            return r.clone();
        }
        let r = sourceview5::GutterRendererText::new();
        r.add_css_class("blame-gutter");
        r.set_xpad(8);
        r.set_xalign(0.0);
        r.set_alignment_mode(sourceview5::GutterRendererAlignmentMode::First);
        r.set_cursor_from_name(Some("pointer"));
        let data = self.blame.clone();
        r.connect_query_data(move |r, lines, line| {
            let Some(b) = data.borrow().clone() else {
                r.set_text("");
                return;
            };
            let first = lines.downcast_ref::<sourceview5::GutterLines>().map(|l| l.first()).unwrap_or(0);
            r.set_markup(&blame_markup(&b, line, line == first || b.starts_range(line)));
        });
        let data = self.blame.clone();
        r.connect_query_activatable(move |_, _, _| data.borrow().is_some());
        let data = self.blame.clone();
        let handler = self.blame_handler.clone();
        r.connect_activate(move |_, iter, _, button, _, _| {
            if button != 1 {
                return;
            }
            let Some(b) = data.borrow().clone() else { return };
            let Some(range) = b.range_at(iter.line().max(0) as u32) else { return };
            if let Some(f) = handler.borrow().as_ref() {
                f(range);
            }
        });
        r.set_has_tooltip(true);
        let data = self.blame.clone();
        r.connect_query_tooltip(move |r, _, y, _, tip| {
            let Some(b) = data.borrow().clone() else { return false };
            let Some(range) = b.range_at(line_under(r, y)) else { return false };
            tip.set_text(Some(&blame_tooltip(range)));
            true
        });
        *self.blame_renderer.borrow_mut() = Some(r.clone());
        r
    }
}

impl Default for CodeView {
    fn default() -> Self {
        Self::new()
    }
}
