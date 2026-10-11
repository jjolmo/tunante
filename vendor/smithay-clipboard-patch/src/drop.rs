//! Tunante patch: files dropped on the app's windows.
//!
//! Not clipboard business, but the clipboard owns the client's first
//! `wl_data_device`, and KWin delivers a drag to that one only. So the handler
//! lives here, process-wide, and the app installs it once.

use std::sync::OnceLock;

pub(crate) const URI_LIST: &str = "text/uri-list";

/// What a drag over the window amounts to.
pub enum DropEvent {
    /// A drag carrying files entered (true) or left (false) a surface.
    Hover(bool),
    /// The `text/uri-list` body of a drop.
    Dropped(String),
}

type Handler = Box<dyn Fn(DropEvent) + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// Accept file drops and report them to `handler`, on the clipboard's thread.
/// Until this is called drags are refused, as upstream does. Only the first
/// call counts.
pub fn set_drop_handler(handler: impl Fn(DropEvent) + Send + Sync + 'static) {
    let _ = HANDLER.set(Box::new(handler));
}

pub(crate) fn wanted() -> bool {
    HANDLER.get().is_some()
}

pub(crate) fn emit(event: DropEvent) {
    if let Some(h) = HANDLER.get() {
        h(event);
    }
}
