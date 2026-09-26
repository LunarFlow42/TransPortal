//! Overlay window contract: transparency, no-activate placement, native hide,
//! outside-click probing, move/resize observation and backdrop materials.
//!
//! Methods are associated functions generic over the Tauri runtime, so the
//! backend is dispatched statically through [`super::PlatformWindow`].

use super::selection::SelectionBounds;
use tauri::{Runtime, WebviewWindow};

/// Phases of an interactive (modal) move or resize of a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeMove {
    /// The user grabbed an edge, the resize grip or a drag handle.
    Start,
    /// The first size change of the current interaction.
    Resize,
    /// The interaction ended; `resized` tells whether the size changed.
    End { resized: bool },
}

/// Samples global mouse presses relative to one of our windows.
///
/// The overlay is often shown without activation, so it never receives a blur
/// event; polling the global button state is the only way to notice a click
/// elsewhere on the desktop.
pub trait PointerProbe: Send + 'static {
    /// Whether the probed window is currently shown.
    fn window_visible(&self) -> bool;
    /// `None` while no mouse button is held; otherwise whether the cursor is
    /// outside every window owned by this process.
    fn pressed_outside(&self) -> Option<bool>;
}

pub trait WindowOps {
    type Probe: PointerProbe;

    /// Strip borders, rounded corners and OS transition animations from a
    /// transparent borderless window.
    fn configure_transparent_window<R: Runtime>(window: &WebviewWindow<R>);

    /// Resize to `width`×`height` logical pixels and keep the window inside
    /// its monitor's work area.
    fn resize_and_clamp_window<R: Runtime>(
        window: &WebviewWindow<R>,
        width: f64,
        height: f64,
    ) -> Result<(), String>;

    /// Tint native backdrop materials for the dark or light theme.
    fn set_window_theme<R: Runtime>(window: &WebviewWindow<R>, dark: bool);

    /// Global cursor position in physical pixels, across monitors.
    fn get_global_cursor_pos() -> Option<(i32, i32)>;

    /// Hide `window` even when it was shown behind the windowing library's
    /// back (no-activate placement).
    fn force_hide_window<R: Runtime>(window: &WebviewWindow<R>);

    /// Whether `window` is the OS foreground window.
    fn is_foreground<R: Runtime>(window: &WebviewWindow<R>) -> bool;

    /// A probe for presses outside `window`, where the platform allows it.
    fn pointer_probe<R: Runtime>(window: &WebviewWindow<R>) -> Option<Self::Probe>;

    /// Report interactive (modal) moves and resizes of `window`.
    fn observe_size_move<R: Runtime>(
        window: &WebviewWindow<R>,
        on_phase: impl Fn(SizeMove) + Send + 'static,
    ) -> Result<(), String>;

    /// Show `window` next to `anchor_bounds` (or the cursor), or centered on
    /// the cursor's monitor, optionally without activating it.
    fn place_window_smartly<R: Runtime>(
        window: &WebviewWindow<R>,
        anchor_bounds: Option<&SelectionBounds>,
        center: bool,
        no_activate: bool,
    ) -> Result<(), String>;
}
