use crate::core::dictionary::eccedict::{self, DictProgress};
use crate::core::dictionary::{DictEntry, ExampleSentence};
use crate::core::dictionary::{DictStatus, LookupService, SourceInfo};
use crate::core::intent::{IntentResult, classify_intent};
use crate::core::translation::{
    StreamChunk, TranslateQuery, TranslateResult, Translator, openai::OpenAiConfig,
};
use crate::platform::CaptureOutcome;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// IPC command wrapper around [`classify_intent`].
#[tauri::command]
pub fn detect_intent(text: String) -> IntentResult {
    classify_intent(&text)
}

/// Build the provider selected by name from the microkernel registry.
fn build_provider(provider: Option<String>, config: Option<OpenAiConfig>) -> Arc<dyn Translator> {
    crate::core::translation::create(provider.as_deref().unwrap_or("google_free"), config)
}

/// Full translation command.
#[tauri::command]
pub async fn translate_text(
    query: TranslateQuery,
    provider: Option<String>,
    openai_config: Option<OpenAiConfig>,
) -> Result<TranslateResult, String> {
    build_provider(provider, openai_config)
        .translate(&query)
        .await
        .map_err(|e| e.to_string())
}

/// Streaming translation command using the native Tauri v2 IPC channel.
#[tauri::command]
pub async fn translate_stream(
    query: TranslateQuery,
    provider: Option<String>,
    openai_config: Option<OpenAiConfig>,
    on_chunk: tauri::ipc::Channel<StreamChunk>,
) -> Result<(), String> {
    build_provider(provider, openai_config)
        .translate_stream(&query, on_chunk)
        .await
        .map_err(|e| e.to_string())
}

/// Word lookup: online-first by default, with the optional local ECCEDICT
/// store as fallback (or priority, when configured).
#[tauri::command]
pub async fn lookup_dict(
    word: String,
    service: State<'_, LookupService>,
) -> Result<Option<DictEntry>, String> {
    Ok(service.lookup(&word).await)
}

/// Dictionary availability snapshot for the settings panel.
#[tauri::command]
pub fn dict_status(service: State<'_, LookupService>) -> DictStatus {
    service.status()
}

/// Ordered list of configurable lookup sources.
#[tauri::command]
pub fn dict_sources(service: State<'_, LookupService>) -> Vec<SourceInfo> {
    service.sources()
}

/// Enable or disable a lookup source.
#[tauri::command]
pub fn set_dict_source_enabled(
    id: String,
    enabled: bool,
    service: State<'_, LookupService>,
) -> Result<(), String> {
    service.set_source_enabled(&id, enabled)
}

/// Move a lookup source up (`delta = -1`) or down (`delta = 1`).
#[tauri::command]
pub fn move_dict_source(
    id: String,
    delta: i32,
    service: State<'_, LookupService>,
) -> Result<(), String> {
    service.move_source(&id, delta)
}

/// Replace the full source order (used to restore persisted settings).
#[tauri::command]
pub fn set_dict_source_order(
    ids: Vec<String>,
    service: State<'_, LookupService>,
) -> Result<(), String> {
    service.set_source_order(&ids)
}

/// Replace only the selection shortcut; OCR shortcuts remain registered.
#[tauri::command]
pub fn set_hotkey(accelerator: String, app: AppHandle) -> Result<(), String> {
    crate::core::shortcut::set_selection_hotkey(&app, &accelerator)
}

/// Configure any global shortcut action individually.
#[tauri::command]
pub fn set_shortcut(
    action: crate::core::shortcut::ShortcutAction,
    accelerator: String,
    app: AppHandle,
) -> Result<(), String> {
    crate::core::shortcut::set_shortcut(&app, action, &accelerator)
}

#[tauri::command]
pub fn shortcut_status(app: AppHandle) -> crate::core::shortcut::ShortcutReport {
    crate::core::shortcut::status(&app)
}

#[tauri::command]
pub fn finish_startup_checks(app: AppHandle) {
    crate::core::shortcut::finish_startup_checks(&app);
}

#[tauri::command]
pub fn retry_shortcuts(app: AppHandle) -> crate::core::shortcut::ShortcutReport {
    crate::core::shortcut::retry(&app)
}

