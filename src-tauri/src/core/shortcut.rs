//! Configurable global shortcuts and non-consuming Windows copy/snip gestures.

use crate::core::ocr::{OcrMode, OcrService};
use crate::platform::PlatformWindow;
use crate::platform::window::WindowOps;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
#[cfg(not(target_os = "windows"))]
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub const STATUS_EVENT: &str = "transportal://shortcut-status";
pub const RECORD_EVENT: &str = "transportal://shortcut-recorded";
pub const SELECTION_EVENT: &str = "transportal://selection";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    Selection,
    OcrCard,
    #[serde(alias = "ocr_translate")]
    OcrPicture,
    OcrText,
    DoubleCopy,
    ReplaceSelection,
    SilentOcr,
    InputTranslate,
}

const ACTIONS: [ShortcutAction; 8] = [
    ShortcutAction::Selection,
    ShortcutAction::OcrCard,
    ShortcutAction::OcrPicture,
    ShortcutAction::OcrText,
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
            Self::OcrText => "ocr_text",
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
            Self::OcrText => "提取文字",
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
    pub ocr_text: String,
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
            ocr_text: "Alt+Shift+S".into(),
            double_copy: "Ctrl+C+C".into(),
            replace_selection: "Alt+Z".into(),
            silent_ocr: "Alt+Shift+C".into(),
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
            ShortcutAction::OcrText => &self.ocr_text,
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
            ShortcutAction::OcrText
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

fn supported(app: &AppHandle, action: ShortcutAction) -> bool {
    match action {
        ShortcutAction::Selection
        | ShortcutAction::DoubleCopy
        | ShortcutAction::ReplaceSelection
        | ShortcutAction::InputTranslate => true,
        ShortcutAction::OcrText
        | ShortcutAction::OcrPicture
        | ShortcutAction::OcrCard
        | ShortcutAction::SilentOcr => app.state::<OcrService>().supported(),
    }
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
            }
            if let Some(BindingKey::Chord(shortcut)) = previous
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
    if !supported(app, action) {
        return Err("当前平台不支持此快捷键功能。".into());
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
        if supported(app, action) {
            let _ = apply_locked(app, &mut state, action, config.get(action));
        }
    }
    drop(state);
    publish_status(app);
    Ok(config)
}

pub fn set_selection_hotkey(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    set_shortcut(app, ShortcutAction::Selection, accelerator)
}
pub fn set_double_copy_enabled(app: &AppHandle, enabled: bool) {
    let _ = set_shortcut(
        app,
        ShortcutAction::DoubleCopy,
        if enabled { "Ctrl+C+C" } else { "" },
    );
}

pub static INK_TRANSLATE_KEY: AtomicU32 = AtomicU32::new(0);
pub static CLIPBOARD_MONITOR_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_clipboard_monitor_enabled(enabled: bool) {
    CLIPBOARD_MONITOR_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_ink_translate_key(key: &str) {
    let vk = match key.trim().to_ascii_lowercase().as_str() {
        "~" | "`" => 0xC0,                // VK_OEM_3
        "capslock" | "caps_lock" => 0x14, // VK_CAPITAL
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        _ => 0,
    };
    INK_TRANSLATE_KEY.store(vk, Ordering::Relaxed);
}

#[cfg(target_os = "windows")]
pub fn start_clipboard_monitor(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("clipboard-monitor".into())
        .spawn(move || {
            use tauri_plugin_clipboard_manager::ClipboardExt;
            use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
            use windows::Win32::System::DataExchange::{
                AddClipboardFormatListener, RemoveClipboardFormatListener,
            };
            use windows::Win32::System::LibraryLoader::GetModuleHandleW;
            use windows::Win32::UI::WindowsAndMessaging::{
                CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
                GetForegroundWindow, GetMessageW, HWND_MESSAGE, MSG, PostQuitMessage,
                RegisterClassExW, TranslateMessage, WINDOW_EX_STYLE, WM_CLIPBOARDUPDATE,
                WM_DESTROY, WNDCLASSEXW, WS_OVERLAPPED,
            };
            use windows::core::PCWSTR;

            unsafe extern "system" fn wndproc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                unsafe {
                    if msg == WM_DESTROY {
                        PostQuitMessage(0);
                        return LRESULT(0);
                    }
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }

            let class_name = windows::core::w!("TransPortalClipboardListener");
            let hinstance = unsafe { GetModuleHandleW(None).unwrap_or_default() };

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };

            unsafe {
                let _ = RegisterClassExW(&wc);
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    PCWSTR(class_name.as_ptr()),
                    windows::core::w!(""),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    None,
                    hinstance,
                    None,
                );

                if let Ok(hwnd) = hwnd
                    && !hwnd.is_invalid()
                {
                    let _ = AddClipboardFormatListener(hwnd);

                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        if msg.message == WM_CLIPBOARDUPDATE
                            && CLIPBOARD_MONITOR_ENABLED.load(Ordering::Relaxed)
                        {
                            let fg = GetForegroundWindow().0 as usize;
                            let is_self = app
                                .get_webview_window("shell")
                                .and_then(|w| w.hwnd().ok())
                                .map(|h| h.0 as usize)
                                .is_some_and(|h| h == fg)
                                || app
                                    .get_webview_window("settings")
                                    .and_then(|w| w.hwnd().ok())
                                    .map(|h| h.0 as usize)
                                    .is_some_and(|h| h == fg);

                            if !is_self && let Ok(text) = app.clipboard().read_text() {
                                let trimmed = text.trim();
                                if !trimmed.is_empty() && trimmed.chars().count() <= 2000 {
                                    let _ = app.emit_to(
                                        "shell",
                                        "transportal://clipboard-text",
                                        trimmed,
                                    );
                                }
                            }
                        }
                        let _ = TranslateMessage(&msg);
                        let _ = DispatchMessageW(&msg);
                    }

                    let _ = RemoveClipboardFormatListener(hwnd);
                    let _ = DestroyWindow(hwnd);
                }
            }
        });
}

