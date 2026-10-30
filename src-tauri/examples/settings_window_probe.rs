//! Isolated native/WebView visibility probe. Uses a synthetic page, never
//! opens the user's settings store, reads clipboard, or captures the desktop.
//! Run scripts/window/settings-probe.ps1 from the repository root on Windows.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::Manager;
use transportal_lib::platform::PlatformWindow;
use transportal_lib::platform::window::WindowOps;

#[allow(deprecated)] // Bounded pump for this diagnostic, not a production loop.
fn pump(app: &mut tauri::App, duration: Duration) {
    let end = Instant::now() + duration;
    while Instant::now() < end {
        app.run_iteration(|_, _| {});
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn main() -> Result<(), String> {
    let mut context = tauri::generate_context!();
    context.config_mut().identifier = "com.transportal.settings-probe".into();
    context.config_mut().build.dev_url = None;
    let profile =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/settings-window-probe");
    context.config_mut().app.app_directories_override =
        Some(tauri::utils::config::AppDirectoriesOverride::Root(profile));
    context
        .config_mut()
        .app
        .windows
        .retain(|window| window.label != "ocr-region");
    for window in &mut context.config_mut().app.windows {
        window.url = tauri::WebviewUrl::External(
            format!("probe://localhost/{}", window.label)
                .parse()
                .unwrap(),
        );
    }
    let loaded = Arc::new(AtomicBool::new(false));
    let on_load = loaded.clone();
    let mut app = tauri::Builder::default()
        .register_uri_scheme_protocol("probe", |_, _| {
            tauri::http::Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .body(b"<!doctype html><html><body style='margin:0;background:#16161b;color:white;min-height:100vh'>Settings visibility probe</body></html>".to_vec())
                .unwrap()
        })
        .on_page_load(move |_, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                on_load.store(true, Ordering::SeqCst);
            }
        })
        .build(context)
        .map_err(|error| error.to_string())?;
    pump(&mut app, Duration::from_millis(500));
    let window = app
        .get_webview_window("settings")
        .ok_or("missing settings")?;
    PlatformWindow::configure_transparent_window(&window);
    let webview: &tauri::Webview = window.as_ref();

    // The host is hidden, but the independent WebView starts visible.
    // Deliberately hide/collapse the content to reproduce the same empty host.
    webview.hide().map_err(|error| error.to_string())?;
    webview
        .set_size(tauri::PhysicalSize::new(0, 0))
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    pump(&mut app, Duration::from_millis(200));
    let size = webview.size().map_err(|error| error.to_string())?;
    println!(
        "host-only show: visible={}, WebView={}x{}",
        PlatformWindow::is_window_visible(&window),
        size.width,
        size.height
    );
    let visible = PlatformWindow::webview_visibility(&window)?;
    println!("host-only show: WebView2 content visible={visible:?}");
    if visible == Some(true) {
        return Err("host-only show unexpectedly restored the hidden WebView".into());
    }

    for cycle in 0..8 {
        if cycle % 2 == 0 {
            window.minimize().map_err(|error| error.to_string())?;
        } else {
            window.hide().map_err(|error| error.to_string())?;
            PlatformWindow::force_hide_window(&window);
        }
        webview.hide().map_err(|error| error.to_string())?;
        pump(&mut app, Duration::from_millis(100));
        transportal_lib::commands::open_settings_window(app.handle().clone())?;
        pump(&mut app, Duration::from_millis(200));
        let size = webview.size().map_err(|error| error.to_string())?;
        let host = window.inner_size().map_err(|error| error.to_string())?;
        if !PlatformWindow::is_window_visible(&window)
            || PlatformWindow::webview_visibility(&window)? == Some(false)
            || size != host
            || size.width == 0
            || size.height == 0
        {
            return Err(format!("cycle {cycle}: host={host:?}, content={size:?}"));
        }
        println!(
            "cycle {cycle}: visible=true, content={}x{}",
            size.width, size.height
        );
        let shell = app.get_webview_window("shell").ok_or("missing shell")?;
        if PlatformWindow::is_window_visible(&shell) {
            return Err(format!("cycle {cycle}: shell was not hidden"));
        }
        transportal_lib::commands::close_settings_window(app.handle().clone())?;
        pump(&mut app, Duration::from_millis(100));
        if !PlatformWindow::is_window_visible(&shell) || !shell.is_always_on_top().unwrap_or(false)
        {
            return Err(format!(
                "cycle {cycle}: shell did not restore its visibility/always-on-top state"
            ));
        }
    }
    if !loaded.load(Ordering::SeqCst) {
        return Err("synthetic settings page did not finish loading".into());
    }
    // An IPC request can originate off the UI thread. It must wait for the
    // complete show transaction, rather than just enqueueing part of it.
    let handle = app.handle().clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(transportal_lib::commands::open_settings_window(handle));
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match rx.try_recv() {
            Ok(result) => {
                result?;
                break;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) if Instant::now() < deadline => {
                pump(&mut app, Duration::from_millis(20));
            }
            Err(error) => return Err(format!("off-thread window transaction failed: {error}")),
        }
    }
    println!(
        "off-thread open: completed synchronously with content visible={:?}",
        PlatformWindow::webview_visibility(&window)?
    );
    window.destroy().map_err(|error| error.to_string())?;
    if let Some(shell) = app.get_webview_window("shell") {
        shell.destroy().map_err(|error| error.to_string())?;
    }
    pump(&mut app, Duration::from_millis(100));
    app.cleanup_before_exit();
    Ok(())
}
