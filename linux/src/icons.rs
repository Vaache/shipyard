use std::path::PathBuf;

const RING_SMALL_TOP: &str = "M2,3.5a2,2 0 1,0 4,0a2,2 0 1,0 -4,0ZM3,3.5a1,1 0 1,1 2,0a1,1 0 1,1 -2,0Z";
const RING_SMALL_BOTTOM: &str = "M2,12.5a2,2 0 1,0 4,0a2,2 0 1,0 -4,0ZM3,12.5a1,1 0 1,1 2,0a1,1 0 1,1 -2,0Z";
const RING_SMALL_RB: &str = "M10,12.5a2,2 0 1,0 4,0a2,2 0 1,0 -4,0ZM11,12.5a1,1 0 1,1 2,0a1,1 0 1,1 -2,0Z";
const RING_BIG: &str = "M1,8a7,7 0 1,0 14,0a7,7 0 1,0 -14,0ZM2.5,8a5.5,5.5 0 1,1 11,0a5.5,5.5 0 1,1 -11,0Z";

fn svg(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| p.to_string()).collect()
}

fn svg_document(paths: &[String]) -> String {
    let mut body = String::new();
    for p in paths {
        body.push_str(&format!("<path fill=\"#2e3436\" fill-rule=\"evenodd\" d=\"{p}\"/>"));
    }
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" viewBox=\"0 0 16 16\">{body}</svg>")
}

mod imp {
    use std::cell::RefCell;

    use gtk::prelude::*;
    use gtk::subclass::prelude::*;
    use gtk::{gdk, glib, gsk};

    #[derive(Default)]
    pub struct GlyphPaintable {
        pub paths: RefCell<Vec<gsk::Path>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GlyphPaintable {
        const NAME: &'static str = "PyGlyphPaintable";
        type Type = super::GlyphPaintable;
        type Interfaces = (gdk::Paintable, gtk::SymbolicPaintable);
    }

    impl ObjectImpl for GlyphPaintable {}

    impl GlyphPaintable {
        fn draw(&self, snapshot: &gdk::Snapshot, width: f64, height: f64, color: &gdk::RGBA) {
            let Some(snapshot) = snapshot.downcast_ref::<gtk::Snapshot>() else { return };
            snapshot.save();
            snapshot.scale(width as f32 / 16.0, height as f32 / 16.0);
            for path in self.paths.borrow().iter() {
                snapshot.append_fill(path, gsk::FillRule::EvenOdd, color);
            }
            snapshot.restore();
        }
    }

    impl PaintableImpl for GlyphPaintable {
        fn intrinsic_width(&self) -> i32 {
            16
        }

        fn intrinsic_height(&self) -> i32 {
            16
        }

        fn flags(&self) -> gdk::PaintableFlags {
            gdk::PaintableFlags::STATIC_SIZE | gdk::PaintableFlags::STATIC_CONTENTS
        }

        fn snapshot(&self, snapshot: &gdk::Snapshot, width: f64, height: f64) {
            self.draw(snapshot, width, height, &gdk::RGBA::new(0.5, 0.5, 0.5, 1.0));
        }
    }

