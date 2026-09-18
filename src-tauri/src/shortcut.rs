//! Configurable global shortcuts and non-consuming Windows copy/snip gestures.

use crate::ocr::{OcrMode, OcrService};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

pub const STATUS_EVENT: &str = "transportal://shortcut-status";
pub const RECORD_EVENT: &str = "transportal://shortcut-recorded";
pub const SELECTION_EVENT: &str = "transportal://selection";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction { Selection, OcrText, OcrTranslate, DoubleCopy }

const ACTIONS: [ShortcutAction; 4] = [ShortcutAction::Selection, ShortcutAction::OcrText,
    ShortcutAction::OcrTranslate, ShortcutAction::DoubleCopy];

impl ShortcutAction {
    fn id(self) -> &'static str { match self {
        Self::Selection => "selection", Self::OcrText => "ocr_text",
        Self::OcrTranslate => "ocr_translate", Self::DoubleCopy => "double_copy",
    } }
    fn label(self) -> &'static str { match self {
        Self::Selection => "划词查询", Self::OcrText => "提取文字",
        Self::OcrTranslate => "截图翻译（OCR）", Self::DoubleCopy => "快捷翻译",
    } }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShortcutConfig {
    pub selection: String,
    pub ocr_text: String,
    pub ocr_translate: String,
    pub double_copy: String,
}
impl Default for ShortcutConfig {
    fn default() -> Self { Self { selection: "Alt+Q".into(), ocr_text: "Alt+Shift+S".into(),
        ocr_translate: "Alt+S".into(), double_copy: "Ctrl+C+C".into() } }
}
impl ShortcutConfig {
    fn get(&self, action: ShortcutAction) -> &str { match action {
        ShortcutAction::Selection => &self.selection, ShortcutAction::OcrText => &self.ocr_text,
        ShortcutAction::OcrTranslate => &self.ocr_translate, ShortcutAction::DoubleCopy => &self.double_copy,
    } }
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
        if result.is_ok() { self.active = (!self.requested.is_empty()).then(|| self.requested.clone()); }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ShortcutReport { pub revision: u64, pub ready: bool, pub shortcuts: Vec<ShortcutStatus> }

#[derive(Clone, Copy, PartialEq, Eq)]
enum BindingKey { Chord(Shortcut), SystemSnip, DoubleCopy }
struct Binding { key: Option<BindingKey>, status: ShortcutStatus }
struct BindingState {
    bindings: BTreeMap<ShortcutAction, Binding>,
    ready: bool,
    revision: u64,
    recording: bool,
}
impl Default for BindingState {
    fn default() -> Self {
        let defaults = ShortcutConfig::default();
        Self { ready: false, revision: 0, recording: false, bindings: ACTIONS.into_iter().map(|action| {
            (action, Binding { key: None, status: ShortcutStatus { id: action.id(), label: action.label(),
                requested: defaults.get(action).into(), active: None, error: None, system: false } })
        }).collect() }
    }
}
#[derive(Default)]
pub struct ShortcutBindings { state: Mutex<BindingState> }

fn parse_binding(action: ShortcutAction, accelerator: &str) -> Result<Option<BindingKey>, String> {
    let value = accelerator.trim();
    if value.is_empty() { return Ok(None); }
    if ["ctrl+c+c", "control+c+c"].contains(&value.replace(' ', "").to_ascii_lowercase().as_str()) {
        return if action == ShortcutAction::DoubleCopy { Ok(Some(BindingKey::DoubleCopy)) }
            else { Err("Ctrl+C+C 用于快捷翻译，请选择普通组合键。".into()) };
    }
    let shortcut: Shortcut = value.parse().map_err(|e| format!("无效的快捷键 '{value}'：{e}"))?;
    if shortcut == Shortcut::new(Some(Modifiers::CONTROL), Code::KeyC) {
        return Err("Ctrl+C 保留给系统复制；双按复制请填写 Ctrl+C+C。".into());
    }
    if shortcut == Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyS) {
        return if matches!(action, ShortcutAction::OcrText | ShortcutAction::OcrTranslate) {
            Ok(Some(BindingKey::SystemSnip))
        } else { Err("Win+Shift+S 是系统截图键，只能关联 OCR 功能。".into()) };
    }
    let key = shortcut.key.to_string();
    let function_key = key.strip_prefix('F').and_then(|number| number.parse::<u8>().ok())
        .is_some_and(|number| (1..=24).contains(&number));
    if shortcut.mods.is_empty() && !function_key {
        return Err("请使用含 Ctrl、Alt、Shift 或 Win 的组合键，或 F1–F24。".into());
    }
    Ok(Some(BindingKey::Chord(shortcut)))
}

