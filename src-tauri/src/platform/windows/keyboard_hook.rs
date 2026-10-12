//! Observe the OS screenshot shortcut; never register or swallow Win+Shift+S.

use crate::platform::gestures::{GestureEvent, GestureObserver, GestureSource, InkKey};
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_ESCAPE, VK_S};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetForegroundWindow, GetMessageW, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL,
    WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

#[derive(Default)]
pub struct KeyboardHook;

/// Virtual-key code of the ink translation key; `0` when disabled.
static INK_KEY: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static EVENTS: RefCell<Option<SyncSender<GestureEvent>>> = const { RefCell::new(None) };
    static SNIP_KEY_DOWN: Cell<bool> = const { Cell::new(false) };
    static INK_KEY_DOWN: Cell<bool> = const { Cell::new(false) };
    static DOUBLE_COPY: RefCell<crate::platform::key_sequence::DoubleCopyDetector> = RefCell::new(Default::default());
    static MODIFIERS: RefCell<crate::platform::key_sequence::ModifierKeys> = RefCell::new(Default::default());
}

fn key_down(key: VIRTUAL_KEY) -> bool {
    // SAFETY: read-only query of a documented virtual key code.
    (unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16 & 0x8000) != 0
}

fn key_matches_ink(vk: u32, ink_vk: u32) -> bool {
    match ink_vk {
        0x11 => matches!(vk, 0x11 | 0xa2 | 0xa3),
        0x12 => matches!(vk, 0x12 | 0xa4 | 0xa5),
        0x10 => matches!(vk, 0x10 | 0xa0 | 0xa1),
        _ => vk == ink_vk,
    }
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: WH_KEYBOARD_LL supplies a valid structure for nonnegative codes.
        let key = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;
        let own_input = key.dwExtraInfo == super::input::INPUT_MARKER;
        if !own_input && (down || up) {
            MODIFIERS.with_borrow_mut(|keys| keys.update(key.vkCode, down));
        }
        if down || up {
            let window = unsafe { GetForegroundWindow() }.0 as usize;
            let (control, other) = MODIFIERS.with_borrow(|keys| (keys.control(), keys.other()));
            let double_copy = DOUBLE_COPY.with_borrow_mut(|detector| {
                detector.observe(
                    key.vkCode, down, control, other, own_input, key.time, window,
                )
            });
            if double_copy {
                let revision = unsafe { GetClipboardSequenceNumber() };
                EVENTS.with_borrow(|sender| {
                    if let Some(sender) = sender {
                        let _ = sender.try_send(GestureEvent::DoubleCopy {
                            clipboard_revision: revision,
                            window,
                        });
                    }
                });
            }

            let ink_vk = INK_KEY.load(Ordering::Relaxed);
            if ink_vk != 0 {
                if key_matches_ink(key.vkCode, ink_vk) {
                    if down {
                        INK_KEY_DOWN.set(true);
                    } else if up && INK_KEY_DOWN.replace(false) {
                        EVENTS.with_borrow(|sender| {
                            if let Some(sender) = sender {
                                let _ = sender.try_send(GestureEvent::InkTranslate { window });
                            }
                        });
                    }
                } else if down {
                    // Any other key press invalidates the pure ink gesture (e.g. Alt+Tab, Ctrl+C).
                    INK_KEY_DOWN.set(false);
                }
            }
        }
        // Observe remapped/accessibility shortcuts too. Nothing here injects
        // this chord, so there is no self-trigger loop.
        if key.vkCode == u32::from(VK_S.0) {
            if up {
                SNIP_KEY_DOWN.set(false);
            }
            if down && !SNIP_KEY_DOWN.replace(true) && MODIFIERS.with_borrow(|keys| keys.snip()) {
                // Sample BEFORE Windows handles the shortcut, so even a very
                // fast snip cannot make an old image look like a new one.
                let revision = unsafe { GetClipboardSequenceNumber() };
                EVENTS.with_borrow(|sender| {
                    if let Some(sender) = sender {
                        let _ = sender.try_send(GestureEvent::Snip {
                            clipboard_revision: revision,
                        });
                    }
                });
            }
        } else if down && key.vkCode == u32::from(VK_ESCAPE.0) {
            EVENTS.with_borrow(|sender| {
                if let Some(sender) = sender {
                    let _ = sender.try_send(GestureEvent::Cancel);
                }
            });
        }
    }
    // SAFETY: forward every keystroke unchanged, including cancellation.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

struct Observer {
    thread_id: u32,
    thread: Option<JoinHandle<()>>,
}
impl GestureObserver for Observer {}
impl Drop for Observer {
    fn drop(&mut self) {
        // SAFETY: this is the dedicated message queue we own, not a UI thread.
        let posted = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) };
        if posted.is_ok()
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
}

impl GestureSource for KeyboardHook {
    fn set_ink_key(key: Option<InkKey>) {
        let vk = match key {
            Some(InkKey::Backquote) => 0xC0, // VK_OEM_3
            Some(InkKey::CapsLock) => 0x14,  // VK_CAPITAL
            Some(InkKey::Control) => 0x11,   // VK_CONTROL
            Some(InkKey::Alt) => 0x12,       // VK_MENU
            Some(InkKey::Shift) => 0x10,     // VK_SHIFT
            Some(InkKey::Function(number)) => 0x6F + u32::from(number), // VK_F1 = 0x70
            None => 0,
        };
        INK_KEY.store(vk, Ordering::Relaxed);
    }

    fn observe(
        &self,
        events: SyncSender<GestureEvent>,
    ) -> Result<Box<dyn GestureObserver>, String> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("ocr-shortcut-observer".into())
            .spawn(move || {
                EVENTS.with_borrow_mut(|slot| *slot = Some(events));
                MODIFIERS.with_borrow_mut(|keys| {
                    for key in [0xa2, 0xa3, 0xa0, 0xa1, 0xa4, 0xa5, 0x5b, 0x5c] {
                        keys.update(key, key_down(VIRTUAL_KEY(key as u16)));
                    }
                });
                // SAFETY: hook and message loop live on the same dedicated thread;
                // the callback only queues bounded messages and always returns.
                unsafe {
                    let hook = GetModuleHandleW(None).and_then(|module| {
                        SetWindowsHookExW(
                            WH_KEYBOARD_LL,
                            Some(keyboard_hook),
                            Some(module.into()),
                            0,
                        )
                    });
                    match hook {
                        Ok(hook) => {
                            if ready_tx.send(Ok(GetCurrentThreadId())).is_ok() {
                                let mut message = MSG::default();
                                while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                                    let _ = TranslateMessage(&message);
                                    DispatchMessageW(&message);
                                }
                            }
                            let _ = UnhookWindowsHookEx(hook);
                        }
                        Err(error) => {
                            let _ = ready_tx.send(Err(format!("无法监听系统截图快捷键：{error}")));
                        }
                    }
                }
                EVENTS.with_borrow_mut(|slot| *slot = None);
            })
            .map_err(|e| e.to_string())?;
        let thread_id = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|e| format!("截图快捷键监听启动失败：{e}"))??;
        Ok(Box::new(Observer {
            thread_id,
            thread: Some(thread),
        }))
    }
}
