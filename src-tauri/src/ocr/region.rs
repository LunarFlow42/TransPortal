//! One shared, temporary native region selector for snips and auto-translation.
//!
//! The desktop is frozen into a full-screen webview (`#/ocr-region`) so the
//! user drags a rectangle over a still image. Snips crop that frozen image,
//! which needs no clipboard, no system screenshot tool and no compositor delay.

use super::picture::Cancellation;
use super::OcrMode;
use crate::native::desktop::{DesktopCapture, ScreenRect};
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

pub const SELECTION_EVENT: &str = "transportal://region-selection";
pub const WINDOW_LABEL: &str = "ocr-region";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionPurpose {
    Text,
    Translate,
    Monitor,
}

impl RegionPurpose {
    pub fn is_snip(self) -> bool {
        matches!(self, Self::Text | Self::Translate)
    }
}

impl From<OcrMode> for RegionPurpose {
    fn from(mode: OcrMode) -> Self {
        match mode {
            OcrMode::Text => Self::Text,
            OcrMode::Translate => Self::Translate,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct RegionSelection {
    pub id: u64,
    pub purpose: RegionPurpose,
    pub bounds: ScreenRect,
}

/// A confirmed rectangle together with the frozen pixels it was drawn on.
pub struct ConfirmedRegion {
    pub purpose: RegionPurpose,
    pub owner: u64,
    pub region: ScreenRect,
    pub bounds: ScreenRect,
    pub frozen: Arc<image::RgbaImage>,
}

impl ConfirmedRegion {
    /// Crop the frozen desktop instead of re-capturing the live screen.
    pub fn crop(&self) -> Result<image::RgbaImage, String> {
        crop(&self.frozen, self.bounds, self.region)
    }
}

pub fn crop(
    frozen: &image::RgbaImage,
    bounds: ScreenRect,
    region: ScreenRect,
) -> Result<image::RgbaImage, String> {
    region.validate()?;
    if !bounds.contains(&region)
        || frozen.width() != bounds.width
        || frozen.height() != bounds.height
    {
        return Err("选区超出屏幕范围。".into());
    }
    let x = (i64::from(region.x) - i64::from(bounds.x)) as u32;
    let y = (i64::from(region.y) - i64::from(bounds.y)) as u32;
    Ok(image::imageops::crop_imm(frozen, x, y, region.width, region.height).to_image())
}

struct Pending {
    selection: RegionSelection,
    owner: u64,
    frozen: Arc<image::RgbaImage>,
    cancel: Cancellation,
}

#[derive(Default)]
struct State {
    next_id: u64,
    pending: Option<Pending>,
}

pub struct RegionService {
    desktop: Arc<dyn DesktopCapture>,
    state: Mutex<State>,
}

impl Default for RegionService {
    fn default() -> Self {
        Self {
            desktop: crate::native::desktop::platform(),
            state: Mutex::new(State::default()),
        }
    }
}

impl RegionService {
    pub fn selection(&self) -> Option<RegionSelection> {
        let state = self.state.lock().unwrap();
        state
            .pending
            .as_ref()
            .filter(|pending| !pending.cancel.is_cancelled())
            .map(|pending| pending.selection.clone())
    }

    /// Keep our own overlay windows out of desktop captures.
    pub fn exclude_windows(&self, app: &AppHandle, enabled: bool) -> Result<(), String> {
        let mut result = Ok(());
        for label in ["shell", "settings"] {
            if let Some(window) = app.get_webview_window(label)
                && let Err(error) = self.desktop.exclude(&window, enabled)
                && enabled
            {
                result = Err(error);
            }
        }
        result
    }

    /// Blocking: grab the desktop in memory and stage it for selection.
    /// Any earlier pending selection is replaced.
    pub fn freeze(
        &self,
        app: &AppHandle,
        purpose: RegionPurpose,
        owner: u64,
        cancel: Cancellation,
    ) -> Result<u64, String> {
        if let Some(window) = app.get_webview_window("shell") {
            let _ = window.hide();
        }
        // A one-shot snip only needs the overlay hidden; continuous monitoring
        // must keep its own output out of every later capture.
        if let Err(error) = self.exclude_windows(app, true)
            && purpose == RegionPurpose::Monitor
        {
            return Err(error);
        }
        let bounds = self.desktop.bounds()?;
        let image = self.desktop.capture(bounds)?;
        if cancel.is_cancelled() {
            return Err("已取消区域选择。".into());
        }
        let mut state = self.state.lock().unwrap();
        state.next_id += 1;
        let id = state.next_id;
        state.pending = Some(Pending {
            selection: RegionSelection {
                id,
                purpose,
                bounds,
            },
            owner,
            frozen: Arc::new(image),
            cancel,
        });
        Ok(id)
    }

    pub fn ensure_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            Ok(window)
        } else {
            WebviewWindowBuilder::new(
                app,
                WINDOW_LABEL,
                WebviewUrl::App(format!("index.html#/{WINDOW_LABEL}").into()),
            )
            .title("选择屏幕区域")
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false)
            .shadow(false)
            .transparent(true)
            .build()
            .map_err(|e| e.to_string())
        }
    }

    pub fn prewarm(app: &AppHandle) {
        let _ = Self::ensure_window(app);
    }

