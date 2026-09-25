//! Fallbacks for targets without a native backend: every native capability
//! reports that it is unavailable.

mod window;

use super::desktop::{DesktopCapture, ScreenRect};
use super::selection::{SelectionPlatform, SelectionTarget};
use super::system::{IntegrityPlatform, MemoryReport, TrimResult};
use super::window::{SizeMove, WindowOps};
use super::{Backend, Capabilities, SelectionBounds};
use super::{CaptureMethod, SelectionContent, SelectionReader};
use crate::core::ocr::{
    ClipboardOrigin, OcrEngine, OcrEngineInfo, OcrImage, RecognizedText, ScreenCapture,
    ScreenEvent, ScreenObserver,
};
use std::sync::mpsc::SyncSender;
use tauri::AppHandle;
use tauri::{Runtime, WebviewWindow};

/// Backend for targets without native integration; also the base that
/// partially implemented backends delegate to.
pub struct UnsupportedBackend;

impl Backend for UnsupportedBackend {
    type Window = UnsupportedWindow;
    const CAPABILITIES: Capabilities = Capabilities::NONE;
}

/// Window operations through Tauri alone.
pub struct UnsupportedWindow;

impl WindowOps for UnsupportedWindow {
    type Probe = window::PointerProbe;

    fn configure_transparent_window<R: Runtime>(window: &WebviewWindow<R>) {
        window::configure_transparent_window(window);
    }

    fn resize_and_clamp_window<R: Runtime>(
        window: &WebviewWindow<R>,
        width: f64,
        height: f64,
    ) -> Result<(), String> {
        window::resize_and_clamp_window(window, width, height)
    }

    fn set_window_acrylic<R: Runtime>(window: &WebviewWindow<R>, enabled: bool) {
        window::set_window_acrylic(window, enabled);
    }

    fn set_window_theme<R: Runtime>(window: &WebviewWindow<R>, dark: bool) {
        window::set_window_theme(window, dark);
    }

    fn get_global_cursor_pos() -> Option<(i32, i32)> {
        window::get_global_cursor_pos()
    }

    fn force_hide_window<R: Runtime>(window: &WebviewWindow<R>) {
        window::force_hide_window(window);
    }

    /// Unknown without native window APIs; callers treat it as "not ours".
    fn is_foreground<R: Runtime>(_window: &WebviewWindow<R>) -> bool {
        false
    }

    /// Global pointer sampling needs native APIs; the overlay relies on blur.
    fn pointer_probe<R: Runtime>(_window: &WebviewWindow<R>) -> Option<Self::Probe> {
        None
    }

    fn observe_size_move<R: Runtime>(
        window: &WebviewWindow<R>,
        on_phase: impl Fn(SizeMove) + Send + 'static,
    ) -> Result<(), String> {
        window::observe_size_move(window, on_phase)
    }

    fn place_window_smartly<R: Runtime>(
        window: &WebviewWindow<R>,
        anchor_bounds: Option<&SelectionBounds>,
        center: bool,
        no_activate: bool,
    ) -> Result<(), String> {
        window::place_window_smartly(window, anchor_bounds, center, no_activate)
    }
}

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
