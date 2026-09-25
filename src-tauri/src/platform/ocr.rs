//! Factories keep all platform conditions out of the OCR workflow and commands.

use crate::core::ocr::{OcrEngine, ScreenCapture};
use std::sync::Arc;

pub fn engine() -> Arc<dyn OcrEngine> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(super::windows::WindowsOcrEngine::default())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(super::unsupported::UnavailableOcr)
    }
}

pub fn screen_capture() -> Arc<dyn ScreenCapture> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(super::windows::WindowsScreenCapture)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(super::unsupported::UnavailableOcr)
    }
}
