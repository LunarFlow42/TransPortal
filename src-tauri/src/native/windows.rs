//! Windows implementation of [`SelectionReader`] backed by UI Automation.
//!
//! Uses the `CUIAutomation` client to query the focused element for its
//! `TextPattern`, then extracts every selection range. This is the
//! "deep integration" path; if the focused control does not expose a
//! `TextPattern`, the caller falls back to the clipboard.

use super::{CaptureMethod, SelectionReader};
use std::time::{Duration, Instant};
use windows::core::BSTR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_LWIN,
    VK_MENU, VK_RWIN, VK_SHIFT,
};

/// Ask the foreground window to copy its selection (Ctrl+C) and wait until the
/// clipboard actually changes.
///
/// This is the fallback for windows that do not expose UI Automation text
/// (PDF viewers, canvas-rendered apps, …). Returns `Err` when the clipboard
/// never changed, in which case the caller should not trust its contents.
pub fn copy_selection_to_clipboard() -> Result<(), String> {
    // SAFETY: `GetClipboardSequenceNumber` takes no arguments.
    let before = unsafe { GetClipboardSequenceNumber() };

    // The hotkey that triggered us (`Alt+T`) is usually still physically held,
    // which would turn our synthesized Ctrl+C into Alt+Ctrl+C. Wait for the
    // user to let go before injecting anything.
    wait_for_keys_released(400);
    release_modifiers();
    std::thread::sleep(Duration::from_millis(20));

    send_ctrl_c()?;

    // The foreground app has to service the keystrokes before the clipboard is
    // updated, so poll briefly (30ms x 14 ≈ 420ms).
    for attempt in 1..=14 {
        std::thread::sleep(Duration::from_millis(30));
        // SAFETY: see above.
        let after = unsafe { GetClipboardSequenceNumber() };
        if after != 0 && after != before {
            eprintln!("[capture] clipboard updated after {attempt} poll(s)");
            return Ok(());
        }
    }

    // Some sessions report sequence 0 when the counter is unavailable; treat
    // that as "unknown" and let the caller attempt a read anyway.
    if before == 0 {
        eprintln!("[capture] clipboard sequence unavailable; assuming the copy was sent");
        return Ok(());
    }

    Err("copy shortcut did not update the clipboard".to_string())
}

/// True when any non-mouse key is physically down.
fn any_key_down() -> bool {
    (0x08u16..=0xFE).any(|vk| {
        // SAFETY: `GetAsyncKeyState` is a simple state query.
        let state = unsafe { GetAsyncKeyState(vk as i32) };
        (state as u16 & 0x8000) != 0
    })
}

/// Wait until the user releases the hotkey, capped so a stuck key cannot hang.
fn wait_for_keys_released(max_ms: u64) {
    let start = Instant::now();
    while any_key_down() && start.elapsed().as_millis() < max_ms as u128 {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Synthesize key-up for modifiers that would corrupt the Ctrl+C chord.
fn release_modifiers() {
    for key in [VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN, VK_CONTROL] {
        let input = key_input(key, true);
        // SAFETY: `input` is a single valid `INPUT` record.
        unsafe {
            let _ = SendInput(&[input], size_of_record());
        }
    }
}

/// Synthesize a Ctrl+C key press.
fn send_ctrl_c() -> Result<(), String> {
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_C, false),
        key_input(VK_C, true),
        key_input(VK_CONTROL, true),
    ];

    // SAFETY: `inputs` is a valid slice of `INPUT` records and the size is the
    // exact size of one record, as required by `SendInput`.
    let sent = unsafe { SendInput(&inputs, size_of_record()) };
    if sent as usize != inputs.len() {
        return Err(format!(
            "SendInput delivered {sent}/{} events",
            inputs.len()
        ));
    }
    Ok(())
}

fn size_of_record() -> i32 {
    std::mem::size_of::<INPUT>() as i32
}

/// Build a single keyboard `INPUT` record (with the scan code filled in, which
/// some apps require to recognise the chord).
pub(super) fn key_input(key: VIRTUAL_KEY, key_up: bool) -> INPUT {
    // SAFETY: `MapVirtualKeyW` is a pure lookup for this virtual key.
    let scan = unsafe { MapVirtualKeyW(key.0 as u32, MAPVK_VK_TO_VSC) } as u16;

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: super::INPUT_MARKER,
            },
        },
    }
}

