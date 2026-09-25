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

pub use desktop::WindowsDesktop;
pub use input::copy_selection_to_clipboard;
pub use keyboard_hook::WindowsScreenCapture;
pub use memory::{get_memory_usage, trim_memory};
pub use ocr::WindowsOcrEngine;
pub use pointer::PointerProbe;
pub use selection::WindowsSelection;
pub use size_move::observe_size_move;
pub use system::WindowsIntegrity;
pub use uia::WindowsSelectionReader;
pub use window::{
    configure_transparent_window, force_hide_window, get_global_cursor_pos, place_window_smartly,
    resize_and_clamp_window, set_window_acrylic, set_window_theme,
};
