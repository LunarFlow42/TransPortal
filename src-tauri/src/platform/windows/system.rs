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

    fn system_language() -> String {
        let lang_id = unsafe { GetUserDefaultUILanguage() };
        // Primary language 0x04 is LANG_CHINESE (Simplified, Traditional, etc.)
        if (lang_id & 0x03FF) == 0x04 {
            return "zh-CN".to_string();
        }
        // Primary language 0x09 is LANG_ENGLISH
        if (lang_id & 0x03FF) == 0x09 {
            return "en-US".to_string();
        }
        let mut buffer = [0u16; 85];
        let len = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
        if len > 1 {
            String::from_utf16_lossy(&buffer[..(len as usize - 1)])
        } else {
            "en-US".to_string()
        }
    }
}

#[cfg(target_os = "windows")]
unsafe extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
    fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
}
