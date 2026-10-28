//! Configurable global shortcuts and non-consuming copy/snip gestures.

use crate::core::ocr::{OcrMode, OcrService};
use crate::platform::PlatformWindow;
use crate::platform::gestures::{GestureSource, InkKey};
use crate::platform::window::WindowOps;
use crate::platform::{CAPABILITIES, Capabilities, PlatformGestures};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub const STATUS_EVENT: &str = "transportal://shortcut-status";
pub const SELECTION_EVENT: &str = "transportal://selection";

const UNSUPPORTED: &str = "当前平台不支持此快捷键功能。";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    Selection,
    OcrCard,
    #[serde(alias = "ocr_translate")]
    OcrPicture,
    #[serde(alias = "ocr_text")]
    OcrSearch,
    DoubleCopy,
    ReplaceSelection,
    SilentOcr,
    InputTranslate,
}

const ACTIONS: [ShortcutAction; 8] = [
    ShortcutAction::Selection,
    ShortcutAction::OcrCard,
    ShortcutAction::OcrPicture,
    ShortcutAction::OcrSearch,
    ShortcutAction::DoubleCopy,
    ShortcutAction::ReplaceSelection,
    ShortcutAction::SilentOcr,
    ShortcutAction::InputTranslate,
];

impl ShortcutAction {
    pub fn id(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::OcrCard => "ocr_card",
            Self::OcrPicture => "ocr_picture",
            Self::OcrSearch => "ocr_search",
            Self::DoubleCopy => "double_copy",
            Self::ReplaceSelection => "replace_selection",
            Self::SilentOcr => "silent_ocr",
            Self::InputTranslate => "input_translate",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Selection => "划词查询",
            Self::OcrCard => "截图对照翻译",
            Self::OcrPicture => "原图直接翻译",
            Self::OcrSearch => "截图搜索",
            Self::DoubleCopy => "快捷翻译",
            Self::ReplaceSelection => "替换翻译",
            Self::SilentOcr => "静默 OCR",
            Self::InputTranslate => "输入翻译",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    pub selection: String,
    pub ocr_card: String,
    #[serde(alias = "ocr_translate")]
    pub ocr_picture: String,
    #[serde(alias = "ocr_text")]
    pub ocr_search: String,
    pub double_copy: String,
    pub replace_selection: String,
    pub silent_ocr: String,
    pub input_translate: String,
}
impl Default for ShortcutConfig {
    fn default() -> Self {
        Self {
            selection: "Alt+Q".into(),
            ocr_card: "Alt+Shift+X".into(),
            ocr_picture: "Alt+S".into(),
            ocr_search: "Alt+Shift+C".into(),
            double_copy: "Ctrl+C+C".into(),
            replace_selection: "Alt+Z".into(),
            silent_ocr: "Alt+Shift+S".into(),
            input_translate: "Alt+Space".into(),
        }
    }
}
impl ShortcutConfig {
    pub fn get(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::Selection => &self.selection,
            ShortcutAction::OcrCard => &self.ocr_card,
            ShortcutAction::OcrPicture => &self.ocr_picture,
            ShortcutAction::OcrSearch => &self.ocr_search,
            ShortcutAction::DoubleCopy => &self.double_copy,
            ShortcutAction::ReplaceSelection => &self.replace_selection,
            ShortcutAction::SilentOcr => &self.silent_ocr,
            ShortcutAction::InputTranslate => &self.input_translate,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub requested: String,
    pub active: Option<String>,
    pub error: Option<String>,
    pub system: bool,
}
impl ShortcutStatus {
    fn finish(&mut self, result: &Result<(), String>) {
        self.error = result.as_ref().err().cloned();
        if result.is_ok() {
            self.active = (!self.requested.is_empty()).then(|| self.requested.clone());
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutReport {
    pub revision: u64,
    pub ready: bool,
    pub shortcuts: Vec<ShortcutStatus>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BindingKey {
    Chord(Shortcut),
    SystemSnip,
    DoubleCopy,
}
struct Binding {
    key: Option<BindingKey>,
    status: ShortcutStatus,
}
struct BindingState {
    bindings: BTreeMap<ShortcutAction, Binding>,
    ready: bool,
    revision: u64,
    recording: bool,
}
impl Default for BindingState {
    fn default() -> Self {
        let defaults = ShortcutConfig::default();
        Self {
            ready: false,
            revision: 0,
            recording: false,
            bindings: ACTIONS
                .into_iter()
                .map(|action| {
                    (
                        action,
                        Binding {
                            key: None,
                            status: ShortcutStatus {
                                id: action.id(),
                                label: action.label(),
                                requested: defaults.get(action).into(),
                                active: None,
                                error: None,
                                system: false,
                            },
                        },
                    )
                })
                .collect(),
        }
    }
}
#[derive(Default)]
pub struct ShortcutBindings {
    state: Mutex<BindingState>,
}

fn parse_binding(action: ShortcutAction, accelerator: &str) -> Result<Option<BindingKey>, String> {
    let value = accelerator.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if ["ctrl+c+c", "control+c+c"].contains(&value.replace(' ', "").to_ascii_lowercase().as_str()) {
        return if action == ShortcutAction::DoubleCopy {
            Ok(Some(BindingKey::DoubleCopy))
        } else {
            Err("Ctrl+C+C 用于快捷翻译，请选择普通组合键。".into())
        };
    }
    let shortcut: Shortcut = value
        .parse()
        .map_err(|e| format!("无效的快捷键 '{value}'：{e}"))?;
    if shortcut == Shortcut::new(Some(Modifiers::CONTROL), Code::KeyC) {
        return Err("Ctrl+C 保留给系统复制；双按复制请填写 Ctrl+C+C。".into());
    }
    if shortcut == Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyS) {
        return if matches!(
            action,
            ShortcutAction::OcrSearch
                | ShortcutAction::OcrPicture
                | ShortcutAction::OcrCard
                | ShortcutAction::SilentOcr
        ) {
            Ok(Some(BindingKey::SystemSnip))
        } else {
            Err("Win+Shift+S 是系统截图键，只能关联 OCR 功能。".into())
        };
    }
    let key = shortcut.key.to_string();
    let function_key = key
        .strip_prefix('F')
        .and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number));
    if shortcut.mods.is_empty() && !function_key {
        return Err("请使用含 Ctrl、Alt、Shift 或 Win 的组合键，或 F1–F24。".into());
    }
    Ok(Some(BindingKey::Chord(shortcut)))
}

/// Whether `action` can work with the given platform capabilities.
/// Unsupported actions are hidden from the status report and rejected.
fn supported_with(capabilities: Capabilities, action: ShortcutAction) -> bool {
    match action {
        ShortcutAction::Selection | ShortcutAction::InputTranslate => true,
        ShortcutAction::DoubleCopy => capabilities.keyboard_gestures,
        ShortcutAction::ReplaceSelection => capabilities.selection_replace,
        ShortcutAction::OcrSearch
        | ShortcutAction::OcrPicture
        | ShortcutAction::OcrCard
        | ShortcutAction::SilentOcr => capabilities.screen_capture,
    }
}

fn supported(action: ShortcutAction) -> bool {
    supported_with(CAPABILITIES, action)
}

fn validate_conflict(
    state: &BindingState,
    action: ShortcutAction,
    key: Option<BindingKey>,
) -> Result<(), String> {
    if let Some(key) = key {
        for (&other, binding) in &state.bindings {
            if other != action
                && (binding.key == Some(key)
                    || parse_binding(other, &binding.status.requested)
                        .ok()
                        .flatten()
                        == Some(key))
            {
                return Err(format!(
                    "此键位已用于「{}」，请先修改或清除该项。",
                    other.label()
                ));
            }
        }
    }
    Ok(())
}

fn apply_locked(
    app: &AppHandle,
    state: &mut BindingState,
    action: ShortcutAction,
    value: &str,
) -> Result<(), String> {
    state.bindings.get_mut(&action).unwrap().status.requested = value.trim().into();
    let result = (|| {
        let key = parse_binding(action, value)?;
        validate_conflict(state, action, key)?;
        if key == Some(BindingKey::SystemSnip) && !CAPABILITIES.system_snip {
            return Err(UNSUPPORTED.into());
        }
        if matches!(key, Some(BindingKey::SystemSnip | BindingKey::DoubleCopy))
            && !app.state::<OcrService>().observer_status().0
        {
            return Err("键盘监听未就绪，请重新检测。".into());
        }
        let previous = state.bindings[&action].key;
        if key != previous {
            let manager = app.global_shortcut();
            if let Some(BindingKey::Chord(shortcut)) = key {
                manager
                    .on_shortcut(shortcut, move |app, _, event| {
                        if event.state == ShortcutState::Pressed {
                            trigger(app, action);
                        }
                    })
                    .map_err(|e| format!("无法注册 {value}，可能已被其他程序占用：{e}"))?;
                if state.recording {
                    let _ = manager.unregister(shortcut);
                }
            }
            if !state.recording
                && let Some(BindingKey::Chord(shortcut)) = previous
                && let Err(error) = manager.unregister(shortcut)
            {
                if let Some(BindingKey::Chord(new)) = key {
                    let _ = manager.unregister(new);
                }
                return Err(format!("无法替换原键位：{error}"));
            }
            state.bindings.get_mut(&action).unwrap().key = key;
        }
        state.bindings.get_mut(&action).unwrap().status.system =
            matches!(key, Some(BindingKey::SystemSnip | BindingKey::DoubleCopy));
        if action == ShortcutAction::DoubleCopy {
            app.state::<crate::core::selection::SelectionService>()
                .set_double_copy(key == Some(BindingKey::DoubleCopy));
        }
        Ok(())
    })();
    state
        .bindings
        .get_mut(&action)
        .unwrap()
        .status
        .finish(&result);
    state.revision += 1;
    result
}

pub fn set_shortcut(
    app: &AppHandle,
    action: ShortcutAction,
    accelerator: &str,
) -> Result<(), String> {
    if !supported(action) {
        return Err(UNSUPPORTED.into());
    }
    let bindings = app.state::<ShortcutBindings>();
    let result = apply_locked(
        app,
        &mut bindings.state.lock().unwrap(),
        action,
        accelerator,
    );
    publish_status(app);
    result
}

fn unregister_chords(app: &AppHandle, state: &BindingState) {
    let manager = app.global_shortcut();
    for binding in state.bindings.values() {
        if let Some(BindingKey::Chord(shortcut)) = binding.key {
            let _ = manager.unregister(shortcut);
        }
    }
}

fn register_chords(app: &AppHandle, state: &BindingState) {
    let manager = app.global_shortcut();
    for (&action, binding) in &state.bindings {
        if let Some(BindingKey::Chord(shortcut)) = binding.key {
            let _ = manager.on_shortcut(shortcut, move |app, _, event| {
                if event.state == ShortcutState::Pressed {
                    trigger(app, action);
                }
            });
        }
    }
}

pub fn set_recording(app: &AppHandle, recording: bool) {
    let bindings = app.state::<ShortcutBindings>();
    let mut state = bindings.state.lock().unwrap();
    if state.recording == recording {
        return;
    }
    state.recording = recording;
    RECORDING.store(recording, Ordering::SeqCst);
    if recording {
        unregister_chords(app, &state);
    } else {
        unregister_chords(app, &state);
        register_chords(app, &state);
    }
}

/// Restore the whole mapping together, allowing saved bindings to swap keys.
pub fn restore(app: &AppHandle, config: ShortcutConfig) -> Result<ShortcutConfig, String> {
    let parsed = ACTIONS
        .into_iter()
        .map(|action| Ok((action, parse_binding(action, config.get(action))?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    for (index, action) in ACTIONS.iter().enumerate() {
        if let Some(key) = parsed[action]
            && ACTIONS[index + 1..]
                .iter()
                .any(|other| parsed[other] == Some(key))
        {
            return Err("保存的快捷键有重复，请重新设置。".into());
        }
    }
    let bindings = app.state::<ShortcutBindings>();
    let mut state = bindings.state.lock().unwrap();
    for &action in &ACTIONS {
        let binding = state.bindings.get_mut(&action).unwrap();
        if binding.key != parsed[&action] {
            if let Some(BindingKey::Chord(shortcut)) = binding.key {
                app.global_shortcut()
                    .unregister(shortcut)
                    .map_err(|e| e.to_string())?;
            }
            binding.key = None;
            binding.status.active = None;
        }
        binding.status.requested = config.get(action).trim().into();
    }
    for action in ACTIONS {
        if supported(action) {
            let _ = apply_locked(app, &mut state, action, config.get(action));
        }
    }
    drop(state);
    publish_status(app);
    Ok(config)
}

pub fn set_double_copy_enabled(app: &AppHandle, enabled: bool) {
    let _ = set_shortcut(
        app,
        ShortcutAction::DoubleCopy,
        if enabled { "Ctrl+C+C" } else { "" },
    );
}

pub fn set_ink_translate_key(key: &str) {
    PlatformGestures::set_ink_key(InkKey::parse(key));
}

pub fn register(app: &AppHandle) {
    let _ = restore(app, ShortcutConfig::default());
}
pub fn status(app: &AppHandle) -> ShortcutReport {
    let bindings = app.state::<ShortcutBindings>();
    let state = bindings.state.lock().unwrap();
    ShortcutReport {
        revision: state.revision,
        ready: state.ready,
        shortcuts: ACTIONS
            .into_iter()
            .filter(|action| supported(*action))
            .map(|action| state.bindings[&action].status.clone())
            .collect(),
    }
}
fn publish_status(app: &AppHandle) {
    let _ = app.emit(STATUS_EVENT, status(app));
}
pub fn finish_startup_checks(app: &AppHandle) {
    let bindings = app.state::<ShortcutBindings>();
    let mut state = bindings.state.lock().unwrap();
    if state.ready {
        return;
    }
    state.ready = true;
    state.revision += 1;
    drop(state);
    let report = status(app);
    let failed = report
        .shortcuts
        .iter()
        .any(|shortcut| shortcut.error.is_some());
    let _ = app.emit(STATUS_EVENT, report);
    if failed {
        show_shell(app);
    }
}
pub fn retry(app: &AppHandle) -> ShortcutReport {
    let ocr = app.state::<OcrService>();
    if CAPABILITIES.keyboard_gestures && !ocr.observer_status().0 {
        ocr.install_observer(app);
    }
    let bindings = app.state::<ShortcutBindings>();
    let mut state = bindings.state.lock().unwrap();
    for action in ACTIONS {
        if supported(action) {
            let value = state.bindings[&action].status.requested.clone();
            let _ = apply_locked(app, &mut state, action, &value);
        }
    }
    drop(state);
    publish_status(app);
    status(app)
}

static RECORDING: AtomicBool = AtomicBool::new(false);

pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

fn trigger(app: &AppHandle, action: ShortcutAction) {
    if is_recording() {
        return;
    }
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
        PlatformWindow::force_hide_window(&settings);
    }
    match action {
        ShortcutAction::Selection | ShortcutAction::DoubleCopy => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || on_hotkey(&app));
        }
        ShortcutAction::ReplaceSelection => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                on_replace_selection(&app).await;
            });
        }
        ShortcutAction::InputTranslate => {
            on_input_translate(app);
        }
        ShortcutAction::OcrSearch
        | ShortcutAction::OcrPicture
        | ShortcutAction::OcrCard
        | ShortcutAction::SilentOcr => {
            let mode = match action {
                ShortcutAction::OcrSearch => OcrMode::Search,
                ShortcutAction::OcrPicture => OcrMode::Picture,
                ShortcutAction::OcrCard => OcrMode::Card,
                ShortcutAction::SilentOcr => OcrMode::Silent,
                _ => unreachable!(),
            };
            let service = app.state::<OcrService>();
            if let Err(error) = service.start(app, mode, None) {
                service.show_start_error(app, mode, error);
            }
        }
    }
}

