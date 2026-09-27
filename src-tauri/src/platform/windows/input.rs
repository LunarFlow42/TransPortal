//! Synthesized keyboard input, tagged so our own hook can ignore it.

use std::time::{Duration, Instant};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_C, VK_CONTROL,
    VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

/// Mark only our synthesized input, leaving accessibility/remapped keys usable.
pub(crate) const INPUT_MARKER: usize = 0x54524e53;

/// Ask the foreground window to copy its selection (Ctrl+C) and wait until the
/// clipboard actually changes.
///
/// This is the fallback for windows that do not expose UI Automation text
/// (PDF viewers, canvas-rendered apps, …). Returns `Err` when the clipboard
/// never changed, in which case the caller should not trust its contents.
pub fn copy_selection_to_clipboard() -> Result<(), String> {
    // SAFETY: `GetClipboardSequenceNumber` takes no arguments.
    let before = unsafe { GetClipboardSequenceNumber() };

    // The hotkey that triggered us (e.g. `Alt+Q`) is usually still physically held,
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
                dwExtraInfo: INPUT_MARKER,
            },
        },
    }
}
