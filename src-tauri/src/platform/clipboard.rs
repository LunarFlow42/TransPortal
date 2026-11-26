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
    /// Native asynchronous readers already wait for the selection owner and
    /// must not be retried repeatedly like an in-flight synthetic copy.
    fn read_retry_count(&self) -> usize {
        8
    }
    /// Read on a worker thread; Linux can use GTK's Wayland clipboard rather
    /// than assuming a compositor implements data-control extensions.
    fn read_text(&self, app: &tauri::AppHandle) -> Result<String, String> {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        app.clipboard()
            .read_text()
            .map_err(|error| error.to_string())
    }
    fn write_text(&self, app: &tauri::AppHandle, text: &str) -> Result<(), String> {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        app.clipboard()
            .write_text(text)
            .map_err(|error| error.to_string())
    }
    fn write_image(
        &self,
        app: &tauri::AppHandle,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        use tauri_plugin_clipboard_manager::ClipboardExt;
        app.clipboard()
            .write_image(&tauri::image::Image::new_owned(rgba, width, height))
            .map_err(|error| error.to_string())
    }
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
