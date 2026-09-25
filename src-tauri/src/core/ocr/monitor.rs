//! Explicit, cancellable monitoring of one physical desktop region.

use super::picture::{
    Cancellation, OcrPicture, PictureTranslation, TranslationCache, translate_blocks,
};
use super::region::{RegionPurpose, RegionService};
use super::{OcrImage, OcrService};
use crate::core::translation::OpenAiConfig;
use crate::platform::desktop::{DesktopCapture, ScreenRect};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub const MONITOR_EVENT: &str = "transportal://auto-ocr";

#[derive(Clone, Deserialize)]
pub struct MonitorOptions {
    pub provider: String,
    pub openai_config: Option<OpenAiConfig>,
    pub target_lang: String,
    pub interval_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MonitorFrame {
    pub number: u64,
    pub picture: OcrPicture,
    pub translation: Option<PictureTranslation>,
    pub engine: String,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MonitorUpdate {
    pub id: u64,
    pub revision: u64,
    pub active: bool,
    pub selecting: bool,
    pub paused: bool,
    pub stage: String,
    pub interval_ms: u64,
    pub checks: u64,
    pub target_lang: String,
    pub region: Option<ScreenRect>,
    pub frame: Option<MonitorFrame>,
    pub error: Option<String>,
}

#[derive(Default)]
struct MonitorState {
    update: MonitorUpdate,
    options: Option<MonitorOptions>,
    cancel: Cancellation,
}

pub struct MonitorService {
    desktop: Arc<dyn DesktopCapture>,
    state: Mutex<MonitorState>,
}

impl Default for MonitorService {
    fn default() -> Self {
        Self {
            desktop: crate::platform::desktop(),
            state: Mutex::new(MonitorState::default()),
        }
    }
}

impl MonitorService {
    pub fn latest(&self) -> MonitorUpdate {
        self.state.lock().unwrap().update.clone()
    }

    fn restore_windows(&self, app: &AppHandle) {
        let _ = app.state::<RegionService>().exclude_windows(app, false);
    }

    fn update(
        &self,
        app: &AppHandle,
        id: u64,
        cancel: &Cancellation,
        change: impl FnOnce(&mut MonitorUpdate),
    ) {
        let mut state = self.state.lock().unwrap();
        if state.update.id != id || cancel.is_cancelled() {
            return;
        }
        change(&mut state.update);
        state.update.revision += 1;
        let _ = app.emit_to("shell", MONITOR_EVENT, &state.update);
    }

    pub async fn begin(&self, app: &AppHandle, options: MonitorOptions) -> Result<u64, String> {
        if !(500..=30_000).contains(&options.interval_ms)
            || options.target_lang.trim().is_empty()
            || options.target_lang.len() > 32
        {
            return Err("请选择有效的刷新间隔和目标语言。".into());
        }
        self.stop(app, true);
        app.state::<OcrService>().cancel(app);
        let (id, cancel) = {
            let mut state = self.state.lock().unwrap();
            let id = state.update.id + 1;
            let revision = state.update.revision + 1;
            state.cancel = Cancellation::default();
            state.options = Some(options.clone());
            state.update = MonitorUpdate {
                id,
                revision,
                active: true,
                selecting: true,
                stage: "selecting".into(),
                interval_ms: options.interval_ms,
                target_lang: options.target_lang.clone(),
                ..Default::default()
            };
            let _ = app.emit_to("shell", MONITOR_EVENT, &state.update);
            (id, state.cancel.clone())
        };
        let capture_app = app.clone();
        let token = cancel.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            capture_app.state::<RegionService>().freeze(
                &capture_app,
                RegionPurpose::Monitor,
                id,
                token,
            )
        })
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result);
        let selection = match result {
            Ok(selection) => selection,
            Err(error) => {
                if !cancel.is_cancelled() {
                    self.restore_windows(app);
                }
                self.update(app, id, &cancel, |state| {
                    state.active = false;
                    state.selecting = false;
                    state.stage = "error".into();
                    state.error = Some(error.clone());
                });
                if !cancel.is_cancelled() {
                    crate::core::shortcut::show_shell(app);
                }
                return Err(error);
            }
        };
        if cancel.is_cancelled() || self.state.lock().unwrap().update.id != id {
            app.state::<RegionService>()
                .dismiss(app, |_, pending| pending == selection);
            return Err("已取消区域选择。".into());
        }
        let window_app = app.clone();
        app.run_on_main_thread(move || {
            let service = window_app.state::<MonitorService>();
            let region = window_app.state::<RegionService>();
            if let Err(error) = region.present(&window_app, selection) {
                region.dismiss(&window_app, |_, pending| pending == selection);
                if !cancel.is_cancelled() {
                    service.restore_windows(&window_app);
                }
                service.update(&window_app, id, &cancel, |state| {
                    state.active = false;
                    state.selecting = false;
                    state.stage = "error".into();
                    state.error = Some(error);
                });
                crate::core::shortcut::show_shell(&window_app);
            }
        })
        .map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// The shared selector confirmed a rectangle for monitor `id`.
    pub fn confirm(&self, app: &AppHandle, id: u64, region: ScreenRect) -> Result<(), String> {
        region.validate()?;
        let (cancel, options) = {
            let mut state = self.state.lock().unwrap();
            if state.update.id != id || !state.update.selecting || state.cancel.is_cancelled() {
                return Err("区域选择已过期。".into());
            }
            let options = state.options.clone().ok_or("自动翻译配置已过期。")?;
            state.update.selecting = false;
            state.update.region = Some(region);
            state.update.stage = "watching".into();
            state.update.revision += 1;
            let _ = app.emit_to("shell", MONITOR_EVENT, &state.update);
            (state.cancel.clone(), options)
        };
        RegionService::hide_window(app);
        crate::core::shortcut::show_shell(app);
        Self::spawn(app, id, region, options, cancel);
        Ok(())
    }

