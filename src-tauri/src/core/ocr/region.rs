//! One shared, temporary native region selector for snips and auto-translation.
//!
//! The desktop is frozen into a full-screen webview (`#/ocr-region`) so the
//! user drags a rectangle over a still image. Snips crop that frozen image,
//! which needs no clipboard, no system screenshot tool and no compositor delay.

use super::OcrMode;
use super::picture::Cancellation;
use crate::platform::PlatformWindow;
use crate::platform::desktop::{DesktopCapture, ScreenRect};
use crate::platform::window::WindowOps;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::ImageEncoder;
use image::codecs::jpeg::JpegEncoder;
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
    Picture,
    Card,
    Silent,
    Search,
    Monitor,
}

impl RegionPurpose {
    pub fn is_snip(self) -> bool {
        matches!(
            self,
            Self::Text | Self::Translate | Self::Picture | Self::Card | Self::Silent | Self::Search
        )
    }
}

impl From<OcrMode> for RegionPurpose {
    fn from(mode: OcrMode) -> Self {
        match mode {
            OcrMode::Text => Self::Text,
            OcrMode::Picture => Self::Picture,
            OcrMode::Card => Self::Card,
            OcrMode::Silent => Self::Silent,
            OcrMode::Search => Self::Search,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct RegionSelection {
    pub id: u64,
    pub purpose: RegionPurpose,
    pub bounds: ScreenRect,
    pub image_data_url: Option<String>,
}

/// Encode the captured desktop snapshot as a lightweight JPEG Data URL for the UI freeze background.
pub fn encode_frozen_jpeg(image: &image::RgbaImage) -> Option<String> {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return None;
    }
    let raw = image.as_raw();
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    let (chunks, _) = raw.as_chunks::<4>();
    for chunk in chunks {
        rgb.extend_from_slice(&chunk[..3]);
    }
    let mut bytes = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut bytes, 85);
    encoder
        .write_image(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)))
}

/// A confirmed rectangle together with the frozen pixels it was drawn on.
pub struct ConfirmedRegion {
    pub purpose: RegionPurpose,
    pub owner: u64,
    pub region: ScreenRect,
    pub bounds: ScreenRect,
    pub frozen: Arc<image::RgbaImage>,
    pub foreground: Option<usize>,
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
    foreground: Option<usize>,
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
            desktop: crate::platform::desktop(),
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

    pub fn get_frozen_pixels(&self, id: u64) -> Result<Vec<u8>, String> {
        let state = self.state.lock().unwrap();
        let pending = state
            .pending
            .as_ref()
            .filter(|p| p.selection.id == id)
            .ok_or("区域选择已过期。")?;
        if pending.cancel.is_cancelled() {
            return Err("区域选择已取消。".into());
        }
        Ok(pending.frozen.as_raw().clone())
    }

    /// Keep our own overlay windows out of desktop captures.
    pub fn exclude_windows(&self, app: &AppHandle, enabled: bool) -> Result<(), String> {
        let mut result = Ok(());
        for label in ["shell", "settings", WINDOW_LABEL] {
            if let Some(window) = app.get_webview_window(label)
                && let Err(error) = self.desktop.exclude(&window, enabled)
                && enabled
            {
                result = Err(error);
            }
        }
        result
    }

