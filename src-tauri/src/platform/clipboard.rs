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
}
