use super::{Session, session};
use crate::platform::desktop::{DesktopCapture, ScreenRect};

#[derive(Default)]
pub struct LinuxDesktop;
impl DesktopCapture for LinuxDesktop {
    fn select_image(
        &self,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Option<Result<image::RgbaImage, String>> {
        Some(if session() == Session::Wayland {
            super::portal::screenshot(cancelled)
        } else {
            Err("A Wayland session with a Screenshot portal is required".into())
        })
    }
    fn supports_monitoring(&self) -> bool {
        false
    }
    fn bounds(&self) -> Result<ScreenRect, String> {
        Err("Wayland does not expose global desktop coordinates; use the compositor screenshot dialog".into())
    }
    fn capture(&self, _: ScreenRect) -> Result<image::RgbaImage, String> {
        Err("Continuous capture requires a ScreenCast/PipeWire session; Screenshot permission is not reused for recording".into())
    }
    fn exclude(&self, _: &tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
        if enabled {
            Err("Wayland compositor capture exclusion is unavailable".into())
        } else {
            Ok(())
        }
    }
}
