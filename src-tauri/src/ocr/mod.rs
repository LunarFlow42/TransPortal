//! Local OCR contracts and screenshot workflow. Platform APIs live in `native`.

pub mod assets;
pub mod onnx;
pub mod registry;
pub mod picture;
pub mod monitor;
pub mod region;
mod workflow;

use serde::{Deserialize, Serialize};
use std::sync::mpsc::SyncSender;
pub use workflow::{OcrService, OcrStatus};

pub const OCR_EVENT: &str = "transportal://ocr";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrMode {
    Text,
    #[serde(alias = "translate")]
    Picture,
    Card,
    Silent,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrStage {
    Capturing,
    Recognizing,
    Ready,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrEvent {
    pub id: u64,
    pub mode: OcrMode,
    pub stage: OcrStage,
    pub text: String,
    pub language: Option<String>,
    pub engine: String,
    pub image_path: Option<String>,
    pub picture: Option<picture::OcrPicture>,
    /// Recognition/preprocessing only; excludes the user's selection time.
    pub elapsed_ms: Option<u64>,
    pub copied: bool,
    pub copy_error: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrLanguage {
    pub tag: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrEngineInfo {
    pub id: String,
    pub label: String,
    pub languages: Vec<OcrLanguage>,
    pub available: bool,
    pub error: Option<String>,
}

pub struct RecognizedText {
    pub text: String,
    pub language: String,
    pub blocks: Vec<picture::OcrBlock>,
}

/// Borrowed RGBA pixels: an ONNX engine can implement the same contract without
/// changing screenshot capture, IPC, translation providers or frontend routing.
#[derive(Clone, Copy)]
pub struct OcrImage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

impl OcrImage<'_> {
    pub fn validate(&self) -> Result<(), String> {
        let pixels = u64::from(self.width) * u64::from(self.height);
        if pixels == 0 || pixels > 32_000_000 {
            return Err("截图尺寸无效或过大，请缩小选区后重试。".into());
        }
        if pixels * 4 != self.rgba.len() as u64 {
            return Err("截图像素数据不完整，请重新截图。".into());
        }
        Ok(())
    }
}

pub trait OcrEngine: Send + Sync {
    fn id(&self) -> &'static str;
    fn info(&self) -> Result<OcrEngineInfo, String>;
    fn prepare(&self) -> Result<(), String> {
        Ok(())
    }
    /// Drop cached model sessions when another engine is selected.
    fn release(&self) {}
    /// Blocking work; called exclusively from a worker thread.
    fn recognize(
        &self,
        image: OcrImage<'_>,
        language: Option<&str>,
    ) -> Result<RecognizedText, String>;
}

#[derive(Debug)]
pub enum ScreenEvent {
    /// Observe the OS-owned Win+Shift+S chord without consuming it.
    Snip {
        clipboard_revision: u32,
    },
    DoubleCopy { clipboard_revision: u32, window: usize },
    InkTranslate { window: usize },
    Cancel,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ClipboardOrigin {
    SnippingTool,
    Other,
    Unavailable,
}

/// Owns the observer's lifetime. Dropping it unhooks the keyboard observer.
pub trait ScreenObserver: Send + Sync {}

pub trait ScreenCapture: Send + Sync {
    fn supported(&self) -> bool;
    fn clipboard_revision(&self) -> u32;
    fn clipboard_origin(&self) -> ClipboardOrigin;
    fn observe(&self, events: SyncSender<ScreenEvent>) -> Result<Box<dyn ScreenObserver>, String>;
}

/// Preserve aspect ratio while satisfying the native engine's dimension limit.
pub fn fit_dimensions(width: u32, height: u32, limit: u32) -> Result<(u32, u32), String> {
    if width == 0 || height == 0 || limit == 0 {
        return Err("OCR 图片尺寸必须大于零。".into());
    }
    let longest = width.max(height);
    if longest <= limit {
        return Ok((width, height));
    }
    let fit = |side| ((u64::from(side) * u64::from(limit) / u64::from(longest)) as u32).max(1);
    Ok((fit(width), fit(height)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_dimensions_keep_aspect_ratio_and_do_not_upscale() {
        assert_eq!(fit_dimensions(800, 200, 2600).unwrap(), (800, 200));
        assert_eq!(fit_dimensions(5200, 2600, 2600).unwrap(), (2600, 1300));
        assert_eq!(fit_dimensions(1, u32::MAX, 2600).unwrap(), (1, 2600));
        assert!(fit_dimensions(0, 200, 2600).is_err());
        assert!(fit_dimensions(20, 200, 0).is_err());
    }

    #[test]
    fn rejects_bad_pixel_buffers_before_native_memory_access() {
        assert!(
            OcrImage {
                width: 2,
                height: 1,
                rgba: &[0; 8]
            }
            .validate()
            .is_ok()
        );
        assert!(
            OcrImage {
                width: 2,
                height: 1,
                rgba: &[0; 4]
            }
            .validate()
            .is_err()
        );
        assert!(
            OcrImage {
                width: u32::MAX,
                height: u32::MAX,
                rgba: &[]
            }
            .validate()
            .is_err()
        );
    }
}