fn supported(app: &AppHandle, action: ShortcutAction) -> bool {
    action == ShortcutAction::Selection || app.state::<OcrService>().supported()
}

fn validate_conflict(state: &BindingState, action: ShortcutAction, key: Option<BindingKey>) -> Result<(), String> {
    if let Some(key) = key {
        for (&other, binding) in &state.bindings {
            if other != action && (binding.key == Some(key)
                || parse_binding(other, &binding.status.requested).ok().flatten() == Some(key)) {
                return Err(format!("此键位已用于「{}」，请先修改或清除该项。", other.label()));
            }
        }
    }
    Ok(())
}

fn apply_locked(app: &AppHandle, state: &mut BindingState, action: ShortcutAction, value: &str) -> Result<(), String> {
    state.bindings.get_mut(&action).unwrap().status.requested = value.trim().into();
    let result = (|| {
        let key = parse_binding(action, value)?;
        validate_conflict(state, action, key)?;
        if matches!(key, Some(BindingKey::SystemSnip | BindingKey::DoubleCopy)) && !app.state::<OcrService>().observer_status().0 {
            return Err("键盘监听未就绪，请重新检测。".into());
        }
        let previous = state.bindings[&action].key;
        if key != previous {
            let manager = app.global_shortcut();
            if let Some(BindingKey::Chord(shortcut)) = key {
                manager.on_shortcut(shortcut, move |app, _, event| {
                    if event.state == ShortcutState::Pressed { trigger(app, action); }
                }).map_err(|e| format!("无法注册 {value}，可能已被其他程序占用：{e}"))?;
            }
            if let Some(BindingKey::Chord(shortcut)) = previous && let Err(error) = manager.unregister(shortcut) {
                if let Some(BindingKey::Chord(new)) = key { let _ = manager.unregister(new); }
                return Err(format!("无法替换原键位：{error}"));
            }
            state.bindings.get_mut(&action).unwrap().key = key;
        }
        state.bindings.get_mut(&action).unwrap().status.system =
            matches!(key, Some(BindingKey::SystemSnip | BindingKey::DoubleCopy));
        if action == ShortcutAction::DoubleCopy {
            app.state::<crate::selection::SelectionService>().set_double_copy(key == Some(BindingKey::DoubleCopy));
        }
        Ok(())
    })();
    state.bindings.get_mut(&action).unwrap().status.finish(&result);
    state.revision += 1;
    result
}

pub fn set_shortcut(app: &AppHandle, action: ShortcutAction, accelerator: &str) -> Result<(), String> {
    if !supported(app, action) { return Err("当前平台不支持此快捷键功能。".into()); }
    let bindings = app.state::<ShortcutBindings>();
    let result = apply_locked(app, &mut bindings.state.lock().unwrap(), action, accelerator);
    publish_status(app);
    result
}

