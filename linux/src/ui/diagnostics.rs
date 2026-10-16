use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use chrono::{DateTime, Local, TimeZone, Utc};
use gtk::{gio, glib};

use super::widgets::*;
use crate::diagnostics::{self, RequestEntry, Snapshot, Stats, TickEntry};
use crate::{app, brand, settings, util};

const RECENT_TICKS: usize = 10;
const SEP: char = '\u{1f}';

thread_local! {
    static OPEN: RefCell<Option<glib::WeakRef<adw::Window>>> = const { RefCell::new(None) };
}

struct Table {
    view: gtk::ColumnView,
    model: gtk::StringList,
    current: RefCell<Vec<String>>,
}

fn encode(cells: &[String]) -> String {
    cells.iter().map(|c| c.replace(SEP, " ")).collect::<Vec<_>>().join(&SEP.to_string())
}

fn splice_plan(old: &[String], new: &[String]) -> Vec<(usize, usize, std::ops::Range<usize>)> {
    if let Some(p) = old.first().and_then(|first| new.iter().position(|r| r == first)) {
        let kept = old.iter().zip(&new[p..]).take_while(|(a, b)| a == b).count();
        return vec![(kept, old.len() - kept, p + kept..new.len()), (0, 0, 0..p)];
    }
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suffix = old.len().min(new.len()) - prefix;
    let suffix = old.iter().rev().zip(new.iter().rev()).take(max_suffix).take_while(|(a, b)| a == b).count();
    vec![(prefix, old.len() - prefix - suffix, prefix..new.len() - suffix)]
}

impl Table {
    fn new(columns: &[(&str, bool, bool)]) -> Table {
        let model = gtk::StringList::new(&[] as &[&str]);
        let view = gtk::ColumnView::new(Some(gtk::NoSelection::new(Some(model.clone()))));
        view.set_show_row_separators(true);
        view.set_show_column_separators(true);
        view.set_reorderable(false);
        for (i, &(title, expand, numeric)) in columns.iter().enumerate() {
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(move |_, obj| {
                let Some(item) = obj.downcast_ref::<gtk::ListItem>() else { return };
                let l = gtk::Label::new(None);
                l.set_xalign(if numeric { 1.0 } else { 0.0 });
                l.set_ellipsize(gtk::pango::EllipsizeMode::End);
                if numeric {
                    l.add_css_class("numeric");
                }
                item.set_child(Some(&l));
            });
            factory.connect_bind(move |_, obj| {
                let Some(item) = obj.downcast_ref::<gtk::ListItem>() else { return };
                let Some(l) = item.child().and_downcast::<gtk::Label>() else { return };
                let row = item.item().and_downcast::<gtk::StringObject>().map(|s| s.string());
                let text = row.as_deref().and_then(|r| r.split(SEP).nth(i)).unwrap_or("");
                l.set_tooltip_text(if expand && !text.is_empty() { Some(text) } else { None });
                l.set_text(text);
            });
            let col = gtk::ColumnViewColumn::new(Some(title), Some(factory));
            col.set_expand(expand);
            col.set_resizable(true);
            view.append_column(&col);
        }
        Table { view, model, current: RefCell::new(vec![]) }
    }

    fn set_rows(&self, rows: Vec<Vec<String>>) {
        let new: Vec<String> = rows.iter().map(|r| encode(r)).collect();
        let old = self.current.replace(vec![]);
        for (at, remove, add) in splice_plan(&old, &new) {
            if remove == 0 && add.is_empty() {
                continue;
            }
            let refs: Vec<&str> = new[add].iter().map(String::as_str).collect();
            self.model.splice(at as u32, remove as u32, &refs);
        }
        *self.current.borrow_mut() = new;
    }

    fn widget(&self) -> gtk::ScrolledWindow {
        scrolled_both(&self.view)
    }
}

struct Stat {
    value: gtk::Label,
    detail: gtk::Label,
}

fn stat(container: &gtk::FlowBox, caption: &str) -> Stat {
    let card = vbox(2);
    card.add_css_class("card");
    pad(&card, 12);
    card.set_size_request(150, -1);
    card.append(&label_with(caption, &["caption", "sy-muted"]));
    let value = label_with("—", &["title-3", "numeric"]);
    card.append(&value);
    let detail = wrap_label("", &["caption", "sy-muted", "numeric"]);
    card.append(&detail);
    container.insert(&card, -1);
    Stat { value, detail }
}