    fn spawn(
        app: &AppHandle,
        id: u64,
        region: ScreenRect,
        options: MonitorOptions,
        cancel: Cancellation,
    ) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            app.state::<MonitorService>()
                .run(&app, id, region, options, cancel)
                .await;
        });
    }

    pub fn stop(&self, app: &AppHandle, discard: bool) {
        let mut state = self.state.lock().unwrap();
        let selecting = state.update.selecting;
        state.cancel.cancel();
        state.options = None;
        state.update.active = false;
        state.update.paused = false;
        state.update.selecting = false;
        state.update.stage = "stopped".into();
        state.update.error = None;
        if discard {
            state.update.frame = None;
        }
        state.update.revision += 1;
        let _ = app.emit_to("shell", MONITOR_EVENT, &state.update);
        drop(state);
        app.state::<RegionService>()
            .dismiss(app, |purpose, _| purpose == RegionPurpose::Monitor);
        self.restore_windows(app);
        if selecting {
            crate::core::shortcut::show_shell(app);
        }
    }

    pub fn pause(&self, app: &AppHandle, paused: bool) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if !state.update.active || state.update.selecting {
            return Err("没有正在运行的区域翻译。".into());
        }
        if state.update.paused == paused {
            return Ok(());
        }
        state.cancel.cancel();
        state.cancel = Cancellation::default();
        state.update.paused = paused;
        state.update.stage = if paused { "paused" } else { "watching" }.into();
        state.update.error = None;
        state.update.revision += 1;
        let _ = app.emit_to("shell", MONITOR_EVENT, &state.update);
        let restart = if paused {
            None
        } else {
            Some((
                state.update.id,
                state.update.region.ok_or("区域丢失。")?,
                state.options.clone().ok_or("配置丢失。")?,
                state.cancel.clone(),
            ))
        };
        drop(state);
        if let Some((id, region, options, cancel)) = restart {
            Self::spawn(app, id, region, options, cancel);
        }
        Ok(())
    }

    async fn run(
        &self,
        app: &AppHandle,
        id: u64,
        region: ScreenRect,
        options: MonitorOptions,
        cancel: Cancellation,
    ) {
        let provider = crate::core::translation::create(&options.provider, options.openai_config);
        let mut cache = TranslationCache::new();
        let mut previous = None;
        // Give the region selector's hide a chance to reach the compositor.
        tokio::time::sleep(Duration::from_millis(120)).await;
        while !cancel.is_cancelled() {
            let result = self
                .tick(
                    app,
                    id,
                    region,
                    &options.target_lang,
                    provider.clone(),
                    &cancel,
                    &mut cache,
                    &mut previous,
                )
                .await;
            let failed = result.is_err();
            if let Err(error) = result {
                self.update(app, id, &cancel, |state| {
                    state.stage = "error".into();
                    state.error = Some(error);
                });
            }
            let delay = if failed {
                options.interval_ms.max(5000)
            } else {
                options.interval_ms
            };
            tokio::select! { biased;
                _ = cancel.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(delay)) => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn tick(
        &self,
        app: &AppHandle,
        id: u64,
        region: ScreenRect,
        target: &str,
        provider: Arc<dyn crate::core::translation::Translator>,
        cancel: &Cancellation,
        cache: &mut TranslationCache,
        previous: &mut Option<[u8; 32]>,
    ) -> Result<(), String> {
        let desktop = self.desktop.clone();
        let image = tauri::async_runtime::spawn_blocking(move || desktop.capture(region))
            .await
            .map_err(|e| e.to_string())??;
        if cancel.is_cancelled() {
            return Ok(());
        }
        let fingerprint: [u8; 32] = Sha256::digest(image.as_raw()).into();
        {
            let mut state = self.state.lock().unwrap();
            if state.update.id != id || cancel.is_cancelled() {
                return Ok(());
            }
            state.update.checks += 1;
        }
        if previous.as_ref() == Some(&fingerprint) {
            return Ok(());
        }
        self.update(app, id, cancel, |state| {
            state.stage = "recognizing".into();
            state.error = None;
        });
        let recognize_app = app.clone();
        let token = cancel.clone();
        let (picture, engine, elapsed_ms) = tauri::async_runtime::spawn_blocking(move || {
            let input = OcrImage {
                width: image.width(),
                height: image.height(),
                rgba: image.as_raw(),
            };
            let (result, engine, elapsed) = recognize_app
                .state::<OcrService>()
                .recognize_frame(input, &token)?;
            Ok::<_, String>((OcrPicture::new(input, result.blocks)?, engine, elapsed))
        })
        .await
        .map_err(|e| e.to_string())??;
        if cancel.is_cancelled() {
            return Ok(());
        }
        self.update(app, id, cancel, |state| {
            state.stage = "translating".into();
            state.frame = Some(MonitorFrame {
                number: state.frame.as_ref().map_or(1, |frame| frame.number + 1),
                picture: picture.clone(),
                translation: None,
                engine,
                elapsed_ms,
            });
        });
        let translation =
            translate_blocks(&picture.blocks, provider, target, cache, cancel).await?;
        let success = translation.succeeded();
        let error = translation
            .blocks
            .iter()
            .find_map(|block| block.error.clone());
        self.update(app, id, cancel, |state| {
            state.stage = if success { "watching" } else { "error" }.into();
            state.error = error.clone();
            if let Some(frame) = &mut state.frame {
                frame.translation = Some(translation);
            }
        });
        if success {
            *previous = Some(fingerprint);
            Ok(())
        } else {
            Err(error.unwrap_or_else(|| "翻译失败，将自动重试。".into()))
        }
    }
}
