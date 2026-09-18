//! One-use replacement tokens bind an explicit action to the original selection.

use crate::native::{
    self, CaptureMethod, CaptureOutcome,
    selection::{SelectionPlatform, SelectionTarget},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

#[derive(Default)]
struct SelectionState {
    next: u64,
    captured: Option<(u64, SelectionTarget, String)>,
}

pub struct SelectionService {
    platform: Arc<dyn SelectionPlatform>,
    state: Mutex<SelectionState>,
    operation: Mutex<()>,
    double_copy: AtomicBool,
}

impl Default for SelectionService {
    fn default() -> Self {
        Self {
            platform: native::selection::platform(),
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
    }
}

pub(crate) fn normalize_selection(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
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
            && self.platform.foreground().as_ref() == Some(&target)
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
        let outcome = native::capture_selected_text(app);
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
            .filter(|target| target.window == window)?;
        if baseline == 0 {
            return Some(self.record(
                failure("无法确认本次复制内容，请重新选择文字后重试。"),
                None,
            ));
        }
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(900) {
            if self.platform.foreground().as_ref() != Some(&target) {
                return None;
            }
            let revision = self.platform.clipboard_revision();
            if revision != 0
                && revision != baseline
                && let Ok(text) = app.clipboard().read_text()
                && self.platform.clipboard_revision() == revision
                && !text.trim().is_empty()
            {
                return Some(self.record(
                    CaptureOutcome {
                        text: text.trim().into(),
                        method: CaptureMethod::Clipboard,
                        error: None,
                        replace_token: None,
                    },
                    Some(target),
                ));
            }
            std::thread::sleep(Duration::from_millis(15));
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
        self.platform.replace(app, &target, &expected, text)?;
        self.state.lock().unwrap().captured = None;
        Ok(())
    }
}