struct Panel {
    window: adw::Window,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    filter: gtk::SearchEntry,
    requests_stat: Stat,
    data_stat: Stat,
    duration_stat: Stat,
    errors_stat: Stat,
    rate_stat: Stat,
    ticks_stat: Stat,
    requests: Table,
    grouped: Table,
    ticks: Table,
    slow: Table,
    seen: Cell<u64>,
}

fn clock(ts: DateTime<Utc>) -> String {
    ts.with_timezone(&Local).format("%H:%M:%S%.3f").to_string()
}

fn reset_clock(reset: i64) -> String {
    Local.timestamp_opt(reset, 0).single().map(|t| t.format("%H:%M:%S").to_string()).unwrap_or_default()
}

fn ms(v: f64) -> String {
    if v >= 100.0 {
        format!("{v:.0} ms")
    } else {
        format!("{v:.1} ms")
    }
}

fn bytes(b: u64) -> String {
    util::byte_count(b as i64)
}

fn status_text(r: &RequestEntry) -> String {
    let base = match (r.status, &r.error) {
        (Some(s), Some(e)) => format!("{s} · {e}"),
        (None, Some(e)) => e.clone(),
        (Some(s), None) => s.to_string(),
        (None, None) => String::new(),
    };
    match &r.note {
        Some(n) => format!("{base} · {n}"),
        None => base,
    }
}

fn rate_text(r: &RequestEntry) -> String {
    match (r.rate_limit_remaining, &r.rate_limit_resource) {
        (Some(n), Some(res)) => format!("{n} {res}"),
        (Some(n), None) => n.to_string(),
        _ => String::new(),
    }
}

fn display_name(r: &RequestEntry) -> String {
    match (&r.path, r.name.split_once(' ')) {
        (Some(path), Some((method, _))) => format!("{method} {path}"),
        _ => r.name.clone(),
    }
}

fn rate_lines(snap: &Snapshot) -> Vec<String> {
    diagnostics::latest_rate_limits(&snap.requests)
        .into_iter()
        .map(|(res, n, reset)| format!("{res} {n}{}", reset.map(|r| format!(", resets {}", reset_clock(r))).unwrap_or_default()))
        .collect()
}

fn recent_ticks(ticks: &[TickEntry]) -> Vec<f64> {
    diagnostics::active_ticks(ticks).rev().take(RECENT_TICKS).map(|t| t.duration_ms).collect()
}

fn span<'a>(newest_first: impl Iterator<Item = &'a RequestEntry> + Clone) -> Option<(String, f64)> {
    let last = newest_first.clone().next()?.ts;
    let first = newest_first.last()?.ts;
    let secs = (last - first).num_milliseconds().max(0) as f64 / 1000.0;
    let label = if secs >= 60.0 { format!("{}m {}s", (secs / 60.0).floor(), (secs % 60.0).floor()) } else { format!("{secs:.0}s") };
    Some((label, secs))
}

fn summary_text(snap: &Snapshot) -> String {
    let all = diagnostics::stats(&snap.requests);
    let graphql = snap.requests.iter().filter(|r| r.kind == "graphql").count();
    let mut out = format!("{} {} diagnostics — {}\n", brand::APP_NAME, brand::VERSION, Local::now().format("%Y-%m-%d %H:%M:%S"));
    out.push_str(&format!("Requests: {} (graphql {}, rest {}) · live-tick {} · user {} · errors {}\n", all.count, graphql, all.count - graphql, all.live, all.count - all.live, all.errors));
    if let Some((label, secs)) = span(snap.requests.iter().rev()) {
        let per_min = if secs > 0.0 { all.count as f64 / secs * 60.0 } else { 0.0 };
        out.push_str(&format!("Window: {label} · {per_min:.1} requests/min\n"));
    }
    out.push_str(&format!("Data: {}\n", bytes(all.bytes)));
    out.push_str(&format!("Duration: avg {} · p95 {}\n", ms(all.avg_ms), ms(all.p95_ms)));
    for line in rate_lines(snap) {
        out.push_str(&format!("Rate limit {line}\n"));
    }
    let active: Vec<&TickEntry> = diagnostics::active_ticks(&snap.ticks).collect();
    if !snap.ticks.is_empty() {
        let idle = snap.ticks.len() - active.len();
        let (avg, reqs) = if active.is_empty() {
            (0.0, 0.0)
        } else {
            (active.iter().map(|t| t.duration_ms).sum::<f64>() / active.len() as f64, active.iter().map(|t| t.requests).sum::<usize>() as f64 / active.len() as f64)
        };
        out.push_str(&format!(
            "Live ticks: {} with requests ({idle} without) · avg {} · avg {reqs:.1} requests · last: {}\n",
            active.len(),
            ms(avg),
            recent_ticks(&snap.ticks).iter().map(|v| format!("{v:.0}")).collect::<Vec<_>>().join(", ")
        ));
    }
    if !snap.slow_ui.is_empty() {
        let worst = snap.slow_ui.iter().max_by(|a, b| a.duration_ms.total_cmp(&b.duration_ms));
        out.push_str(&format!(
            "Slow UI (>{:.0} ms): {}{}\n",
            diagnostics::SLOW_UI_MS,
            snap.slow_ui.len(),
            worst.map(|w| format!(" · worst {} ({})", ms(w.duration_ms), w.name)).unwrap_or_default()
        ));
    }
    out.push_str("\nBy name: count · avg · p95 · bytes · errors · live-tick\n");
    for (name, s) in diagnostics::grouped(&snap.requests) {
        out.push_str(&format!("{name}\t{}\t{}\t{}\t{}\t{}\t{}\n", s.count, ms(s.avg_ms), ms(s.p95_ms), bytes(s.bytes), s.errors, s.live));
    }
    out
}