#[tauri::command]
pub fn integrity_status(
    state: State<'_, crate::core::integrity::IntegrityState>,
) -> crate::core::integrity::IntegrityReport {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
pub async fn verify_integrity(
    app: AppHandle,
) -> Result<crate::core::integrity::IntegrityReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let report = crate::core::integrity::startup_report();
        *app.state::<crate::core::integrity::IntegrityState>()
            .0
            .lock()
            .unwrap() = report.clone();
        if report.blocks_startup() {
            crate::platform::system::platform()
                .show_failure(report.error.as_deref().unwrap_or("程序完整性检查失败。"));
            app.exit(2);
        }
        report
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn ocr_status(app: AppHandle) -> Result<crate::core::ocr::OcrStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::core::ocr::OcrService>().status()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_ocr_language(language: Option<String>, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::core::ocr::OcrService>()
            .set_language(language)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_ocr_config(app: AppHandle) -> crate::core::ocr::registry::OcrConfig {
    app.state::<crate::core::ocr::OcrService>().config()
}

#[tauri::command]
pub async fn set_ocr_config(
    config: crate::core::ocr::registry::OcrConfig,
    app: AppHandle,
) -> Result<crate::core::ocr::registry::OcrConfig, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::core::ocr::OcrService>()
            .configure(config)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_onnx_ocr(
    app: AppHandle,
    on_progress: tauri::ipc::Channel<crate::core::ocr::assets::InstallProgress>,
) -> Result<(), String> {
    let assets = app.state::<crate::core::ocr::OcrService>().assets.clone();
    assets
        .install(&move |progress| {
            let _ = on_progress.send(progress);
        })
        .await?;
    let _ = app.emit("transportal://ocr-assets-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn ocr_image(
    path: String,
    mode: crate::core::ocr::OcrMode,
    app: AppHandle,
) -> Result<u64, String> {
    app.state::<crate::core::ocr::OcrService>().start_image(
        &app,
        mode,
        std::path::PathBuf::from(path),
    )
}

#[tauri::command]
pub async fn start_ocr(mode: crate::core::ocr::OcrMode, app: AppHandle) -> Result<u64, String> {
    app.state::<crate::core::ocr::OcrService>()
        .start(&app, mode, None)
}

#[tauri::command]
pub async fn cancel_ocr(app: AppHandle) {
    app.state::<crate::core::ocr::OcrService>().cancel(&app);
}

#[tauri::command]
pub fn ocr_latest(
    service: State<'_, crate::core::ocr::OcrService>,
) -> Option<crate::core::ocr::OcrEvent> {
    service.latest()
}

#[tauri::command]
pub async fn translate_ocr_picture(
    id: u64,
    texts: Option<Vec<String>>,
    target_lang: String,
    provider: Option<String>,
    openai_config: Option<OpenAiConfig>,
    app: AppHandle,
) -> Result<crate::core::ocr::picture::PictureTranslation, String> {
    let (picture, cancel) = app
        .state::<crate::core::ocr::OcrService>()
        .picture_translation(id, texts)?;
    crate::core::ocr::picture::translate_blocks(
        &picture.blocks,
        build_provider(provider, openai_config),
        &target_lang,
        &mut Default::default(),
        &cancel,
    )
    .await
}

#[tauri::command]
pub async fn export_ocr_picture(
    data_url: String,
    path: Option<String>,
    app: AppHandle,
) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    tauri::async_runtime::spawn_blocking(move || {
        let image = crate::core::ocr::picture::decode_png(&data_url)?;
        if let Some(path) = path {
            image
                .save_with_format(path, image::ImageFormat::Png)
                .map_err(|error| format!("保存图片失败：{error}"))
        } else {
            let (width, height) = image.dimensions();
            app.clipboard()
                .write_image(&tauri::image::Image::new_owned(
                    image.into_raw(),
                    width,
                    height,
                ))
                .map_err(|error| format!("复制图片失败：{error}"))
        }
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn begin_auto_ocr(
    options: crate::core::ocr::monitor::MonitorOptions,
    app: AppHandle,
) -> Result<u64, String> {
    app.state::<crate::core::ocr::monitor::MonitorService>()
        .begin(&app, options)
        .await
}

#[tauri::command]
pub fn auto_ocr_latest(
    service: State<'_, crate::core::ocr::monitor::MonitorService>,
) -> crate::core::ocr::monitor::MonitorUpdate {
    service.latest()
}

/// Shared native region selector, used by snips and auto-translation alike.
#[tauri::command]
pub fn region_selection(
    service: State<'_, crate::core::ocr::region::RegionService>,
) -> Option<crate::core::ocr::region::RegionSelection> {
    service.selection()
}

/// Stream raw RGBA physical pixels for zero-latency, 100% lossless canvas freeze.
#[tauri::command]
pub fn get_frozen_pixels(
    id: u64,
    service: State<'_, crate::core::ocr::region::RegionService>,
) -> Result<tauri::ipc::Response, String> {
    let raw = service.get_frozen_pixels(id)?;
    Ok(tauri::ipc::Response::new(raw))
}

#[tauri::command]
pub fn confirm_region(
    id: u64,
    region: crate::platform::desktop::ScreenRect,
    app: AppHandle,
) -> Result<(), String> {
    crate::core::ocr::region::confirm(&app, id, region)
}

#[tauri::command]
pub fn cancel_region(id: Option<u64>, app: AppHandle) {
    crate::core::ocr::region::cancel(&app, id);
}

#[tauri::command]
pub async fn benchmark_ocr(app: AppHandle) -> Result<u64, String> {
    app.state::<crate::core::ocr::OcrService>().benchmark()
}

#[tauri::command]
pub fn stop_auto_ocr(discard: bool, app: AppHandle) {
    app.state::<crate::core::ocr::monitor::MonitorService>()
        .stop(&app, discard);
}

#[tauri::command]
pub fn pause_auto_ocr(paused: bool, app: AppHandle) -> Result<(), String> {
    app.state::<crate::core::ocr::monitor::MonitorService>()
        .pause(&app, paused)
}

/// Import an ECCEDICT-format CSV file into the local redb store.
#[tauri::command]
pub async fn import_eccedict(
    path: String,
    service: State<'_, LookupService>,
) -> Result<usize, String> {
    eccedict::import_file(&service, path).await
}

/// Download the full (~63 MB) ECCEDICT CSV with progress, then import it.
#[tauri::command]
pub async fn download_eccedict(
    on_progress: tauri::ipc::Channel<DictProgress>,
    service: State<'_, LookupService>,
) -> Result<usize, String> {
    eccedict::download_and_import(&service, |progress| {
        let _ = on_progress.send(progress);
    })
    .await
}

/// Capture the current foreground selection (UIA, then clipboard fallback).
#[tauri::command]
pub async fn capture_selection(app: AppHandle) -> Result<CaptureOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::core::selection::SelectionService>()
            .capture(&app)
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_double_copy_enabled(enabled: bool, app: AppHandle) {
    crate::core::shortcut::set_double_copy_enabled(&app, enabled);
}

#[tauri::command]
pub fn set_clipboard_monitor_enabled(enabled: bool) {
    crate::core::shortcut::set_clipboard_monitor_enabled(enabled);
}

#[tauri::command]
pub fn set_ink_translate_key(key: String) {
    crate::core::shortcut::set_ink_translate_key(&key);
}

#[tauri::command]
pub async fn replace_captured_selection(
    token: u64,
    text: String,
    app: AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::core::selection::SelectionService>()
            .replace(&app, token, &text)
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Physical cursor position, used to place the shell bubble.
#[derive(Debug, Clone, Serialize)]
pub struct CursorPosition {
    pub x: f64,
    pub y: f64,
}

/// Return the current cursor position in physical screen coordinates across all monitors.
#[tauri::command]
pub fn get_cursor_position(app: AppHandle) -> Option<CursorPosition> {
    if let Some((x, y)) = crate::platform::get_global_cursor_pos() {
        return Some(CursorPosition {
            x: x as f64,
            y: y as f64,
        });
    }
    app.get_webview_window("shell")
        .and_then(|window| window.cursor_position().ok())
        .map(|position| CursorPosition {
            x: position.x,
            y: position.y,
        })
}

/// Sets whether the shell overlay window is pinned in place.
#[tauri::command]
pub fn set_window_pinned(pinned: bool) {
    crate::core::shortcut::set_pinned(pinned);
}

/// Probe AnkiConnect and return its API version.
#[tauri::command]
pub async fn anki_test_connection(endpoint: String) -> Result<i64, String> {
    crate::core::anki::version(&endpoint).await
}

/// List Anki deck names.
#[tauri::command]
pub async fn anki_list_decks(endpoint: String) -> Result<Vec<String>, String> {
    crate::core::anki::deck_names(&endpoint).await
}

/// Add a note (word + phonetic + original context sentence) to Anki.
#[tauri::command]
pub async fn anki_add_note(note: crate::core::anki::AnkiNoteRequest) -> Result<i64, String> {
    crate::core::anki::add_note(&note).await
}

/// Generate example sentences for a word via the configured LLM.
///
/// Returns an empty list when no usable OpenAI-compatible key is configured, so
/// the caller can silently skip enhancement.
#[tauri::command]
pub async fn generate_examples(
    word: String,
    openai_config: OpenAiConfig,
) -> Result<Vec<ExampleSentence>, String> {
    crate::core::dictionary::examples::generate(&word, openai_config).await
}

#[tauri::command]
pub async fn open_settings_window(app: AppHandle) -> Result<(), String> {
    crate::open_or_focus_settings(&app)?;
    Ok(())
}

#[tauri::command]
pub fn resize_and_clamp_window(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    crate::platform::resize_and_clamp_window(&window, width, height)
}

/// Hide the overlay unless a newer show superseded the frontend's dismiss.
#[tauri::command]
pub async fn hide_shell(app: AppHandle, generation: Option<u64>) -> Result<bool, String> {
    crate::core::shell::hide(&app, generation).await
}

#[tauri::command]
pub fn set_window_acrylic(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    crate::platform::set_window_acrylic(&window, enabled);
    Ok(())
}

#[tauri::command]
pub fn set_window_theme(window: tauri::WebviewWindow, dark: bool) -> Result<(), String> {
    crate::platform::set_window_theme(&window, dark);
    Ok(())
}

#[tauri::command]
pub fn get_memory_usage() -> Result<crate::platform::system::MemoryReport, String> {
    crate::platform::system::get_memory_usage()
}

#[tauri::command]
pub fn trim_memory() -> Result<crate::platform::system::TrimResult, String> {
    crate::platform::system::trim_memory()
}
