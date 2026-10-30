pub mod commands;
pub mod core;
pub mod platform;

use crate::core::{clipboard_monitor, dictionary, integrity, ocr, selection, shell, shortcut};
use crate::platform::PlatformWindow;
use crate::platform::window::WindowOps;

use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

/// Bring the shell bubble to the foreground.
pub(crate) fn focus_shell<R: tauri::Runtime>(app: &AppHandle<R>) {
    shell::present(app, false, |window| {
        PlatformWindow::configure_transparent_window(window);
        let _ = window.unminimize();
        let _ = window.show();
        PlatformWindow::force_show_window(window);
        let _ = window.set_focus();
    });
}

fn attach_settings_window_events<R: tauri::Runtime>(
    window: &tauri::WebviewWindow<R>,
    app: &AppHandle<R>,
) {
    let win_clone = window.clone();
    let app_handle = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = win_clone.hide();
            PlatformWindow::force_hide_window(&win_clone);
            if commands::SHELL_RESUME_AFTER_SETTINGS
                .swap(false, std::sync::atomic::Ordering::SeqCst)
            {
                focus_shell(&app_handle);
            }
        }
    });
}

/// Restore both the native host and its independent WebView rendering surface.
fn show_settings_window<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) -> Result<(), String> {
    window
        .unminimize()
        .map_err(|e| format!("恢复设置窗口失败：{e}"))?;
    PlatformWindow::set_ignore_cursor_events(window, false)?;
    window
        .show()
        .map_err(|e| format!("显示设置窗口失败：{e}"))?;

    // WebviewWindow::show() only shows the host. A hidden or zero-sized WebView2
    // controller otherwise leaves a visible border around a transparent client.
    let webview: &tauri::Webview<R> = window.as_ref();
    let size = window.inner_size().map_err(|e| e.to_string())?;
    if size.width == 0 || size.height == 0 {
        return Err("设置窗口尺寸无效，请重试。".into());
    }
    webview
        .set_size(size)
        .map_err(|e| format!("恢复设置内容尺寸失败：{e}"))?;
    webview
        .show()
        .map_err(|e| format!("显示设置内容失败：{e}"))?;
    PlatformWindow::force_show_window(window);
    window
        .set_focus()
        .map_err(|e| format!("聚焦设置窗口失败：{e}"))?;
    if !PlatformWindow::is_window_visible(window) {
        return Err("设置窗口未能显示，请重试。".into());
    }
    if PlatformWindow::webview_visibility(window)? == Some(false) {
        return Err("设置内容未能显示，请重试。".into());
    }
    Ok(())
}

/// Keep synchronous window IPC synchronous while serializing native and WebView
/// operations on their owning UI thread. Tauri runs this inline when already on
/// that thread, so receiving the result does not block the UI loop.
pub(crate) fn run_window_task<R: tauri::Runtime, T: Send + 'static>(
    app: &AppHandle<R>,
    task: impl FnOnce(AppHandle<R>) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let _ = tx.send(task(handle));
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}

/// Focus the declared settings window; recreate from its config only if missing.
pub fn open_or_focus_settings<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri::WebviewWindow<R>, String> {
    run_window_task(app, |app| open_or_focus_settings_on_main(&app))
}

fn open_or_focus_settings_on_main<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri::WebviewWindow<R>, String> {
    let window = if let Some(window) = app.get_webview_window("settings") {
        window
    } else {
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|config| config.label == "settings")
            .ok_or("设置窗口配置缺失")?;
        let window = tauri::WebviewWindowBuilder::from_config(app, config)
            .map_err(|e| e.to_string())?
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?;

        PlatformWindow::configure_transparent_window(&window);
        attach_settings_window_events(&window, app);
        if let Some(icon) = app.default_window_icon() {
            let _ = window.set_icon(icon.clone());
        }
        window
    };
    if let Err(error) = show_settings_window(&window) {
        // Do not leave a partially shown, empty host intercepting desktop input.
        let _ = window.hide();
        PlatformWindow::force_hide_window(&window);
        return Err(error);
    }
    Ok(window)
}

/// Initialize the system tray with show/hide, settings, and quit options.
fn setup_system_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, "show", "Show TransPortal", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide", "Hide to Tray", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "Settings...", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit TransPortal", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &hide_item,
            &settings_item,
            &separator,
            &quit_item,
        ],
    )?;

    if let Some(icon) = app.default_window_icon() {
        let _tray = TrayIconBuilder::new()
            .icon(icon.clone())
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id.as_ref() {
                "show" => focus_shell(app),
                "hide" => shell::hide_now(app),
                "settings" => {
                    if let Err(error) = open_or_focus_settings(app) {
                        eprintln!("[settings] {error}");
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let app: &AppHandle = tray.app_handle();
                    if let Some(window) = app.get_webview_window("shell") {
                        let is_visible = window.is_visible().unwrap_or(false);
                        if is_visible {
                            shell::hide_now(app);
                        } else {
                            focus_shell(app);
                        }
                    }
                }
            })
            .build(app)?;
    }

    Ok(())
}