/// UI Automation based selection reader.
pub struct WindowsSelectionReader;

struct ComApartment(bool);
impl Drop for ComApartment { fn drop(&mut self) { if self.0 { unsafe { CoUninitialize(); } } } }

impl WindowsSelectionReader {
    pub fn new() -> Self {
        Self
    }

    /// Query the focused element's `TextPattern` and concatenate its selection
    /// ranges along with physical bounding rectangles.
    fn read_via_uia(&self) -> Result<super::SelectionContent, String> {
        // SAFETY: every call below is a documented Win32/UIA operation. COM is
        // initialized (or already initialized) on the calling thread before any
        // interface is created.
        unsafe {
            // `CoInitializeEx` returns `S_FALSE` when COM is already initialized
            // and `RPC_E_CHANGED_MODE` when the apartment mode differs. Both are
            // harmless here, so the result is intentionally ignored.
            let _apartment = ComApartment(CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok());

            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(CUIAutomation) failed: {e}"))?;

            let focused = automation
                .GetFocusedElement()
                .map_err(|e| format!("GetFocusedElement failed: {e}"))?;

            let text_pattern: IUIAutomationTextPattern = focused
                .GetCurrentPatternAs(UIA_TextPatternId)
                .map_err(|e| format!("UIA TextPattern unavailable: {e}"))?;

            let ranges = text_pattern
                .GetSelection()
                .map_err(|e| format!("TextPattern::GetSelection failed: {e}"))?;

            let count = ranges.Length().unwrap_or(0);
            let mut captured = String::new();
            let mut combined_bounds: Option<super::SelectionBounds> = None;

            for index in 0..count {
                let range = ranges
                    .GetElement(index)
                    .map_err(|e| format!("TextRangeArray::GetElement failed: {e}"))?;
                // `-1` requests the entire range without a length cap.
                let text: BSTR = range
                    .GetText(-1)
                    .map_err(|e| format!("TextRange::GetText failed: {e}"))?;
                captured.push_str(&text.to_string());

                if let Ok(sa) = range.GetBoundingRectangles() {
                    if let Some(bounds) = extract_bounds_from_safearray(sa) {
                        combined_bounds = match combined_bounds {
                            Some(existing) => Some(existing.union(&bounds)),
                            None => Some(bounds),
                        };
                    }
                }
            }

            Ok(super::SelectionContent {
                text: captured,
                bounds: combined_bounds,
            })
        }
    }
}

/// Extract bounding box in physical screen coordinates from a SAFEARRAY of doubles [left, top, width, height, ...].
fn extract_bounds_from_safearray(
    sa: *mut windows::Win32::System::Com::SAFEARRAY,
) -> Option<super::SelectionBounds> {
    if sa.is_null() {
        return None;
    }
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
        SafeArrayUnaccessData,
    };

    unsafe {
        let lbound = SafeArrayGetLBound(sa, 1).unwrap_or(0);
        let ubound = SafeArrayGetUBound(sa, 1).unwrap_or(-1);
        if ubound < lbound {
            let _ = SafeArrayDestroy(sa);
            return None;
        }
        let count = (ubound - lbound + 1) as usize;
        if count < 4 {
            let _ = SafeArrayDestroy(sa);
            return None;
        }
        let mut pdata: *mut std::ffi::c_void = std::ptr::null_mut();
        if SafeArrayAccessData(sa, &mut pdata).is_err() || pdata.is_null() {
            let _ = SafeArrayDestroy(sa);
            return None;
        }
        let doubles = std::slice::from_raw_parts(pdata as *const f64, count);
        let mut min_left = i32::MAX;
        let mut min_top = i32::MAX;
        let mut max_right = i32::MIN;
        let mut max_bottom = i32::MIN;
        let mut valid_rect_count = 0;

        for chunk in doubles.chunks_exact(4) {
            let left = chunk[0].round() as i32;
            let top = chunk[1].round() as i32;
            let width = chunk[2].round() as i32;
            let height = chunk[3].round() as i32;

            if width > 0 && height > 0 {
                min_left = min_left.min(left);
                min_top = min_top.min(top);
                max_right = max_right.max(left + width);
                max_bottom = max_bottom.max(top + height);
                valid_rect_count += 1;
            }
        }

        let _ = SafeArrayUnaccessData(sa);
        let _ = SafeArrayDestroy(sa);

        if valid_rect_count > 0 && max_right > min_left && max_bottom > min_top {
            Some(super::SelectionBounds {
                left: min_left,
                top: min_top,
                width: max_right - min_left,
                height: max_bottom - min_top,
            })
        } else {
            None
        }
    }
}

