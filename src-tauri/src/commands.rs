use crate::core::dictionary::eccedict::{self, DictProgress};
use crate::core::dictionary::{DictEntry, ExampleSentence};
use crate::core::dictionary::{DictStatus, LookupService, SourceInfo};
use crate::core::intent::{IntentResult, classify_intent};
use crate::core::translation::{
    StreamChunk, TranslateQuery, TranslateResult, Translator, openai::OpenAiConfig,
};
use crate::platform::system::SystemOps;
use crate::platform::window::WindowOps;
use crate::platform::{PlatformSystem, PlatformWindow};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// IPC command wrapper around [`classify_intent`].
#[tauri::command]
pub fn detect_intent(text: String) -> IntentResult {
    classify_intent(&text)
}

/// Build the provider selected by name from the microkernel registry.
fn build_provider(provider: Option<String>, config: Option<OpenAiConfig>) -> Arc<dyn Translator> {
    crate::core::translation::create(provider.as_deref().unwrap_or("bing_free"), config)
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
            crate::core::integrity::show_failure(&report);
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
pub async fn tts_status(
    app: AppHandle,
) -> Result<crate::core::tts::assets::TtsInstallStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::core::tts::TtsService>().status()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn tts_voices(app: AppHandle) -> Vec<crate::core::tts::assets::VoiceInfo> {
    app.state::<crate::core::tts::TtsService>().voices()
}

#[tauri::command]
pub async fn install_onnx_tts(
    app: AppHandle,
    on_progress: tauri::ipc::Channel<crate::core::tts::assets::InstallProgress>,
) -> Result<(), String> {
    let tts = app.state::<crate::core::tts::TtsService>();
    tts.install(&move |progress| {
        let _ = on_progress.send(progress);
    })
    .await?;
    let _ = app.emit("transportal://tts-assets-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn delete_onnx_tts(app: AppHandle) -> Result<(), String> {
    let app_handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app_handle.state::<crate::core::tts::TtsService>().delete()
    })
    .await
    .map_err(|e| e.to_string())??;
    let _ = app.emit("transportal://tts-assets-changed", ());
    Ok(())
}

#[tauri::command]
pub async fn tts_synthesize(
    app: AppHandle,
    text: String,
    voice: String,
    speed: f32,
) -> Result<tauri::ipc::Response, String> {
    let tts = app.state::<crate::core::tts::TtsService>();
    let wav_bytes = tts
        .synthesize(&text, &voice, speed)
        .await
        .map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(wav_bytes))
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
pub fn show_region_window(app: AppHandle) -> Result<(), String> {
    let service = app.state::<crate::core::ocr::region::RegionService>();
    if service.selection().is_some() {
        crate::core::ocr::region::RegionService::show_window(&app)?;
    }
    Ok(())
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

#[tauri::command]
pub fn set_double_copy_enabled(enabled: bool, app: AppHandle) {
    crate::core::shortcut::set_double_copy_enabled(&app, enabled);
}

#[tauri::command]
pub fn set_clipboard_monitor_enabled(enabled: bool) {
    crate::core::clipboard_monitor::set_enabled(enabled);
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

/// Sets whether the shell overlay window is pinned in place.
#[tauri::command]
pub fn set_window_pinned(pinned: bool) {
    crate::core::shortcut::set_pinned(pinned);
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

/// Fetch the list of available models from an OpenAI-compatible endpoint.
#[tauri::command]
pub async fn fetch_openai_models(config: OpenAiConfig) -> Result<Vec<String>, String> {
    let provider = crate::core::translation::openai::OpenAiCompatibleProvider::new(config);
    provider.fetch_models().await.map_err(|e| e.to_string())
}

use std::sync::atomic::{AtomicBool, Ordering};

pub static SHELL_RESUME_AFTER_SETTINGS: AtomicBool = AtomicBool::new(false);

#[tauri::command]
pub async fn open_settings_window(app: AppHandle) -> Result<(), String> {
    let was_shell_visible =
        if let Some(shell) = app.get_webview_window(crate::core::shell::SHELL_LABEL) {
            let visible = shell.is_visible().unwrap_or(false);
            if visible {
                crate::core::shell::hide_now(&app);
            }
            visible
        } else {
            false
        };
    if was_shell_visible {
        SHELL_RESUME_AFTER_SETTINGS.store(true, Ordering::SeqCst);
    }
    crate::open_or_focus_settings(&app)?;
    Ok(())
}

#[tauri::command]
pub async fn close_settings_window(app: AppHandle) -> Result<(), String> {
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
        crate::platform::PlatformWindow::force_hide_window(&settings);
    }
    if SHELL_RESUME_AFTER_SETTINGS.swap(false, Ordering::SeqCst) {
        crate::core::shell::present(&app, false, |w| {
            let _ = w.show();
            let _ = w.set_focus();
        });
    }
    Ok(())
}

#[tauri::command]
pub fn resize_and_clamp_window(
    window: tauri::WebviewWindow,
    width: f64,
    height: f64,
) -> Result<(), String> {
    PlatformWindow::resize_and_clamp_window(&window, width, height)
}

/// Hide the overlay unless a newer show superseded the frontend's dismiss.
#[tauri::command]
pub async fn hide_shell(app: AppHandle, generation: Option<u64>) -> Result<bool, String> {
    crate::core::shell::hide(&app, generation).await
}

#[tauri::command]
pub fn set_window_theme(window: tauri::WebviewWindow, dark: bool) -> Result<(), String> {
    PlatformWindow::set_window_theme(&window, dark);
    Ok(())
}

#[tauri::command]
pub fn get_memory_usage() -> Result<crate::platform::system::MemoryReport, String> {
    PlatformSystem::memory_usage()
}

#[tauri::command]
pub fn trim_memory() -> Result<crate::platform::system::TrimResult, String> {
    PlatformSystem::trim_memory()
}

#[tauri::command]
pub fn get_capture_diagnostics() -> Option<crate::platform::desktop::CaptureDiagnostics> {
    crate::platform::desktop::get_last_diagnostics()
}

#[tauri::command]
pub async fn test_screen_capture(
    delay_secs: u32,
) -> Result<crate::platform::desktop::CaptureDiagnostics, String> {
    if delay_secs > 0 {
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs as u64)).await;
    }
    let desktop = crate::platform::desktop();
    let bounds = desktop.bounds()?;
    let _ = desktop.capture(bounds)?;
    crate::platform::desktop::get_last_diagnostics().ok_or_else(|| "未能生成诊断记录".into())
}

#[tauri::command]
pub async fn open_capture_debug_image(app: AppHandle) -> Result<String, String> {
    let mut temp = std::env::temp_dir();
    temp.push("transportal_last_capture.png");
    for _ in 0..10 {
        if temp.exists() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    if !temp.exists() {
        return Err("未找到已保存的调试截图图片，请先触发一次截屏。".into());
    }
    let path_str = temp.to_string_lossy().into_owned();
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(&path_str, None::<&str>)
        .map_err(|e| format!("打开图片失败: {e}"))?;
    Ok(path_str)
}