/// Initialize the lookup service (online-first + optional local ECCEDICT).
fn setup_dictionary(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let db_path = data_dir.join("transportal.redb");
    let store =
        dictionary::DictStore::open(&db_path).map_err(|e| -> Box<dyn std::error::Error> {
            Box::new(std::io::Error::other(e.to_string()))
        })?;
    let service = dictionary::LookupService::new(std::sync::Arc::new(store), db_path);
    app.manage(service);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let report = integrity::startup_report();
    if report.blocks_startup() {
        integrity::show_failure(&report);
        std::process::exit(2);
    }
    tauri::Builder::default()
        .manage(integrity::IntegrityState(std::sync::Mutex::new(report)))
        // `single-instance` must be the first plugin registered.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            focus_shell(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let ocr_data = app.path().app_data_dir()?.join("ocr");
            let ocr_resources = if cfg!(debug_assertions) {
                Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr"))
            } else {
                app.path().resource_dir().ok().map(|path| path.join("ocr"))
            };
            let ocr_assets = std::sync::Arc::new(ocr::assets::OnnxAssets::new(
                ocr_resources,
                ocr_data.clone(),
                ocr_data.join("downloads"),
            ));
            if ocr_assets.resolve().is_ok() {
                ocr_assets.clean_cache();
            }
            app.manage(ocr::OcrService::new(ocr_assets, platform::native_ocr()));
            let tts_data = app.path().app_data_dir()?.join("tts");
            let tts_assets = std::sync::Arc::new(core::tts::assets::TtsAssets::new(
                tts_data.clone(),
                tts_data.join("downloads"),
                Some(
                    ocr_data
                        .join("ppocr-v5-fp32-ort-1.22.0")
                        .join(core::ocr::assets::current_platform()),
                ),
            ));
            if tts_assets.status().installed {
                tts_assets.clean_cache();
            }
            app.manage(core::tts::TtsService::new(tts_assets));
            app.manage(ocr::region::RegionService::default());
            ocr::region::RegionService::prewarm(app.handle());
            app.manage(ocr::monitor::MonitorService::default());
            app.manage(shortcut::ShortcutBindings::default());
            app.manage(selection::SelectionService::default());
            if let Err(e) = setup_dictionary(app) {
                eprintln!("Failed to initialize dictionary: {e}");
            }
            if let Err(e) = setup_system_tray(app) {
                eprintln!("Failed to setup system tray: {e}");
            }
            app.state::<ocr::OcrService>()
                .install_observer(app.handle());
            if let (Some(icon), Some(settings)) = (
                app.default_window_icon(),
                app.get_webview_window("settings"),
            ) {
                let _ = settings.set_icon(icon.clone());
            }
            if let Some(window) = app.get_webview_window("shell") {
                PlatformWindow::configure_transparent_window(&window);
            }
            if let Some(settings) = app.get_webview_window("settings") {
                PlatformWindow::configure_transparent_window(&settings);
                attach_settings_window_events(&settings, app.handle());
            }
            shortcut::register(app.handle());
            clipboard_monitor::start(app.handle());
            shell::observe_size_move(app.handle());
            focus_shell(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::detect_intent,
            commands::translate_text,
            commands::translate_stream,
            commands::lookup_dict,
            commands::dict_status,
            commands::dict_sources,
            commands::set_dict_source_enabled,
            commands::move_dict_source,
            commands::set_dict_source_order,
            commands::set_shortcut,
            commands::set_shortcut_recording,
            commands::shortcut_status,
            commands::finish_startup_checks,
            commands::retry_shortcuts,
            commands::integrity_status,
            commands::verify_integrity,
            commands::ocr_status,
            commands::set_ocr_config,
            commands::get_ocr_config,
            commands::install_onnx_ocr,
            commands::tts_status,
            commands::tts_voices,
            commands::install_onnx_tts,
            commands::delete_onnx_tts,
            commands::tts_synthesize,
            commands::ocr_image,
            commands::start_ocr,
            commands::cancel_ocr,
            commands::ocr_latest,
            commands::translate_ocr_picture,
            commands::export_ocr_picture,
            commands::begin_auto_ocr,
            commands::auto_ocr_latest,
            commands::region_selection,
            commands::get_frozen_pixels,
            commands::confirm_region,
            commands::cancel_region,
            commands::show_region_window,
            commands::benchmark_ocr,
            commands::pause_auto_ocr,
            commands::stop_auto_ocr,
            commands::import_eccedict,
            commands::download_eccedict,
            commands::set_double_copy_enabled,
            commands::set_clipboard_monitor_enabled,
            commands::set_ink_translate_key,
            commands::replace_captured_selection,
            commands::generate_examples,
            commands::fetch_openai_models,
            commands::open_settings_window,
            commands::close_settings_window,
            commands::resize_and_clamp_window,
            commands::hide_shell,
            commands::set_window_theme,
            commands::get_memory_usage,
            commands::trim_memory,
            commands::set_window_pinned,
            commands::get_capture_diagnostics,
            commands::test_screen_capture,
            commands::open_capture_debug_image
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<ocr::monitor::MonitorService>().stop(app, true);
                app.state::<ocr::OcrService>().shutdown(app);
            }
        });
}
