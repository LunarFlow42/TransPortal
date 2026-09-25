//! Factories keep all platform conditions out of the OCR workflow and commands.

use crate::core::ocr::{OcrEngine, ScreenCapture};
use std::sync::Arc;

#[cfg(not(target_os = "windows"))]
mod fallback;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
mod windows_capture;

pub fn engine() -> Arc<dyn OcrEngine> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsOcrEngine::default())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(fallback::UnavailableOcr)
    }
}

pub fn screen_capture() -> Arc<dyn ScreenCapture> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows_capture::WindowsScreenCapture)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(fallback::UnavailableOcr)
    }
}
