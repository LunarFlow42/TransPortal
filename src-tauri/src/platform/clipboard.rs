//! Clipboard contract: change counter, origin of the current content and
//! change notifications.

/// Which program published the current clipboard content.
#[derive(Debug, PartialEq, Eq)]
pub enum ClipboardOrigin {
    /// The OS screenshot tool.
    SnippingTool,
    Other,
    Unavailable,
}

pub trait ClipboardOps: Send + Sync {
    /// Changes whenever the clipboard content changes; `0` when unknown.
    fn revision(&self) -> u32;

    /// Which program published the current content.
    fn origin(&self) -> ClipboardOrigin;

    /// Block the calling thread and call `on_change` for every clipboard
    /// update. Without change notifications (see
    /// [`Capabilities::clipboard_events`](super::Capabilities)) it is called
    /// on every poll instead. Returns only if listening fails.
    fn listen(&self, on_change: &mut dyn FnMut()) -> Result<(), String>;
}