#[cfg(not(target_os = "windows"))]
pub fn start_clipboard_monitor(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new()
        .name("clipboard-monitor".into())
        .spawn(move || {
            let mut last_text = String::new();
            loop {
                std::thread::sleep(Duration::from_millis(500));
                if !CLIPBOARD_MONITOR_ENABLED.load(Ordering::Relaxed) {
                    continue;
                }
                use tauri_plugin_clipboard_manager::ClipboardExt;
                if let Ok(text) = app.clipboard().read_text() {
                    let trimmed = text.trim();
                    if !trimmed.is_empty()
                        && trimmed != last_text
                        && trimmed.chars().count() <= 2000
                    {
                        last_text = trimmed.to_string();
                        let _ = app.emit_to("shell", "transportal://clipboard-text", trimmed);
                    }
                }
            }
        });
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
            .filter(|action| supported(app, *action))
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
    if ocr.supported() && !ocr.observer_status().0 {
        ocr.install_observer(app);
    }
    let bindings = app.state::<ShortcutBindings>();
    let mut state = bindings.state.lock().unwrap();
    for action in ACTIONS {
        if supported(app, action) {
            let value = state.bindings[&action].status.requested.clone();
            let _ = apply_locked(app, &mut state, action, &value);
        }
    }
    drop(state);
    publish_status(app);
    status(app)
}

pub fn set_recording(app: &AppHandle, recording: bool) {
    app.state::<ShortcutBindings>()
        .state
        .lock()
        .unwrap()
        .recording = recording;
}
pub fn recording(app: &AppHandle) -> bool {
    app.state::<ShortcutBindings>()
        .state
        .lock()
        .unwrap()
        .recording
        && app
            .get_webview_window("settings")
            .is_some_and(|window| window.is_focused().unwrap_or(false))
}
fn record(app: &AppHandle, value: &str) -> bool {
    if !recording(app) {
        return false;
    }
    let _ = app.emit_to("settings", RECORD_EVENT, value);
    true
}
fn trigger(app: &AppHandle, action: ShortcutAction) {
    let value = app
        .state::<ShortcutBindings>()
        .state
        .lock()
        .unwrap()
        .bindings[&action]
        .status
        .active
        .clone();
    if record(app, value.as_deref().unwrap_or("")) {
        return;
    }
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
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
        ShortcutAction::OcrText
        | ShortcutAction::OcrPicture
        | ShortcutAction::OcrCard
        | ShortcutAction::SilentOcr => {
            let mode = match action {
                ShortcutAction::OcrText => OcrMode::Text,
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
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
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

pub async fn on_replace_selection(app: &AppHandle) {
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
    .unwrap_or_else(|_| crate::platform::CaptureOutcome {
        text: String::new(),
        method: crate::platform::CaptureMethod::Clipboard,
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
    let target_lang = crate::core::translation::resolve_target_lang(&outcome.text, Some("zh-CN"));
    let provider = crate::core::translation::create("google_free", None);
    let query = crate::core::translation::TranslateQuery {
        text: outcome.text,
        source_lang: None,
        target_lang: Some(target_lang.to_string()),
    };

    match provider.translate(&query).await {
        Ok(result) => {
            let app_clone = app.clone();
            let translated = result.translated_text;
            let replace_res = tauri::async_runtime::spawn_blocking(move || {
                app_clone
                    .state::<crate::core::selection::SelectionService>()
                    .replace(&app_clone, token, &translated)
            })
            .await;
            if let Err(e) = replace_res {
                let _ = app.emit_to("shell", "transportal://notice", format!("替换失败：{e}"));
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
    if record(app, "Win+Shift+S") {
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
            _ => OcrMode::Text,
        };
        let service = app.state::<OcrService>();
        if let Err(error) = service.start(app, mode, Some(revision)) {
            eprintln!("[ocr] ignored screenshot trigger: {error}");
        }
    }
}
pub fn on_hotkey(app: &AppHandle) {
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
    if record(app, "Ctrl+C+C") {
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
    anchor_bounds: Option<&crate::platform::SelectionBounds>,
    no_activate: bool,
) {
    if let Some(settings) = app.get_webview_window("settings") {
        let _ = settings.hide();
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
