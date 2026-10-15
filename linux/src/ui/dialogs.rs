use adw::prelude::*;

use super::widgets::*;

pub struct Sheet {
    pub dialog: adw::Dialog,
    pub body: gtk::Box,
    pub header: adw::HeaderBar,
    pub footer: gtk::Box,
}

impl Sheet {
    pub fn new(title: &str, width: i32, height: i32) -> Sheet {
        let dialog = adw::Dialog::new();
        dialog.set_title(title);
        dialog.set_content_width(width);
        if height > 0 {
            dialog.set_content_height(height);
        }
        let tv = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        tv.add_top_bar(&header);
        let body = vbox(12);
        pad(&body, 18);
        let scroll = gtk::ScrolledWindow::new();
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        scroll.set_propagate_natural_height(true);
        scroll.set_child(Some(&body));
        scroll.set_vexpand(true);
        tv.set_content(Some(&scroll));
        let footer = hbox(8);
        margins(&footer, 6, 18, 14, 18);
        tv.add_bottom_bar(&footer);
        dialog.set_child(Some(&tv));
        Sheet { dialog, body, header, footer }
    }

    pub fn present(&self) {
        let w = crate::app::main_window();
        self.dialog.present(w.as_ref());
    }

    pub fn close(&self) {
        self.dialog.close();
    }

    pub fn add_footer_spacer(&self) {
        self.footer.append(&spacer());
    }

    pub fn cancel_button(&self) -> gtk::Button {
        let b = button("Cancel");
        let d = self.dialog.clone();
        b.connect_clicked(move |_| {
            d.close();
        });
        self.footer.append(&b);
        b
    }
}

pub fn confirm(heading: &str, body: Option<&str>, confirm_label: &str, destructive: bool, on_confirm: impl Fn() + 'static) {
    let d = adw::AlertDialog::new(Some(heading), body);
    d.add_response("cancel", "Cancel");
    d.add_response("ok", confirm_label);
    d.set_response_appearance("ok", if destructive { adw::ResponseAppearance::Destructive } else { adw::ResponseAppearance::Suggested });
    d.set_default_response(Some("ok"));
    d.set_close_response("cancel");
    d.connect_response(None, move |_, r| {
        if r == "ok" {
            on_confirm();
        }
    });
    let w = crate::app::main_window();
    d.present(w.as_ref());
}

pub fn error(message: &str) {
    let d = adw::AlertDialog::new(Some("Error"), Some(message));
    d.add_response("ok", "OK");
    d.set_default_response(Some("ok"));
    d.set_close_response("ok");
    let w = crate::app::main_window();
    d.present(w.as_ref());
}

pub fn info(heading: &str, message: &str) {
    let d = adw::AlertDialog::new(Some(heading), Some(message));
    d.add_response("ok", "OK");
    d.set_close_response("ok");
    let w = crate::app::main_window();
    d.present(w.as_ref());
}

pub fn choose_folder(title: &str, on_pick: impl Fn(String) + 'static) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title(title);
    dialog.set_modal(true);
    let w = crate::app::main_window();
    dialog.select_folder(w.as_ref(), None::<&gtk::gio::Cancellable>, move |res| {
        if let Ok(file) = res {
            if let Some(p) = file.path() {
                on_pick(p.to_string_lossy().to_string());
            }
        }
    });
}

pub fn choose_images(on_pick: impl Fn(Vec<std::path::PathBuf>) + 'static) {
    let dialog = gtk::FileDialog::new();
    dialog.set_title("Attach images");
    let filter = gtk::FileFilter::new();
    filter.set_name(Some("Images"));
    for m in ["image/png", "image/jpeg", "image/gif", "image/webp"] {
        filter.add_mime_type(m);
    }
    let filters = gtk::gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    dialog.set_filters(Some(&filters));
    let w = crate::app::main_window();
    dialog.open_multiple(w.as_ref(), None::<&gtk::gio::Cancellable>, move |res| {
        if let Ok(list) = res {
            let mut paths = vec![];
            for i in 0..list.n_items() {
                if let Some(f) = list.item(i).and_downcast::<gtk::gio::File>() {
                    if let Some(p) = f.path() {
                        paths.push(p);
                    }
                }
            }
            on_pick(paths);
        }
    });
}
