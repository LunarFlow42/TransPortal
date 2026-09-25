pub mod anki;
pub mod commands;
pub mod dict;
pub mod dictionary;
pub mod integrity;
pub mod native;
pub mod ocr;
pub mod provider;
pub mod selection;
pub mod shell;
pub mod shortcut;
mod shortcut_sequence;

use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

/// Bring the shell bubble to the foreground.
fn focus_shell<R: tauri::Runtime>(app: &AppHandle<R>) {
    shell::present(app, false, |window| {
        native::configure_transparent_window(window);
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    });
}

/// Dynamically create or focus the settings window on demand.
pub fn open_or_focus_settings<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<tauri::WebviewWindow<R>, String> {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        Ok(window)
    } else {
        let window = tauri::WebviewWindowBuilder::new(
            app,
            "settings",
            tauri::WebviewUrl::App("index.html#/settings".into()),
        )
        .title("TransPortal Settings")
        .inner_size(480.0, 660.0)
        .min_inner_size(420.0, 480.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(false)
        .center()
        .visible(true)
        .skip_taskbar(false)
        .build()
        .map_err(|e| e.to_string())?;

        native::configure_transparent_window(&window);
        let win_clone = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = win_clone.hide();
            }
        });
        let _ = window.set_focus();
        Ok(window)
    }
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
                    let _ = open_or_focus_settings(app);
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
    let store = dict::DictStore::open(&db_path).map_err(|e| -> Box<dyn std::error::Error> {
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
        native::integrity::platform()
            .show_failure(report.error.as_deref().unwrap_or("程序完整性检查失败。"));
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
            app.manage(ocr::OcrService::new(ocr_assets));
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
            if let Some(window) = app.get_webview_window("shell") {
                native::configure_transparent_window(&window);
            }
            shortcut::register(app.handle());
            shortcut::start_clipboard_monitor(app.handle());
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
            commands::set_hotkey,
            commands::set_shortcut,
            commands::shortcut_status,
            commands::finish_startup_checks,
            commands::retry_shortcuts,
            commands::integrity_status,
            commands::verify_integrity,
            commands::ocr_status,
            commands::set_ocr_language,
            commands::set_ocr_config,
            commands::get_ocr_config,
            commands::install_onnx_ocr,
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
            commands::benchmark_ocr,
            commands::pause_auto_ocr,
            commands::stop_auto_ocr,
            commands::import_eccedict,
            commands::download_eccedict,
            commands::capture_selection,
            commands::set_double_copy_enabled,
            commands::set_clipboard_monitor_enabled,
            commands::set_ink_translate_key,
            commands::replace_captured_selection,
            commands::get_cursor_position,
            commands::anki_test_connection,
            commands::anki_list_decks,
            commands::anki_add_note,
            commands::generate_examples,
            commands::open_settings_window,
            commands::resize_and_clamp_window,
            commands::hide_shell,
            commands::set_window_acrylic,
            commands::set_window_theme,
            commands::get_memory_usage,
            commands::trim_memory,
            commands::set_window_pinned
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
