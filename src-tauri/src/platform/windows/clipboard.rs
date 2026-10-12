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

    fn listen(&self, on_change: &mut dyn FnMut()) -> Result<(), String> {
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::DataExchange::{
            AddClipboardFormatListener, RemoveClipboardFormatListener,
        };
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            HWND_MESSAGE, MSG, PostQuitMessage, RegisterClassExW, TranslateMessage,
            WINDOW_EX_STYLE, WM_CLIPBOARDUPDATE, WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPED,
        };
        use windows::core::PCWSTR;

        unsafe extern "system" fn wndproc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            unsafe {
                if msg == WM_DESTROY {
                    PostQuitMessage(0);
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }

        let class_name = windows::core::w!("TransPortalClipboardListener");
        let hinstance = unsafe { GetModuleHandleW(None).unwrap_or_default() };

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        unsafe {
            let _ = RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                windows::core::w!(""),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                Some(hinstance.into()),
                None,
            );

            if let Ok(hwnd) = hwnd
                && !hwnd.is_invalid()
            {
                let _ = AddClipboardFormatListener(hwnd);

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    if msg.message == WM_CLIPBOARDUPDATE {
                        on_change();
                    }
                    let _ = TranslateMessage(&msg);
                    let _ = DispatchMessageW(&msg);
                }

                let _ = RemoveClipboardFormatListener(hwnd);
                let _ = DestroyWindow(hwnd);
                Ok(())
            } else {
                Err("无法创建剪贴板监听窗口。".into())
            }
        }
    }
}