    /// Main thread only: size the selector over the desktop and display the transparent overlay immediately.
    pub fn present(&self, app: &AppHandle, id: u64) -> Result<(), String> {
        let bounds = {
            let state = self.state.lock().unwrap();
            let pending = state
                .pending
                .as_ref()
                .filter(|pending| pending.selection.id == id)
                .ok_or("区域选择已取消。")?;
            if pending.cancel.is_cancelled() {
                return Err("区域选择已取消。".into());
            }
            pending.selection.bounds
        };
        let window = Self::ensure_window(app)?;
        window
            .set_position(PhysicalPosition::new(bounds.x, bounds.y))
            .map_err(|e| e.to_string())?;
        window
            .set_size(PhysicalSize::new(bounds.width, bounds.height))
            .map_err(|e| e.to_string())?;
        let _ = window.emit(SELECTION_EVENT, id);
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Consume the pending selection `id` with the user's rectangle.
    pub fn take(
        &self,
        app: &AppHandle,
        id: u64,
        region: ScreenRect,
    ) -> Result<ConfirmedRegion, String> {
        region.validate()?;
        let pending = {
            let mut state = self.state.lock().unwrap();
            let current = state
                .pending
                .as_ref()
                .filter(|pending| pending.selection.id == id)
                .ok_or("区域选择已过期。")?;
            if current.cancel.is_cancelled() {
                state.pending = None;
                return Err("区域选择已取消。".into());
            }
            if !current.selection.bounds.contains(&region) {
                return Err("选区超出屏幕范围。".into());
            }
            state.pending.take().unwrap()
        };
        Self::hide_window(app);
        Ok(ConfirmedRegion {
            purpose: pending.selection.purpose,
            owner: pending.owner,
            region,
            bounds: pending.selection.bounds,
            frozen: pending.frozen,
        })
    }

    /// Drop a pending selection when `matches` accepts it, hiding the selector.
    /// Returns the dropped selection's purpose and owner.
    pub fn dismiss(
        &self,
        app: &AppHandle,
        matches: impl Fn(RegionPurpose, u64) -> bool,
    ) -> Option<(RegionPurpose, u64)> {
        let dropped = {
            let mut state = self.state.lock().unwrap();
            let accept = state
                .pending
                .as_ref()
                .is_some_and(|pending| matches(pending.selection.purpose, pending.selection.id));
            if accept {
                let pending = state.pending.take().unwrap();
                pending.cancel.cancel();
                Some((pending.selection.purpose, pending.owner))
            } else {
                None
            }
        };
        if dropped.is_some() {
            Self::hide_window(app);
        }
        dropped
    }

    pub fn hide_window(app: &AppHandle) {
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = window.hide();
        }
    }
}

/// Route a confirmed rectangle to whichever feature asked for it.
pub fn confirm(app: &AppHandle, id: u64, region: ScreenRect) -> Result<(), String> {
    let confirmed = app.state::<RegionService>().take(app, id, region)?;
    match confirmed.purpose {
        RegionPurpose::Monitor => app
            .state::<super::monitor::MonitorService>()
            .confirm(app, confirmed.owner, region),
        RegionPurpose::Text | RegionPurpose::Translate => {
            app.state::<super::OcrService>().finish_capture(app, confirmed);
            Ok(())
        }
    }
}

/// Cancel the pending selection (`id`, or whatever is pending) and notify its owner.
pub fn cancel(app: &AppHandle, id: Option<u64>) {
    let dropped = app
        .state::<RegionService>()
        .dismiss(app, |_, pending| id.is_none_or(|id| id == pending));
    match dropped {
        Some((RegionPurpose::Monitor, _)) => {
            app.state::<super::monitor::MonitorService>().stop(app, true);
        }
        Some((RegionPurpose::Text | RegionPurpose::Translate, _)) => {
            app.state::<super::OcrService>().cancel(app);
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_uses_offsets_relative_to_a_negative_desktop_origin() {
        let bounds = ScreenRect { x: -100, y: -50, width: 300, height: 200 };
        let mut frozen = image::RgbaImage::new(300, 200);
        frozen.put_pixel(120, 60, image::Rgba([1, 2, 3, 255]));
        let region = ScreenRect { x: 20, y: 10, width: 4, height: 4 };
        let cropped = crop(&frozen, bounds, region).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (4, 4));
        assert_eq!(cropped.get_pixel(0, 0), &image::Rgba([1, 2, 3, 255]));
    }

    #[test]
    fn crop_rejects_regions_outside_or_mismatched_with_the_frozen_image() {
        let bounds = ScreenRect { x: 0, y: 0, width: 100, height: 100 };
        let frozen = image::RgbaImage::new(100, 100);
        assert!(crop(&frozen, bounds, ScreenRect { x: 90, y: 0, width: 20, height: 20 }).is_err());
        assert!(crop(&frozen, bounds, ScreenRect { x: 0, y: 0, width: 1, height: 1 }).is_err());
        let stale = ScreenRect { x: 0, y: 0, width: 200, height: 100 };
        assert!(crop(&frozen, stale, ScreenRect { x: 0, y: 0, width: 20, height: 20 }).is_err());
    }

    #[test]
    fn snip_purposes_follow_the_ocr_mode() {
        assert_eq!(RegionPurpose::from(OcrMode::Text), RegionPurpose::Text);
        assert_eq!(RegionPurpose::from(OcrMode::Translate), RegionPurpose::Translate);
        assert!(RegionPurpose::Translate.is_snip());
        assert!(!RegionPurpose::Monitor.is_snip());
    }
}
