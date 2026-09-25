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

use desktop::DesktopCapture;
use serde::Serialize;
use std::sync::Arc;
use system::SystemOps;
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;
use window::WindowOps;

pub mod desktop;
pub mod imaging;
pub mod key_sequence;
pub mod ocr;
pub mod placement;
pub mod selection;
pub mod system;
pub mod window;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(test, not(target_os = "windows")))]
pub mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use self::linux::LinuxBackend as ActiveBackend;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub use self::unsupported::UnsupportedBackend as ActiveBackend;
#[cfg(target_os = "windows")]
pub use self::windows::WindowsBackend as ActiveBackend;

/// Native features the active backend provides. Shared code reads these flags
/// instead of checking the target OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Global keyboard observation: Ctrl+C+C, the ink key and Esc.
    pub keyboard_gestures: bool,
    /// Observing the OS screenshot chord and the image it publishes.
    pub system_snip: bool,
    /// Replacing the selection inside the foreground application.
    pub selection_replace: bool,
    /// Desktop capture for region OCR and auto region translation.
    pub screen_capture: bool,
    /// Clipboard change notifications; without them the clipboard is polled.
    pub clipboard_events: bool,
}

impl Capabilities {
    pub const NONE: Self = Self {
        keyboard_gestures: false,
        system_snip: false,
        selection_replace: false,
        screen_capture: false,
        clipboard_events: false,
    };
    pub const ALL: Self = Self {
        keyboard_gestures: true,
        system_snip: true,
        selection_replace: true,
        screen_capture: true,
        clipboard_events: true,
    };
}

/// One implementation per target OS. Every backend must name a type for each
/// contract, so a backend that falls behind the contracts fails to compile.
pub trait Backend {
    type Window: WindowOps;
    type System: SystemOps;
    type Desktop: DesktopCapture + Default;
    const CAPABILITIES: Capabilities;
}

pub type PlatformWindow = <ActiveBackend as Backend>::Window;
pub type PlatformSystem = <ActiveBackend as Backend>::System;
pub type PlatformDesktop = <ActiveBackend as Backend>::Desktop;

/// A desktop capturer of the active backend.
pub fn desktop() -> Arc<dyn DesktopCapture> {
    Arc::new(PlatformDesktop::default())
}

/// Capabilities of the backend compiled for this target.
pub const CAPABILITIES: Capabilities = <ActiveBackend as Backend>::CAPABILITIES;

#[cfg(not(target_os = "windows"))]
use unsupported::FallbackSelectionReader;
#[cfg(target_os = "windows")]
use windows::WindowsSelectionReader;

/// Screen bounding box of selected text in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectionBounds {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl SelectionBounds {
    pub fn union(&self, other: &Self) -> Self {
        let left = self.left.min(other.left);
        let top = self.top.min(other.top);
        let right = (self.left + self.width).max(other.left + other.width);
        let bottom = (self.top + self.height).max(other.top + other.height);
        Self {
            left,
            top,
            width: (right - left).max(0),
            height: (bottom - top).max(0),
        }
    }
}

/// Content returned by a native selection reader.
#[derive(Debug, Clone)]
pub struct SelectionContent {
    pub text: String,
    pub bounds: Option<SelectionBounds>,
}

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
    pub bounds: Option<SelectionBounds>,
}

/// Platform-agnostic abstraction over native selected-text retrieval.
pub trait SelectionReader: Send + Sync {
    /// Human-readable identifier of the concrete strategy.
    fn method(&self) -> CaptureMethod;
    /// Attempt to read the current foreground selection.
    fn read_selection(&self) -> Result<SelectionContent, String>;
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
                    bounds: None,
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
        bounds: None,
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
        Ok(content) if !content.text.trim().is_empty() => {
            eprintln!(
                "[capture] UI Automation returned {} chars",
                content.text.trim().len()
            );
            return CaptureOutcome {
                text: normalize(&content.text),
                method: reader.method(),
                error: None,
                replace_token: None,
                bounds: content.bounds,
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
        return CaptureOutcome {
            text: String::new(),
            method: CaptureMethod::Clipboard,
            error: Some(format!("本次取词未复制到新文字：{err}")),
            replace_token: None,
            bounds: None,
        };
    }

    let mut outcome = read_clipboard_with_retry(app);
    eprintln!(
        "[capture] clipboard read: {} chars (error: {:?})",
        outcome.text.len(),
        outcome.error
    );

    // Report why the preferred path was skipped, for diagnostics.
    let diagnostics: Vec<String> = [preferred_error, copy_error]
        .into_iter()
        .flatten()
        .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time and runtime check that a backend provides every contract.
    fn exercise<B: Backend>() -> Capabilities {
        let _probe: Option<<B::Window as WindowOps>::Probe> = None;
        let _: fn() -> bool = B::System::enforce_release_hash;
        let _: fn() -> B::Desktop = B::Desktop::default;
        B::CAPABILITIES
    }

    #[test]
    fn active_backend_provides_every_contract() {
        assert_eq!(exercise::<ActiveBackend>(), CAPABILITIES);
    }

    #[test]
    fn unsupported_backend_provides_every_contract() {
        assert_eq!(
            exercise::<unsupported::UnsupportedBackend>(),
            Capabilities::NONE
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_backend_provides_every_capability() {
        assert_eq!(CAPABILITIES, Capabilities::ALL);
    }

    #[test]
    fn test_selection_bounds_union() {
        let b1 = SelectionBounds {
            left: 100,
            top: 200,
            width: 50,
            height: 20,
        };
        let b2 = SelectionBounds {
            left: 120,
            top: 220,
            width: 80,
            height: 30,
        };
        let union = b1.union(&b2);
        assert_eq!(union.left, 100);
        assert_eq!(union.top, 200);
        assert_eq!(union.width, 100); // right = 200, left = 100 -> 100
        assert_eq!(union.height, 50); // bottom = 250, top = 200 -> 50
    }
}
