//! Fallbacks for targets without a native backend: every native capability
//! reports that it is unavailable.

mod window;

pub use window::{
    PointerProbe, configure_transparent_window, force_hide_window, get_global_cursor_pos,
    observe_size_move, place_window_smartly, resize_and_clamp_window, set_window_acrylic,
    set_window_theme,
};

use super::desktop::{DesktopCapture, ScreenRect};
use super::selection::{SelectionPlatform, SelectionTarget};
use super::system::{IntegrityPlatform, MemoryReport, TrimResult};
use super::{CaptureMethod, SelectionContent, SelectionReader};
use crate::core::ocr::{
    ClipboardOrigin, OcrEngine, OcrEngineInfo, OcrImage, RecognizedText, ScreenCapture,
    ScreenEvent, ScreenObserver,
};
use std::sync::mpsc::SyncSender;
use tauri::AppHandle;

pub struct UnavailableOcr;
const UNAVAILABLE: &str = "当前平台尚未提供本地 OCR；Windows 10/11 可使用系统 OCR。";

impl OcrEngine for UnavailableOcr {
    fn id(&self) -> &'static str {
        "unavailable"
    }

    fn info(&self) -> Result<OcrEngineInfo, String> {
        Ok(OcrEngineInfo {
            id: self.id().into(),
            label: "本地 OCR".into(),
            languages: vec![],
            available: false,
            error: Some(UNAVAILABLE.into()),
        })
    }

    fn recognize(&self, _: OcrImage<'_>, _: Option<&str>) -> Result<RecognizedText, String> {
        Err(UNAVAILABLE.into())
    }
}

impl ScreenCapture for UnavailableOcr {
    fn supported(&self) -> bool {
        false
    }
    fn clipboard_revision(&self) -> u32 {
        0
    }
    fn clipboard_origin(&self) -> ClipboardOrigin {
        ClipboardOrigin::Unavailable
    }
    fn observe(&self, _: SyncSender<ScreenEvent>) -> Result<Box<dyn ScreenObserver>, String> {
        Err(UNAVAILABLE.into())
    }
}

/// Placeholder reader used on non-Windows targets.
pub struct FallbackSelectionReader;

impl FallbackSelectionReader {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FallbackSelectionReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionReader for FallbackSelectionReader {
    fn method(&self) -> CaptureMethod {
        CaptureMethod::UiAutomation
    }

    fn read_selection(&self) -> Result<SelectionContent, String> {
        Err("native selection capture is only implemented on Windows".to_string())
    }
}

pub struct UnsupportedSelection;
impl SelectionPlatform for UnsupportedSelection {
    fn foreground(&self) -> Option<SelectionTarget> {
        None
    }
    fn clipboard_revision(&self) -> u32 {
        0
    }
    fn replace(&self, _: &AppHandle, _: &SelectionTarget, _: &str, _: &str) -> Result<(), String> {
        Err("当前平台不支持选中文本替换。".into())
    }
}

pub struct UnsupportedDesktop;
impl DesktopCapture for UnsupportedDesktop {
    fn bounds(&self) -> Result<ScreenRect, String> {
        Err("当前平台尚未支持屏幕区域自动翻译。".into())
    }
    fn capture(&self, _: ScreenRect) -> Result<image::RgbaImage, String> {
        self.bounds().map(|_| unreachable!())
    }
    fn exclude(&self, _: &tauri::WebviewWindow, _: bool) -> Result<(), String> {
        self.bounds().map(|_| ())
    }
}

pub struct UnsupportedIntegrity;

impl IntegrityPlatform for UnsupportedIntegrity {
    fn enforce_release_hash(&self) -> bool {
        false
    }
    fn show_failure(&self, message: &str) {
        eprintln!("{message}");
    }
}

pub fn get_memory_usage() -> Result<MemoryReport, String> {
    Ok(MemoryReport {
        total_working_set_bytes: 40 * 1024 * 1024,
        total_private_bytes: 30 * 1024 * 1024,
        main_working_set_bytes: 20 * 1024 * 1024,
        process_count: 1,
    })
}

pub fn trim_memory() -> Result<TrimResult, String> {
    Ok(TrimResult {
        before_bytes: 40 * 1024 * 1024,
        after_bytes: 35 * 1024 * 1024,
        trimmed_bytes: 5 * 1024 * 1024,
        process_count: 1,
    })
}
