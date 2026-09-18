//! Non-Windows fallback for [`SelectionReader`].
//!
//! The deep native selection API is only implemented for Windows. On every
//! other platform this reader reports that UI Automation is unavailable so the
//! caller degrades to the clipboard.

use super::{CaptureMethod, SelectionReader};

/// Placeholder reader used on non-Windows targets.
pub struct FallbackSelectionReader;

impl FallbackSelectionReader {
    pub fn new() -> Self {
        Self
    }
}

impl Default for FallbackSelectionReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionReader for FallbackSelectionReader {
    fn method(&self) -> CaptureMethod {
        CaptureMethod::UiAutomation
    }

    fn read_selection(&self) -> Result<String, String> {
        Err("native selection capture is only implemented on Windows".to_string())
    }
}
