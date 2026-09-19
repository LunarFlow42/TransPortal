//! Observe the OS screenshot shortcut; never register or swallow Win+Shift+S.

use crate::ocr::{ClipboardOrigin, ScreenCapture, ScreenEvent, ScreenObserver};
use std::cell::{Cell, RefCell};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{GetClipboardOwner, GetClipboardSequenceNumber};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VIRTUAL_KEY, VK_ESCAPE, VK_S};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, GetWindowThreadProcessId, GetForegroundWindow, KBDLLHOOKSTRUCT, MSG,
    PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP,
};
use windows::core::PWSTR;

pub struct WindowsScreenCapture;

thread_local! {
    static EVENTS: RefCell<Option<SyncSender<ScreenEvent>>> = const { RefCell::new(None) };
    static SNIP_KEY_DOWN: Cell<bool> = const { Cell::new(false) };
    static INK_KEY_DOWN: Cell<bool> = const { Cell::new(false) };
    static DOUBLE_COPY: RefCell<crate::shortcut_sequence::DoubleCopyDetector> = RefCell::new(Default::default());
    static MODIFIERS: RefCell<crate::shortcut_sequence::ModifierKeys> = RefCell::new(Default::default());
}

fn key_down(key: VIRTUAL_KEY) -> bool {
    // SAFETY: read-only query of a documented virtual key code.
    (unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16 & 0x8000) != 0
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        // SAFETY: WH_KEYBOARD_LL supplies a valid structure for nonnegative codes.
        let key = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let message = wparam.0 as u32;
        let down = message == WM_KEYDOWN || message == WM_SYSKEYDOWN;
        let up = message == WM_KEYUP || message == WM_SYSKEYUP;
        let own_input = key.dwExtraInfo == crate::native::INPUT_MARKER;
        if !own_input && (down || up) { MODIFIERS.with_borrow_mut(|keys| keys.update(key.vkCode, down)); }
        if down || up {
            let window = unsafe { GetForegroundWindow() }.0 as usize;
            let (control, other) = MODIFIERS.with_borrow(|keys| (keys.control(), keys.other()));
            let double_copy = DOUBLE_COPY.with_borrow_mut(|detector| detector.observe(key.vkCode, down,
                control, other, own_input, key.time, window));
            if double_copy {
                let revision = unsafe { GetClipboardSequenceNumber() };
                EVENTS.with_borrow(|sender| { if let Some(sender) = sender {
                    let _ = sender.try_send(ScreenEvent::DoubleCopy { clipboard_revision: revision, window });
                } });
            }

            let ink_vk = crate::shortcut::INK_TRANSLATE_KEY.load(std::sync::atomic::Ordering::Relaxed);
            if ink_vk != 0 && key.vkCode == ink_vk {
                if down {
                    INK_KEY_DOWN.set(true);
                } else if up && INK_KEY_DOWN.replace(false) {
                    EVENTS.with_borrow(|sender| {
                        if let Some(sender) = sender {
                            let _ = sender.try_send(ScreenEvent::InkTranslate { window });
                        }
                    });
                }
            }
        }
        // Observe remapped/accessibility shortcuts too. Nothing here injects
        // this chord, so there is no self-trigger loop.
        if key.vkCode == u32::from(VK_S.0) {
            if up {
                SNIP_KEY_DOWN.set(false);
            }
            if down
                && !SNIP_KEY_DOWN.replace(true)
                && MODIFIERS.with_borrow(|keys| keys.snip())
            {
                // Sample BEFORE Windows handles the shortcut, so even a very
                // fast snip cannot make an old image look like a new one.
                let revision = unsafe { GetClipboardSequenceNumber() };
                EVENTS.with_borrow(|sender| {
                    if let Some(sender) = sender {
                        let _ = sender.try_send(ScreenEvent::Snip {
                            clipboard_revision: revision,
                        });
                    }
                });
            }
        } else if down && key.vkCode == u32::from(VK_ESCAPE.0) {
            EVENTS.with_borrow(|sender| {
                if let Some(sender) = sender {
                    let _ = sender.try_send(ScreenEvent::Cancel);
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
impl ScreenObserver for Observer {}
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

impl ScreenCapture for WindowsScreenCapture {
    fn supported(&self) -> bool {
        true
    }

    fn clipboard_revision(&self) -> u32 {
        // SAFETY: a read-only, argument-free sequence query.
        unsafe { GetClipboardSequenceNumber() }
    }

    fn clipboard_origin(&self) -> ClipboardOrigin {
        // Only accept images published by Windows' snipping processes. Closing
        // the snip with its X button must not OCR a later, unrelated image copy.
        let owner_name = unsafe {
            let Ok(owner) = GetClipboardOwner() else {
                return ClipboardOrigin::Unavailable;
            };
            let mut pid = 0;
            GetWindowThreadProcessId(owner, Some(&mut pid));
            let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return ClipboardOrigin::Unavailable;
            };
            let mut path = [0u16; 32768];
            let mut len = path.len() as u32;
            let result = QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut len,
            );
            let _ = CloseHandle(process);
            if result.is_err() {
                return ClipboardOrigin::Unavailable;
            }
            String::from_utf16_lossy(&path[..len as usize])
        };
        let filename = owner_name.rsplit(['\\', '/']).next().unwrap_or("");
        if [
            "SnippingTool.exe",
            "ScreenClippingHost.exe",
            "ScreenSketch.exe",
        ]
        .iter()
        .any(|name| filename.eq_ignore_ascii_case(name))
        {
            ClipboardOrigin::SnippingTool
        } else {
            ClipboardOrigin::Other
        }
    }

    fn observe(&self, events: SyncSender<ScreenEvent>) -> Result<Box<dyn ScreenObserver>, String> {
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
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0)
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