impl Panel {
    fn refresh(&self) {
        self.seen.set(diagnostics::generation());
        self.banner.set_revealed(!diagnostics::enabled());
        let snap = diagnostics::snapshot();
        let needle = self.filter.text().to_string();
        let matches = |text: &str| needle.is_empty() || util::contains_ci(text, &needle);
        let reqs: Vec<&RequestEntry> = snap.requests.iter().rev().filter(|r| matches(&r.name) || r.path.as_deref().is_some_and(matches)).collect();
        self.render_summary(&snap, &reqs);
        self.requests.set_rows(
            reqs.iter()
                .map(|r| vec![clock(r.ts), r.kind.to_string(), display_name(r), r.origin.to_string(), ms(r.duration_ms), bytes(r.bytes), status_text(r), rate_text(r)])
                .collect(),
        );
        self.grouped.set_rows(
            diagnostics::grouped(reqs.iter().copied())
                .into_iter()
                .map(|(name, s)| vec![name, s.count.to_string(), ms(s.avg_ms), ms(s.p95_ms), bytes(s.bytes), s.errors.to_string(), s.live.to_string()])
                .collect(),
        );
        self.ticks.set_rows(snap.ticks.iter().rev().map(|t| vec![clock(t.ts), ms(t.duration_ms), t.requests.to_string()]).collect());
        self.slow.set_rows(snap.slow_ui.iter().rev().filter(|u| matches(u.name)).map(|u| vec![clock(u.ts), u.name.to_string(), ms(u.duration_ms)]).collect());
    }