static PINNED: AtomicBool = AtomicBool::new(false);

pub fn is_pinned() -> bool {
    PINNED.load(Ordering::Relaxed)
}

pub fn set_pinned(pinned: bool) {
    PINNED.store(pinned, Ordering::Relaxed);
}

pub fn on_input_translate(app: &AppHandle) {
    if is_recording() {
        return;
    }
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
        PlatformWindow::force_hide_window(&settings);
    }
    if let Some(window) = app.get_webview_window("shell") {
        if !is_pinned() || !window.is_visible().unwrap_or(false) {
            let _ = PlatformWindow::place_window_smartly(&window, None, true, false);
        }
        crate::core::shell::present(app, false, |window| {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        });
        let _ = app.emit_to("shell", "transportal://focus-input", ());
    }
}

fn resolve_replace_translation_config(
    app: &AppHandle,
) -> (
    String,
    Option<String>,
    Option<crate::core::translation::OpenAiConfig>,
) {
    use tauri_plugin_store::StoreExt;
    if let Ok(store) = app.store("settings.json") {
        let provider = store
            .get("provider")
            .and_then(|v| v.as_str().map(String::from));
        let target_lang = store
            .get("targetLanguage")
            .and_then(|v| v.as_str().map(String::from));
        let openai_config = store
            .get("openaiConfig")
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        return (
            provider.unwrap_or_else(|| "bing_free".to_string()),
            target_lang,
            openai_config,
        );
    }
    ("bing_free".to_string(), None, None)
}

