//! Foreground targets are opaque tokens outside the platform adapter.

use std::sync::Arc;
use tauri::AppHandle;

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
        Arc::new(super::windows::WindowsSelection)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Arc::new(super::unsupported::UnsupportedSelection)
    }
}
