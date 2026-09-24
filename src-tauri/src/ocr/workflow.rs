use super::assets::{OnnxAssets, OnnxInstallStatus};
use super::onnx::OnnxOcrEngine;
use super::picture::{Cancellation, OcrPicture};
use super::region::{ConfirmedRegion, RegionService};
use super::registry::{OcrConfig, OcrRegistry};
use super::{
    ClipboardOrigin, OCR_EVENT, OcrEngine, OcrEngineInfo, OcrEvent, OcrImage, OcrMode, OcrStage,
    ScreenCapture, ScreenEvent, ScreenObserver,
};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Serialize)]
pub struct OcrStatus {
    pub supported: bool,
    pub engine: OcrEngineInfo,
    pub engines: Vec<OcrEngineInfo>,
    pub config: OcrConfig,
    pub onnx: OnnxInstallStatus,
    pub observing_shortcut: bool,
    pub shortcut_error: Option<String>,
    pub translate_shortcut_error: Option<String>,
}

#[derive(Default)]
struct SessionState {
    next_id: u64,
    active: Option<OcrEvent>,
    latest: Option<OcrEvent>,
}

impl SessionState {
    fn begin(&mut self, mode: OcrMode, engine: &str) -> Result<OcrEvent, String> {
        if self.active.is_some() {
            return Err("已有截图正在处理，请完成或按 Esc 取消后重试。".into());
        }
        self.next_id += 1;
        let event = OcrEvent {
            id: self.next_id,
            mode,
            stage: OcrStage::Capturing,
            text: String::new(),
            language: None,
            engine: engine.into(),
            image_path: None,
            picture: None,
            elapsed_ms: None,
            copied: false,
            copy_error: None,
            error: None,
        };
        self.active = Some(event.clone());
        self.latest = Some(event.clone());
        Ok(event)
    }

    fn is_current(&self, id: u64) -> bool {
        self.active.as_ref().is_some_and(|event| event.id == id)
    }

    fn update(&mut self, event: OcrEvent) -> bool {
        if !self.is_current(event.id) {
            return false;
        }
        self.active = match event.stage {
            OcrStage::Capturing | OcrStage::Recognizing => Some(event.clone()),
            _ => None,
        };
        self.latest = Some(event);
        true
    }
}

/// One explicit screenshot at a time. No image/history listener runs while idle.
pub struct OcrService {
    registry: OcrRegistry,
    pub assets: Arc<OnnxAssets>,
    capture: Arc<dyn ScreenCapture>,
    sessions: Mutex<SessionState>,
    /// Cancelled native inference cannot be forcibly interrupted; serialize it
    /// with the next request so rapid retries cannot accumulate model instances.
    inference: Mutex<()>,
    observer: Mutex<Option<Box<dyn ScreenObserver>>>,
    observer_error: Mutex<Option<String>>,
    translate_shortcut_error: Mutex<Option<String>>,
    translation: Mutex<Cancellation>,
    /// Cancels the native region selection of the active session.
    capture_cancel: Mutex<Cancellation>,
}

impl OcrService {
    pub fn new(assets: Arc<OnnxAssets>) -> Self {
        let native = crate::native::ocr::engine();
        let native_available = native.id() != "unavailable";
        let default_engine = if native_available {
            native.id()
        } else {
            super::onnx::ENGINE_ID
        };
        let mut engines: Vec<Arc<dyn OcrEngine>> =
            vec![Arc::new(OnnxOcrEngine::new(assets.clone()))];
        if native_available {
            engines.insert(0, native);
        }
        Self {
            registry: OcrRegistry::new(engines, default_engine),
            assets,
            capture: crate::native::ocr::screen_capture(),
            sessions: Mutex::new(SessionState::default()),
            inference: Mutex::new(()),
            observer: Mutex::new(None),
            observer_error: Mutex::new(None),
            translate_shortcut_error: Mutex::new(None),
            translation: Mutex::new(Cancellation::default()),
            capture_cancel: Mutex::new(Cancellation::default()),
        }
    }
}

impl OcrService {
    pub fn supported(&self) -> bool {
        self.capture.supported()
    }

    pub fn config(&self) -> OcrConfig {
        self.registry.config()
    }

