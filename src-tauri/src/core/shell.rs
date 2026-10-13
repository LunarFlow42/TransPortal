//! Overlay (`shell` window) visibility: show/hide ordering, outside-click
//! dismissal and move/resize reporting.

use crate::platform::PlatformWindow;
use crate::platform::window::{PointerProbe, SizeMove, WindowOps};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, Runtime, WebviewWindow};

pub const SHELL_LABEL: &str = "shell";
pub const SHELL_SHOWN_EVENT: &str = "transportal://shell-shown";
pub const OUTSIDE_CLICK_EVENT: &str = "transportal://outside-click";
/// `true` while the user moves or resizes the overlay, `false` when done.
pub const SIZE_MOVE_EVENT: &str = "transportal://shell-size-move";
/// `true` once a move/resize actually changes the size, `false` when done.
pub const RESIZING_EVENT: &str = "transportal://shell-resizing";

const OUTSIDE_CLICK_POLL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShellShownPayload {
    pub no_activate: bool,
    /// Bumped on every show; the frontend echoes it back in [`hide`].
    pub generation: u64,
}

static GENERATION: AtomicU64 = AtomicU64::new(0);
static WATCHING_CLICKS: AtomicBool = AtomicBool::new(false);
/// Written by the window subclass and read by [`hide`], both on the main thread.
static SIZE_MOVING: AtomicBool = AtomicBool::new(false);

/// Show the overlay through `show`, then announce it to the frontend.
///
/// The generation is bumped *before* showing, so a frontend dismiss that was
/// already in flight (see [`hide`]) is rejected instead of hiding the fresh
/// overlay.
pub fn present<R: Runtime>(
    app: &AppHandle<R>,
    no_activate: bool,
    show: impl FnOnce(&WebviewWindow<R>),
) {
    let Some(window) = app.get_webview_window(SHELL_LABEL) else {
        return;
    };
    let _ = PlatformWindow::set_ignore_cursor_events(&window, false);
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    show(&window);
    let _ = app.emit_to(
        SHELL_LABEL,
        SHELL_SHOWN_EVENT,
        ShellShownPayload {
            no_activate,
            generation,
        },
    );
    watch_outside_clicks(app, &window);
}

/// Hide the overlay unless it was shown again after `generation`, or the user
/// is moving or resizing it (grabbing it steals focus from the webview, and
/// that blur must not dismiss the window being dragged).
///
/// The check and the hide run as one main-thread task. A concurrent
/// [`present`] has either bumped the generation before that task (rejected)
/// or queues its show behind it (shown again). Must not be awaited on the
/// main thread. Returns whether the window was hidden.
pub async fn hide<R: Runtime>(app: &AppHandle<R>, generation: Option<u64>) -> Result<bool, String> {
    let Some(window) = app.get_webview_window(SHELL_LABEL) else {
        return Ok(false);
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let current = GENERATION.load(Ordering::SeqCst);
        let result = if may_hide(generation, current, SIZE_MOVING.load(Ordering::SeqCst)) {
            conceal(&window).map(|_| true)
        } else {
            Ok(false)
        };
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())?
}

/// Hide the overlay unconditionally (tray, OCR capture). Use this instead of
/// `window.hide()`, which is a no-op after a no-activate show; see [`conceal`].
pub fn hide_now<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(SHELL_LABEL) {
        let _ = conceal(&window);
    }
}

/// `window.hide()` keeps tao's cached flags in sync, but tao skips the actual
/// `ShowWindow` when it never saw the show (no-activate placement bypasses it),
/// so the native hide makes it stick either way.
fn conceal<R: Runtime>(window: &WebviewWindow<R>) -> Result<(), String> {
    let _ = PlatformWindow::set_ignore_cursor_events(window, true);
    let result = window.hide().map_err(|e| e.to_string());
    PlatformWindow::force_hide_window(window);
    result
}

fn may_hide(generation: Option<u64>, current: u64, size_moving: bool) -> bool {
    !size_moving && generation.is_none_or(|generation| generation == current)
}

/// Emit [`OUTSIDE_CLICK_EVENT`] for presses outside our windows while the
/// overlay is visible. A no-activate overlay never loses focus, so blur alone
/// cannot dismiss it.
fn watch_outside_clicks<R: Runtime>(app: &AppHandle<R>, window: &WebviewWindow<R>) {
    let Some(probe) = PlatformWindow::pointer_probe(window) else {
        return;
    };
    if WATCHING_CLICKS.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            // A press already held when the overlay appears is not a dismissal.
            let mut was_pressed = true;
            while probe.window_visible() {
                let outside = probe.pressed_outside();
                if outside == Some(true) && !was_pressed {
                    let _ = app.emit_to(SHELL_LABEL, OUTSIDE_CLICK_EVENT, ());
                }
                was_pressed = outside.is_some();
                std::thread::sleep(OUTSIDE_CLICK_POLL);
            }
            WATCHING_CLICKS.store(false, Ordering::SeqCst);
            // A show that raced with the exit above saw the flag still set.
            if !probe.window_visible() || WATCHING_CLICKS.swap(true, Ordering::SeqCst) {
                break;
            }
        }
    });
}

/// Forward moves and resizes of the overlay as [`SIZE_MOVE_EVENT`] and
/// [`RESIZING_EVENT`]: the frontend suspends auto-hide during the whole
/// interaction, stops auto-fitting while the size changes and saves the final
/// size.
pub fn observe_size_move<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(SHELL_LABEL) else {
        return;
    };
    let emitter = app.clone();
    let result = PlatformWindow::observe_size_move(&window, move |phase| match phase {
        SizeMove::Start => {
            SIZE_MOVING.store(true, Ordering::SeqCst);
            let _ = emitter.emit_to(SHELL_LABEL, SIZE_MOVE_EVENT, true);
        }
        SizeMove::Resize => {
            let _ = emitter.emit_to(SHELL_LABEL, RESIZING_EVENT, true);
        }
        SizeMove::End { resized } => {
            SIZE_MOVING.store(false, Ordering::SeqCst);
            if resized {
                let _ = emitter.emit_to(SHELL_LABEL, RESIZING_EVENT, false);
            }
            let _ = emitter.emit_to(SHELL_LABEL, SIZE_MOVE_EVENT, false);
        }
    });
    if let Err(e) = result {
        eprintln!("Overlay move/resize observation unavailable: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::may_hide;

    #[test]
    fn stale_hide_requests_are_rejected() {
        assert!(may_hide(None, 7, false));
        assert!(may_hide(Some(7), 7, false));
        assert!(!may_hide(Some(6), 7, false));
    }

    #[test]
    fn hides_wait_for_the_user_to_finish_moving_or_resizing() {
        assert!(!may_hide(None, 7, true));
        assert!(!may_hide(Some(7), 7, true));
    }
}