    fn render_summary(&self, snap: &Snapshot, reqs: &[&RequestEntry]) {
        let s: Stats = diagnostics::stats(reqs.iter().copied());
        let graphql = reqs.iter().filter(|r| r.kind == "graphql").count();
        self.requests_stat.value.set_text(&s.count.to_string());
        let mut detail = format!("graphql {graphql} · rest {}\nlive-tick {} · user {}", s.count - graphql, s.live, s.count - s.live);
        if let Some((label, secs)) = span(reqs.iter().copied()) {
            if secs > 0.0 {
                detail.push_str(&format!("\n{:.1}/min over {label}", s.count as f64 / secs * 60.0));
            }
        }
        self.requests_stat.detail.set_text(&detail);
        self.data_stat.value.set_text(&bytes(s.bytes));
        self.data_stat.detail.set_text(&if s.count > 0 { format!("{} per request", bytes(s.bytes / s.count as u64)) } else { String::new() });
        self.duration_stat.value.set_text(&format!("avg {}", ms(s.avg_ms)));
        self.duration_stat.detail.set_text(&format!("p95 {}", ms(s.p95_ms)));
        self.errors_stat.value.set_text(&s.errors.to_string());
        self.errors_stat.detail.set_text(&reqs.iter().find(|r| r.error.is_some()).map(|r| format!("last: {}", r.name)).unwrap_or_default());
        let limits = diagnostics::latest_rate_limits(&snap.requests);
        self.rate_stat.value.set_text(&if limits.is_empty() { "—".to_string() } else { limits.iter().map(|(res, n, _)| format!("{res} {n}")).collect::<Vec<_>>().join("\n") });
        self.rate_stat.detail.set_text(
            &limits
                .iter()
                .filter_map(|(res, _, reset)| reset.map(|r| format!("{res} resets {}", reset_clock(r))))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        let recent = recent_ticks(&snap.ticks);
        let idle = snap.ticks.len() - diagnostics::active_ticks(&snap.ticks).count();
        if recent.is_empty() {
            self.ticks_stat.value.set_text("—");
        } else {
            self.ticks_stat.value.set_text(&format!("avg {}", ms(recent.iter().sum::<f64>() / recent.len() as f64)));
        }
        let mut detail = if recent.is_empty() { String::new() } else { format!("last: {} ms", recent.iter().map(|v| format!("{v:.0}")).collect::<Vec<_>>().join(" · ")) };
        if idle > 0 {
            if !detail.is_empty() {
                detail.push('\n');
            }
            detail.push_str(&format!("{} without requests", util::plural(idle as i64, "tick", "ticks")));
        }
        self.ticks_stat.detail.set_text(&detail);
    }

    fn toast(&self, msg: &str) {
        let t = adw::Toast::new(msg);
        t.set_timeout(2);
        self.toasts.add_toast(t);
    }

    fn export(self: &Rc<Self>) {
        let data = diagnostics::export_lines(&diagnostics::snapshot());
        if data.is_empty() {
            self.toast("Nothing to export yet");
            return;
        }
        let count = data.lines().count();
        let dialog = gtk::FileDialog::new();
        dialog.set_title("Export Diagnostics");
        dialog.set_modal(true);
        let name = format!("{}-diagnostics-{}.jsonl", brand::BIN_NAME, Local::now().format("%Y%m%d-%H%M%S"));
        dialog.set_initial_name(Some(name.as_str()));
        let weak = Rc::downgrade(self);
        dialog.save(Some(&self.window), None::<&gio::Cancellable>, move |res| {
            let Ok(file) = res else { return };
            let Some(path) = file.path() else { return };
            let result = std::fs::write(&path, data);
            if let Some(me) = weak.upgrade() {
                match result {
                    Ok(()) => me.toast(&format!("Exported {}", util::plural(count as i64, "record", "records"))),
                    Err(e) => me.toast(&format!("Export failed: {e}")),
                }
            }
        });
    }
}

pub fn show() {
    if let Some(w) = OPEN.with(|o| o.borrow().as_ref().and_then(|w| w.upgrade())) {
        w.present();
        return;
    }
    let window = adw::Window::new();
    window.set_title(Some("Diagnostics"));
    window.set_default_size(1120, 760);
    if let Some(parent) = app::main_window() {
        window.set_transient_for(Some(&parent));
        window.set_destroy_with_parent(true);
    }

    let tv = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    let stack = adw::ViewStack::new();
    let switcher = adw::ViewSwitcher::new();
    switcher.set_stack(Some(&stack));
    switcher.set_policy(adw::ViewSwitcherPolicy::Wide);
    header.set_title_widget(Some(&switcher));
    let clear_btn = flat("Clear");
    header.pack_start(&clear_btn);
    let export_btn = button("Export…");
    header.pack_end(&export_btn);
    let copy_btn = button("Copy Summary");
    header.pack_end(&copy_btn);
    tv.add_top_bar(&header);

    let banner = adw::Banner::new("Recording is off. Turn on Diagnostics to record network requests and live refresh timings.");
    banner.set_button_label(Some("Turn On"));
    tv.add_top_bar(&banner);

    let body = vbox(12);
    margins(&body, 12, 12, 12, 12);
    let cards = gtk::FlowBox::new();
    cards.set_selection_mode(gtk::SelectionMode::None);
    cards.set_homogeneous(true);
    cards.set_max_children_per_line(6);
    cards.set_min_children_per_line(2);
    cards.set_column_spacing(8);
    cards.set_row_spacing(8);
    let requests_stat = stat(&cards, "Requests");
    let data_stat = stat(&cards, "Downloaded");
    let duration_stat = stat(&cards, "Duration");
    let errors_stat = stat(&cards, "Errors");
    let rate_stat = stat(&cards, "Rate limit remaining");
    let ticks_stat = stat(&cards, &format!("Last {RECENT_TICKS} live ticks with requests"));
    body.append(&cards);

    let filter = gtk::SearchEntry::new();
    filter.set_placeholder_text(Some("Filter by name"));
    filter.set_hexpand(true);
    body.append(&filter);

    let requests = Table::new(&[
        ("Time", false, false),
        ("Kind", false, false),
        ("Name", true, false),
        ("Origin", false, false),
        ("Duration", false, true),
        ("Size", false, true),
        ("Status", false, false),
        ("Rate limit", false, true),
    ]);
    let grouped = Table::new(&[
        ("Name", true, false),
        ("Count", false, true),
        ("Avg", false, true),
        ("p95", false, true),
        ("Total size", false, true),
        ("Errors", false, true),
        ("Live-tick", false, true),
    ]);
    let ticks = Table::new(&[("Time", false, false), ("Duration", true, true), ("Requests", false, true)]);
    let slow = Table::new(&[("Time", false, false), ("Name", true, false), ("Duration", false, true)]);
    stack.add_titled_with_icon(&requests.widget(), Some("requests"), "Requests", "sy-globe-symbolic");
    stack.add_titled_with_icon(&grouped.widget(), Some("grouped"), "By Name", "sy-stack-symbolic");
    stack.add_titled_with_icon(&ticks.widget(), Some("ticks"), "Live Ticks", "sy-clock-symbolic");
    stack.add_titled_with_icon(&slow.widget(), Some("slow"), "Slow UI", "sy-gauge-symbolic");
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&stack));
    frame.set_vexpand(true);
    body.append(&frame);

    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&body));
    tv.set_content(Some(&toasts));
    window.set_content(Some(&tv));

    let panel = Rc::new(Panel {
        window: window.clone(),
        toasts,
        banner: banner.clone(),
        filter: filter.clone(),
        requests_stat,
        data_stat,
        duration_stat,
        errors_stat,
        rate_stat,
        ticks_stat,
        requests,
        grouped,
        ticks,
        slow,
        seen: Cell::new(u64::MAX),
    });

    let w = Rc::downgrade(&panel);
    banner.connect_button_clicked(move |_| {
        settings::update(|s| s.diagnostics_enabled = true);
        diagnostics::set_enabled(true);
        if let Some(p) = w.upgrade() {
            p.refresh();
        }
    });
    let w = Rc::downgrade(&panel);
    filter.connect_search_changed(move |_| {
        if let Some(p) = w.upgrade() {
            p.refresh();
        }
    });
    let w = Rc::downgrade(&panel);
    clear_btn.connect_clicked(move |_| {
        diagnostics::clear();
        if let Some(p) = w.upgrade() {
            p.refresh();
        }
    });
    let w = Rc::downgrade(&panel);
    copy_btn.connect_clicked(move |_| {
        util::copy(&summary_text(&diagnostics::snapshot()));
        if let Some(p) = w.upgrade() {
            p.toast("Summary copied");
        }
    });
    let w = Rc::downgrade(&panel);
    export_btn.connect_clicked(move |_| {
        if let Some(p) = w.upgrade() {
            p.export();
        }
    });
    let w = Rc::downgrade(&panel);
    glib::timeout_add_local(Duration::from_secs(1), move || {
        let Some(p) = w.upgrade() else { return glib::ControlFlow::Break };
        if p.seen.get() != diagnostics::generation() {
            p.refresh();
        }
        glib::ControlFlow::Continue
    });

    panel.refresh();
    keep_alive(&window, panel);
    OPEN.with(|o| *o.borrow_mut() = Some(window.downgrade()));
    window.present();
}

