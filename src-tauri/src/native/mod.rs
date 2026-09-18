//! Platform-agnostic selected-text capture with a graceful fallback chain.
//!
//! Strategy on Windows:
//! 1. Try UI Automation / `TextPattern` against the focused element.
//! 2. If the focused element does not support `TextPattern`, or the selection is
//!    empty, silently fall back to reading the clipboard.
//!
//! Every other platform falls back to the clipboard only. The native
//! implementation is isolated behind [`SelectionReader`] so `#[cfg]` never
//! leaks into the rest of the crate and cross-platform builds stay green.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

pub mod ocr;
pub mod integrity;
pub mod desktop;
pub mod selection;

/// Mark only our synthesized input, leaving accessibility/remapped keys usable.
pub(crate) const INPUT_MARKER: usize = 0x54524e53;

#[cfg(not(target_os = "windows"))]
mod fallback;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(not(target_os = "windows"))]
use fallback::FallbackSelectionReader;
#[cfg(target_os = "windows")]
use windows::WindowsSelectionReader;

/// How a piece of text was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    /// Native UI Automation / `TextPattern` (Windows only).
    UiAutomation,
    /// Clipboard fallback.
    Clipboard,
}

/// Outcome of a capture attempt, serialized straight to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct CaptureOutcome {
    pub text: String,
    pub method: CaptureMethod,
    /// Populated when the preferred method failed, even if the fallback worked.
    pub error: Option<String>,
    pub replace_token: Option<u64>,
}

/// Platform-agnostic abstraction over native selected-text retrieval.
pub trait SelectionReader: Send + Sync {
    /// Human-readable identifier of the concrete strategy.
    fn method(&self) -> CaptureMethod;
    /// Attempt to read the current foreground selection.
    fn read_selection(&self) -> Result<String, String>;
}

/// Instantiate the best reader available for the current target.
fn platform_reader() -> Box<dyn SelectionReader> {
    #[cfg(target_os = "windows")]
    {
        Box::new(WindowsSelectionReader::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(FallbackSelectionReader::new())
    }
}

/// Ask the foreground window to copy its selection into the clipboard.
///
/// Only Windows implements this (Ctrl+C via `SendInput`); other platforms are
/// a no-op and rely on whatever is already in the clipboard.
fn copy_selection_to_clipboard() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        windows::copy_selection_to_clipboard()
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}

/// Read the clipboard, retrying briefly because the source app may still be
/// writing to it when we get here.
fn read_clipboard_with_retry(app: &AppHandle) -> CaptureOutcome {
    let mut last_error = "clipboard is empty".to_string();

    for _ in 0..8 {
        match app.clipboard().read_text() {
            Ok(text) if !text.trim().is_empty() => {
                return CaptureOutcome {
                    text: normalize(&text),
                    method: CaptureMethod::Clipboard,
                    error: None,
                    replace_token: None,
                };
            }
            Ok(_) => last_error = "clipboard is empty".to_string(),
            Err(err) => last_error = format!("clipboard read failed: {err}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    CaptureOutcome {
        text: String::new(),
        method: CaptureMethod::Clipboard,
        error: Some(last_error),
        replace_token: None,
    }
}

/// Capture the current selection using the preferred native strategy, then the
/// clipboard if that fails or returns nothing.
///
/// UI Automation cannot read the selection in many apps (notably PDF viewers),
/// so the clipboard path first sends Ctrl+C to the still-focused source window
/// and waits for the clipboard to change before reading it.
pub fn capture_selected_text(app: &AppHandle) -> CaptureOutcome {
    let reader = platform_reader();

    let preferred_error = match reader.read_selection() {
        Ok(text) if !text.trim().is_empty() => {
            eprintln!("[capture] UI Automation returned {} chars", text.trim().len());
            return CaptureOutcome {
                text: normalize(&text),
                method: reader.method(),
                error: None,
                replace_token: None,
            };
        }
        Ok(_) => Some("UI Automation returned an empty selection".to_string()),
        Err(err) => Some(err),
    };
    eprintln!(
        "[capture] UI Automation unavailable: {}",
        preferred_error.as_deref().unwrap_or("empty selection")
    );

    let copy_error = copy_selection_to_clipboard().err();
    if let Some(ref err) = copy_error {
        eprintln!("[capture] Ctrl+C fallback issue: {err}");
        return CaptureOutcome { text: String::new(), method: CaptureMethod::Clipboard,
            error: Some(format!("本次取词未复制到新文字：{err}")), replace_token: None };
    }

    let mut outcome = read_clipboard_with_retry(app);
    eprintln!(
        "[capture] clipboard read: {} chars (error: {:?})",
        outcome.text.len(),
        outcome.error
    );

    // Report why the preferred path was skipped, for diagnostics.
    let diagnostics: Vec<String> = [preferred_error, copy_error].into_iter().flatten().collect();
    if diagnostics.is_empty() {
        return outcome;
    }

    let detail = outcome.error.take().unwrap_or_default();
    let joined = diagnostics.join("; ");
    outcome.error = Some(if detail.is_empty() {
        joined
    } else {
        format!("{joined} ({detail})")
    });
    outcome
}

/// Collapse surrounding whitespace without touching interior formatting.
fn normalize(text: &str) -> String {
    text.trim().to_string()
}