impl Default for WindowsSelectionReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionReader for WindowsSelectionReader {
    fn method(&self) -> CaptureMethod {
        CaptureMethod::UiAutomation
    }

    fn read_selection(&self) -> Result<super::SelectionContent, String> {
        self.read_via_uia()
    }
}

/// Configure a transparent borderless window on Windows:
/// - Disables DWM border drawing (`DWMWA_BORDER_COLOR` = `DWMWA_COLOR_NONE`)
/// - Disables DWM native corner rounding (`DWMWA_WINDOW_CORNER_PREFERENCE` = `DWMWCP_DONOTROUND`)
pub fn configure_transparent_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE,
        DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
    };

    if let Ok(handle) = window.hwnd() {
        unsafe {
            let hwnd = HWND(handle.0);
            let color = DWMWA_COLOR_NONE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                &color as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );
            let corner = DWMWCP_DONOTROUND.0;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
}

/// Atomically resizes the window and clamps its position within the current monitor's working area.
/// This executes in a single Win32 compositor transaction, preventing visual jumps or off-screen overflow.
pub fn resize_and_clamp_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    width: f64,
    height: f64,
) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;
    let target_w = (width * scale_factor).round() as i32;
    let target_h = (height * scale_factor).round() as i32;

    let handle = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd = HWND(handle.0);

    unsafe {
        let mut window_rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut window_rect);

        let hmonitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(hmonitor, &mut mi).as_bool() {
            let work = mi.rcWork;
            let margin = (8.0 * scale_factor).round() as i32;
            let min_x = work.left + margin;
            let min_y = work.top + margin;
            let max_x = (work.right - target_w - margin).max(min_x);
            let max_y = (work.bottom - target_h - margin).max(min_y);

            let current_x = window_rect.left;
            let current_y = window_rect.top;
            let new_x = current_x.clamp(min_x, max_x);
            let new_y = current_y.clamp(min_y, max_y);

            let _ = SetWindowPos(
                hwnd,
                HWND::default(),
                new_x,
                new_y,
                target_w,
                target_h,
                SWP_NOACTIVATE | SWP_NOZORDER,
            );
            return Ok(());
        }
    }

    let physical_size = tauri::PhysicalSize::new(target_w as u32, target_h as u32);
    window.set_size(physical_size).map_err(|e| e.to_string())
}

/// Configures DWM system backdrop (Acrylic frosted glass or None) on Windows 11 (22H2+).
/// DWMSBT_TRANSIENTWINDOW (3) provides the Acrylic effect for transient/popover windows.
/// DWMSBT_NONE (1) disables the backdrop for transparent rendering.
pub fn set_window_acrylic<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, enabled: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};

    if let Ok(handle) = window.hwnd() {
        unsafe {
            let hwnd = HWND(handle.0);
            // DWMWA_SYSTEMBACKDROP_TYPE = 38
            // DWMSBT_AUTO = 0, DWMSBT_NONE = 1, DWMSBT_MAINWINDOW = 2, DWMSBT_TRANSIENTWINDOW = 3
            let backdrop_type: u32 = if enabled { 3 } else { 1 };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(38),
                &backdrop_type as *const _ as *const _,
                std::mem::size_of::<u32>() as u32,
            );
        }
    }
}

/// Updates DWM immersive dark mode preference so DWM backdrop materials (like Acrylic)
/// tint appropriately according to the active theme.
pub fn set_window_theme<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>, dark: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};

    if let Ok(handle) = window.hwnd() {
        unsafe {
            let hwnd = HWND(handle.0);
            // DWMWA_USE_IMMERSIVE_DARK_MODE = 20
            let dark_mode: i32 = if dark { 1 } else { 0 };
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWINDOWATTRIBUTE(20),
                &dark_mode as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
}

/// Returns the global physical cursor position across any monitor.
pub fn get_global_cursor_pos() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            Some((pt.x, pt.y))
        } else {
            None
        }
    }
}

