use super::{OcrEngine, OcrEngineInfo};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OcrConfig {
    pub engine: String,
    pub language: Option<String>,
}

pub struct OcrRegistry {
    engines: Vec<Arc<dyn OcrEngine>>,
    config: Mutex<OcrConfig>,
}

impl OcrRegistry {
    pub fn new(engines: Vec<Arc<dyn OcrEngine>>, default_engine: &str) -> Self {
        Self {
            engines,
            config: Mutex::new(OcrConfig {
                engine: default_engine.into(),
                language: None,
            }),
        }
    }

    pub fn config(&self) -> OcrConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn engine(&self, id: &str) -> Result<Arc<dyn OcrEngine>, String> {
        self.engines
            .iter()
            .find(|engine| engine.id() == id)
            .cloned()
            .ok_or_else(|| format!("未注册 OCR 引擎：{id}"))
    }

    pub fn snapshot(&self) -> Result<(Arc<dyn OcrEngine>, OcrConfig), String> {
        let config = self
            .config
            .try_lock()
            .map_err(|_| "OCR 引擎正在切换，请稍候重试。")?
            .clone();
        Ok((self.engine(&config.engine)?, config))
    }

    pub fn infos(&self) -> Vec<OcrEngineInfo> {
        self.engines
            .iter()
            .map(|engine| {
                engine.info().unwrap_or_else(|error| OcrEngineInfo {
                    id: engine.id().into(),
                    label: engine.id().into(),
                    languages: vec![],
                    available: false,
                    error: Some(error),
                })
            })
            .collect()
    }

    /// The old engine/config stays usable until validation and model loading
    /// succeed. The command layer never switches on an OCR engine's name.
    pub fn configure(&self, config: OcrConfig) -> Result<OcrConfig, String> {
        let engine = self.engine(&config.engine)?;
        let info = engine.info()?;
        if !info.available {
            return Err(info.error.unwrap_or_else(|| "OCR 引擎不可用".into()));
        }
        if let Some(ref tag) = config.language
            && !info.languages.iter().any(|language| &language.tag == tag)
        {
            return Err(format!("{} 不支持识别语言 {tag}", info.label));
        }
        let mut current = self.config.lock().unwrap();
        engine.prepare()?;
        if current.engine != config.engine
            && let Ok(previous) = self.engine(&current.engine)
        {
            previous.release();
        }
        *current = config.clone();
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{OcrImage, RecognizedText};
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FakeEngine {
        id: &'static str,
        fails: bool,
        released: AtomicBool,
    }
    impl OcrEngine for FakeEngine {
        fn id(&self) -> &'static str {
            self.id
        }
        fn info(&self) -> Result<OcrEngineInfo, String> {
            Ok(OcrEngineInfo {
                id: self.id.into(),
                label: self.id.into(),
                languages: vec![],
                available: true,
                error: None,
            })
        }
        fn prepare(&self) -> Result<(), String> {
            if self.fails {
                Err("load failed".into())
            } else {
                Ok(())
            }
        }
        fn release(&self) {
            self.released.store(true, Ordering::Relaxed);
        }
        fn recognize(&self, _: OcrImage<'_>, _: Option<&str>) -> Result<RecognizedText, String> {
            unreachable!()
        }
    }

    #[test]
    fn failed_engine_load_preserves_config_and_successful_switch_releases_old_models() {
        let old = Arc::new(FakeEngine {
            id: "old",
            fails: false,
            released: AtomicBool::new(false),
        });
        let broken = Arc::new(FakeEngine {
            id: "broken",
            fails: true,
            released: AtomicBool::new(false),
        });
        let next = Arc::new(FakeEngine {
            id: "next",
            fails: false,
            released: AtomicBool::new(false),
        });
        let registry = OcrRegistry::new(vec![old.clone(), broken, next], "old");
        assert!(
            registry
                .configure(OcrConfig {
                    engine: "broken".into(),
                    language: None
                })
                .is_err()
        );
        assert_eq!(registry.config().engine, "old");
        assert!(!old.released.load(Ordering::Relaxed));
        assert!(
            registry
                .configure(OcrConfig {
                    engine: "missing".into(),
                    language: None
                })
                .is_err()
        );
        registry
            .configure(OcrConfig {
                engine: "next".into(),
                language: None,
            })
            .unwrap();
        assert_eq!(registry.config().engine, "next");
        assert!(old.released.load(Ordering::Relaxed));
    }
}