pub async fn on_replace_selection(app: &AppHandle) {
    if is_recording() {
        return;
    }
    app.state::<crate::core::ocr::monitor::MonitorService>()
        .stop(app, true);
    app.state::<OcrService>().cancel(app);
    let app_clone = app.clone();
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        app_clone
            .state::<crate::core::selection::SelectionService>()
            .capture(&app_clone)
    })
    .await
    .unwrap_or_else(|_| crate::core::selection::CaptureOutcome {
        text: String::new(),
        method: crate::platform::selection::CaptureMethod::Clipboard,
        error: Some("捕获失败".into()),
        replace_token: None,
        bounds: None,
    });

    if outcome.text.trim().is_empty() || outcome.replace_token.is_none() {
        let _ = app.emit_to(
            "shell",
            "transportal://notice",
            "未选中任何文字或目标窗口不支持原地替换。",
        );
        return;
    }

    let token = outcome.replace_token.unwrap();
    let (provider_name, target_lang_pref, openai_config) = resolve_replace_translation_config(app);
    let target_lang = crate::core::translation::resolve_target_lang(
        &outcome.text,
        target_lang_pref.as_deref().or(Some("zh-CN")),
    );
    let provider = crate::core::translation::create(&provider_name, openai_config);
    let query = crate::core::translation::TranslateQuery {
        text: outcome.text,
        source_lang: None,
        target_lang: Some(target_lang.to_string()),
    };

    let result = match provider.translate(&query).await {
        Ok(result) => Ok(result),
        Err(err) => {
            if provider_name != "bing_free" {
                eprintln!(
                    "[replace_selection] provider {provider_name} failed: {err}; retrying with bing_free"
                );
                let fallback = crate::core::translation::create("bing_free", None);
                fallback.translate(&query).await
            } else {
                Err(err)
            }
        }
    };

    match result {
        Ok(result) => {
            let app_clone = app.clone();
            let translated = result.translated_text;
            let replace_res = tauri::async_runtime::spawn_blocking(move || {
                app_clone
                    .state::<crate::core::selection::SelectionService>()
                    .replace(&app_clone, token, &translated)
            })
            .await;
            match replace_res {
                Ok(Err(err)) => {
                    let _ =
                        app.emit_to("shell", "transportal://notice", format!("替换失败：{err}"));
                }
                Err(join_err) => {
                    let _ = app.emit_to(
                        "shell",
                        "transportal://notice",
                        format!("替换任务执行失败：{join_err}"),
                    );
                }
                Ok(Ok(())) => {}
            }
        }
        Err(err) => {
            let _ = app.emit_to(
                "shell",
                "transportal://notice",
                format!("替换翻译失败：{err}"),
            );
        }
    }
}