/// Intelligently places the window near the given anchor bounds or cursor,
/// with multi-monitor awareness, collision detection, and smart flip-to-top.
pub fn place_window_smartly<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    anchor_bounds: Option<&super::SelectionBounds>,
    center: bool,
    no_activate: bool,
) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOSIZE,
        SWP_SHOWWINDOW, SW_SHOWNOACTIVATE,
    };

    let handle = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd = HWND(handle.0);
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;

    let mut window_rect = RECT::default();
    let has_rect = unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_ok();
    let (win_w, win_h) = if has_rect && (window_rect.right - window_rect.left) > 0 {
        (window_rect.right - window_rect.left, window_rect.bottom - window_rect.top)
    } else if let Ok(size) = window.outer_size() {
        (size.width as i32, size.height as i32)
    } else {
        ((420.0 * scale_factor).round() as i32, (360.0 * scale_factor).round() as i32)
    };

    let cursor_pos = get_global_cursor_pos();

    let ref_pt = if center {
        cursor_pos
            .map(|(cx, cy)| POINT { x: cx, y: cy })
            .unwrap_or(POINT { x: window_rect.left, y: window_rect.top })
    } else if let Some(bounds) = anchor_bounds {
        POINT {
            x: bounds.left + (bounds.width / 2),
            y: bounds.top + (bounds.height / 2),
        }
    } else {
        cursor_pos
            .map(|(cx, cy)| POINT { x: cx, y: cy })
            .unwrap_or(POINT { x: window_rect.left, y: window_rect.top })
    };

    let hmonitor = unsafe { MonitorFromPoint(ref_pt, MONITOR_DEFAULTTONEAREST) };
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let has_monitor = unsafe { GetMonitorInfoW(hmonitor, &mut mi) }.as_bool();
    let work = if has_monitor {
        mi.rcWork
    } else {
        RECT {
            left: 0,
            top: 0,
            right: (1920.0 * scale_factor).round() as i32,
            bottom: (1080.0 * scale_factor).round() as i32,
        }
    };

    let (clamped_x, clamped_y) = compute_smart_placement(
        (work.left, work.top, work.right, work.bottom),
        (win_w, win_h),
        scale_factor,
        anchor_bounds,
        cursor_pos,
        center,
    );

    unsafe {
        if no_activate {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                clamped_x,
                clamped_y,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_SHOWWINDOW,
            );
        } else {
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                clamped_x,
                clamped_y,
                0,
                0,
                SWP_NOSIZE | SWP_SHOWWINDOW,
            );
        }
    }

    Ok(())
}

