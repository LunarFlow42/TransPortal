//! Selected-text contract: reading the foreground selection, synthesizing a
//! copy, identifying the foreground input target and replacing the selection
//! in place. Foreground targets are opaque tokens outside the backend.

use serde::Serialize;
use tauri::{AppHandle, WebviewWindow};

/// Screen bounding box of selected text in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SelectionBounds {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl SelectionBounds {
    pub fn union(&self, other: &Self) -> Self {
        let left = self.left.min(other.left);
        let top = self.top.min(other.top);
        let right = (self.left + self.width).max(other.left + other.width);
        let bottom = (self.top + self.height).max(other.top + other.height);
        Self {
            left,
            top,
            width: (right - left).max(0),
            height: (bottom - top).max(0),
        }
    }
}

/// Content returned by a native selection reader.
#[derive(Debug, Clone)]
pub struct SelectionContent {
    pub text: String,
    pub bounds: Option<SelectionBounds>,
}

/// How a piece of text was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMethod {
    /// Native UI Automation / `TextPattern` (Windows only).
    UiAutomation,
    /// Clipboard fallback.
    Clipboard,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionTarget {
    pub window: usize,
    pub process: u32,
    pub focus: usize,
}

impl SelectionTarget {
    pub fn matches(&self, other: &Self) -> bool {
        if self.window != other.window || self.process != other.process {
            return false;
        }
        if self.focus != 0 && other.focus != 0 && self.focus != other.focus {
            return false;
        }
        true
    }
}

/// Compare selections independently of line-ending style and outer whitespace.
pub fn normalize_selection(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

pub trait SelectionOps: Send + Sync {
    /// How [`SelectionOps::read_selection`] obtains its text.
    fn method(&self) -> CaptureMethod;

    /// Read the foreground selection natively.
    fn read_selection(&self) -> Result<SelectionContent, String>;

    /// Ask the foreground window to copy its selection and wait until the
    /// clipboard changes. A no-op where copies cannot be synthesized; the
    /// caller then reads whatever the clipboard holds.
    fn copy_to_clipboard(&self) -> Result<(), String>;

    /// The foreground input target, unless it belongs to this process.
    fn foreground(&self) -> Option<SelectionTarget>;

    /// Replace the selection `expected` inside `target` with `replacement`,
    /// preserving the user's clipboard. `owner` is our window that owns the
    /// clipboard while it is borrowed.
    fn replace(
        &self,
        app: &AppHandle,
        owner: &WebviewWindow,
        target: &SelectionTarget,
        expected: &str,
        replacement: &str,
    ) -> Result<(), String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_selection_bounds_union() {
        let b1 = SelectionBounds {
            left: 100,
            top: 200,
            width: 50,
            height: 20,
        };
        let b2 = SelectionBounds {
            left: 120,
            top: 220,
            width: 80,
            height: 30,
        };
        let union = b1.union(&b2);
        assert_eq!(union.left, 100);
        assert_eq!(union.top, 200);
        assert_eq!(union.width, 100); // right = 200, left = 100 -> 100
        assert_eq!(union.height, 50); // bottom = 250, top = 200 -> 50
    }

    #[test]
    fn normalize_selection_ignores_line_endings_and_outer_whitespace() {
        assert_eq!(normalize_selection("  a\r\nb\rc \n"), "a\nb\nc");
        assert_eq!(
            normalize_selection("line one\r\nline two"),
            normalize_selection("line one\nline two")
        );
        assert_ne!(normalize_selection("a b"), normalize_selection("a  b"));
    }

    #[test]
    fn test_selection_target_matches() {
        let t1 = SelectionTarget {
            window: 100,
            process: 200,
            focus: 0,
        };
        let t2 = SelectionTarget {
            window: 100,
            process: 200,
            focus: 300,
        };
        assert!(t1.matches(&t2));
        assert!(t2.matches(&t1));

        let t3 = SelectionTarget {
            window: 100,
            process: 200,
            focus: 300,
        };
        assert!(t2.matches(&t3));

        let t4 = SelectionTarget {
            window: 100,
            process: 200,
            focus: 400,
        };
        assert!(!t2.matches(&t4));

        let t5 = SelectionTarget {
            window: 101,
            process: 200,
            focus: 0,
        };
        assert!(!t1.matches(&t5));

        let t6 = SelectionTarget {
            window: 100,
            process: 201,
            focus: 0,
        };
        assert!(!t1.matches(&t6));
    }
}
