//! Wayland-native Linux adapters. DISPLAY/XWayland is never used as a fallback
//! to bypass compositor permissions. X11 is intentionally not maintained here.

mod atspi;
mod clipboard;
mod desktop;
mod gestures;
mod portal;
mod remote_desktop;
mod selection;
mod shortcuts;
mod system;
#[cfg(test)]
mod test_bus;
mod window;

use super::{Backend, Capabilities};
use crate::core::ocr::OcrEngine;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Session {
    Wayland,
    Unsupported,
}

fn detect(session: Option<&str>, wayland: Option<&str>) -> Session {
    let present = |value: Option<&str>| value.is_some_and(|value| !value.trim().is_empty());
    if session.is_some_and(|value| value.eq_ignore_ascii_case("wayland")) || present(wayland) {
        Session::Wayland
    } else {
        Session::Unsupported
    }
}

fn session() -> Session {
    detect(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        std::env::var("WAYLAND_DISPLAY").ok().as_deref(),
    )
}

pub(super) fn capabilities() -> Capabilities {
    available(
        session(),
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        gestures::active(),
        gestures::replacement_available(),
    )
}

fn available(session: Session, bus: bool, gestures: bool, replacement: bool) -> Capabilities {
    match session {
        Session::Wayland => Capabilities {
            screen_capture: bus,
            keyboard_gestures: bus && gestures,
            // Keyboard authorization alone cannot enable text replacement.
            selection_replace: bus && replacement,
            ..Capabilities::NONE
        },
        Session::Unsupported => Capabilities::NONE,
    }
}

pub struct LinuxBackend;
impl Backend for LinuxBackend {
    type Window = window::LinuxWindow;
    type System = system::LinuxSystem;
    type Selection = selection::LinuxSelection;
    type Clipboard = clipboard::LinuxClipboard;
    type Gestures = gestures::KWinGestures;
    type Desktop = desktop::LinuxDesktop;
    type Shortcuts = shortcuts::WaylandShortcuts;
    // Static baseline. Shared code uses platform::capabilities() at runtime.
    const CAPABILITIES: Capabilities = Capabilities::NONE;
    fn native_ocr() -> Option<Arc<dyn OcrEngine>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wayland_wins_over_xwayland_and_headless_never_enables_native_capture() {
        assert_eq!(detect(Some("wayland"), None), Session::Wayland);
        assert_eq!(detect(None, Some("wayland-0")), Session::Wayland);
        assert_eq!(detect(Some("x11"), None), Session::Unsupported);
        assert_eq!(detect(None, Some("")), Session::Unsupported);
        assert_eq!(detect(None, None), Session::Unsupported);
        assert_eq!(
            available(Session::Unsupported, true, true, true),
            Capabilities::NONE
        );
        assert_eq!(
            available(Session::Wayland, false, true, true),
            Capabilities::NONE
        );
        let supported = available(Session::Wayland, true, false, false);
        assert!(supported.screen_capture);
        assert!(
            !supported.keyboard_gestures
                && !supported.selection_replace
                && !supported.clipboard_events
        );
        use crate::platform::desktop::DesktopCapture;
        assert!(!desktop::LinuxDesktop.supports_monitoring());
        assert!(available(Session::Wayland, true, true, true).keyboard_gestures);
        assert!(available(Session::Wayland, true, true, true).selection_replace);
        assert!(!available(Session::Wayland, true, true, false).selection_replace);
    }
}