    pub fn get_saved_foreground(&self, id: u64) -> Option<usize> {
        let state = self.state.lock().unwrap();
        state
            .pending
            .as_ref()
            .filter(|p| p.selection.id == id)
            .and_then(|p| p.foreground)
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
        let foreground = PlatformWindow::get_foreground_window();
        crate::core::shell::hide_now(app);
        // Exclude all overlay windows (including ocr-region) from capture
        let _ = self.exclude_windows(app, true);
        let bounds = match self.desktop.bounds() {
            Ok(b) => b,
            Err(e) => {
                Self::hide_window(app);
                if let Some(hwnd) = foreground {
                    PlatformWindow::restore_foreground_window(hwnd);
                }
                return Err(e);
            }
        };
        let image = match self.desktop.capture(bounds) {
            Ok(img) => img,
            Err(e) => {
                Self::hide_window(app);
                if let Some(hwnd) = foreground {
                    PlatformWindow::restore_foreground_window(hwnd);
                }
                return Err(e);
            }
        };
        if cancel.is_cancelled() {
            Self::hide_window(app);
            if let Some(hwnd) = foreground {
                PlatformWindow::restore_foreground_window(hwnd);
            }
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
                image_data_url: None,
            },
            owner,
            frozen: Arc::new(image),
            cancel,
            foreground,
        });
        Ok(id)
    }

    pub fn ensure_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = app.state::<RegionService>().desktop.exclude(&window, true);
            PlatformWindow::configure_transparent_window(&window);
            let _ = PlatformWindow::set_ignore_cursor_events(&window, true);
            Ok(window)
        } else {
            let bounds = app
                .state::<RegionService>()
                .desktop
                .bounds()
                .unwrap_or(ScreenRect {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                });
            let window = WebviewWindowBuilder::new(
                app,
                WINDOW_LABEL,
                WebviewUrl::App(format!("index.html#/{WINDOW_LABEL}").into()),
            )
            .title("TransPortal Region")
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .visible(false)
            .shadow(false)
            .transparent(true)
            .no_redirection_bitmap(true)
            .position(bounds.x as f64, bounds.y as f64)
            .inner_size(bounds.width as f64, bounds.height as f64)
            .build()
            .map_err(|e| e.to_string())?;
            PlatformWindow::configure_transparent_window(&window);
            let _ = PlatformWindow::set_ignore_cursor_events(&window, true);
            let _ = app.state::<RegionService>().desktop.exclude(&window, true);
            Ok(window)
        }
    }

    pub fn prewarm(app: &AppHandle) {
        if let Ok(window) = Self::ensure_window(app) {
            let service = app.state::<RegionService>();
            if let Ok(bounds) = service.desktop.bounds() {
                let _ = window.set_position(PhysicalPosition::new(bounds.x, bounds.y));
                let _ = window.set_size(PhysicalSize::new(bounds.width, bounds.height));
            }
        }
    }

    /// Main thread only: size the selector over the desktop and stage the transparent overlay.
    pub fn present(&self, app: &AppHandle, id: u64) -> Result<(), String> {
        let selection = {
            let state = self.state.lock().unwrap();
            let pending = state
                .pending
                .as_ref()
                .filter(|pending| pending.selection.id == id)
                .ok_or("区域选择已取消。")?;
            if pending.cancel.is_cancelled() {
                return Err("区域选择已取消。".into());
            }
            pending.selection.clone()
        };
        let bounds = selection.bounds;
        let window = Self::ensure_window(app)?;
        window
            .set_position(PhysicalPosition::new(bounds.x, bounds.y))
            .map_err(|e| e.to_string())?;
        window
            .set_size(PhysicalSize::new(bounds.width, bounds.height))
            .map_err(|e| e.to_string())?;
        let _ = window.emit(SELECTION_EVENT, &selection);
        // Fallback: display the ocr-region window if the frontend
        // doesn't trigger show_region_window within 350ms (prevents getting stuck on slow UI).
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
            let service = app_handle.state::<RegionService>();
            let should_show = {
                let state = service.state.lock().unwrap();
                state
                    .pending
                    .as_ref()
                    .is_some_and(|p| p.selection.id == id && !p.cancel.is_cancelled())
            };
            if should_show {
                let _ = Self::show_window(&app_handle);
            }
        });
        Ok(())
    }

    pub fn show_window(app: &AppHandle) -> Result<(), String> {
        let service = app.state::<RegionService>();
        {
            let state = service.state.lock().unwrap();
            let Some(pending) = state.pending.as_ref() else {
                return Ok(());
            };
            if pending.cancel.is_cancelled() {
                return Ok(());
            }
        }
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = PlatformWindow::set_ignore_cursor_events(&window, false);
            window.show().map_err(|e| e.to_string())?;
        }
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
                let foreground = current.foreground;
                state.pending = None;
                if let Some(hwnd) = foreground {
                    PlatformWindow::restore_foreground_window(hwnd);
                }
                return Err("区域选择已取消。".into());
            }
            if !current.selection.bounds.contains(&region) {
                return Err("选区超出屏幕范围。".into());
            }
            state.pending.take().ok_or("区域选择已过期。")?
        };
        Self::hide_window(app);
        Ok(ConfirmedRegion {
            purpose: pending.selection.purpose,
            owner: pending.owner,
            region,
            bounds: pending.selection.bounds,
            frozen: pending.frozen,
            foreground: pending.foreground,
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
                state.pending.take().map(|pending| {
                    pending.cancel.cancel();
                    (pending.selection.purpose, pending.owner, pending.foreground)
                })
            } else {
                None
            }
        };
        if let Some((purpose, owner, foreground)) = dropped {
            Self::hide_window(app);
            if let Some(hwnd) = foreground {
                PlatformWindow::restore_foreground_window(hwnd);
            }
            Some((purpose, owner))
        } else {
            None
        }
    }

    pub fn hide_window(app: &AppHandle) {
        if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
            let _ = window.emit("transportal://region-reset", ());
            let _ = PlatformWindow::set_ignore_cursor_events(&window, true);
            let _ = window.hide();
            PlatformWindow::force_hide_window(&window);
        }
    }
}

