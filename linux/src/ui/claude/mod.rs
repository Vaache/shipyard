pub mod auth_view;
pub mod chat_view;
pub mod column;
pub mod inspector;
pub mod workspace_view;

use adw::prelude::*;

use crate::claude::memory;
use crate::claude::TabState;
use crate::ui::widgets::*;

pub fn state_badge(state: TabState, memory_bytes: Option<i64>, compact: bool) -> gtk::Box {
    let b = hbox(4);
    if state == TabState::Working {
        b.append(&spinner(10));
    } else {
        b.append(&colored_icon(state.icon(), if state == TabState::Active { 8 } else { 11 }, state.color()));
    }
    if !compact {
        b.append(&label_with(state.title(), &[&format!("fg-{}", state.color()), "sy-small"]));
        if let Some(m) = memory_bytes.filter(|m| *m > 0 && state != TabState::Sleeping) {
            b.append(&label_with(&format!("· {}", memory::format(m)), &["sy-muted", "sy-small"]));
        }
    }
    let mut tip = state.help().to_string();
    if let Some(m) = memory_bytes.filter(|m| *m > 0) {
        tip.push_str(&format!(" — {}", memory::format(m)));
    }
    b.set_tooltip_text(Some(&tip));
    b.set_valign(gtk::Align::Center);
    b
}