    impl SymbolicPaintableImpl for GlyphPaintable {
        fn snapshot_symbolic(&self, snapshot: &gdk::Snapshot, width: f64, height: f64, colors: &[gdk::RGBA]) {
            let color = colors.first().copied().unwrap_or(gdk::RGBA::new(0.5, 0.5, 0.5, 1.0));
            self.draw(snapshot, width, height, &color);
        }
    }
}

gtk::glib::wrapper! {
    pub struct GlyphPaintable(ObjectSubclass<imp::GlyphPaintable>) @implements gtk::gdk::Paintable, gtk::SymbolicPaintable;
}

thread_local! {
    static GLYPHS: std::cell::RefCell<Option<std::collections::HashMap<&'static str, Vec<gtk::gsk::Path>>>> = const { std::cell::RefCell::new(None) };
}

fn glyph_paths(name: &str) -> Option<Vec<gtk::gsk::Path>> {
    let key = name.strip_suffix("-symbolic").unwrap_or(name);
    GLYPHS.with(|g| {
        let mut g = g.borrow_mut();
        let map = g.get_or_insert_with(|| {
            icons()
                .into_iter()
                .filter_map(|(n, paths)| {
                    let parsed: Option<Vec<gtk::gsk::Path>> = paths.iter().map(|p| gtk::gsk::Path::parse(p).ok()).collect();
                    parsed.map(|p| (n, p))
                })
                .collect()
        });
        map.get(key).cloned()
    })
}

pub fn glyph(name: &str) -> Option<GlyphPaintable> {
    use gtk::subclass::prelude::ObjectSubclassIsExt;
    let paths = glyph_paths(name)?;
    let p: GlyphPaintable = gtk::glib::Object::new();
    *p.imp().paths.borrow_mut() = paths;
    Some(p)
}

pub fn image(name: &str, size: i32) -> gtk::Image {
    use gtk::prelude::*;
    let i = match glyph(name) {
        Some(p) => gtk::Image::from_paintable(Some(&p)),
        None => gtk::Image::from_icon_name(name),
    };
    i.set_pixel_size(size);
    i
}

pub fn set_button_icon(button: &impl gtk::prelude::IsA<gtk::Button>, name: &str) {
    use gtk::prelude::*;
    let button = button.as_ref();
    if glyph(name).is_some() {
        button.set_child(Some(&image(name, 16)));
        button.add_css_class("image-button");
    } else {
        button.set_icon_name(name);
    }
}

fn icons() -> Vec<(&'static str, Vec<String>)> {
    let left_trunk = "M3.25,5.5h1.5v5h-1.5Z";
    vec![
        ("sy-pr", svg(&[RING_SMALL_TOP, left_trunk, RING_SMALL_BOTTOM, RING_SMALL_RB, "M11.25,2.75h1.5v7.75h-1.5Z", "M8.5,2.75h3v1.5h-3Z", "M8.5,1L5.75,3.5L8.5,6Z"])),
        ("sy-pr-draft", svg(&[RING_SMALL_TOP, left_trunk, RING_SMALL_BOTTOM, RING_SMALL_RB, "M11.25,2.5h1.5v1.75h-1.5Z", "M11.25,5.75h1.5v1.75h-1.5Z", "M11.25,9h1.5v1.5h-1.5Z"])),
        (
            "sy-pr-closed",
            svg(&[RING_SMALL_TOP, left_trunk, RING_SMALL_BOTTOM, RING_SMALL_RB, "M9.94,1.15L12,3.21L14.06,1.15L14.85,1.94L12.79,4L14.85,6.06L14.06,6.85L12,4.79L9.94,6.85L9.15,6.06L11.21,4L9.15,1.94Z"]),
        ),
        ("sy-merge", svg(&[RING_SMALL_TOP, left_trunk, RING_SMALL_BOTTOM, "M10,8a2,2 0 1,0 4,0a2,2 0 1,0 -4,0ZM11,8a1,1 0 1,1 2,0a1,1 0 1,1 -2,0Z", "M4.75,5.5C4.75,7.2 6.5,7.25 10,7.25L10,8.75C6,8.75 3.25,8.5 3.25,5.5Z"])),
        ("sy-branch", svg(&[RING_SMALL_TOP, "M3.25,5.5h1.5v5h-1.5Z", RING_SMALL_BOTTOM, "M10,3.5a2,2 0 1,0 4,0a2,2 0 1,0 -4,0ZM11,3.5a1,1 0 1,1 2,0a1,1 0 1,1 -2,0Z", "M11.25,5.5L12.75,5.5C12.75,9 9,10.25 4.75,10.25L4.75,8.75C8.5,8.75 11.25,8 11.25,5.5Z"])),
        ("sy-issue", svg(&[RING_BIG, "M6.25,8a1.75,1.75 0 1,0 3.5,0a1.75,1.75 0 1,0 -3.5,0Z"])),
        ("sy-issue-done", svg(&[RING_BIG, "M5,8.2L6.1,7.1L7.3,8.3L10.2,5.4L11.3,6.5L7.3,10.5Z"])),
        ("sy-issue-skip", svg(&[RING_BIG, "M4.5,10.4L10.4,4.5L11.5,5.6L5.6,11.5Z"])),
        ("sy-comment", svg(&["M2.5,2h11a1.5,1.5 0 0,1 1.5,1.5v7a1.5,1.5 0 0,1 -1.5,1.5h-6l-3,3v-3h-2a1.5,1.5 0 0,1 -1.5,-1.5v-7a1.5,1.5 0 0,1 1.5,-1.5Z"])),
        ("sy-sparkles", svg(&["M7,1Q7.8,6.2 13,7Q7.8,7.8 7,13Q6.2,7.8 1,7Q6.2,6.2 7,1Z", "M13,10Q13.3,12.2 15.5,12.5Q13.3,12.8 13,15Q12.7,12.8 10.5,12.5Q12.7,12.2 13,10Z"])),
        ("sy-repo", svg(&["M3.5,1h9a1.5,1.5 0 0,1 1.5,1.5v11a1.5,1.5 0 0,1 -1.5,1.5h-9a1.5,1.5 0 0,1 -1.5,-1.5v-11a1.5,1.5 0 0,1 1.5,-1.5ZM5,2.5v8h7.5v-8Z", "M5,12h7.5v1.5h-7.5Z"])),
        ("sy-project", svg(&["M2,2h3a1,1 0 0,1 1,1v10a1,1 0 0,1 -1,1h-3a1,1 0 0,1 -1,-1v-10a1,1 0 0,1 1,-1Z", "M7,2h3a1,1 0 0,1 1,1v6a1,1 0 0,1 -1,1h-3a1,1 0 0,1 -1,-1v-6a1,1 0 0,1 1,-1Z", "M12,2h2a1,1 0 0,1 1,1v8a1,1 0 0,1 -1,1h-2a1,1 0 0,1 -1,-1v-8a1,1 0 0,1 1,-1Z"])),
        ("sy-bell", svg(&["M8,1.5a1,1 0 0,1 1,1v.6a4.5,4.5 0 0,1 3.5,4.4v3l1.5,2v1h-12v-1l1.5,-2v-3a4.5,4.5 0 0,1 3.5,-4.4v-.6a1,1 0 0,1 1,-1Z", "M6.25,14h3.5a1.75,1.75 0 0,1 -3.5,0Z"])),
        ("sy-eye", svg(&["M1,8C3,4 5.5,3 8,3C10.5,3 13,4 15,8C13,12 10.5,13 8,13C5.5,13 3,12 1,8ZM2.8,8C4.5,10.8 6.2,11.5 8,11.5C9.8,11.5 11.5,10.8 13.2,8C11.5,5.2 9.8,4.5 8,4.5C6.2,4.5 4.5,5.2 2.8,8Z", "M5.75,8a2.25,2.25 0 1,0 4.5,0a2.25,2.25 0 1,0 -4.5,0Z"])),
        ("sy-at", svg(&["M5,8a3,3 0 1,0 6,0a3,3 0 1,0 -6,0ZM6.5,8a1.5,1.5 0 1,1 3,0a1.5,1.5 0 1,1 -3,0Z", "M8,1a7,7 0 1,0 3.5,13.06l-.75,-1.3a5.5,5.5 0 1,1 2.75,-4.76v1a1,1 0 0,1 -2,0v-1h-1.5v1a2.5,2.5 0 0,0 5,0v-1a7,7 0 0,0 -7,-7Z"])),
        ("sy-person", svg(&["M5,5a3,3 0 1,0 6,0a3,3 0 1,0 -6,0Z", "M2.5,14.5C2.5,11 5,9.5 8,9.5C11,9.5 13.5,11 13.5,14.5Z"])),
        ("sy-person-add", svg(&["M3.5,5a2.5,2.5 0 1,0 5,0a2.5,2.5 0 1,0 -5,0Z", "M1,14.5C1,11.25 3.25,9.75 6,9.75C8.75,9.75 11,11.25 11,14.5Z", "M12.25,3h1.5v2.25h2.25v1.5h-2.25v2.25h-1.5v-2.25h-2.25v-1.5h2.25Z"])),
        ("sy-stack", svg(&["M8,1L15,4.5L8,8L1,4.5Z", "M1,8L2.5,7.25L8,10L13.5,7.25L15,8L8,11.5Z", "M1,11.5L2.5,10.75L8,13.5L13.5,10.75L15,11.5L8,15Z"])),
        ("sy-external", svg(&["M2,3h5v1.5h-3.5v8h8v-3.5h1.5v5h-11Z", "M9,1.5h5.5v5.5l-2,-2l-3.75,3.75l-1.25,-1.25l3.75,-3.75Z"])),
        ("sy-check", svg(&["M2.2,8.4L3.6,7L6.3,9.7L12.4,3.6L13.8,5L6.3,12.5Z"])),
        ("sy-check-circle", svg(&["M1,8a7,7 0 1,0 14,0a7,7 0 1,0 -14,0ZM4.4,8.2L5.5,7.1L7,8.6L10.5,5.1L11.6,6.2L7,10.8Z"])),
        ("sy-x-circle", svg(&["M1,8a7,7 0 1,0 14,0a7,7 0 1,0 -14,0ZM5.2,6.3L6.3,5.2L8,6.9L9.7,5.2L10.8,6.3L9.1,8L10.8,9.7L9.7,10.8L8,9.1L6.3,10.8L5.2,9.7L6.9,8Z"])),
        ("sy-x", svg(&["M3.4,2.3L8,6.9L12.6,2.3L13.7,3.4L9.1,8L13.7,12.6L12.6,13.7L8,9.1L3.4,13.7L2.3,12.6L6.9,8L2.3,3.4Z"])),
        ("sy-dot", svg(&["M4,8a4,4 0 1,0 8,0a4,4 0 1,0 -8,0Z"])),
        ("sy-clock", svg(&[RING_BIG, "M7.25,4h1.5v4.25h3v1.5h-4.5Z"])),
        ("sy-lock", svg(&["M5,7v-2a3,3 0 0,1 6,0v2h-1.5v-2a1.5,1.5 0 0,0 -3,0v2Z", "M4.5,7h7a1.5,1.5 0 0,1 1.5,1.5v5a1.5,1.5 0 0,1 -1.5,1.5h-7a1.5,1.5 0 0,1 -1.5,-1.5v-5a1.5,1.5 0 0,1 1.5,-1.5Z"])),
        ("sy-star", svg(&["M8,1.5L9.65,6.23L14.66,6.34L10.66,9.37L12.11,14.16L8,11.3L3.89,14.16L5.34,9.37L1.34,6.34L6.35,6.23Z"])),
        ("sy-commit", svg(&["M5,8a3,3 0 1,0 6,0a3,3 0 1,0 -6,0ZM6.5,8a1.5,1.5 0 1,1 3,0a1.5,1.5 0 1,1 -3,0Z", "M0.5,7.25h4.5v1.5h-4.5Z", "M11,7.25h4.5v1.5h-4.5Z"])),
        (
            "sy-checklist",
            svg(&[
                "M7,2.75h8v1.5h-8Z",
                "M7,7.25h8v1.5h-8Z",
                "M7,11.75h8v1.5h-8Z",
                "M1,3.6L1.9,2.7L2.9,3.7L4.8,1.8L5.7,2.7L2.9,5.5Z",
                "M1,8.1L1.9,7.2L2.9,8.2L4.8,6.3L5.7,7.2L2.9,10Z",
                "M1,12.6L1.9,11.7L2.9,12.7L4.8,10.8L5.7,11.7L2.9,14.5Z",
            ]),
        ),
        ("sy-file", svg(&["M3,1h6.5l3.5,3.5v10.5h-10ZM4.5,2.5v11h7v-7.5h-3v-3.5Z"])),
        ("sy-code", svg(&["M5.5,3.5L6.6,4.6L3.2,8L6.6,11.4L5.5,12.5L1,8Z", "M10.5,3.5L15,8L10.5,12.5L9.4,11.4L12.8,8L9.4,4.6Z", "M9.2,2L10.6,2.5L6.8,14L5.4,13.5Z"])),
        ("sy-moon", svg(&["M10,1.5A6.5,6.5 0 1,0 14.5,11A5.5,5.5 0 0,1 10,1.5Z"])),
        ("sy-bolt", svg(&["M9.5,1L3,9H7.5L6.5,15L13,7H8.5Z"])),
        ("sy-attention", svg(&[RING_BIG, "M7.25,4h1.5v5h-1.5Z", "M7.1,11.25a.9,.9 0 1,0 1.8,0a.9,.9 0 1,0 -1.8,0Z"])),
        ("sy-memory", svg(&["M4,3h8a1,1 0 0,1 1,1v8a1,1 0 0,1 -1,1h-8a1,1 0 0,1 -1,-1v-8a1,1 0 0,1 1,-1ZM5,5v6h6v-6Z", "M1,5h2v1.25h-2Z", "M1,9.75h2v1.25h-2Z", "M13,5h2v1.25h-2Z", "M13,9.75h2v1.25h-2Z"])),
        ("sy-terminal", svg(&["M2.5,2h11a1.5,1.5 0 0,1 1.5,1.5v9a1.5,1.5 0 0,1 -1.5,1.5h-11a1.5,1.5 0 0,1 -1.5,-1.5v-9a1.5,1.5 0 0,1 1.5,-1.5ZM2.5,3.5v9h11v-9Z", "M4,6L5,5L8,8L5,11L4,10L6,8Z", "M8.5,10h3.5v1.25h-3.5Z"])),
        ("sy-folder", svg(&["M1.5,3h5l1.5,1.5h6.5v9.5h-13Z"])),
        ("sy-tag", svg(&["M1.5,1.5h6l7,7l-6,6l-7,-7ZM3.5,4.75a1.25,1.25 0 1,0 2.5,0a1.25,1.25 0 1,0 -2.5,0Z"])),
        ("sy-photo", svg(&["M2.5,2.5h11a1.5,1.5 0 0,1 1.5,1.5v8a1.5,1.5 0 0,1 -1.5,1.5h-11a1.5,1.5 0 0,1 -1.5,-1.5v-8a1.5,1.5 0 0,1 1.5,-1.5ZM2.5,4v6.5l3,-3l3,3l2,-2l3,3v-7.5Z", "M10,6a1,1 0 1,0 2,0a1,1 0 1,0 -2,0Z"])),
        ("sy-send", svg(&["M8,2L13.5,7.5L12.4,8.6L8.75,4.95V14H7.25V4.95L3.6,8.6L2.5,7.5Z"])),
        ("sy-stop", svg(&["M4.5,3h7a1.5,1.5 0 0,1 1.5,1.5v7a1.5,1.5 0 0,1 -1.5,1.5h-7a1.5,1.5 0 0,1 -1.5,-1.5v-7a1.5,1.5 0 0,1 1.5,-1.5Z"])),
        ("sy-gauge", svg(&["M1.5,11a6.5,6.5 0 0,1 13,0h-1.5a5,5 0 0,0 -10,0Z", "M7.4,11.3L10.9,5.6L11.9,6.2L8.6,11.9Z"])),
        ("sy-panel-right", svg(&["M2.5,2h11a1.5,1.5 0 0,1 1.5,1.5v9a1.5,1.5 0 0,1 -1.5,1.5h-11a1.5,1.5 0 0,1 -1.5,-1.5v-9a1.5,1.5 0 0,1 1.5,-1.5ZM2.5,3.5v9h11v-9Z", "M9.5,3.5h4v9h-4Z"])),
        ("sy-archive", svg(&["M1,2h14v4h-14Z", "M2,7h12v7h-12ZM6,8.5v1.5h4v-1.5Z"])),
        ("sy-plusminus", svg(&["M7.25,1h1.5v3h3v1.5h-3v3h-1.5v-3h-3v-1.5h3Z", "M4.25,11.5h7.5v1.5h-7.5Z"])),
        ("sy-up", svg(&["M8,4L13,9L11.9,10.1L8,6.2L4.1,10.1L3,9Z"])),
        ("sy-down", svg(&["M8,12L3,7L4.1,5.9L8,9.8L11.9,5.9L13,7Z"])),
        ("sy-key", svg(&["M5,4a4,4 0 1,0 3.6,5.75h1.4l1,1l1,-1l1,1l2,-2l-1.5,-1.5h-5.9A4,4 0 0,0 5,4ZM3.5,8a1.5,1.5 0 1,1 3,0a1.5,1.5 0 1,1 -3,0Z"])),
        ("sy-globe", svg(&[RING_BIG, "M7.25,2h1.5v12h-1.5Z", "M2,7.25h12v1.5h-12Z", "M8,1.5C5.5,4 5.5,12 8,14.5L7,14.5C4.2,12 4.2,4 7,1.5ZM8,1.5C10.5,4 10.5,12 8,14.5L9,14.5C11.8,12 11.8,4 9,1.5Z"])),
        ("sy-download", svg(&["M7.25,1h1.5v8.05l2.65,-2.65l1.1,1.1l-4.5,4.5l-4.5,-4.5l1.1,-1.1l2.65,2.65Z", "M2,13h12v1.5h-12Z"])),
    ]
}

pub fn install() {
    let dir: PathBuf = crate::settings::cache_dir().join("icons").join(crate::brand::VERSION);
    if std::fs::create_dir_all(&dir).is_ok() {
        for (name, paths) in icons() {
            let svg = svg_document(&paths);
            let path = dir.join(format!("{name}-symbolic.svg"));
            let current = std::fs::read_to_string(&path).ok();
            if current.as_deref() != Some(svg.as_str()) {
                let _ = std::fs::write(&path, svg);
            }
        }
    }
    if let Some(display) = gtk::gdk::Display::default() {
        let theme = gtk::IconTheme::for_display(&display);
        theme.add_search_path(&dir);
    }
}
