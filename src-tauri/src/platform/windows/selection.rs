use super::clipboard::WindowsClipboard;
use super::clipboard_restore::ClipboardRestore;
use super::input::{copy_selection_to_clipboard, key_input, release_modifiers};
use super::uia::WindowsSelectionReader;
use crate::platform::clipboard::ClipboardOps;
use crate::platform::selection::{
    CaptureMethod, SelectionContent, SelectionOps, SelectionTarget, normalize_selection,
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_clipboard_manager::ClipboardExt;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, SendInput, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo,
    GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

#[derive(Default)]
pub struct WindowsSelection;

fn target() -> Option<SelectionTarget> {
    unsafe {
        let window = GetForegroundWindow();
        if window.is_invalid() {
            return None;
        }
        let mut process = 0;
        let thread = GetWindowThreadProcessId(window, Some(&mut process));
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let focus = if GetGUIThreadInfo(thread, &mut info).is_ok() {
            info.hwndFocus.0 as usize
        } else {
            0
        };
        Some(SelectionTarget {
            window: window.0 as usize,
            process,
            focus,
        })
    }
}

fn wait_for_modifiers() -> Result<(), String> {
    let start = Instant::now();
    while [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .iter()
        .any(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) } as u16 & 0x8000 != 0)
    {
        if start.elapsed() > Duration::from_millis(1500) {
            release_modifiers();
            break;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    release_modifiers();
    Ok(())
}

impl SelectionOps for WindowsSelection {
    fn method(&self) -> CaptureMethod {
        CaptureMethod::UiAutomation
    }

    fn read_selection(&self) -> Result<SelectionContent, String> {
        WindowsSelectionReader::new().read_via_uia()
    }

    fn copy_to_clipboard(&self) -> Result<(), String> {
        copy_selection_to_clipboard()
    }

    fn foreground(&self) -> Option<SelectionTarget> {
        target().filter(|target| target.process != unsafe { GetCurrentProcessId() })
    }

    fn replace(
        &self,
        app: &AppHandle,
        owner: &WebviewWindow,
        source: &SelectionTarget,
        expected: &str,
        replacement: &str,
    ) -> Result<(), String> {
        let owner = owner.hwnd().map_err(|e| e.to_string())?;
        let mut restore = ClipboardRestore::snapshot(HWND(owner.0)).ok();
        let window = HWND(source.window as *mut _);
        unsafe {
            let mut process = 0;
            GetWindowThreadProcessId(window, Some(&mut process));
            if !IsWindow(Some(window)).as_bool() || process != source.process {
                return Err("原窗口已关闭，请重新选中文字。".into());
            }
        }
        let current = target().ok_or("无法确认当前窗口。")?;
        if !current.matches(source) && current.process != unsafe { GetCurrentProcessId() } {
            return Err("当前焦点已转到其他软件，请重新取词后替换。".into());
        }
        wait_for_modifiers()?;
        unsafe {
            let _ = SetForegroundWindow(window);
            let _ = BringWindowToTop(window);
        }
        for _ in 0..25 {
            if let Some(cur) = target()
                && cur.matches(source)
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let current_target = target().ok_or("无法确认当前窗口。")?;
        if !current_target.matches(source) {
            return Err("无法恢复原输入框焦点，请重新选中文字。".into());
        }
        // Read the actual selection again; never blindly paste at a stale caret.
        let selected = match WindowsSelectionReader::new().read_via_uia() {
            Ok(content) if !content.text.trim().is_empty() => content.text,
            _ => {
                std::thread::sleep(Duration::from_millis(50));
                let copy_res = copy_selection_to_clipboard();
                if let Some(ref mut r) = restore {
                    r.owned_revision = Some(WindowsClipboard.revision());
                }
                match app.clipboard().read_text() {
                    Ok(text) if !text.trim().is_empty() => text,
                    _ => {
                        copy_res.map_err(|error| format!("无法核对选中文字：{error}"))?;
                        app.clipboard()
                            .read_text()
                            .map_err(|error| format!("无法核对选中文字：{error}"))?
                    }
                }
            }
        };
        if normalize_selection(&selected) != normalize_selection(expected) {
            return Err("选中的文字已变化，已取消替换。请重新取词。".into());
        }
        wait_for_modifiers()?;
        if !target().as_ref().is_some_and(|cur| cur.matches(source)) {
            return Err("输入焦点已变化，已取消替换。".into());
        }
        app.clipboard()
            .write_text(replacement.to_string())
            .map_err(|error| format!("无法准备译文：{error}"))?;
        if let Some(ref mut r) = restore {
            r.owned_revision = Some(WindowsClipboard.revision());
        }
        // Recheck after clipboard access, which can briefly yield to another app.
        if !target().as_ref().is_some_and(|cur| cur.matches(source)) {
            return Err("输入焦点已变化，已取消替换。".into());
        }
        let inputs = [
            key_input(VK_CONTROL, false),
            key_input(VK_V, false),
            key_input(VK_V, true),
            key_input(VK_CONTROL, true),
        ];
        let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != inputs.len() {
            return Err("目标软件拒绝了粘贴，请手动复制译文（可能权限不一致）。".into());
        }
        // Let the target consume WM_PASTE before restoring the user's clipboard.
        std::thread::sleep(Duration::from_millis(350));
        if let Some(mut r) = restore {
            let _ = r.restore();
        }
        Ok(())
    }
}
