use crate::platform::system::IntegrityPlatform;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MB_SETFOREGROUND, MessageBoxW};
use windows::core::HSTRING;

pub struct WindowsIntegrity;

impl IntegrityPlatform for WindowsIntegrity {
    fn enforce_release_hash(&self) -> bool {
        true
    }

    fn show_failure(&self, message: &str) {
        // SAFETY: both HSTRINGs live for the synchronous call; no parent window
        // exists yet because integrity is checked before Tauri initialization.
        unsafe {
            MessageBoxW(
                None,
                &HSTRING::from(message),
                &HSTRING::from("TransPortal 完整性检查失败"),
                MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
            );
        }
    }
}
