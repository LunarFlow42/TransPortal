use super::memory;
use crate::platform::system::{MemoryReport, SystemOps, TrimResult};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW};
use windows::core::HSTRING;

pub struct WindowsSystem;

impl SystemOps for WindowsSystem {
    fn memory_usage() -> Result<MemoryReport, String> {
        memory::get_memory_usage()
    }

    fn trim_memory() -> Result<TrimResult, String> {
        memory::trim_memory()
    }

    fn enforce_release_hash() -> bool {
        true
    }

    fn show_fatal_error(title: &str, message: &str) {
        // SAFETY: both HSTRINGs live for the synchronous call; no parent window
        // exists yet because integrity is checked before Tauri initialization.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(message),
                &HSTRING::from(title),
                MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
            );
        }
    }
}
