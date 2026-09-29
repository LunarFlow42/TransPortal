//! Clipboard translation: forward text copied in other applications to the
//! overlay while the feature is enabled.

use crate::core::shell::SHELL_LABEL;
use crate::platform::clipboard::ClipboardOps;
use crate::platform::window::WindowOps;
use crate::platform::{CAPABILITIES, PlatformClipboard, PlatformWindow};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

pub const CLIPBOARD_TEXT_EVENT: &str = "transportal://clipboard-text";

/// Longer clipboard contents are not translated automatically.
const MAX_CHARS: usize = 2000;

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Copies made inside our own windows are not re-translated.
fn own_window_in_front(app: &AppHandle) -> bool {
    [SHELL_LABEL, "settings"].into_iter().any(|label| {
        app.get_webview_window(label)
            .is_some_and(|window| PlatformWindow::is_foreground(&window))
    })
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("clipboard-monitor".into())
        .spawn(move || {
            // Polling backends report every tick, so repeated content is
            // skipped; change notifications forward every copy.
            let mut last_text = String::new();
            let mut on_change = || {
                if !ENABLED.load(Ordering::Relaxed) || own_window_in_front(&app) {
                    return;
                }
                if let Ok(text) = app.clipboard().read_text() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && trimmed.chars().count() <= MAX_CHARS
                        && (CAPABILITIES.clipboard_events || trimmed != last_text)
                    {
                        last_text = trimmed.to_string();
                        let _ = app.emit_to(SHELL_LABEL, CLIPBOARD_TEXT_EVENT, trimmed);
                        crate::core::shortcut::show_shell_with_anchor(&app, None, true);
                    }
                }
            };
            if let Err(error) = PlatformClipboard::default().listen(&mut on_change) {
                eprintln!("[clipboard] monitor unavailable: {error}");
            }
        });
}
