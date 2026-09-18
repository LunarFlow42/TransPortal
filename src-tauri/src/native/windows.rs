//! Windows implementation of [`SelectionReader`] backed by UI Automation.
//!
//! Uses the `CUIAutomation` client to query the focused element for its
//! `TextPattern`, then extracts every selection range. This is the
//! "deep integration" path; if the focused control does not expose a
//! `TextPattern`, the caller falls back to the clipboard.

use super::{CaptureMethod, SelectionReader};
use std::time::{Duration, Instant};
use windows::core::BSTR;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_C, VK_CONTROL, VK_LWIN,
    VK_MENU, VK_RWIN, VK_SHIFT,
};

/// Ask the foreground window to copy its selection (Ctrl+C) and wait until the
/// clipboard actually changes.
///
/// This is the fallback for windows that do not expose UI Automation text
/// (PDF viewers, canvas-rendered apps, …). Returns `Err` when the clipboard
/// never changed, in which case the caller should not trust its contents.
pub fn copy_selection_to_clipboard() -> Result<(), String> {
    // SAFETY: `GetClipboardSequenceNumber` takes no arguments.
    let before = unsafe { GetClipboardSequenceNumber() };

    // The hotkey that triggered us (`Alt+T`) is usually still physically held,
    // which would turn our synthesized Ctrl+C into Alt+Ctrl+C. Wait for the
    // user to let go before injecting anything.
    wait_for_keys_released(400);
    release_modifiers();
    std::thread::sleep(Duration::from_millis(20));

    send_ctrl_c()?;

    // The foreground app has to service the keystrokes before the clipboard is
    // updated, so poll briefly (30ms x 14 ≈ 420ms).
    for attempt in 1..=14 {
        std::thread::sleep(Duration::from_millis(30));
        // SAFETY: see above.
        let after = unsafe { GetClipboardSequenceNumber() };
        if after != 0 && after != before {
            eprintln!("[capture] clipboard updated after {attempt} poll(s)");
            return Ok(());
        }
    }

    // Some sessions report sequence 0 when the counter is unavailable; treat
    // that as "unknown" and let the caller attempt a read anyway.
    if before == 0 {
        eprintln!("[capture] clipboard sequence unavailable; assuming the copy was sent");
        return Ok(());
    }

    Err("copy shortcut did not update the clipboard".to_string())
}

/// True when any non-mouse key is physically down.
fn any_key_down() -> bool {
    (0x08u16..=0xFE).any(|vk| {
        // SAFETY: `GetAsyncKeyState` is a simple state query.
        let state = unsafe { GetAsyncKeyState(vk as i32) };
        (state as u16 & 0x8000) != 0
    })
}

/// Wait until the user releases the hotkey, capped so a stuck key cannot hang.
fn wait_for_keys_released(max_ms: u64) {
    let start = Instant::now();
    while any_key_down() && start.elapsed().as_millis() < max_ms as u128 {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Synthesize key-up for modifiers that would corrupt the Ctrl+C chord.
fn release_modifiers() {
    for key in [VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN, VK_CONTROL] {
        let input = key_input(key, true);
        // SAFETY: `input` is a single valid `INPUT` record.
        unsafe {
            let _ = SendInput(&[input], size_of_record());
        }
    }
}

/// Synthesize a Ctrl+C key press.
fn send_ctrl_c() -> Result<(), String> {
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(VK_C, false),
        key_input(VK_C, true),
        key_input(VK_CONTROL, true),
    ];

    // SAFETY: `inputs` is a valid slice of `INPUT` records and the size is the
    // exact size of one record, as required by `SendInput`.
    let sent = unsafe { SendInput(&inputs, size_of_record()) };
    if sent as usize != inputs.len() {
        return Err(format!(
            "SendInput delivered {sent}/{} events",
            inputs.len()
        ));
    }
    Ok(())
}

fn size_of_record() -> i32 {
    std::mem::size_of::<INPUT>() as i32
}

/// Build a single keyboard `INPUT` record (with the scan code filled in, which
/// some apps require to recognise the chord).
pub(super) fn key_input(key: VIRTUAL_KEY, key_up: bool) -> INPUT {
    // SAFETY: `MapVirtualKeyW` is a pure lookup for this virtual key.
    let scan = unsafe { MapVirtualKeyW(key.0 as u32, MAPVK_VK_TO_VSC) } as u16;

    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
                dwFlags: if key_up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: super::INPUT_MARKER,
            },
        },
    }
}

/// UI Automation based selection reader.
pub struct WindowsSelectionReader;

struct ComApartment(bool);
impl Drop for ComApartment { fn drop(&mut self) { if self.0 { unsafe { CoUninitialize(); } } } }

impl WindowsSelectionReader {
    pub fn new() -> Self {
        Self
    }

    /// Query the focused element's `TextPattern` and concatenate its selection
    /// ranges.
    fn read_via_uia(&self) -> Result<String, String> {
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
            for index in 0..count {
                let range = ranges
                    .GetElement(index)
                    .map_err(|e| format!("TextRangeArray::GetElement failed: {e}"))?;
                // `-1` requests the entire range without a length cap.
                let text: BSTR = range
                    .GetText(-1)
                    .map_err(|e| format!("TextRange::GetText failed: {e}"))?;
                captured.push_str(&text.to_string());
            }

            Ok(captured)
        }
    }
}

impl Default for WindowsSelectionReader {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectionReader for WindowsSelectionReader {
    fn method(&self) -> CaptureMethod {
        CaptureMethod::UiAutomation
    }

    fn read_selection(&self) -> Result<String, String> {
        self.read_via_uia()
    }
}