    pub fn status(&self) -> OcrStatus {
        let config = self.registry.config();
        let engines = self.registry.infos();
        let engine = engines
            .iter()
            .find(|engine| engine.id == config.engine)
            .cloned()
            .expect("selected OCR engine remains registered");
        OcrStatus {
            supported: self.supported(),
            engine,
            engines,
            config,
            onnx: self.assets.status(),
            observing_shortcut: self.observer.lock().unwrap().is_some(),
            shortcut_error: self.observer_error.lock().unwrap().clone(),
            translate_shortcut_error: self.translate_shortcut_error.lock().unwrap().clone(),
        }
    }

    pub fn set_translate_shortcut_error(&self, error: Option<String>) {
        *self.translate_shortcut_error.lock().unwrap() = error;
    }

    pub fn observer_status(&self) -> (bool, Option<String>) {
        (
            self.observer.lock().unwrap().is_some(),
            self.observer_error.lock().unwrap().clone(),
        )
    }

    pub fn set_language(&self, language: Option<String>) -> Result<(), String> {
        let mut config = self.registry.config();
        config.language = language;
        self.configure(config).map(|_| ())
    }

    pub fn configure(&self, config: OcrConfig) -> Result<OcrConfig, String> {
        if self.sessions.lock().unwrap().active.is_some() {
            return Err("请先完成或取消当前 OCR，再切换引擎或识别语言。".into());
        }
        let _inference = self.inference.lock().unwrap();
        self.registry.configure(config)
    }

    pub fn latest(&self) -> Option<OcrEvent> {
        self.sessions.lock().unwrap().latest.clone()
    }

    pub fn picture_translation(
        &self,
        id: u64,
        texts: Option<Vec<String>>,
    ) -> Result<(OcrPicture, Cancellation), String> {
        let sessions = self.sessions.lock().unwrap();
        let event = sessions
            .latest
            .as_ref()
            .filter(|e| e.id == id && e.stage == OcrStage::Ready)
            .ok_or("图片已更换，请重新识别后翻译。")?;
        let mut picture = event.picture.clone().ok_or("识别结果没有图片坐标。")?;
        if let Some(texts) = texts {
            if texts.len() != picture.blocks.len()
                || texts.iter().any(|t| t.chars().count() > 10000)
            {
                return Err("文字块数量不匹配或文字过长。".into());
            }
            for (block, text) in picture.blocks.iter_mut().zip(texts) {
                block.text = text;
            }
        }
        let mut previous = self.translation.lock().unwrap();
        previous.cancel();
        *previous = Cancellation::default();
        Ok((picture, previous.clone()))
    }

    /// The monitor shares the same inference lock and engine registry as snips.
    pub fn recognize_frame(
        &self,
        image: OcrImage<'_>,
        cancel: &Cancellation,
    ) -> Result<(super::RecognizedText, String, u64), String> {
        let _inference = self.inference.lock().unwrap();
        if cancel.is_cancelled() {
            return Err("已停止自动翻译".into());
        }
        let (engine, config) = self.registry.snapshot()?;
        let start = Instant::now();
        let result =
            recognize_with_chinese_retry(engine.as_ref(), image, config.language.as_deref())?;
        if cancel.is_cancelled() {
            return Err("已停止自动翻译".into());
        }
        Ok((
            result,
            engine.id().into(),
            start.elapsed().as_millis() as u64,
        ))
    }

    pub fn benchmark(&self) -> Result<u64, String> {
        let _inference = self.inference.lock().unwrap();
        let (engine, config) = self.registry.snapshot()?;
        let width = 320u32;
        let height = 80u32;
        let mut rgba = vec![255u8; width as usize * height as usize * 4];
        for y in 20..60 {
            for x in 20..300 {
                if (x % 24 < 12) && (y % 8 < 5) {
                    let idx = (y * width + x) as usize * 4;
                    rgba[idx] = 0;
                    rgba[idx + 1] = 0;
                    rgba[idx + 2] = 0;
                }
            }
        }
        let image = OcrImage {
            width,
            height,
            rgba: &rgba,
        };
        let start = Instant::now();
        let _ = engine.recognize(image, config.language.as_deref())?;
        Ok(start.elapsed().as_millis() as u64)
    }

