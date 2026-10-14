//! Selected-text capture and one-use replacement tokens.
//!
//! Capture tries the backend's native selection reader first (UI Automation
//! on Windows); if it fails or returns nothing, the backend is asked to copy
//! the selection and the clipboard is read instead.
//!
//! A replacement token binds an explicit replace action to the original
//! selection and its foreground target.

use crate::core::shell::SHELL_LABEL;
use crate::platform::clipboard::ClipboardOps;
use crate::platform::selection::{CaptureMethod, SelectionBounds, SelectionOps, SelectionTarget};
use crate::platform::{PlatformClipboard, PlatformSelection};
use serde::Serialize;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

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

#[derive(Default)]
struct SelectionState {
    next: u64,
    captured: Option<(u64, SelectionTarget, String)>,
}

pub struct SelectionService {
    platform: PlatformSelection,
    clipboard: PlatformClipboard,
    state: Mutex<SelectionState>,
    operation: Mutex<()>,
    double_copy: AtomicBool,
}

impl Default for SelectionService {
    fn default() -> Self {
        Self {
            platform: PlatformSelection::default(),
            clipboard: PlatformClipboard::default(),
            state: Mutex::new(SelectionState::default()),
            operation: Mutex::new(()),
            double_copy: AtomicBool::new(true),
        }
    }
}

fn failure(error: impl Into<String>) -> CaptureOutcome {
    CaptureOutcome {
        text: String::new(),
        method: CaptureMethod::Clipboard,
        error: Some(error.into()),
        replace_token: None,
        bounds: None,
    }
}

/// Collapse surrounding whitespace without touching interior formatting.
fn normalize(text: &str) -> String {
    text.trim().to_string()
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
fn capture_selected_text(platform: &impl SelectionOps, app: &AppHandle) -> CaptureOutcome {
    let preferred_error = match platform.read_selection() {
        Ok(content) if !content.text.trim().is_empty() => {
            eprintln!(
                "[capture] UI Automation returned {} chars",
                content.text.trim().len()
            );
            return CaptureOutcome {
                text: normalize(&content.text),
                method: platform.method(),
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

    let copy_error = platform.copy_to_clipboard().err();
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

impl SelectionService {
    pub fn double_copy_enabled(&self) -> bool {
        self.double_copy.load(Ordering::Relaxed)
    }
    pub fn set_double_copy(&self, enabled: bool) {
        self.double_copy.store(enabled, Ordering::Relaxed);
    }

    fn record(
        &self,
        mut outcome: CaptureOutcome,
        target: Option<SelectionTarget>,
    ) -> CaptureOutcome {
        let mut state = self.state.lock().unwrap();
        state.captured = None;
        if !outcome.text.trim().is_empty()
            && let Some(target) = target
            && self
                .platform
                .foreground()
                .as_ref()
                .is_some_and(|cur| cur.matches(&target))
        {
            state.next += 1;
            outcome.replace_token = Some(state.next);
            state.captured = Some((state.next, target, outcome.text.clone()));
        }
        outcome
    }

    pub fn capture(&self, app: &AppHandle) -> CaptureOutcome {
        let Ok(_operation) = self.operation.try_lock() else {
            return failure("正在处理上一次取词或替换，请稍后重试。");
        };
        let target = self.platform.foreground();
        let outcome = capture_selected_text(&self.platform, app);
        self.record(outcome, target)
    }

    pub fn double_copy(
        &self,
        app: &AppHandle,
        baseline: u32,
        window: usize,
    ) -> Option<CaptureOutcome> {
        if !self.double_copy_enabled() {
            return None;
        }
        let _operation = self.operation.try_lock().ok()?;
        let target = self
            .platform
            .foreground()
            .filter(|target| target.window == window);
        if baseline == 0 {
            return Some(self.record(
                failure("无法确认本次复制内容，请重新选择文字后重试。"),
                None,
            ));
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(500) {
            if let (Some(target), Some(current)) = (&target, self.platform.foreground())
                && current.window != target.window
                && current.process != target.process
            {
                return None;
            }
            let revision = self.clipboard.revision();
            if revision != 0
                && revision != baseline
                && let Ok(text) = app.clipboard().read_text()
                && self.clipboard.revision() == revision
                && !text.trim().is_empty()
            {
                return Some(self.record(
                    CaptureOutcome {
                        text: text.trim().into(),
                        method: CaptureMethod::Clipboard,
                        error: None,
                        replace_token: None,
                        bounds: None,
                    },
                    target,
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
        }

        // Fallback: If sequence number didn't change (e.g. host app debounced the 2nd
        // copy or baseline was sampled after the sequence incremented),
        // read whatever is currently on the clipboard.
        if let Ok(text) = app.clipboard().read_text()
            && !text.trim().is_empty()
        {
            return Some(self.record(
                CaptureOutcome {
                    text: text.trim().into(),
                    method: CaptureMethod::Clipboard,
                    error: None,
                    replace_token: None,
                    bounds: None,
                },
                target,
            ));
        }

        Some(self.record(
            failure("本次复制未提供新文字，请重新选择文字后再双按 C。"),
            None,
        ))
    }

    pub fn replace(&self, app: &AppHandle, token: u64, text: &str) -> Result<(), String> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| "正在处理上一次取词或替换。")?;
        let (_, target, expected) = self
            .state
            .lock()
            .unwrap()
            .captured
            .clone()
            .filter(|(id, _, _)| *id == token)
            .ok_or("取词已过期，请重新选择要替换的文字。")
            .map_err(String::from)?;
        if text.trim().is_empty() || text.chars().count() > 100_000 {
            return Err("译文为空或过长，无法替换。".into());
        }
        let owner = app
            .get_webview_window(SHELL_LABEL)
            .ok_or("译文窗口不可用。")?;
        self.platform
            .replace(app, &owner, &target, &expected, text)?;
        self.state.lock().unwrap().captured = None;
        Ok(())
    }
}
