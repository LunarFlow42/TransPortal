//! Overlay window plumbing: DWM attributes, sizing, placement and native hide.

use crate::platform::SelectionBounds;
use crate::platform::placement::compute_smart_placement;

/// Configure a transparent borderless window on Windows:
/// - Disables DWM border drawing (`DWMWA_BORDER_COLOR` = `DWMWA_COLOR_NONE`)
/// - Disables DWM native corner rounding (`DWMWA_WINDOW_CORNER_PREFERENCE` = `DWMWCP_DONOTROUND`)
pub fn configure_transparent_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_COLOR_NONE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
        DwmSetWindowAttribute,
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
            // Disable OS-default window transition animations (DWMWA_TRANSITIONS_FORCEDISABLED = 3)
            // so Windows doesn't apply its default zoom/fade or minimize/maximize stutter.
            let disable_transitions: windows::Win32::Foundation::BOOL = true.into();
            let _ = DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWINDOWATTRIBUTE(3),
                &disable_transitions as *const _ as *const _,
                std::mem::size_of::<windows::Win32::Foundation::BOOL>() as u32,
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
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
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
    use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

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
    use windows::Win32::Graphics::Dwm::{DWMWINDOWATTRIBUTE, DwmSetWindowAttribute};

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

/// Hide `window` with Win32 directly.
///
/// tao only calls `ShowWindow` when its cached visibility flag changes, and the
/// no-activate show path bypasses tao. After such a show tao still believes
/// the window is hidden, so `window.hide()` alone silently does nothing.
pub fn force_hide_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, ShowWindow};

    if let Ok(handle) = window.hwnd() {
        unsafe {
            let _ = ShowWindow(HWND(handle.0), SW_HIDE);
        }
    }
}

/// Whether `window` is the foreground window.
pub fn is_foreground_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    let foreground = unsafe { GetForegroundWindow() }.0 as usize;
    window
        .hwnd()
        .ok()
        .is_some_and(|handle| handle.0 as usize == foreground)
}

/// Intelligently places the window near the given anchor bounds or cursor,
/// with multi-monitor awareness, collision detection, and smart flip-to-top.
pub fn place_window_smartly<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    anchor_bounds: Option<&SelectionBounds>,
    center: bool,
    no_activate: bool,
) -> Result<(), String> {
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, HWND_TOPMOST, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOSIZE, SWP_SHOWWINDOW,
        SetWindowPos, ShowWindow,
    };

    let handle = window.hwnd().map_err(|e| e.to_string())?;
    let hwnd = HWND(handle.0);
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;

    let mut window_rect = RECT::default();
    let has_rect = unsafe { GetWindowRect(hwnd, &mut window_rect) }.is_ok();
    let (win_w, win_h) = if has_rect && (window_rect.right - window_rect.left) > 0 {
        (
            window_rect.right - window_rect.left,
            window_rect.bottom - window_rect.top,
        )
    } else if let Ok(size) = window.outer_size() {
        (size.width as i32, size.height as i32)
    } else {
        (
            (420.0 * scale_factor).round() as i32,
            (360.0 * scale_factor).round() as i32,
        )
    };

    let cursor_pos = get_global_cursor_pos();

    let ref_pt = if center {
        cursor_pos
            .map(|(cx, cy)| POINT { x: cx, y: cy })
            .unwrap_or(POINT {
                x: window_rect.left,
                y: window_rect.top,
            })
    } else if let Some(bounds) = anchor_bounds {
        POINT {
            x: bounds.left + (bounds.width / 2),
            y: bounds.top + (bounds.height / 2),
        }
    } else {
        cursor_pos
            .map(|(cx, cy)| POINT { x: cx, y: cy })
            .unwrap_or(POINT {
                x: window_rect.left,
                y: window_rect.top,
            })
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