#[cfg(test)]
mod tests {
    use super::splice_plan;

    fn apply(old: &[&str], new: &[&str]) -> Vec<String> {
        let old: Vec<String> = old.iter().map(|s| s.to_string()).collect();
        let new: Vec<String> = new.iter().map(|s| s.to_string()).collect();
        let mut model = old.clone();
        for (at, remove, add) in splice_plan(&old, &new) {
            model.splice(at..at + remove, new[add].iter().cloned());
        }
        model
    }

    #[test]
    fn splice_plan_reproduces_new_rows() {
        let cases: [(&[&str], &[&str]); 7] = [
            (&[], &["a", "b"]),
            (&["a", "b", "c"], &["x", "a", "b"]),
            (&["a", "b", "c"], &["y", "x", "a", "b", "c"]),
            (&["a", "b", "c"], &["a", "q", "c"]),
            (&["a", "b"], &[]),
            (&["a", "b", "c"], &["c", "b", "a"]),
            (&["a", "a", "b"], &["z", "a", "a"]),
        ];
        for (old, new) in cases {
            assert_eq!(apply(old, new), new.iter().map(|s| s.to_string()).collect::<Vec<_>>(), "{old:?} -> {new:?}");
        }
    }

    #[test]
    fn prepending_keeps_existing_rows_in_place() {
        let old: Vec<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let new: Vec<String> = ["n", "a", "b"].iter().map(|s| s.to_string()).collect();
        let plan = splice_plan(&old, &new);
        assert!(plan.iter().all(|(at, remove, _)| *remove == 0 || *at > 0));
    }
}
