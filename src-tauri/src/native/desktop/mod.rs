//! Physical screen coordinates and capture; no platform APIs escape this trait.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[cfg(target_os = "windows")]
mod windows;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ScreenRect {
    pub fn validate(&self) -> Result<(), String> {
        if self.width < 2
            || self.height < 2
            || self.width > 16000
            || self.height > 16000
            || u64::from(self.width) * u64::from(self.height) > 32_000_000
        {
            return Err("选区无效或过大，请重新选择较小的区域。".into());
        }
        Ok(())
    }
    pub fn contains(&self, other: &Self) -> bool {
        i64::from(other.x) >= i64::from(self.x)
            && i64::from(other.y) >= i64::from(self.y)
            && i64::from(other.x) + i64::from(other.width)
                <= i64::from(self.x) + i64::from(self.width)
            && i64::from(other.y) + i64::from(other.height)
                <= i64::from(self.y) + i64::from(self.height)
    }
}

pub trait DesktopCapture: Send + Sync {
    fn bounds(&self) -> Result<ScreenRect, String>;
    fn capture(&self, region: ScreenRect) -> Result<image::RgbaImage, String>;
    /// Keep translated output out of later screenshots to avoid feedback loops.
    fn exclude(&self, window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String>;
}

pub fn platform() -> Arc<dyn DesktopCapture> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsDesktop)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(UnsupportedDesktop)
    }
}

#[cfg(not(target_os = "windows"))]
struct UnsupportedDesktop;
#[cfg(not(target_os = "windows"))]
impl DesktopCapture for UnsupportedDesktop {
    fn bounds(&self) -> Result<ScreenRect, String> {
        Err("当前平台尚未支持屏幕区域自动翻译。".into())
    }
    fn capture(&self, _: ScreenRect) -> Result<image::RgbaImage, String> {
        self.bounds().map(|_| unreachable!())
    }
    fn exclude(&self, _: &tauri::WebviewWindow, _: bool) -> Result<(), String> {
        self.bounds().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_regions_handle_negative_monitor_origins_without_overflow() {
        let desktop = ScreenRect {
            x: -1920,
            y: -200,
            width: 4480,
            height: 1640,
        };
        assert!(desktop.contains(&ScreenRect {
            x: -1800,
            y: -100,
            width: 600,
            height: 200
        }));
        assert!(!desktop.contains(&ScreenRect {
            x: i32::MAX,
            y: 0,
            width: u32::MAX,
            height: 20
        }));
        assert!(
            ScreenRect {
                x: 0,
                y: 0,
                width: u32::MAX,
                height: u32::MAX
            }
            .validate()
            .is_err()
        );
    }
}