/// Route a confirmed rectangle to whichever feature asked for it.
pub fn confirm(app: &AppHandle, id: u64, region: ScreenRect) -> Result<(), String> {
    let confirmed = app.state::<RegionService>().take(app, id, region)?;
    match confirmed.purpose {
        RegionPurpose::Monitor => {
            app.state::<super::monitor::MonitorService>()
                .confirm(app, confirmed.owner, region)
        }
        RegionPurpose::Text
        | RegionPurpose::Translate
        | RegionPurpose::Picture
        | RegionPurpose::Card
        | RegionPurpose::Silent
        | RegionPurpose::Search => {
            app.state::<super::OcrService>()
                .finish_capture(app, confirmed);
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
            app.state::<super::monitor::MonitorService>()
                .stop(app, true);
        }
        Some((
            RegionPurpose::Text
            | RegionPurpose::Translate
            | RegionPurpose::Picture
            | RegionPurpose::Card
            | RegionPurpose::Silent
            | RegionPurpose::Search,
            _,
        )) => {
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
        let bounds = ScreenRect {
            x: -100,
            y: -50,
            width: 300,
            height: 200,
        };
        let mut frozen = image::RgbaImage::new(300, 200);
        frozen.put_pixel(120, 60, image::Rgba([1, 2, 3, 255]));
        let region = ScreenRect {
            x: 20,
            y: 10,
            width: 4,
            height: 4,
        };
        let cropped = crop(&frozen, bounds, region).unwrap();
        assert_eq!((cropped.width(), cropped.height()), (4, 4));
        assert_eq!(cropped.get_pixel(0, 0), &image::Rgba([1, 2, 3, 255]));
    }

    #[test]
    fn crop_rejects_regions_outside_or_mismatched_with_the_frozen_image() {
        let bounds = ScreenRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let frozen = image::RgbaImage::new(100, 100);
        assert!(
            crop(
                &frozen,
                bounds,
                ScreenRect {
                    x: 90,
                    y: 0,
                    width: 20,
                    height: 20
                }
            )
            .is_err()
        );
        assert!(
            crop(
                &frozen,
                bounds,
                ScreenRect {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1
                }
            )
            .is_err()
        );
        let stale = ScreenRect {
            x: 0,
            y: 0,
            width: 200,
            height: 100,
        };
        assert!(
            crop(
                &frozen,
                stale,
                ScreenRect {
                    x: 0,
                    y: 0,
                    width: 20,
                    height: 20
                }
            )
            .is_err()
        );
    }

    #[test]
    fn snip_purposes_follow_the_ocr_mode() {
        assert_eq!(RegionPurpose::from(OcrMode::Text), RegionPurpose::Text);
        assert_eq!(
            RegionPurpose::from(OcrMode::Picture),
            RegionPurpose::Picture
        );
        assert_eq!(RegionPurpose::from(OcrMode::Card), RegionPurpose::Card);
        assert_eq!(RegionPurpose::from(OcrMode::Silent), RegionPurpose::Silent);
        assert!(RegionPurpose::Picture.is_snip());
        assert!(RegionPurpose::Card.is_snip());
        assert!(RegionPurpose::Silent.is_snip());
        assert!(!RegionPurpose::Monitor.is_snip());
    }

    #[test]
    fn test_encode_frozen_jpeg() {
        let mut image = image::RgbaImage::new(10, 10);
        image.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let url = encode_frozen_jpeg(&image).expect("encode succeeds");
        assert!(url.starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn test_get_frozen_pixels() {
        let service = RegionService::default();
        let cancel = Cancellation::default();
        let mut state = service.state.lock().unwrap();
        let mut image = image::RgbaImage::new(10, 10);
        image.put_pixel(0, 0, image::Rgba([12, 34, 56, 255]));
        let bounds = ScreenRect {
            x: 0,
            y: 0,
            width: 10,
            height: 10,
        };
        state.pending = Some(Pending {
            selection: RegionSelection {
                id: 42,
                purpose: RegionPurpose::Text,
                bounds,
                image_data_url: None,
            },
            owner: 1,
            frozen: Arc::new(image),
            cancel,
            foreground: None,
        });
        drop(state);

        let pixels = service.get_frozen_pixels(42).expect("get pixels");
        assert_eq!(pixels.len(), 10 * 10 * 4);
        assert_eq!(&pixels[0..4], &[12, 34, 56, 255]);
    }
}
