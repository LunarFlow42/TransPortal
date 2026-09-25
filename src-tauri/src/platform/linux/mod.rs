//! Linux backend.
//!
//! Phase 1 (this file): every contract is served by [`unsupported`], so Linux
//! behaves like any target without native integration. The app runs with
//! ONNX OCR on image files, online translation and dictionaries, the tray,
//! regular global shortcuts and a polling clipboard monitor.
//!
//! Phase 2 replaces the delegated types one by one, each behind its own
//! capability flag:
//!
//! | Contract | Planned implementation |
//! |---|---|
//! | `WindowOps` | X11 EWMH hints; Wayland via layer-shell / xdg-activation where the compositor allows it |
//! | `SelectionOps` | AT-SPI (`atspi`) for reading the selection; synthesized copy/paste through libei (`reis`) or the RemoteDesktop portal |
//! | `ClipboardOps` | `wlr-data-control` / `ext-data-control` on Wayland, XFixes selection events on X11 |
//! | `GestureSource` | GlobalShortcuts portal (`ashpd`); libei for passive key observation where permitted |
//! | `DesktopCapture` | Screenshot portal for one-shot snips, ScreenCast portal + PipeWire (`pipewire`) for auto region translation |
//! | `SystemOps` | `/proc/self` statistics for the memory panel |
//!
//! [`unsupported`]: super::unsupported

use super::unsupported::UnsupportedBackend;
use super::{Backend, Capabilities};

pub struct LinuxBackend;

impl Backend for LinuxBackend {
    type Window = <UnsupportedBackend as Backend>::Window;
    type System = <UnsupportedBackend as Backend>::System;
    type Desktop = <UnsupportedBackend as Backend>::Desktop;
    const CAPABILITIES: Capabilities = <UnsupportedBackend as Backend>::CAPABILITIES;
}
