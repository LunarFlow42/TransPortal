//! Windows implementation of [`SelectionOps::read_selection`](crate::platform::selection::SelectionOps::read_selection) backed by UI Automation.
//!
//! Uses the `CUIAutomation` client to query the focused element for its
//! `TextPattern`, then extracts every selection range. This is the
//! "deep integration" path; if the focused control does not expose a
//! `TextPattern`, the caller falls back to the clipboard.

use crate::platform::selection::{SelectionBounds, SelectionContent};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::core::BSTR;

/// UI Automation based selection reader.
pub struct WindowsSelectionReader;

struct ComApartment(bool);
impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

impl WindowsSelectionReader {
    pub fn new() -> Self {
        Self
    }

    /// Query the focused element's `TextPattern` and concatenate its selection
    /// ranges along with physical bounding rectangles.
    pub fn read_via_uia(&self) -> Result<SelectionContent, String> {
        // SAFETY: every call below is a documented Win32/UIA operation. COM is
        // initialized (or already initialized) on the calling thread before any
        // interface is created.
        unsafe {
            // `CoInitializeEx` returns `S_FALSE` when COM is already initialized
            // and `RPC_E_CHANGED_MODE` when the apartment mode differs. Both are
            // harmless here, so the result is intentionally ignored.
            let _apartment = ComApartment(CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok());

            let automation: IUIAutomation =
                CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("CoCreateInstance(CUIAutomation) failed: {e}"))?;

            let focused = automation
                .GetFocusedElement()
                .map_err(|e| format!("GetFocusedElement failed: {e}"))?;

            let text_pattern: IUIAutomationTextPattern = focused
                .GetCurrentPatternAs(UIA_TextPatternId)
                .map_err(|e| format!("UIA TextPattern unavailable: {e}"))?;

            let ranges = text_pattern
                .GetSelection()
                .map_err(|e| format!("TextPattern::GetSelection failed: {e}"))?;

            let count = ranges.Length().unwrap_or(0);
            let mut captured = String::new();
            let mut combined_bounds: Option<SelectionBounds> = None;

            for index in 0..count {
                let range = ranges
                    .GetElement(index)
                    .map_err(|e| format!("TextRangeArray::GetElement failed: {e}"))?;
                // `-1` requests the entire range without a length cap.
                let text: BSTR = range
                    .GetText(-1)
                    .map_err(|e| format!("TextRange::GetText failed: {e}"))?;
                captured.push_str(&text.to_string());

                if let Some(bounds) = range
                    .GetBoundingRectangles()
                    .ok()
                    .and_then(extract_bounds_from_safearray)
                {
                    combined_bounds = match combined_bounds {
                        Some(existing) => Some(existing.union(&bounds)),
                        None => Some(bounds),
                    };
                }
            }

            Ok(SelectionContent {
                text: captured,
                bounds: combined_bounds,
            })
        }
    }
}

/// Extract bounding box in physical screen coordinates from a SAFEARRAY of doubles [left, top, width, height, ...].
fn extract_bounds_from_safearray(
    sa: *mut windows::Win32::System::Com::SAFEARRAY,
) -> Option<SelectionBounds> {
    if sa.is_null() {
        return None;
    }
    use windows::Win32::System::Ole::{
        SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
        SafeArrayUnaccessData,
    };

    unsafe {
        let lbound = SafeArrayGetLBound(sa, 1).unwrap_or(0);
        let ubound = SafeArrayGetUBound(sa, 1).unwrap_or(-1);
        if ubound < lbound {
            let _ = SafeArrayDestroy(sa);
            return None;
        }
        let count = (ubound - lbound + 1) as usize;
        if count < 4 {
            let _ = SafeArrayDestroy(sa);
            return None;
        }
        let mut pdata: *mut std::ffi::c_void = std::ptr::null_mut();
        if SafeArrayAccessData(sa, &mut pdata).is_err() || pdata.is_null() {
            let _ = SafeArrayDestroy(sa);
            return None;
        }
        let doubles = std::slice::from_raw_parts(pdata as *const f64, count);
        let mut min_left = i32::MAX;
        let mut min_top = i32::MAX;
        let mut max_right = i32::MIN;
        let mut max_bottom = i32::MIN;
        let mut valid_rect_count = 0;

        for chunk in doubles.as_chunks::<4>().0 {
            let left = chunk[0].round() as i32;
            let top = chunk[1].round() as i32;
            let width = chunk[2].round() as i32;
            let height = chunk[3].round() as i32;

            if width > 0 && height > 0 {
                min_left = min_left.min(left);
                min_top = min_top.min(top);
                max_right = max_right.max(left + width);
                max_bottom = max_bottom.max(top + height);
                valid_rect_count += 1;
            }
        }

        let _ = SafeArrayUnaccessData(sa);
        let _ = SafeArrayDestroy(sa);

        if valid_rect_count > 0 && max_right > min_left && max_bottom > min_top {
            Some(SelectionBounds {
                left: min_left,
                top: min_top,
                width: max_right - min_left,
                height: max_bottom - min_top,
            })
        } else {
            None
        }
    }
}

impl Default for WindowsSelectionReader {
    fn default() -> Self {
        Self::new()
    }
}