    pub fn show_start_error(&self, app: &AppHandle, mode: OcrMode, error: String) {
        let Ok((engine, _)) = self.registry.snapshot() else {
            return;
        };
        let event = self.sessions.lock().unwrap().begin(mode, engine.id());
        // A busy shortcut press should leave the existing session intact.
        if let Ok(event) = event {
            self.finish_error(app, event, error);
        }
    }

    pub fn install_observer(&self, app: &AppHandle) {
        if !self.supported() {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(16);
        match self.capture.observe(sender) {
            Ok(observer) => {
                *self.observer.lock().unwrap() = Some(observer);
                *self.observer_error.lock().unwrap() = None;
                let app = app.clone();
                std::thread::spawn(move || {
                    while let Ok(event) = receiver.recv() {
                        let service = app.state::<OcrService>();
                        match event {
                            ScreenEvent::DoubleCopy {
                                clipboard_revision,
                                window,
                            } => {
                                let app = app.clone();
                                tauri::async_runtime::spawn_blocking(move || {
                                    crate::shortcut::on_double_copy(
                                        &app,
                                        clipboard_revision,
                                        window,
                                    )
                                });
                            }
                            ScreenEvent::InkTranslate { window: _ } => {
                                let app = app.clone();
                                tauri::async_runtime::spawn_blocking(move || {
                                    crate::shortcut::on_hotkey(&app)
                                });
                            }
                            ScreenEvent::Snip { clipboard_revision } => {
                                crate::shortcut::on_system_snip(&app, clipboard_revision);
                            }
                            ScreenEvent::Cancel => {
                                service.cancel(&app);
                                let monitor = app.state::<super::monitor::MonitorService>();
                                if monitor.latest().active {
                                    monitor.stop(&app, false);
                                }
                            }
                        }
                    }
                });
            }
            Err(error) => {
                eprintln!("[ocr] {error}");
                *self.observer_error.lock().unwrap() = Some(error);
            }
        }
    }

    pub fn shutdown(&self, app: &AppHandle) {
        self.cancel(app);
        self.observer.lock().unwrap().take();
    }

    /// Without a clipboard revision the built-in region selector freezes the
    /// desktop and crops the user's rectangle locally. A supplied revision means
    /// the user pressed the OS-owned Win+Shift+S chord that they explicitly bound
    /// to OCR, so the system screenshot tool is already opening and its clipboard
    /// image is awaited instead.
    pub fn start(
        &self,
        app: &AppHandle,
        mode: OcrMode,
        revision: Option<u32>,
    ) -> Result<u64, String> {
        app.state::<super::monitor::MonitorService>()
            .stop(app, true);
        if !self.supported() {
            return Err("当前平台尚未提供截图 OCR；请在 Windows 10/11 使用此功能。".into());
        }
        if revision.is_some() && self.observer.lock().unwrap().is_none() {
            return Err("截图快捷键监听不可用，请重启 TransPortal 后重试。".into());
        }
        let (engine, config) = self.registry.snapshot()?;
        let event = self.sessions.lock().unwrap().begin(mode, engine.id())?;
        self.translation.lock().unwrap().cancel();
        let cancel = Cancellation::default();
        *self.capture_cancel.lock().unwrap() = cancel.clone();
        let id = event.id;
        let _ = app.emit_to("shell", OCR_EVENT, &event);
        let app = app.clone();
        match revision {
            Some(baseline) => {
                eprintln!("[ocr] session={id} mode={mode:?} waiting for a new system screenshot");
                let language = config.language;
                tauri::async_runtime::spawn_blocking(move || {
                    let service = app.state::<OcrService>();
                    if let Err(error) =
                        service.run_capture(&app, event.clone(), baseline, engine, language)
                    {
                        service.finish_error(&app, event, error);
                    }
                });
            }
            None => {
                eprintln!("[ocr] session={id} mode={mode:?} selecting a screen region");
                tauri::async_runtime::spawn_blocking(move || {
                    let service = app.state::<OcrService>();
                    if let Err(error) = service.select_region(&app, &event, cancel) {
                        let _ = app.state::<RegionService>().exclude_windows(&app, false);
                        service.finish_error(&app, event, error);
                    }
                });
            }
        }
        Ok(id)
    }

    /// Freeze the desktop and open the selector over it. The session stays in
    /// `Capturing` until the selector confirms or cancels.
    fn select_region(
        &self,
        app: &AppHandle,
        event: &OcrEvent,
        cancel: Cancellation,
    ) -> Result<(), String> {
        if let Some(window) = app.get_webview_window("shell") {
            let _ = window.hide();
        }
        let region = app.state::<RegionService>();
        let selection = region.freeze(app, event.mode.into(), event.id, cancel)?;
        if !self.is_current(event.id) {
            region.dismiss(app, |_, id| id == selection);
            let _ = region.exclude_windows(app, false);
            return Ok(());
        }
        let owner = event.id;
        let window_app = app.clone();
        app.run_on_main_thread(move || {
            let region = window_app.state::<RegionService>();
            if let Err(error) = region.present(&window_app, selection) {
                region.dismiss(&window_app, |_, id| id == selection);
                let _ = region.exclude_windows(&window_app, false);
                let service = window_app.state::<OcrService>();
                let active = service.sessions.lock().unwrap().active.clone();
                if let Some(event) = active.filter(|event| event.id == owner) {
                    service.finish_error(&window_app, event, error);
                }
            }
        })
        .map_err(|e| e.to_string())
    }

    /// The selector confirmed a rectangle for session `confirmed.owner`.
    pub fn finish_capture(&self, app: &AppHandle, confirmed: ConfirmedRegion) {
        debug_assert!(confirmed.purpose.is_snip());
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let service = app.state::<OcrService>();
            let _ = app.state::<RegionService>().exclude_windows(&app, false);
            let active = service.sessions.lock().unwrap().active.clone();
            let Some(mut event) = active.filter(|event| event.id == confirmed.owner) else {
                return;
            };
            event.stage = OcrStage::Recognizing;
            service.publish(&app, event.clone(), true);
            let result = (|| {
                let (engine, config) = service.registry.snapshot()?;
                let image = confirmed.crop()?;
                // Sampled after the selection so a copy made while selecting
                // still counts as newer than the screenshot.
                let revision = service.capture.clipboard_revision();
                service.recognize_image(
                    &app,
                    event.clone(),
                    OcrImage {
                        width: image.width(),
                        height: image.height(),
                        rgba: image.as_raw(),
                    },
                    revision,
                    engine,
                    config.language,
                )
            })();
            if let Err(error) = result {
                service.finish_error(&app, event, error);
            }
        });
    }

    /// Local image input also makes the ONNX engine usable on platforms whose
    /// screen-selection adapter has not been implemented yet.
    pub fn start_image(
        &self,
        app: &AppHandle,
        mode: OcrMode,
        path: PathBuf,
    ) -> Result<u64, String> {
        app.state::<super::monitor::MonitorService>()
            .stop(app, true);
        if self.sessions.lock().unwrap().active.is_some() {
            self.cancel(app);
        }
        let (engine, config) = self.registry.snapshot()?;
        let mut event = self.sessions.lock().unwrap().begin(mode, engine.id())?;
        self.translation.lock().unwrap().cancel();
        event.image_path = Some(path.to_string_lossy().into_owned());
        self.sessions.lock().unwrap().update(event.clone());
        let id = event.id;
        let revision = self.capture.clipboard_revision();
        let _ = app.emit_to("shell", OCR_EVENT, &event);
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let service = app.state::<OcrService>();
            let result = (|| {
                let mut progress = event.clone();
                progress.stage = OcrStage::Recognizing;
                service.publish(&app, progress, true);
                let mut reader = image::ImageReader::open(&path)
                    .map_err(|e| format!("无法读取图片：{e}"))?
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(16000);
                limits.max_image_height = Some(16000);
                limits.max_alloc = Some(128_000_000);
                reader.limits(limits);
                let image = reader
                    .decode()
                    .map_err(|e| format!("无法解码图片或图片过大：{e}"))?
                    .into_rgba8();
                service.recognize_image(
                    &app,
                    event.clone(),
                    OcrImage {
                        width: image.width(),
                        height: image.height(),
                        rgba: image.as_raw(),
                    },
                    revision,
                    engine,
                    config.language,
                )
            })();
            if let Err(error) = result {
                service.finish_error(&app, event, error);
            }
        });
        Ok(id)
    }

    pub fn cancel(&self, app: &AppHandle) {
        self.translation.lock().unwrap().cancel();
        self.capture_cancel.lock().unwrap().cancel();
        let region = app.state::<RegionService>();
        if region
            .dismiss(app, |purpose, _| purpose.is_snip())
            .is_some()
        {
            let _ = region.exclude_windows(app, false);
        }
        let (event, was_active) = {
            let mut sessions = self.sessions.lock().unwrap();
            let was_active = sessions.active.is_some();
            let Some(mut event) = sessions.active.clone().or_else(|| {
                sessions
                    .latest
                    .clone()
                    .filter(|event| event.stage == OcrStage::Ready)
            }) else {
                return;
            };
            event.stage = OcrStage::Cancelled;
            event.picture = None;
            event.text.clear();
            sessions.active = None;
            sessions.latest = Some(event.clone());
            (event, was_active)
        };
        let _ = app.emit_to("shell", OCR_EVENT, &event);
        if was_active {
            Self::update_window(app, &event);
        }
    }

    fn is_current(&self, id: u64) -> bool {
        self.sessions.lock().unwrap().is_current(id)
    }

    fn publish(&self, app: &AppHandle, event: OcrEvent, show: bool) {
        if !self.sessions.lock().unwrap().update(event.clone()) {
            return;
        }
        let _ = app.emit_to("shell", OCR_EVENT, &event);
        if show && event.mode != OcrMode::Silent {
            Self::update_window(app, &event);
        }
    }

    /// Validate again on the UI thread: a queued show from an older result must
    /// not reopen the overlay over a new snip or after the user cancels it.
    fn update_window(app: &AppHandle, event: &OcrEvent) {
        let (id, stage) = (event.id, event.stage);
        let window_app = app.clone();
        let _ = app.run_on_main_thread(move || {
            let service = window_app.state::<OcrService>();
            let sessions = service.sessions.lock().unwrap();
            if !sessions
                .latest
                .as_ref()
                .is_some_and(|latest| latest.id == id && latest.stage == stage)
            {
                return;
            }
            if stage == OcrStage::Cancelled {
                if let Some(window) = window_app.get_webview_window("shell") {
                    let _ = window.hide();
                }
            } else {
                crate::shortcut::show_shell(&window_app);
            }
        });
    }

    fn finish_error(&self, app: &AppHandle, mut event: OcrEvent, error: String) {
        eprintln!("[ocr] session={} failed: {error}", event.id);
        event.stage = OcrStage::Error;
        event.error = Some(error);
        self.publish(app, event, true);
    }

    /// Legacy path: wait for the image the system screenshot tool publishes.
    fn run_capture(
        &self,
        app: &AppHandle,
        event: OcrEvent,
        baseline: u32,
        engine: Arc<dyn OcrEngine>,
        language: Option<String>,
    ) -> Result<(), String> {
        if !self.is_current(event.id) {
            return Ok(());
        }
        if let Some(window) = app.get_webview_window("shell") {
            let _ = window.hide();
        }
        if baseline == 0 {
            return Err("无法读取剪贴板版本，已停止截图识别以避免读取旧图片。".into());
        }

        let wait_start = Instant::now();
        let mut image_pending_since = None;
        let (image, image_revision) = loop {
            if !self.is_current(event.id) {
                return Ok(());
            }
            if wait_start.elapsed() >= CAPTURE_TIMEOUT {
                return Err(
                    "截图等待已超时。请重新截图，并确认 Windows 截图工具已开启自动复制到剪贴板。"
                        .into(),
                );
            }
            let revision = self.capture.clipboard_revision();
            if revision != 0 && revision != baseline {
                match self.capture.clipboard_origin() {
                    ClipboardOrigin::Other => {
                        // A later unrelated copy ends this request instead of
                        // silently feeding its contents into OCR/translation.
                        eprintln!(
                            "[ocr] session={} cancelled after an unrelated clipboard update",
                            event.id
                        );
                        self.cancel(app);
                        return Ok(());
                    }
                    ClipboardOrigin::SnippingTool => {
                        if let Ok(image) = app.clipboard().read_image()
                            && self.capture.clipboard_revision() == revision
                        {
                            break (image, revision);
                        }
                        let pending = image_pending_since.get_or_insert_with(Instant::now);
                        if pending.elapsed() > Duration::from_secs(3) {
                            return Err("截图工具未提供可读取的图片。请开启自动复制到剪贴板，再重新框选文字。".into());
                        }
                    }
                    ClipboardOrigin::Unavailable => {}
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        };

        self.recognize_image(
            app,
            event,
            OcrImage {
                width: image.width(),
                height: image.height(),
                rgba: image.rgba(),
            },
            image_revision,
            engine,
            language,
        )
    }
}

fn recognize_with_chinese_retry(
    engine: &dyn OcrEngine,
    image: OcrImage<'_>,
    language: Option<&str>,
) -> Result<super::RecognizedText, String> {
    let mut result = engine.recognize(image, language)?;
    if result.text.trim().is_empty()
        && let Ok(info) = engine.info()
    {
        let chinese_tag = info
            .languages
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case("zh-Hans-CN"))
            .or_else(|| {
                info.languages
                    .iter()
                    .find(|l| l.tag.to_ascii_lowercase().starts_with("zh-hans"))
            })
            .or_else(|| {
                info.languages
                    .iter()
                    .find(|l| l.tag.to_ascii_lowercase().starts_with("zh"))
            })
            .map(|l| l.tag.as_str());
        if let Some(target_lang) = chinese_tag {
            let already_target = language.is_some_and(|l| l.eq_ignore_ascii_case(target_lang))
                || result.language.eq_ignore_ascii_case(target_lang);
            if !already_target
                && let Ok(retry_result) = engine.recognize(image, Some(target_lang))
                && !retry_result.text.trim().is_empty()
            {
                result = retry_result;
            }
        }
    }
    Ok(result)
}

