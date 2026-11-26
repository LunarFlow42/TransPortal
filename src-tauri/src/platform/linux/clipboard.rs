//! GTK maps clipboard operations to the native Wayland protocols. No wl-paste
//! executable, data-control extension, XWayland, or automatic key injection.

use crate::platform::clipboard::{ClipboardOps, ClipboardOrigin};
use gtk::prelude::ObjectExt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

#[derive(Default)]
pub struct LinuxClipboard;

fn clipboard(primary: bool) -> Result<gtk::Clipboard, String> {
    if !gtk::is_initialized_main_thread() {
        return Err("GTK clipboard is not initialized".into());
    }
    let display = gtk::gdk::Display::default().ok_or("No GDK display")?;
    if display.type_().name() != "GdkWaylandDisplay" {
        return Err("GTK is not using Wayland; launch with GDK_BACKEND=wayland".into());
    }
    let selection = if primary {
        gtk::gdk::SELECTION_PRIMARY
    } else {
        gtk::gdk::SELECTION_CLIPBOARD
    };
    Ok(gtk::Clipboard::for_display(&display, &selection))
}

pub(super) fn read(primary: bool) -> Result<String, String> {
    if gtk::is_initialized_main_thread() {
        return Err("Clipboard reads must run on a worker thread".into());
    }
    if super::session() != super::Session::Wayland {
        return Err("Native clipboard access requires a Wayland session".into());
    }
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    gtk::glib::MainContext::default().invoke(move || {
        let clipboard = match clipboard(primary) { Ok(clipboard) => clipboard, Err(error) => { let _ = sender.send(Err(error)); return; } };
        clipboard.request_text(move |_, value| {
            let result = value.filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "The compositor/source did not provide clipboard text; copy the selection and retry".to_string())
                .and_then(|value| {
                    if value.chars().count() > 100_000 { Err("Selected text exceeds the size limit".into()) } else { Ok(value.to_string()) }
                });
            let _ = sender.send(result);
        });
    });
    receiver
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| "Wayland clipboard request timed out".to_string())?
}

impl ClipboardOps for LinuxClipboard {
    fn read_retry_count(&self) -> usize {
        1
    }
    fn read_text(&self, _: &tauri::AppHandle) -> Result<String, String> {
        read(false)
    }
    fn write_text(&self, _: &tauri::AppHandle, text: &str) -> Result<(), String> {
        if super::session() != super::Session::Wayland {
            return Err("Native clipboard access requires a Wayland session".into());
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let text = text.to_string();
        let active = Arc::new(AtomicBool::new(true));
        let pending = active.clone();
        gtk::glib::MainContext::default().invoke(move || {
            if !pending.load(Ordering::Acquire) {
                return;
            }
            let clipboard = match clipboard(false) {
                Ok(clipboard) => clipboard,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            };
            clipboard.set_text(&text);
            let _ = sender.send(Ok(()));
        });
        let result = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Wayland clipboard write timed out".to_string());
        active.store(false, Ordering::Release);
        result?
    }
    fn write_image(
        &self,
        _: &tauri::AppHandle,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        if super::session() != super::Session::Wayland {
            return Err("Native clipboard access requires a Wayland session".into());
        }
        crate::platform::desktop::ScreenRect {
            x: 0,
            y: 0,
            width,
            height,
        }
        .validate()?;
        if rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
            return Err("Clipboard image buffer size mismatch".into());
        }
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        let active = Arc::new(AtomicBool::new(true));
        let pending = active.clone();
        gtk::glib::MainContext::default().invoke(move || {
            if !pending.load(Ordering::Acquire) {
                return;
            }
            let clipboard = match clipboard(false) {
                Ok(clipboard) => clipboard,
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            };
            let bytes = gtk::glib::Bytes::from_owned(rgba);
            let pixels = gtk::gdk_pixbuf::Pixbuf::from_bytes(
                &bytes,
                gtk::gdk_pixbuf::Colorspace::Rgb,
                true,
                8,
                width as i32,
                height as i32,
                width as i32 * 4,
            );
            clipboard.set_image(&pixels);
            let _ = sender.send(Ok(()));
        });
        let result = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| "Wayland clipboard image write timed out".to_string());
        active.store(false, Ordering::Release);
        result?
    }
    fn revision(&self) -> u32 {
        0
    }
    fn origin(&self) -> ClipboardOrigin {
        ClipboardOrigin::Unavailable
    }
    fn listen(&self, on_change: &mut dyn FnMut()) -> Result<(), String> {
        // Wayland has no universal unrestricted change observer. The core only
        // reads text while the user-enabled monitor is active, deduplicating it.
        loop {
            std::thread::sleep(Duration::from_millis(500));
            on_change();
        }
    }
}
