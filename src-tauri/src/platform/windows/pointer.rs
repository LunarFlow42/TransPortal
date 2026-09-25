//! Global pointer sampling for outside-click dismissal.

use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

/// Samples global mouse presses relative to one of our windows.
///
/// The overlay is often shown without activation, so it never receives a blur
/// event; polling the global button state is the only way to notice a click
/// elsewhere on the desktop.
pub struct PointerProbe {
    hwnd: isize,
}

impl PointerProbe {
    pub fn new<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Option<Self> {
        let handle = window.hwnd().ok()?;
        Some(Self {
            hwnd: handle.0 as isize,
        })
    }
}

impl crate::platform::window::PointerProbe for PointerProbe {
    fn window_visible(&self) -> bool {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::IsWindowVisible;

        unsafe { IsWindowVisible(HWND(self.hwnd as *mut _)).as_bool() }
    }

    fn pressed_outside(&self) -> Option<bool> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::System::Threading::GetCurrentProcessId;
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
        use windows::Win32::UI::WindowsAndMessaging::{
            GA_ROOT, GetAncestor, GetCursorPos, GetWindowThreadProcessId, WindowFromPoint,
        };

        unsafe {
            let pressed = [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON]
                .iter()
                .any(|key| GetAsyncKeyState(key.0 as i32) < 0);
            if !pressed {
                return None;
            }
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return Some(false);
            }
            let root = GetAncestor(WindowFromPoint(pt), GA_ROOT);
            let mut pid = 0u32;
            GetWindowThreadProcessId(root, Some(&mut pid));
            Some(pid != GetCurrentProcessId())
        }
    }
}
