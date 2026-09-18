//! Foreground targets are opaque tokens outside the platform adapter.

use std::sync::Arc;
use tauri::AppHandle;

#[cfg(target_os = "windows")]
mod clipboard;
#[cfg(target_os = "windows")]
mod windows;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionTarget {
    pub window: usize,
    pub process: u32,
    pub focus: usize,
}

pub trait SelectionPlatform: Send + Sync {
    fn foreground(&self) -> Option<SelectionTarget>;
    fn clipboard_revision(&self) -> u32;
    fn replace(
        &self,
        app: &AppHandle,
        target: &SelectionTarget,
        expected: &str,
        replacement: &str,
    ) -> Result<(), String>;
}

pub fn platform() -> Arc<dyn SelectionPlatform> {
    #[cfg(target_os = "windows")]
    {
        Arc::new(windows::WindowsSelection)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(UnsupportedSelection)
    }
}

#[cfg(not(target_os = "windows"))]
struct UnsupportedSelection;
#[cfg(not(target_os = "windows"))]
impl SelectionPlatform for UnsupportedSelection {
    fn foreground(&self) -> Option<SelectionTarget> {
        None
    }
    fn clipboard_revision(&self) -> u32 {
        0
    }
    fn replace(&self, _: &AppHandle, _: &SelectionTarget, _: &str, _: &str) -> Result<(), String> {
        Err("当前平台不支持选中文本替换。".into())
    }
}
