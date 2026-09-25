//! Window operations through tao/Tauri alone, without native window APIs.

pub fn configure_transparent_window<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}

pub fn resize_and_clamp_window<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;
    let target_w = (width * scale_factor).round() as u32;
    let target_h = (height * scale_factor).round() as u32;
    window
        .set_size(tauri::PhysicalSize::new(target_w, target_h))
        .map_err(|e| e.to_string())?;
    if let Ok(Some(monitor)) = window.current_monitor() {
        let work_pos = monitor.position();
        let work_size = monitor.size();
        if let Ok(pos) = window.outer_position() {
            let margin = (8.0 * scale_factor).round() as i32;
            let min_x = work_pos.x + margin;
            let min_y = work_pos.y + margin;
            let max_x = (work_pos.x + work_size.width as i32 - target_w as i32 - margin).max(min_x);
            let max_y =
                (work_pos.y + work_size.height as i32 - target_h as i32 - margin).max(min_y);
            let new_x = pos.x.clamp(min_x, max_x);
            let new_y = pos.y.clamp(min_y, max_y);
            let _ = window.set_position(tauri::PhysicalPosition::new(new_x, new_y));
        }
    }
    Ok(())
}

pub fn set_window_acrylic<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>, _enabled: bool) {}

pub fn set_window_theme<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>, _dark: bool) {}

pub fn get_global_cursor_pos() -> Option<(i32, i32)> {
    None
}

/// Other platforms only show windows through tao, so its own hide suffices.
pub fn force_hide_window<R: tauri::Runtime>(_window: &tauri::WebviewWindow<R>) {}

/// Global pointer sampling is Windows-only; the overlay relies on blur here.
pub struct PointerProbe;

impl crate::platform::window::PointerProbe for PointerProbe {
    fn window_visible(&self) -> bool {
        false
    }

    fn pressed_outside(&self) -> Option<bool> {
        None
    }
}

pub fn observe_size_move<R: tauri::Runtime>(
    _window: &tauri::WebviewWindow<R>,
    _on_phase: impl Fn(crate::platform::window::SizeMove) + Send + 'static,
) -> Result<(), String> {
    Err("move/resize observation is only implemented on Windows".to_string())
}

pub fn place_window_smartly<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    _anchor_bounds: Option<&crate::platform::SelectionBounds>,
    center: bool,
    _no_activate: bool,
) -> Result<(), String> {
    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;
    let window_size = window.outer_size().map_err(|e| e.to_string())?;
    let win_w = window_size.width as i32;
    let win_h = window_size.height as i32;

    if let Ok(Some(monitor)) = window.current_monitor() {
        let work_pos = monitor.position();
        let work_size = monitor.size();
        let margin = (8.0 * scale_factor).round() as i32;

        let (target_x, target_y) = if center {
            (
                work_pos.x + ((work_size.width as i32 - win_w) / 2),
                work_pos.y + ((work_size.height as i32 - win_h) / 2),
            )
        } else {
            let cursor = window.cursor_position().unwrap_or_default();
            (
                cursor.x as i32 + (12.0 * scale_factor).round() as i32,
                cursor.y as i32 + (16.0 * scale_factor).round() as i32,
            )
        };

        let clamped_x = target_x.clamp(
            work_pos.x + margin,
            (work_pos.x + work_size.width as i32 - win_w - margin).max(work_pos.x + margin),
        );
        let clamped_y = target_y.clamp(
            work_pos.y + margin,
            (work_pos.y + work_size.height as i32 - win_h - margin).max(work_pos.y + margin),
        );

        let _ = window.set_position(tauri::PhysicalPosition::new(clamped_x, clamped_y));
    }
    Ok(())
}