/// Only reached when the user explicitly bound the OS snip chord to an OCR
/// action; the default bindings use the built-in region selector instead.
pub fn on_system_snip(app: &AppHandle, revision: u32) {
    if is_recording() {
        return;
    }
    let action = app
        .state::<ShortcutBindings>()
        .state
        .lock()
        .unwrap()
        .bindings
        .iter()
        .find_map(|(&action, binding)| {
            (binding.key == Some(BindingKey::SystemSnip)).then_some(action)
        });
    if let Some(action) = action {
        let mode = match action {
            ShortcutAction::OcrCard => OcrMode::Card,
            ShortcutAction::OcrPicture => OcrMode::Picture,
            ShortcutAction::OcrSearch => OcrMode::Search,
            _ => OcrMode::Text,
        };
        let service = app.state::<OcrService>();
        if let Err(error) = service.start(app, mode, Some(revision)) {
            eprintln!("[ocr] ignored screenshot trigger: {error}");
        }
    }
}
pub fn on_hotkey(app: &AppHandle) {
    if is_recording() {
        return;
    }
    app.state::<crate::core::ocr::monitor::MonitorService>()
        .stop(app, true);
    app.state::<OcrService>().cancel(app);
    let outcome = app
        .state::<crate::core::selection::SelectionService>()
        .capture(app);
    let _ = app.emit_to("shell", SELECTION_EVENT, &outcome);
    show_shell_with_anchor(app, outcome.bounds.as_ref(), true);
}
pub fn on_double_copy(app: &AppHandle, revision: u32, window: usize) {
    if is_recording() {
        return;
    }
    let enabled = app
        .state::<ShortcutBindings>()
        .state
        .lock()
        .unwrap()
        .bindings[&ShortcutAction::DoubleCopy]
        .key
        == Some(BindingKey::DoubleCopy);
    if !enabled {
        return;
    }
    if let Some(outcome) = app
        .state::<crate::core::selection::SelectionService>()
        .double_copy(app, revision, window)
    {
        app.state::<crate::core::ocr::monitor::MonitorService>()
            .stop(app, true);
        app.state::<OcrService>().cancel(app);
        let _ = app.emit_to("shell", SELECTION_EVENT, &outcome);
        show_shell_with_anchor(app, outcome.bounds.as_ref(), true);
    }
}

