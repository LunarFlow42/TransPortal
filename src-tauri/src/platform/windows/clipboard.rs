//! Clipboard sequence numbers and ownership.

use crate::platform::clipboard::{ClipboardOps, ClipboardOrigin};
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::DataExchange::{GetClipboardOwner, GetClipboardSequenceNumber};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
use windows::core::PWSTR;

#[derive(Default)]
pub struct WindowsClipboard;

impl ClipboardOps for WindowsClipboard {
    fn revision(&self) -> u32 {
        // SAFETY: a read-only, argument-free sequence query.
        unsafe { GetClipboardSequenceNumber() }
    }

    fn origin(&self) -> ClipboardOrigin {
        // Only accept images published by Windows' snipping processes. Closing
        // the snip with its X button must not OCR a later, unrelated image copy.
        let owner_name = unsafe {
            let Ok(owner) = GetClipboardOwner() else {
                return ClipboardOrigin::Unavailable;
            };
            let mut pid = 0;
            GetWindowThreadProcessId(owner, Some(&mut pid));
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return ClipboardOrigin::Unavailable;
            };
            let mut path = [0u16; 32768];
            let mut len = path.len() as u32;
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(process);
            if result.is_err() {
                return ClipboardOrigin::Unavailable;
            }
            String::from_utf16_lossy(&path[..len as usize])
        };
        let filename = owner_name.rsplit(['\\', '/']).next().unwrap_or("");
        if [
            "SnippingTool.exe",
            "ScreenClippingHost.exe",
            "ScreenSketch.exe",
        ]
        .iter()
        .any(|name| filename.eq_ignore_ascii_case(name))
        {
            ClipboardOrigin::SnippingTool
        } else {
            ClipboardOrigin::Other
        }
    }
}