impl OcrService {
    fn recognize_image(
        &self,
        app: &AppHandle,
        mut event: OcrEvent,
        image: OcrImage<'_>,
        image_revision: u32,
        engine: Arc<dyn OcrEngine>,
        language: Option<String>,
    ) -> Result<(), String> {
        // Inference is serialized even if an earlier session was cancelled.
        let _inference = self.inference.lock().unwrap();
        if !self.is_current(event.id) {
            return Ok(());
        }
        event.stage = OcrStage::Recognizing;
        self.publish(app, event.clone(), true);
        let start = Instant::now();
        let result = recognize_with_chinese_retry(engine.as_ref(), image, language.as_deref())?;
        event.elapsed_ms = Some(start.elapsed().as_millis() as u64);
        if result.text.trim().is_empty() {
            return Err("未识别到文字。请缩小选区、放大文字，或在设置中切换 OCR 识别语言。".into());
        }
        event.stage = OcrStage::Ready;
        event.text = result.text;
        event.language = Some(result.language);
        event.picture = Some(OcrPicture::new(image, result.blocks)?);

        {
            let mut sessions = self.sessions.lock().unwrap();
            if !sessions.is_current(event.id) {
                return Ok(());
            }
            if event.mode == OcrMode::Text || event.mode == OcrMode::Silent {
                if event.mode == OcrMode::Silent {
                    let _ = app.clipboard().write_text(event.text.clone());
                    event.copied = true;
                } else if image_revision != 0 && self.capture.clipboard_revision() == image_revision
                {
                    match app.clipboard().write_text(event.text.clone()) {
                        Ok(()) => event.copied = true,
                        Err(error) => {
                            event.copy_error =
                                Some(format!("自动复制失败，可点击复制重试：{error}"))
                        }
                    }
                } else {
                    event.copy_error = Some(if image_revision == 0 {
                        "无法确认剪贴板是否变化，请点击复制文字。".into()
                    } else {
                        "剪贴板已有新内容，识别文字未自动复制。".into()
                    });
                }
            }
            sessions.update(event.clone());
        }
        eprintln!(
            "[ocr] session={} engine={} language={} chars={} recognition_ms={}",
            event.id,
            event.engine,
            event.language.as_deref().unwrap_or(""),
            event.text.chars().count(),
            event.elapsed_ms.unwrap_or(0)
        );
        let _ = app.emit_to("shell", OCR_EVENT, &event);
        if event.mode != OcrMode::Silent {
            Self::update_window(app, &event);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_trigger_cannot_change_an_active_translation_to_text_only() {
        let mut state = SessionState::default();
        let first = state.begin(OcrMode::Picture, "windows").unwrap();
        assert!(state.begin(OcrMode::Text, "windows").is_err());
        assert_eq!(state.active.unwrap().mode, first.mode);
    }

    #[test]
    fn cancel_invalidates_in_flight_results_even_after_a_new_request() {
        let mut state = SessionState::default();
        let mut first = state.begin(OcrMode::Text, "windows").unwrap();
        first.stage = OcrStage::Cancelled;
        assert!(state.update(first.clone()));
        let second = state.begin(OcrMode::Picture, "windows").unwrap();
        first.stage = OcrStage::Ready;
        first.text = "stale text".into();
        assert!(!state.update(first));
        assert!(state.is_current(second.id));
        assert_eq!(state.latest.unwrap().stage, OcrStage::Capturing);
    }

    #[test]
    fn ready_and_error_release_the_session_and_cannot_be_replayed() {
        let mut state = SessionState::default();
        for stage in [OcrStage::Ready, OcrStage::Error] {
            let mut event = state.begin(OcrMode::Text, "windows").unwrap();
            event.stage = stage;
            assert!(state.update(event.clone()));
            assert!(!state.update(event));
            assert!(state.active.is_none());
        }
    }

    struct MockBilingualRetryEngine;
    impl OcrEngine for MockBilingualRetryEngine {
        fn id(&self) -> &'static str {
            "mock"
        }
        fn info(&self) -> Result<OcrEngineInfo, String> {
            Ok(OcrEngineInfo {
                id: "mock".into(),
                label: "Mock Engine".into(),
                languages: vec![
                    crate::ocr::OcrLanguage {
                        tag: "en-US".into(),
                        label: "English".into(),
                    },
                    crate::ocr::OcrLanguage {
                        tag: "zh-Hans-CN".into(),
                        label: "Chinese".into(),
                    },
                ],
                available: true,
                error: None,
            })
        }
        fn recognize(
            &self,
            _: OcrImage<'_>,
            lang: Option<&str>,
        ) -> Result<crate::ocr::RecognizedText, String> {
            if lang == Some("zh-Hans-CN") {
                Ok(crate::ocr::RecognizedText {
                    text: "测试文字识别".into(),
                    language: "zh-Hans-CN".into(),
                    blocks: vec![],
                })
            } else {
                Ok(crate::ocr::RecognizedText {
                    text: "".into(),
                    language: "en-US".into(),
                    blocks: vec![],
                })
            }
        }
    }

    #[test]
    fn empty_recognition_retries_with_chinese_when_available() {
        let engine = MockBilingualRetryEngine;
        let dummy_pixels = vec![255u8; 16];
        let image = OcrImage {
            width: 2,
            height: 2,
            rgba: &dummy_pixels,
        };
        let result = recognize_with_chinese_retry(&engine, image, None).unwrap();
        assert_eq!(result.text, "测试文字识别");
        assert_eq!(result.language, "zh-Hans-CN");
    }

    #[test]
    fn recognition_does_not_retry_if_already_chinese() {
        struct MockAlwaysEmptyChineseEngine;
        impl OcrEngine for MockAlwaysEmptyChineseEngine {
            fn id(&self) -> &'static str {
                "mock_zh"
            }
            fn info(&self) -> Result<OcrEngineInfo, String> {
                Ok(OcrEngineInfo {
                    id: "mock_zh".into(),
                    label: "Mock Chinese".into(),
                    languages: vec![crate::ocr::OcrLanguage {
                        tag: "zh-Hans-CN".into(),
                        label: "Chinese".into(),
                    }],
                    available: true,
                    error: None,
                })
            }
            fn recognize(
                &self,
                _: OcrImage<'_>,
                _: Option<&str>,
            ) -> Result<crate::ocr::RecognizedText, String> {
                Ok(crate::ocr::RecognizedText {
                    text: "".into(),
                    language: "zh-Hans-CN".into(),
                    blocks: vec![],
                })
            }
        }
        let engine = MockAlwaysEmptyChineseEngine;
        let dummy_pixels = vec![255u8; 16];
        let image = OcrImage {
            width: 2,
            height: 2,
            rgba: &dummy_pixels,
        };
        let result = recognize_with_chinese_retry(&engine, image, Some("zh-Hans-CN")).unwrap();
        assert!(result.text.is_empty());
    }
}