/// Restore the whole mapping together, allowing saved bindings to swap keys.
pub fn restore(app: &AppHandle, config: ShortcutConfig) -> Result<ShortcutConfig, String> {
    let parsed = ACTIONS.into_iter().map(|action| Ok((action, parse_binding(action, config.get(action))?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    for (index, action) in ACTIONS.iter().enumerate() {
        if let Some(key) = parsed[action] && ACTIONS[index + 1..].iter().any(|other| parsed[other] == Some(key)) {
            return Err("保存的快捷键有重复，请重新设置。".into());
        }
    }
    let bindings = app.state::<ShortcutBindings>();
    let mut state = bindings.state.lock().unwrap();
    for &action in &ACTIONS {
        let binding = state.bindings.get_mut(&action).unwrap();
        if binding.key != parsed[&action] {
            if let Some(BindingKey::Chord(shortcut)) = binding.key {
                app.global_shortcut().unregister(shortcut).map_err(|e| e.to_string())?;
            }
            binding.key = None;
            binding.status.active = None;
        }
        binding.status.requested = config.get(action).trim().into();
    }
    for action in ACTIONS {
        if supported(app, action) { let _ = apply_locked(app, &mut state, action, config.get(action)); }
    }
    drop(state);
    publish_status(app);
    Ok(config)
}

pub fn set_selection_hotkey(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    set_shortcut(app, ShortcutAction::Selection, accelerator)
}
pub fn set_double_copy_enabled(app: &AppHandle, enabled: bool) {
    let _ = set_shortcut(app, ShortcutAction::DoubleCopy, if enabled { "Ctrl+C+C" } else { "" });
}

pub fn register(app: &AppHandle) {
    let _ = restore(app, ShortcutConfig::default());
}
pub fn status(app: &AppHandle) -> ShortcutReport {
    let bindings = app.state::<ShortcutBindings>();
    let state = bindings.state.lock().unwrap();
    ShortcutReport { revision: state.revision, ready: state.ready,
        shortcuts: ACTIONS.into_iter().filter(|action| supported(app, *action))
            .map(|action| state.bindings[&action].status.clone()).collect() }
}
fn publish_status(app: &AppHandle) { let _ = app.emit(STATUS_EVENT, status(app)); }
pub fn finish_startup_checks(app: &AppHandle) {
    let bindings = app.state::<ShortcutBindings>();
    let mut state = bindings.state.lock().unwrap();
    if state.ready { return; }
    state.ready = true;
    state.revision += 1;
    drop(state);
    let report = status(app);
    let failed = report.shortcuts.iter().any(|shortcut| shortcut.error.is_some());
    let _ = app.emit(STATUS_EVENT, report);
    if failed { show_shell(app); }
}
pub fn retry(app: &AppHandle) -> ShortcutReport {
    let ocr = app.state::<OcrService>();
    if ocr.supported() && !ocr.observer_status().0 { ocr.install_observer(app); }
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
    app.state::<ShortcutBindings>().state.lock().unwrap().recording = recording;
}
pub fn recording(app: &AppHandle) -> bool {
    app.state::<ShortcutBindings>().state.lock().unwrap().recording
        && app.get_webview_window("settings").is_some_and(|window| window.is_focused().unwrap_or(false))
}
fn record(app: &AppHandle, value: &str) -> bool {
    if !recording(app) { return false; }
    let _ = app.emit_to("settings", RECORD_EVENT, value);
    true
}
fn trigger(app: &AppHandle, action: ShortcutAction) {
    let value = app.state::<ShortcutBindings>().state.lock().unwrap().bindings[&action].status.active.clone();
    if record(app, value.as_deref().unwrap_or("")) { return; }
    if let Some(settings) = app.get_webview_window("settings") { let _ = settings.hide(); }
    match action {
        ShortcutAction::Selection | ShortcutAction::DoubleCopy => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || on_hotkey(&app));
        }
        ShortcutAction::OcrText | ShortcutAction::OcrTranslate => {
            let mode = if action == ShortcutAction::OcrText { OcrMode::Text } else { OcrMode::Translate };
            let service = app.state::<OcrService>();
            if let Err(error) = service.start(app, mode, None) { service.show_start_error(app, mode, error); }
        }
    }
}

/// Only reached when the user explicitly bound the OS snip chord to an OCR
/// action; the default bindings use the built-in region selector instead.
pub fn on_system_snip(app: &AppHandle, revision: u32) {
    if record(app, "Win+Shift+S") { return; }
    let action = app.state::<ShortcutBindings>().state.lock().unwrap().bindings.iter()
        .find_map(|(&action, binding)| (binding.key == Some(BindingKey::SystemSnip)).then_some(action));
    if let Some(action) = action {
        let mode = if action == ShortcutAction::OcrTranslate { OcrMode::Translate } else { OcrMode::Text };
        let service = app.state::<OcrService>();
        if let Err(error) = service.start(app, mode, Some(revision)) {
            eprintln!("[ocr] ignored screenshot trigger: {error}");
        }
    }
}
pub fn on_hotkey(app: &AppHandle) {
    app.state::<crate::ocr::monitor::MonitorService>().stop(app, true);
    app.state::<OcrService>().cancel(app);
    let outcome = app.state::<crate::selection::SelectionService>().capture(app);
    let _ = app.emit_to("shell", SELECTION_EVENT, &outcome);
    show_shell(app);
}
pub fn on_double_copy(app: &AppHandle, revision: u32, window: usize) {
    if record(app, "Ctrl+C+C") { return; }
    let enabled = app.state::<ShortcutBindings>().state.lock().unwrap().bindings[&ShortcutAction::DoubleCopy].key == Some(BindingKey::DoubleCopy);
    if !enabled { return; }
    if let Some(outcome) = app.state::<crate::selection::SelectionService>().double_copy(app, revision, window) {
        app.state::<crate::ocr::monitor::MonitorService>().stop(app, true);
        app.state::<OcrService>().cancel(app);
        let _ = app.emit_to("shell", SELECTION_EVENT, &outcome);
        show_shell(app);
    }
}

pub fn show_shell(app: &AppHandle) {
    if let Some(settings) = app.get_webview_window("settings") { let _ = settings.hide(); }
    if let Some(window) = app.get_webview_window("shell") {
        place_near_cursor(&window);
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Move `window` next to the cursor, clamped inside the monitor under the
/// cursor so the overlay never lands off-screen.
fn place_near_cursor(window: &tauri::WebviewWindow) {
    let Ok(cursor) = window.cursor_position() else {
        return;
    };

    let mut x = cursor.x as i32 + 12;
    let mut y = cursor.y as i32 + 16;

    let monitor = window
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .find(|monitor| {
            let origin = monitor.position();
            let size = monitor.size();
            cursor.x >= origin.x as f64
                && cursor.x < (origin.x + size.width as i32) as f64
                && cursor.y >= origin.y as f64
                && cursor.y < (origin.y + size.height as i32) as f64
        });

    if let Some(monitor) = monitor {
        let origin = monitor.position();
        let size = monitor.size();
        let window_size = window.outer_size().unwrap_or_default();

        let min_x = origin.x + 8;
        let min_y = origin.y + 8;
        let max_x = origin.x + size.width as i32 - window_size.width as i32 - 8;
        let max_y = origin.y + size.height as i32 - window_size.height as i32 - 8;

        x = x.clamp(min_x, max_x.max(min_x));
        y = y.clamp(min_y, max_y.max(min_y));
    }

    let _ = window.set_position(PhysicalPosition::new(x, y));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_cannot_take_over_ocr_or_system_snip_hotkeys() {
        assert!(parse_binding(ShortcutAction::Selection, "Alt+T").is_ok());
        let state = BindingState::default();
        assert!(validate_conflict(&state, ShortcutAction::Selection, parse_binding(ShortcutAction::Selection, "Alt+Shift+S").unwrap()).is_err());
        assert!(validate_conflict(&state, ShortcutAction::Selection, parse_binding(ShortcutAction::Selection, "Alt+S").unwrap()).is_err());
        assert!(parse_binding(ShortcutAction::Selection, "Super+Shift+S").is_err());
        assert!(parse_binding(ShortcutAction::Selection, "Shift+Super+KeyS").is_err());
        assert!(parse_binding(ShortcutAction::Selection, "Alt+Q").is_ok());
        assert!(parse_binding(ShortcutAction::Selection, "Ctrl+C").is_err());
        assert!(parse_binding(ShortcutAction::Selection, "Ctrl+Shift+D").is_ok());
    }

    #[test]
    fn a_failed_request_keeps_the_working_binding_and_retry_clears_the_error() {
        let mut binding = BindingState::default().bindings.remove(&ShortcutAction::Selection).unwrap().status;
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
