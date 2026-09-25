//! Interactive move/resize observation through a window subclass.

use crate::platform::SizeMove;

struct SizeMoveObserver {
    on_phase: Box<dyn Fn(SizeMove)>,
    resized: std::cell::Cell<bool>,
}

unsafe extern "system" fn size_move_subclass_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_SIZING};

    // SAFETY: `data` is the leaked `SizeMoveObserver` installed by `observe_size_move`.
    let observer = unsafe { &*(data as *const SizeMoveObserver) };
    match msg {
        WM_ENTERSIZEMOVE => {
            observer.resized.set(false);
            (observer.on_phase)(SizeMove::Start);
        }
        WM_SIZING if !observer.resized.replace(true) => (observer.on_phase)(SizeMove::Resize),
        WM_EXITSIZEMOVE => (observer.on_phase)(SizeMove::End {
            resized: observer.resized.replace(false),
        }),
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Reports interactive (modal) moves and resizes of `window`, whether started
/// from our own grip via `startResizeDragging`, Tauri's native edge handles or
/// a caption drag.
///
/// The frontend cannot see these itself: `startResizeDragging` resolves as
/// soon as the OS loop starts, and edge presses never reach the webview.
pub fn observe_size_move<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    on_phase: impl Fn(SizeMove) + Send + 'static,
) -> Result<(), String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Shell::SetWindowSubclass;

    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    // Subclassing must happen on the thread that owns the window.
    window
        .run_on_main_thread(move || {
            let observer = Box::new(SizeMoveObserver {
                on_phase: Box::new(on_phase),
                resized: std::cell::Cell::new(false),
            });
            // The overlay lives as long as the app, so the observer is leaked.
            let data = Box::into_raw(observer) as usize;
            unsafe {
                let _ = SetWindowSubclass(
                    HWND(hwnd as *mut _),
                    Some(size_move_subclass_proc),
                    0x5452_5A31,
                    data,
                );
            }
        })
        .map_err(|e| e.to_string())
}
