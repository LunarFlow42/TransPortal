//! Tauri/GTK windows under compositor control. No global coordinate queries,
//! EWMH requests, fake HWNDs, forced focus stealing, or XWayland fallback.

use crate::platform::selection::SelectionBounds;
use crate::platform::window::{SizeMove, WindowOps};
use gtk::prelude::GtkWindowExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{Runtime, WebviewWindow};

static ACTIVATION: Mutex<Option<(String, std::time::Instant)>> = Mutex::new(None);
pub(super) fn activation_token(token: Option<String>) {
    if let Ok(mut current) = ACTIVATION.lock() {
        *current = token.map(|token| (token, std::time::Instant::now()));
    }
}
fn activate<R: Runtime>(window: &WebviewWindow<R>) {
    focus_policy(window, true);
    let _ = window.set_focus();
}

fn focus_policy<R: Runtime>(window: &WebviewWindow<R>, accept: bool) {
    let handle = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(native) = handle.gtk_window() {
            // Keep click-to-focus/typing available. Disabling accept-focus
            // permanently would make the overlay's query input unusable.
            native.set_accept_focus(true);
            native.set_focus_on_map(accept);
            if accept
                && let Some((token, time)) =
                    ACTIVATION.lock().ok().and_then(|mut token| token.take())
                && time.elapsed() < std::time::Duration::from_secs(5)
            {
                native.set_startup_id(&token);
            }
        }
    });
}

pub struct LinuxWindow;
pub struct PointerProbe;
impl crate::platform::window::PointerProbe for PointerProbe {
    fn window_visible(&self) -> bool {
        false
    }
    fn pressed_outside(&self) -> Option<bool> {
        None
    }
}

impl WindowOps for LinuxWindow {
    type Probe = PointerProbe;
    fn configure_transparent_window<R: Runtime>(_: &WebviewWindow<R>) {}
    fn resize_and_clamp_window<R: Runtime>(
        window: &WebviewWindow<R>,
        width: f64,
        height: f64,
    ) -> Result<(), String> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err("Invalid window dimensions".into());
        }
        let scale = window.scale_factor().map_err(|error| error.to_string())?;
        let (mut width, mut height) = (width, height);
        if let Some(monitor) = window
            .current_monitor()
            .map_err(|error| error.to_string())?
        {
            width = width.min(f64::from(monitor.size().width) / scale);
            height = height.min(f64::from(monitor.size().height) / scale);
        }
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|error| error.to_string())
    }
    fn set_window_theme<R: Runtime>(window: &WebviewWindow<R>, dark: bool) {
        let _ = window.set_theme(Some(if dark {
            tauri::Theme::Dark
        } else {
            tauri::Theme::Light
        }));
    }
    fn get_global_cursor_pos() -> Option<(i32, i32)> {
        None
    }
    fn force_hide_window<R: Runtime>(window: &WebviewWindow<R>) {
        let _ = window.hide();
    }
    fn force_show_window<R: Runtime>(window: &WebviewWindow<R>) {
        if window
            .title()
            .is_ok_and(|title| title.starts_with("TransPortal Overlay ["))
        {
            let _ = window.set_title("TransPortal");
        }
        focus_policy(window, true);
        let _ = window.unminimize();
        let _ = window.show();
        activate(window); // Compositor may deny activation; never bypass it.
    }
    fn is_window_visible<R: Runtime>(window: &WebviewWindow<R>) -> bool {
        window.is_visible().unwrap_or(false)
    }
    fn webview_visibility<R: Runtime>(_: &WebviewWindow<R>) -> Result<Option<bool>, String> {
        Ok(None)
    }
    fn is_foreground<R: Runtime>(window: &WebviewWindow<R>) -> bool {
        window.is_focused().unwrap_or(false)
    }
    fn pointer_probe<R: Runtime>(_: &WebviewWindow<R>) -> Option<Self::Probe> {
        None
    }
    fn observe_size_move<R: Runtime>(
        _: &WebviewWindow<R>,
        _: impl Fn(SizeMove) + Send + 'static,
    ) -> Result<(), String> {
        // GTK interactive drag lifecycle is not observable portably through Tao.
        Err("Wayland compositor owns interactive move/resize lifecycle".into())
    }
    fn place_window_smartly<R: Runtime>(
        window: &WebviewWindow<R>,
        _: Option<&SelectionBounds>,
        center: bool,
        no_activate: bool,
    ) -> Result<(), String> {
        // Only an explicitly installed KWin script understands this marker.
        // It uses KWin's logical coordinates; none are fabricated by GTK.
        static PLACEMENT: AtomicU64 = AtomicU64::new(0);
        if !center && no_activate {
            window
                .set_title(&format!(
                    "TransPortal Overlay [{}]",
                    PLACEMENT.fetch_add(1, Ordering::Relaxed)
                ))
                .map_err(|error| error.to_string())?;
        } else {
            window
                .set_title("TransPortal")
                .map_err(|error| error.to_string())?;
        }
        focus_policy(window, !no_activate);
        window.show().map_err(|error| error.to_string())?;
        if !no_activate {
            activate(window);
        }
        Ok(())
    }
    fn get_foreground_window() -> Option<usize> {
        None
    }
    fn restore_foreground_window(_: usize) {}
    fn set_ignore_cursor_events<R: Runtime>(
        window: &WebviewWindow<R>,
        ignore: bool,
    ) -> Result<(), String> {
        window
            .set_ignore_cursor_events(ignore)
            .map_err(|error| error.to_string())
    }
}