/// Pure calculation of target position given monitor work area, window size, scale factor, anchor bounds or cursor, and center flag.
pub fn compute_smart_placement(
    work: (i32, i32, i32, i32), // (left, top, right, bottom)
    win_size: (i32, i32),       // (width, height)
    scale_factor: f64,
    anchor_bounds: Option<&super::SelectionBounds>,
    cursor_pos: Option<(i32, i32)>,
    center: bool,
) -> (i32, i32) {
    let (work_left, work_top, work_right, work_bottom) = work;
    let (win_w, win_h) = win_size;
    let margin = (8.0 * scale_factor).round() as i32;
    let offset = (12.0 * scale_factor).round() as i32;

    let (target_x, target_y) = if center {
        let x = work_left + ((work_right - work_left - win_w) / 2);
        let y = work_top + ((work_bottom - work_top - win_h) / 2);
        (x, y)
    } else if let Some(bounds) = anchor_bounds {
        let ideal_x = bounds.left;
        let bottom_y = bounds.top + bounds.height + offset;
        let top_y = bounds.top - win_h - offset;

        let chosen_y = if bottom_y + win_h <= work_bottom - margin {
            bottom_y
        } else if top_y >= work_top + margin {
            top_y
        } else {
            let space_below = work_bottom - bottom_y;
            let space_above = bounds.top - work_top;
            if space_below >= space_above {
                bottom_y
            } else {
                top_y
            }
        };
        (ideal_x, chosen_y)
    } else {
        let (cx, cy) = cursor_pos.unwrap_or((work_left + margin, work_top + margin));
        let cursor_offset_x = (12.0 * scale_factor).round() as i32;
        let cursor_offset_y = (16.0 * scale_factor).round() as i32;

        let ideal_x = cx + cursor_offset_x;
        let chosen_x = if ideal_x + win_w > work_right - margin
            && (cx - win_w - cursor_offset_x) >= work_left + margin
        {
            cx - win_w - cursor_offset_x
        } else {
            ideal_x
        };

        let bottom_y = cy + cursor_offset_y;
        let top_y = cy - win_h - cursor_offset_y;
        let chosen_y = if bottom_y + win_h <= work_bottom - margin {
            bottom_y
        } else if top_y >= work_top + margin {
            top_y
        } else {
            bottom_y
        };
        (chosen_x, chosen_y)
    };

    let clamped_x = target_x.clamp(
        work_left + margin,
        (work_right - win_w - margin).max(work_left + margin),
    );
    let clamped_y = target_y.clamp(
        work_top + margin,
        (work_bottom - win_h - margin).max(work_top + margin),
    );

    (clamped_x, clamped_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::SelectionBounds;

    #[test]
    fn test_selection_bounds_union() {
        let b1 = SelectionBounds {
            left: 100,
            top: 200,
            width: 50,
            height: 20,
        };
        let b2 = SelectionBounds {
            left: 120,
            top: 220,
            width: 80,
            height: 30,
        };
        let union = b1.union(&b2);
        assert_eq!(union.left, 100);
        assert_eq!(union.top, 200);
        assert_eq!(union.width, 100); // right = 200, left = 100 -> 100
        assert_eq!(union.height, 50); // bottom = 250, top = 200 -> 50
    }

    #[test]
    fn test_compute_smart_placement_center() {
        let work = (0, 0, 1920, 1040); // 1040 due to taskbar
        let win_size = (420, 500);
        let (x, y) = compute_smart_placement(work, win_size, 1.0, None, None, true);
        assert_eq!(x, (1920 - 420) / 2);
        assert_eq!(y, (1040 - 500) / 2);
    }

    #[test]
    fn test_compute_smart_placement_below_text() {
        let work = (0, 0, 1920, 1040);
        let win_size = (400, 300);
        let bounds = SelectionBounds {
            left: 200,
            top: 100,
            width: 150,
            height: 24,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        assert_eq!(x, 200);
        // Default offset is 12px below text (100 + 24 + 12 = 136)
        assert_eq!(y, 136);
    }

    #[test]
    fn test_compute_smart_placement_flips_above_when_near_bottom() {
        let work = (0, 0, 1920, 1040);
        let win_size = (400, 300);
        // Text near bottom of screen (top: 850, bottom: 874)
        // bottom_y = 874 + 12 = 886. 886 + 300 = 1186 > 1040 - 8 (1032) -> collision!
        // Should flip above: 850 - 300 - 12 = 538.
        let bounds = SelectionBounds {
            left: 200,
            top: 850,
            width: 150,
            height: 24,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        assert_eq!(x, 200);
        assert_eq!(y, 538);
    }

    #[test]
    fn test_compute_smart_placement_clamps_right_edge() {
        let work = (0, 0, 1920, 1040);
        let win_size = (400, 300);
        // Text near right edge (left: 1700, width: 200)
        let bounds = SelectionBounds {
            left: 1700,
            top: 200,
            width: 200,
            height: 24,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        // work.right - win_w - margin = 1920 - 400 - 8 = 1512
        assert_eq!(x, 1512);
        assert_eq!(y, 236);
    }

    #[test]
    fn test_compute_smart_placement_secondary_monitor() {
        // Secondary monitor positioned to the right: left = 1920, right = 3840
        let work = (1920, 0, 3840, 1080);
        let win_size = (400, 300);
        let bounds = SelectionBounds {
            left: 2100,
            top: 300,
            width: 100,
            height: 20,
        };
        let (x, y) = compute_smart_placement(work, win_size, 1.0, Some(&bounds), None, false);
        assert_eq!(x, 2100);
        assert_eq!(y, 332);
    }
}
