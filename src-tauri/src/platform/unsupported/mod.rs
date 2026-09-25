//! Fallbacks for targets without a native backend: every native capability
//! reports that it is unavailable.

mod window;

use super::clipboard::{ClipboardOps, ClipboardOrigin};
use super::desktop::{DesktopCapture, ScreenRect};
use super::gestures::{GestureEvent, GestureObserver, GestureSource, InkKey};
use super::selection::{
    CaptureMethod, SelectionBounds, SelectionContent, SelectionOps, SelectionTarget,
};
use super::system::{MemoryReport, SystemOps, TrimResult};
use super::window::{SizeMove, WindowOps};
use super::{Backend, Capabilities};
use crate::core::ocr::OcrEngine;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;
use tauri::{AppHandle, Runtime, WebviewWindow};

/// Backend for targets without native integration; also the base that
/// partially implemented backends delegate to.
pub struct UnsupportedBackend;

impl Backend for UnsupportedBackend {
    type Window = UnsupportedWindow;
    type System = UnsupportedSystem;
    type Selection = UnsupportedSelection;
    type Clipboard = UnsupportedClipboard;
    type Gestures = UnsupportedGestures;
    type Desktop = UnsupportedDesktop;
    const CAPABILITIES: Capabilities = Capabilities::NONE;

    fn native_ocr() -> Option<Arc<dyn OcrEngine>> {
        None
    }
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

/// No global keyboard observation.
#[derive(Default)]
pub struct UnsupportedGestures;

impl GestureSource for UnsupportedGestures {
    fn set_ink_key(_: Option<InkKey>) {}
    fn observe(&self, _: SyncSender<GestureEvent>) -> Result<Box<dyn GestureObserver>, String> {
        Err("当前平台不支持全局键盘监听。".into())
    }
}

/// No native selection: capture degrades to whatever the clipboard holds.
#[derive(Default)]
pub struct UnsupportedSelection;

impl SelectionOps for UnsupportedSelection {
    fn method(&self) -> CaptureMethod {
        CaptureMethod::UiAutomation
    }
    fn read_selection(&self) -> Result<SelectionContent, String> {
        Err("native selection capture is only implemented on Windows".to_string())
    }
    fn copy_to_clipboard(&self) -> Result<(), String> {
        Ok(())
    }
    fn foreground(&self) -> Option<SelectionTarget> {
        None
    }
    fn replace(
        &self,
        _: &AppHandle,
        _: &WebviewWindow,
        _: &SelectionTarget,
        _: &str,
        _: &str,
    ) -> Result<(), String> {
        Err("当前平台不支持选中文本替换。".into())
    }
}

/// No change counter or owner information.
#[derive(Default)]
pub struct UnsupportedClipboard;

impl ClipboardOps for UnsupportedClipboard {
    fn revision(&self) -> u32 {
        0
    }
    fn origin(&self) -> ClipboardOrigin {
        ClipboardOrigin::Unavailable
    }
    /// Polls every 500 ms; the caller skips unchanged content.
    fn listen(&self, on_change: &mut dyn FnMut()) -> Result<(), String> {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            on_change();
        }
    }
}

#[derive(Default)]
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

/// Placeholder statistics, no integrity enforcement, errors on stderr.
pub struct UnsupportedSystem;

impl SystemOps for UnsupportedSystem {
    fn memory_usage() -> Result<MemoryReport, String> {
        get_memory_usage()
    }
    fn trim_memory() -> Result<TrimResult, String> {
        trim_memory()
    }
    fn enforce_release_hash() -> bool {
        false
    }
    fn show_fatal_error(_title: &str, message: &str) {
        eprintln!("{message}");
    }
}

fn get_memory_usage() -> Result<MemoryReport, String> {
    Ok(MemoryReport {
        total_working_set_bytes: 40 * 1024 * 1024,
        total_private_bytes: 30 * 1024 * 1024,
        main_working_set_bytes: 20 * 1024 * 1024,
        process_count: 1,
    })
}

fn trim_memory() -> Result<TrimResult, String> {
    Ok(TrimResult {
        before_bytes: 40 * 1024 * 1024,
        after_bytes: 35 * 1024 * 1024,
        trimmed_bytes: 5 * 1024 * 1024,
        process_count: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::system::SystemOps;

    #[test]
    fn every_native_feature_reports_unavailable() {
        assert_eq!(UnsupportedBackend::CAPABILITIES, Capabilities::NONE);
        assert!(UnsupportedBackend::native_ocr().is_none());

        let selection = UnsupportedSelection;
        assert!(selection.read_selection().is_err());
        assert!(selection.copy_to_clipboard().is_ok());
        assert!(selection.foreground().is_none());

        let clipboard = UnsupportedClipboard;
        assert_eq!(clipboard.revision(), 0);
        assert_eq!(clipboard.origin(), ClipboardOrigin::Unavailable);

        let (events, _receiver) = std::sync::mpsc::sync_channel(1);
        assert!(UnsupportedGestures.observe(events).is_err());
        assert!(UnsupportedDesktop.bounds().is_err());
        assert!(UnsupportedWindow::get_global_cursor_pos().is_none());
        assert!(!UnsupportedSystem::enforce_release_hash());
        assert!(UnsupportedSystem::memory_usage().is_ok());
    }
}
