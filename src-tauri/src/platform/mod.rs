//! Platform layer: OS contracts, portable helpers and one backend per target.
//!
//! `crate::core` depends only on what this module exports: the contracts
//! (`WindowOps`, `SelectionOps`, `ClipboardOps`, `GestureSource`,
//! `DesktopCapture`, `SystemOps`), their types, the `Platform*` aliases,
//! the factories and [`CAPABILITIES`]. `#[cfg(target_os)]` appears only in
//! this file and inside the backends.

use clipboard::ClipboardOps;
use desktop::DesktopCapture;
use selection::SelectionOps;
use std::sync::Arc;
use system::SystemOps;
use window::WindowOps;

pub mod clipboard;
pub mod desktop;
pub mod imaging;
pub mod key_sequence;
pub mod ocr;
pub mod placement;
pub mod selection;
pub mod system;
pub mod window;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(test, not(target_os = "windows")))]
pub mod unsupported;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use self::linux::LinuxBackend as ActiveBackend;
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub use self::unsupported::UnsupportedBackend as ActiveBackend;
#[cfg(target_os = "windows")]
pub use self::windows::WindowsBackend as ActiveBackend;

/// Native features the active backend provides. Shared code reads these flags
/// instead of checking the target OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Global keyboard observation: Ctrl+C+C, the ink key and Esc.
    pub keyboard_gestures: bool,
    /// Observing the OS screenshot chord and the image it publishes.
    pub system_snip: bool,
    /// Replacing the selection inside the foreground application.
    pub selection_replace: bool,
    /// Desktop capture for region OCR and auto region translation.
    pub screen_capture: bool,
    /// Clipboard change notifications; without them the clipboard is polled.
    pub clipboard_events: bool,
}

impl Capabilities {
    pub const NONE: Self = Self {
        keyboard_gestures: false,
        system_snip: false,
        selection_replace: false,
        screen_capture: false,
        clipboard_events: false,
    };
    pub const ALL: Self = Self {
        keyboard_gestures: true,
        system_snip: true,
        selection_replace: true,
        screen_capture: true,
        clipboard_events: true,
    };
}

/// One implementation per target OS. Every backend must name a type for each
/// contract, so a backend that falls behind the contracts fails to compile.
pub trait Backend {
    type Window: WindowOps;
    type System: SystemOps;
    type Selection: SelectionOps + Default;
    type Clipboard: ClipboardOps + Default;
    type Desktop: DesktopCapture + Default;
    const CAPABILITIES: Capabilities;
}

pub type PlatformWindow = <ActiveBackend as Backend>::Window;
pub type PlatformSystem = <ActiveBackend as Backend>::System;
pub type PlatformSelection = <ActiveBackend as Backend>::Selection;
pub type PlatformClipboard = <ActiveBackend as Backend>::Clipboard;
pub type PlatformDesktop = <ActiveBackend as Backend>::Desktop;

/// A desktop capturer of the active backend.
pub fn desktop() -> Arc<dyn DesktopCapture> {
    Arc::new(PlatformDesktop::default())
}

/// Capabilities of the backend compiled for this target.
pub const CAPABILITIES: Capabilities = <ActiveBackend as Backend>::CAPABILITIES;

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-time and runtime check that a backend provides every contract.
    fn exercise<B: Backend>() -> Capabilities {
        let _probe: Option<<B::Window as WindowOps>::Probe> = None;
        let _: fn() -> bool = B::System::enforce_release_hash;
        let _: fn() -> B::Selection = B::Selection::default;
        let _: fn() -> B::Clipboard = B::Clipboard::default;
        let _: fn() -> B::Desktop = B::Desktop::default;
        B::CAPABILITIES
    }

    #[test]
    fn active_backend_provides_every_contract() {
        assert_eq!(exercise::<ActiveBackend>(), CAPABILITIES);
    }

    #[test]
    fn unsupported_backend_provides_every_contract() {
        assert_eq!(
            exercise::<unsupported::UnsupportedBackend>(),
            Capabilities::NONE
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_backend_provides_every_capability() {
        assert_eq!(CAPABILITIES, Capabilities::ALL);
    }
}