pub fn show_shell(app: &AppHandle) {
    show_shell_with_anchor(app, None, false);
}

pub fn show_shell_with_anchor(
    app: &AppHandle,
    anchor_bounds: Option<&crate::platform::selection::SelectionBounds>,
    no_activate: bool,
) {
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
        PlatformWindow::force_hide_window(&settings);
    }
    if let Some(window) = app.get_webview_window("shell") {
        let reposition = !is_pinned() || !window.is_visible().unwrap_or(false);
        crate::core::shell::present(app, no_activate, |window| {
            // In no-activate mode the placement call is what shows the window.
            if reposition {
                let _ =
                    PlatformWindow::place_window_smartly(window, anchor_bounds, false, no_activate);
            }
            if !no_activate || !window.is_visible().unwrap_or(false) {
                let _ = window.unminimize();
                let _ = window.show();
                if !no_activate {
                    let _ = window.set_focus();
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_cannot_take_over_ocr_or_system_snip_hotkeys() {
        assert!(parse_binding(ShortcutAction::Selection, "Alt+T").is_ok());
        let state = BindingState::default();
        assert!(
            validate_conflict(
                &state,
                ShortcutAction::Selection,
                parse_binding(ShortcutAction::Selection, "Alt+Shift+S").unwrap()
            )
            .is_err()
        );
        assert!(
            validate_conflict(
                &state,
                ShortcutAction::Selection,
                parse_binding(ShortcutAction::Selection, "Alt+S").unwrap()
            )
            .is_err()
        );
        assert!(
            validate_conflict(
                &state,
                ShortcutAction::Selection,
                parse_binding(ShortcutAction::Selection, "Alt+Shift+X").unwrap()
            )
            .is_err()
        );
        assert!(
            validate_conflict(
                &state,
                ShortcutAction::Selection,
                parse_binding(ShortcutAction::Selection, "Alt+Z").unwrap()
            )
            .is_err()
        );
        assert!(
            validate_conflict(
                &state,
                ShortcutAction::Selection,
                parse_binding(ShortcutAction::Selection, "Alt+Shift+C").unwrap()
            )
            .is_err()
        );
        assert!(
            validate_conflict(
                &state,
                ShortcutAction::Selection,
                parse_binding(ShortcutAction::Selection, "Alt+Space").unwrap()
            )
            .is_err()
        );
        assert!(parse_binding(ShortcutAction::Selection, "Super+Shift+S").is_err());
        assert!(parse_binding(ShortcutAction::Selection, "Shift+Super+KeyS").is_err());
        assert!(parse_binding(ShortcutAction::Selection, "Alt+Q").is_ok());
        assert!(parse_binding(ShortcutAction::Selection, "Ctrl+C").is_err());
        assert!(parse_binding(ShortcutAction::Selection, "Ctrl+Shift+D").is_ok());
    }

    #[test]
    fn capabilities_decide_which_shortcuts_exist() {
        for action in ACTIONS {
            assert!(supported_with(Capabilities::ALL, action), "{action:?}");
        }
        let available: Vec<_> = ACTIONS
            .into_iter()
            .filter(|action| supported_with(Capabilities::NONE, *action))
            .collect();
        assert_eq!(
            available,
            [ShortcutAction::Selection, ShortcutAction::InputTranslate]
        );
        let gestures_only = Capabilities {
            keyboard_gestures: true,
            ..Capabilities::NONE
        };
        assert!(supported_with(gestures_only, ShortcutAction::DoubleCopy));
        assert!(!supported_with(
            gestures_only,
            ShortcutAction::ReplaceSelection
        ));
    }

    #[test]
    fn a_failed_request_keeps_the_working_binding_and_retry_clears_the_error() {
        let mut binding = BindingState::default()
            .bindings
            .remove(&ShortcutAction::Selection)
            .unwrap()
            .status;
        binding.finish(&Ok(()));
        binding.requested = "Ctrl+Shift+D".into();
        binding.finish(&Err("occupied".into()));
        assert_eq!(binding.active.as_deref(), Some("Alt+Q"));
        assert_eq!(binding.requested, "Ctrl+Shift+D");
        assert_eq!(binding.error.as_deref(), Some("occupied"));
        binding.finish(&Ok(()));
        assert_eq!(binding.active.as_deref(), Some("Ctrl+Shift+D"));
        assert!(binding.error.is_none());
    }
}
