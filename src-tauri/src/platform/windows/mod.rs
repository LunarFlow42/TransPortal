//! Windows backend: Win32, UI Automation, DWM, DXGI and Windows.Media.Ocr.

mod clipboard_restore;
mod desktop;
mod dxgi;
mod input;
mod keyboard_hook;
mod memory;
mod ocr;
mod pointer;
mod selection;
mod size_move;
mod system;
mod uia;
mod window;

use super::window::{SizeMove, WindowOps};
use super::{Backend, Capabilities, SelectionBounds};
use tauri::{Runtime, WebviewWindow};

pub use desktop::WindowsDesktop;
pub use input::copy_selection_to_clipboard;
pub use keyboard_hook::WindowsScreenCapture;
pub use ocr::WindowsOcrEngine;
pub use selection::WindowsSelection;
pub use system::WindowsSystem;
pub use uia::WindowsSelectionReader;

pub struct WindowsBackend;

impl Backend for WindowsBackend {
    type Window = WindowsWindow;
    type System = WindowsSystem;
    const CAPABILITIES: Capabilities = Capabilities::ALL;
}

pub struct WindowsWindow;

impl WindowOps for WindowsWindow {
    type Probe = pointer::PointerProbe;

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

    fn is_foreground<R: Runtime>(window: &WebviewWindow<R>) -> bool {
        window::is_foreground_window(window)
    }

    fn pointer_probe<R: Runtime>(window: &WebviewWindow<R>) -> Option<Self::Probe> {
        pointer::PointerProbe::new(window)
    }

    fn observe_size_move<R: Runtime>(
        window: &WebviewWindow<R>,
        on_phase: impl Fn(SizeMove) + Send + 'static,
    ) -> Result<(), String> {
        size_move::observe_size_move(window, on_phase)
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
