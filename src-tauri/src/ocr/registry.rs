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
    pub fn configure(&self, mut config: OcrConfig) -> Result<OcrConfig, String> {
        let engine = self.engine(&config.engine)?;
        let info = engine.info()?;
        if !info.available {
            return Err(info.error.unwrap_or_else(|| "OCR 引擎不可用".into()));
        }
        if let Some(ref tag) = config.language {
            if let Some(matched) = match_language_tag(&info.languages, tag) {
                config.language = Some(matched.tag.clone());
            } else {
                return Err(format!("{} 不支持识别语言 {tag}", info.label));
            }
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

pub fn match_language_tag<'a>(
    languages: &'a [super::OcrLanguage],
    requested: &str,
) -> Option<&'a super::OcrLanguage> {
    let clean = requested.trim().replace('_', "-");
    let lower = clean.to_ascii_lowercase();

    // 1. Exact match (case-insensitive)
    if let Some(lang) = languages
        .iter()
        .find(|l| l.tag.eq_ignore_ascii_case(&clean))
    {
        return Some(lang);
    }

    // 2. Chinese alias matching
    if lower == "zh" || lower == "zh-cn" || lower == "zh-hans" || lower == "zh-hans-cn" {
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case("zh-Hans-CN"))
        {
            return Some(lang);
        }
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.to_ascii_lowercase().starts_with("zh-hans"))
        {
            return Some(lang);
        }
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.to_ascii_lowercase().starts_with("zh"))
        {
            return Some(lang);
        }
    } else if lower == "zh-tw" || lower == "zh-hant" || lower == "zh-hant-tw" {
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case("zh-Hant-TW"))
        {
            return Some(lang);
        }
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.to_ascii_lowercase().starts_with("zh-hant"))
        {
            return Some(lang);
        }
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.to_ascii_lowercase().starts_with("zh"))
        {
            return Some(lang);
        }
    } else if lower == "zh-hk" || lower == "zh-hant-hk" {
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.eq_ignore_ascii_case("zh-Hant-HK"))
        {
            return Some(lang);
        }
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.to_ascii_lowercase().starts_with("zh-hant"))
        {
            return Some(lang);
        }
        if let Some(lang) = languages
            .iter()
            .find(|l| l.tag.to_ascii_lowercase().starts_with("zh"))
        {
            return Some(lang);
        }
    }

    // 3. Prefix / subtag match: e.g. "en" matches "en-US", "en-us" matches "en-US", etc.
    languages.iter().find(|l| {
        let tag_lower = l.tag.to_ascii_lowercase();
        tag_lower.starts_with(&format!("{lower}-"))
            || lower.starts_with(&format!("{tag_lower}-"))
            || tag_lower.contains(&lower)
    })
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

    #[test]
    fn language_tag_matching_handles_aliases_case_and_prefixes() {
        let languages = vec![
            crate::ocr::OcrLanguage {
                tag: "en-US".into(),
                label: "English".into(),
            },
            crate::ocr::OcrLanguage {
                tag: "zh-Hans-CN".into(),
                label: "Chinese (Simplified)".into(),
            },
        ];

        // Chinese aliases
        assert_eq!(
            match_language_tag(&languages, "zh-CN").unwrap().tag,
            "zh-Hans-CN"
        );
        assert_eq!(
            match_language_tag(&languages, "zh").unwrap().tag,
            "zh-Hans-CN"
        );
        assert_eq!(
            match_language_tag(&languages, "zh_cn").unwrap().tag,
            "zh-Hans-CN"
        );
        assert_eq!(
            match_language_tag(&languages, "zh-hans").unwrap().tag,
            "zh-Hans-CN"
        );
        assert_eq!(
            match_language_tag(&languages, "zh-hans-cn").unwrap().tag,
            "zh-Hans-CN"
        );

        // English prefix and case
        assert_eq!(match_language_tag(&languages, "en").unwrap().tag, "en-US");
        assert_eq!(
            match_language_tag(&languages, "en-us").unwrap().tag,
            "en-US"
        );
        assert_eq!(
            match_language_tag(&languages, "EN-US").unwrap().tag,
            "en-US"
        );

        // Unsupported
        assert!(match_language_tag(&languages, "ja").is_none());
        assert!(match_language_tag(&languages, "fr").is_none());
    }

    #[test]
    fn language_tag_matching_supports_onnx_bilingual_tag() {
        let languages = vec![crate::ocr::OcrLanguage {
            tag: "zh-Hans-en".into(),
            label: "Chinese + English".into(),
        }];

        assert_eq!(
            match_language_tag(&languages, "zh-CN").unwrap().tag,
            "zh-Hans-en"
        );
        assert_eq!(
            match_language_tag(&languages, "zh").unwrap().tag,
            "zh-Hans-en"
        );
        assert_eq!(
            match_language_tag(&languages, "en").unwrap().tag,
            "zh-Hans-en"
        );
    }

    #[test]
    fn configure_normalizes_language_tag() {
        struct LangEngine;
        impl OcrEngine for LangEngine {
            fn id(&self) -> &'static str {
                "lang"
            }
            fn info(&self) -> Result<OcrEngineInfo, String> {
                Ok(OcrEngineInfo {
                    id: "lang".into(),
                    label: "Lang Engine".into(),
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
            fn prepare(&self) -> Result<(), String> {
                Ok(())
            }
            fn release(&self) {}
            fn recognize(
                &self,
                _: OcrImage<'_>,
                _: Option<&str>,
            ) -> Result<RecognizedText, String> {
                unreachable!()
            }
        }

        let registry = OcrRegistry::new(vec![Arc::new(LangEngine)], "lang");
        let result = registry
            .configure(OcrConfig {
                engine: "lang".into(),
                language: Some("zh-CN".into()),
            })
            .unwrap();
        assert_eq!(result.language, Some("zh-Hans-CN".into()));
        assert_eq!(registry.config().language, Some("zh-Hans-CN".into()));
    }
}
