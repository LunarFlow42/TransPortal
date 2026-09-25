//! Physical screen coordinates and capture; no platform APIs escape this trait.

use serde::{Deserialize, Serialize};

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

    #[test]
    fn test_desktop_capture_pipeline() {
        let p = crate::platform::desktop();
        if let Ok(bounds) = p.bounds() {
            assert!(bounds.width > 0 && bounds.height > 0);
            let test_region = ScreenRect {
                x: bounds.x,
                y: bounds.y,
                width: 50,
                height: 50,
            };
            assert!(test_region.validate().is_ok());
            let result = p.capture(test_region);
            if let Ok(img) = result {
                assert_eq!(img.width(), 50);
                assert_eq!(img.height(), 50);
            }
        }
    }
}
