use crate::dict::{DictEntry, ExampleSentence};
use crate::dictionary::{DictStatus, LookupService, SourceInfo};
use crate::native::CaptureOutcome;
use crate::provider::{
    StreamChunk, TranslateQuery, TranslateResult, Translator,
    lang::{DetectedLang, detect_language},
    openai::{OpenAiCompatibleProvider, OpenAiConfig},
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextIntent {
    Word,
    Sentence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentResult {
    pub intent: TextIntent,
    pub word_count: usize,
    pub char_count: usize,
    pub clean_text: String,
    pub is_pure_ascii: bool,
}

/// Chinese and Japanese are written without spaces, so whitespace cannot
/// separate words there. Dictionary headwords rarely exceed four characters
/// (idioms included); longer runs are phrases or sentences to translate.
const MAX_UNSPACED_WORD_CHARS: usize = 4;

/// Fast heuristic text granularity detector (< 1ms).
///
/// Distinguishes single words / short phrases (word card) from sentences and
/// paragraphs (ghost bubble). Shared by the IPC command and the hotkey path.
pub fn classify_intent(text: &str) -> IntentResult {
    let clean = text.trim().to_string();
    let char_count = clean.chars().count();
    let is_pure_ascii = clean.is_ascii();

    let has_sentence_punctuation =
        clean.contains(['.', '?', '!', ';', '。', '？', '！', '；', '\n']);

    let words: Vec<&str> = clean.split_whitespace().collect();
    let word_count = words.len();
    let unspaced = matches!(
        detect_language(&clean),
        DetectedLang::Chinese | DetectedLang::Japanese
    );

    let intent = if word_count == 0 {
        TextIntent::Word
    } else if has_sentence_punctuation {
        TextIntent::Sentence
    } else if unspaced {
        if word_count == 1 && char_count <= MAX_UNSPACED_WORD_CHARS {
            TextIntent::Word
        } else {
            TextIntent::Sentence
        }
    } else if word_count <= 2 && char_count <= 40 {
        TextIntent::Word
    } else {
        TextIntent::Sentence
    };

    IntentResult {
        intent,
        word_count,
        char_count,
        clean_text: clean,
        is_pure_ascii,
    }
}

/// IPC command wrapper around [`classify_intent`].
#[tauri::command]
pub fn detect_intent(text: String) -> IntentResult {
    classify_intent(&text)
}

/// Build the provider selected by name from the microkernel registry.
fn build_provider(provider: Option<String>, config: Option<OpenAiConfig>) -> Arc<dyn Translator> {
    crate::provider::create(provider.as_deref().unwrap_or("google_free"), config)
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
    crate::shortcut::set_selection_hotkey(&app, &accelerator)
}

/// Configure any global shortcut action individually.
#[tauri::command]
pub fn set_shortcut(
    action: crate::shortcut::ShortcutAction,
    accelerator: String,
    app: AppHandle,
) -> Result<(), String> {
    crate::shortcut::set_shortcut(&app, action, &accelerator)
}

#[tauri::command]
pub fn shortcut_status(app: AppHandle) -> crate::shortcut::ShortcutReport {
    crate::shortcut::status(&app)
}

#[tauri::command]
pub fn finish_startup_checks(app: AppHandle) {
    crate::shortcut::finish_startup_checks(&app);
}

#[tauri::command]
pub fn retry_shortcuts(app: AppHandle) -> crate::shortcut::ShortcutReport {
    crate::shortcut::retry(&app)
}

#[tauri::command]
pub fn integrity_status(
    state: State<'_, crate::integrity::IntegrityState>,
) -> crate::integrity::IntegrityReport {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
pub async fn verify_integrity(app: AppHandle) -> Result<crate::integrity::IntegrityReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let report = crate::integrity::startup_report();
        *app.state::<crate::integrity::IntegrityState>()
            .0
            .lock()
            .unwrap() = report.clone();
        if report.blocks_startup() {
            crate::native::integrity::platform()
                .show_failure(report.error.as_deref().unwrap_or("程序完整性检查失败。"));
            app.exit(2);
        }
        report
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn ocr_status(app: AppHandle) -> Result<crate::ocr::OcrStatus, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<crate::ocr::OcrService>().status())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_ocr_language(language: Option<String>, app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::ocr::OcrService>().set_language(language)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_ocr_config(app: AppHandle) -> crate::ocr::registry::OcrConfig {
    app.state::<crate::ocr::OcrService>().config()
}

#[tauri::command]
pub async fn set_ocr_config(
    config: crate::ocr::registry::OcrConfig,
    app: AppHandle,
) -> Result<crate::ocr::registry::OcrConfig, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::ocr::OcrService>().configure(config)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn install_onnx_ocr(
    app: AppHandle,
    on_progress: tauri::ipc::Channel<crate::ocr::assets::InstallProgress>,
) -> Result<(), String> {
    let assets = app.state::<crate::ocr::OcrService>().assets.clone();
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
    mode: crate::ocr::OcrMode,
    app: AppHandle,
) -> Result<u64, String> {
    app.state::<crate::ocr::OcrService>()
        .start_image(&app, mode, std::path::PathBuf::from(path))
}

#[tauri::command]
pub async fn start_ocr(mode: crate::ocr::OcrMode, app: AppHandle) -> Result<u64, String> {
    app.state::<crate::ocr::OcrService>()
        .start(&app, mode, None)
}

#[tauri::command]
pub async fn cancel_ocr(app: AppHandle) {
    app.state::<crate::ocr::OcrService>().cancel(&app);
}

#[tauri::command]
pub fn ocr_latest(service: State<'_, crate::ocr::OcrService>) -> Option<crate::ocr::OcrEvent> {
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
) -> Result<crate::ocr::picture::PictureTranslation, String> {
    let (picture, cancel) = app
        .state::<crate::ocr::OcrService>()
        .picture_translation(id, texts)?;
    crate::ocr::picture::translate_blocks(
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
        let image = crate::ocr::picture::decode_png(&data_url)?;
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
    options: crate::ocr::monitor::MonitorOptions,
    app: AppHandle,
) -> Result<u64, String> {
    app.state::<crate::ocr::monitor::MonitorService>()
        .begin(&app, options)
        .await
}

#[tauri::command]
pub fn auto_ocr_latest(
    service: State<'_, crate::ocr::monitor::MonitorService>,
) -> crate::ocr::monitor::MonitorUpdate {
    service.latest()
}

/// Shared native region selector, used by snips and auto-translation alike.
#[tauri::command]
pub fn region_selection(
    service: State<'_, crate::ocr::region::RegionService>,
) -> Option<crate::ocr::region::RegionSelection> {
    service.selection()
}

/// Stream raw RGBA physical pixels for zero-latency, 100% lossless canvas freeze.
#[tauri::command]
pub fn get_frozen_pixels(
    id: u64,
    service: State<'_, crate::ocr::region::RegionService>,
) -> Result<tauri::ipc::Response, String> {
    let raw = service.get_frozen_pixels(id)?;
    Ok(tauri::ipc::Response::new(raw))
}

#[tauri::command]
pub fn confirm_region(
    id: u64,
    region: crate::native::desktop::ScreenRect,
    app: AppHandle,
) -> Result<(), String> {
    crate::ocr::region::confirm(&app, id, region)
}

#[tauri::command]
pub fn cancel_region(id: Option<u64>, app: AppHandle) {
    crate::ocr::region::cancel(&app, id);
}

#[tauri::command]
pub async fn benchmark_ocr(app: AppHandle) -> Result<u64, String> {
    app.state::<crate::ocr::OcrService>().benchmark()
}

#[tauri::command]
pub fn stop_auto_ocr(discard: bool, app: AppHandle) {
    app.state::<crate::ocr::monitor::MonitorService>()
        .stop(&app, discard);
}

#[tauri::command]
pub fn pause_auto_ocr(paused: bool, app: AppHandle) -> Result<(), String> {
    app.state::<crate::ocr::monitor::MonitorService>()
        .pause(&app, paused)
}

/// Progress payload for the full-dictionary download + import pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct DictProgress {
    /// `downloading` | `importing` | `done`
    pub stage: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub imported: Option<usize>,
}

/// Import an ECCEDICT-format CSV file into the local redb store.
#[tauri::command]
pub async fn import_eccedict(
    path: String,
    service: State<'_, LookupService>,
) -> Result<usize, String> {
    let store = service.local_handle();
    let count = tauri::async_runtime::spawn_blocking(move || store.import_csv_file(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    service.invalidate_cache();
    Ok(count)
}

/// Full ECCEDICT CSV mirrors, tried in order (jsdelivr first for CN networks).
const ECCEDICT_CSV_URLS: [&str; 2] = [
    "https://cdn.jsdelivr.net/gh/H1DDENADM1N/ECCEDICT@master/ecdict.csv",
    "https://raw.githubusercontent.com/H1DDENADM1N/ECCEDICT/master/ecdict.csv",
];

/// Download the full (~63 MB) ECCEDICT CSV with progress, then import it.
#[tauri::command]
pub async fn download_eccedict(
    on_progress: tauri::ipc::Channel<DictProgress>,
    service: State<'_, LookupService>,
) -> Result<usize, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let mut response = None;
    let mut last_error = String::new();
    for url in ECCEDICT_CSV_URLS {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                response = Some(resp);
                break;
            }
            Ok(resp) => last_error = format!("{url} -> HTTP {}", resp.status()),
            Err(err) => last_error = format!("{url} -> {err}"),
        }
    }
    let response = response.ok_or_else(|| format!("all mirrors failed: {last_error}"))?;

    let total = response.content_length();
    let path = std::env::temp_dir().join("transportal-eccedict.csv");
    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| format!("failed to create temp file: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download stream error: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("failed to write download: {e}"))?;
        downloaded += chunk.len() as u64;
        let _ = on_progress.send(DictProgress {
            stage: "downloading".to_string(),
            downloaded,
            total,
            imported: None,
        });
    }
    drop(file);

    let _ = on_progress.send(DictProgress {
        stage: "importing".to_string(),
        downloaded,
        total,
        imported: None,
    });

    let path = path.to_string_lossy().to_string();
    let store = service.local_handle();
    let count = tauri::async_runtime::spawn_blocking(move || store.import_csv_file(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    service.invalidate_cache();

    let _ = on_progress.send(DictProgress {
        stage: "done".to_string(),
        downloaded,
        total,
        imported: Some(count),
    });

    Ok(count)
}

/// Capture the current foreground selection (UIA, then clipboard fallback).
#[tauri::command]
pub async fn capture_selection(app: AppHandle) -> Result<CaptureOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::selection::SelectionService>()
            .capture(&app)
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_double_copy_enabled(enabled: bool, app: AppHandle) {
    crate::shortcut::set_double_copy_enabled(&app, enabled);
}

#[tauri::command]
pub fn set_clipboard_monitor_enabled(enabled: bool) {
    crate::shortcut::set_clipboard_monitor_enabled(enabled);
}

#[tauri::command]
pub fn set_ink_translate_key(key: String) {
    crate::shortcut::set_ink_translate_key(&key);
}

#[tauri::command]
pub async fn replace_captured_selection(
    token: u64,
    text: String,
    app: AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::selection::SelectionService>()
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
    if let Some((x, y)) = crate::native::get_global_cursor_pos() {
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
    crate::shortcut::set_pinned(pinned);
}

/// Probe AnkiConnect and return its API version.
#[tauri::command]
pub async fn anki_test_connection(endpoint: String) -> Result<i64, String> {
    crate::anki::version(&endpoint).await
}

/// List Anki deck names.
#[tauri::command]
pub async fn anki_list_decks(endpoint: String) -> Result<Vec<String>, String> {
    crate::anki::deck_names(&endpoint).await
}

/// Add a note (word + phonetic + original context sentence) to Anki.
#[tauri::command]
pub async fn anki_add_note(note: crate::anki::AnkiNoteRequest) -> Result<i64, String> {
    crate::anki::add_note(&note).await
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
    if openai_config.api_key.trim().is_empty() {
        return Ok(Vec::new());
    }

    const SYSTEM: &str = "You are an English dictionary assistant. \
Return ONLY a JSON array of 2 to 3 example sentences. Each item must be \
{\"en\": \"<English sentence>\", \"zh\": \"<Chinese translation>\"}. \
No prose, no markdown, no code fences.";

    let provider = OpenAiCompatibleProvider::new(openai_config);
    let content = provider
        .chat(SYSTEM, &format!("Word: {word}"))
        .await
        .map_err(|e| e.to_string())?;

    Ok(parse_examples(&content))
}

/// Extract a JSON array of `{en, zh}` objects from an LLM reply.
fn parse_examples(content: &str) -> Vec<ExampleSentence> {
    let start = content.find('[');
    let end = content.rfind(']');
    let Some((start, end)) = start.zip(end) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }

    serde_json::from_str::<Vec<ExampleSentence>>(&content[start..=end]).unwrap_or_default()
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
    crate::native::resize_and_clamp_window(&window, width, height)
}

/// Hide the overlay unless a newer show superseded the frontend's dismiss.
#[tauri::command]
pub async fn hide_shell(app: AppHandle, generation: Option<u64>) -> Result<bool, String> {
    crate::shell::hide(&app, generation).await
}

#[tauri::command]
pub fn set_window_acrylic(window: tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    crate::native::set_window_acrylic(&window, enabled);
    Ok(())
}

#[tauri::command]
pub fn set_window_theme(window: tauri::WebviewWindow, dark: bool) -> Result<(), String> {
    crate::native::set_window_theme(&window, dark);
    Ok(())
}

#[tauri::command]
pub fn get_memory_usage() -> Result<crate::native::memory::MemoryReport, String> {
    crate::native::memory::get_memory_usage()
}

#[tauri::command]
pub fn trim_memory() -> Result<crate::native::memory::TrimResult, String> {
    crate::native::memory::trim_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_intent_word() {
        let res = detect_intent("portal".to_string());
        assert_eq!(res.intent, TextIntent::Word);
        assert_eq!(res.word_count, 1);

        let res2 = detect_intent("cognitive load".to_string());
        assert_eq!(res2.intent, TextIntent::Word);
        assert_eq!(res2.word_count, 2);
    }

    #[test]
    fn test_detect_intent_sentence() {
        let res = detect_intent("This is a cognitive portal for languages.".to_string());
        assert_eq!(res.intent, TextIntent::Sentence);

        let res2 = detect_intent("Hello world! How are you doing today?".to_string());
        assert_eq!(res2.intent, TextIntent::Sentence);
    }

    #[test]
    fn test_detect_intent_unspaced_scripts() {
        // Whole unpunctuated Chinese phrases are one whitespace "word".
        for word in ["打", "你好", "取词", "一石二鸟"] {
            assert_eq!(
                detect_intent(word.to_string()).intent,
                TextIntent::Word,
                "{word}"
            );
        }
        for phrase in [
            "今天天气真不错",
            "计算机科学与技术",
            "你好 世界",
            "これは日本語です",
        ] {
            assert_eq!(
                detect_intent(phrase.to_string()).intent,
                TextIntent::Sentence,
                "{phrase}"
            );
        }
    }

    #[test]
    fn test_parse_examples() {
        let raw = "```json\n[{\"en\":\"A portal opened.\",\"zh\":\"传送门打开了。\"}]\n```";
        let examples = parse_examples(raw);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].en, "A portal opened.");
        assert_eq!(examples[0].zh, "传送门打开了。");

        assert!(parse_examples("no json here").is_empty());
    }
}
