use adw::prelude::*;
use sourceview5::prelude::*;

pub struct CodeView {
    pub widget: gtk::ScrolledWindow,
    pub view: sourceview5::View,
    buffer: sourceview5::Buffer,
}

fn scheme_id() -> &'static str {
    if crate::theme::is_dark() {
        "Adwaita-dark"
    } else {
        "Adwaita"
    }
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
        let me = CodeView { widget, view, buffer };
        me.apply_scheme();
        me
    }

    pub fn apply_scheme(&self) {
        if let Some(s) = sourceview5::StyleSchemeManager::default().scheme(scheme_id()) {
            self.buffer.set_style_scheme(Some(&s));
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
}

impl Default for CodeView {
    fn default() -> Self {
        Self::new()
    }
}
