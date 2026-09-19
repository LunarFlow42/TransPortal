//! Original image coordinates and provider-independent image translation.

use super::OcrImage;
use crate::provider::{TranslateQuery, Translator};
use base64::{Engine, engine::general_purpose::STANDARD};
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrBlock {
    pub id: u32,
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl OcrBlock {
    pub fn clipped(mut self, width: u32, height: u32) -> Option<Self> {
        if [self.x, self.y, self.width, self.height]
            .iter()
            .any(|v| !v.is_finite())
            || self.width <= 0.0
            || self.height <= 0.0
            || self.text.trim().is_empty()
        {
            return None;
        }
        let right = (self.x + self.width).clamp(0.0, width as f32);
        let bottom = (self.y + self.height).clamp(0.0, height as f32);
        self.x = self.x.clamp(0.0, width as f32);
        self.y = self.y.clamp(0.0, height as f32);
        self.width = right - self.x;
        self.height = bottom - self.y;
        (self.width >= 1.0 && self.height >= 1.0).then_some(self)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OcrPicture {
    pub width: u32,
    pub height: u32,
    pub data_url: String,
    pub blocks: Vec<OcrBlock>,
}

impl OcrPicture {
    pub fn new(image: OcrImage<'_>, blocks: Vec<OcrBlock>) -> Result<Self, String> {
        image.validate()?;
        let blocks = blocks
            .into_iter()
            .filter_map(|block| block.clipped(image.width, image.height))
            .enumerate()
            .map(|(id, mut block)| {
                block.id = id as u32;
                block
            })
            .collect();
        Ok(Self {
            width: image.width,
            height: image.height,
            data_url: encode_png(image)?,
            blocks,
        })
    }
}

pub fn encode_png(image: OcrImage<'_>) -> Result<String, String> {
    use image::ImageEncoder;
    image.validate()?;
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new_with_quality(
        &mut bytes,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(
        image.rgba,
        image.width,
        image.height,
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|error| format!("无法生成图片预览：{error}"))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

pub fn decode_png(data_url: &str) -> Result<image::RgbaImage, String> {
    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or("仅支持 PNG 图片。")?;
    if encoded.len() > 180_000_000 {
        return Err("图片过大。".into());
    }
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())?;
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16000);
    limits.max_image_height = Some(16000);
    limits.max_alloc = Some(128_000_000);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|error| format!("无法读取 PNG：{error}"))?
        .into_rgba8();
    OcrImage {
        width: image.width(),
        height: image.height(),
        rgba: image.as_raw(),
    }
    .validate()?;
    Ok(image)
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslatedBlock {
    pub id: u32,
    pub text: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PictureTranslation {
    pub blocks: Vec<TranslatedBlock>,
    pub target_lang: String,
}

impl PictureTranslation {
    pub fn succeeded(&self) -> bool {
        self.blocks.iter().all(|block| block.error.is_none())
    }
}

/// A watch channel also wakes requests that are waiting on a slow provider.
#[derive(Clone)]
pub struct Cancellation(tokio::sync::watch::Sender<bool>);

impl Default for Cancellation {
    fn default() -> Self {
        Self(tokio::sync::watch::channel(false).0)
    }
}

impl Cancellation {
    pub fn cancel(&self) {
        self.0.send_replace(true);
    }
    pub fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
    pub async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        if !*receiver.borrow_and_update() {
            let _ = receiver.changed().await;
        }
    }
}

/// Cache lives only for one monitor session (one provider/target snapshot).
pub type TranslationCache = HashMap<String, String>;

pub async fn translate_blocks(
    blocks: &[OcrBlock],
    provider: Arc<dyn Translator>,
    target: &str,
    cache: &mut TranslationCache,
    cancel: &Cancellation,
) -> Result<PictureTranslation, String> {
    if blocks.len() > 500 {
        return Err("图片文字块过多，请缩小选区（最多 500 块）。".into());
    }
    let inputs: Vec<_> = blocks
        .iter()
        .map(|block| {
            (
                block.id,
                block.text.clone(),
                cache.get(&block.text).cloned(),
            )
        })
        .collect();
    let jobs = inputs.into_iter().map(|(id, source, cached)| {
        let provider = provider.clone();
        let target = target.to_string();
        async move {
            let result = if let Some(text) = cached {
                Ok(text)
            } else {
                let resolved_target = crate::provider::resolve_target_lang(&source, Some(&target));
                let query = TranslateQuery {
                    text: source.clone(),
                    source_lang: None,
                    target_lang: Some(resolved_target),
                };
                match tokio::time::timeout(Duration::from_secs(30), provider.translate(&query))
                    .await
                {
                    Ok(Ok(result)) if !result.translated_text.trim().is_empty() => {
                        Ok(result.translated_text)
                    }
                    Ok(Ok(_)) => Err("未返回译文".to_string()),
                    Ok(Err(error)) => Err(error.to_string()),
                    Err(_) => Err("翻译超时，请重试".to_string()),
                }
            };
            let translated = match result {
                Ok(text) => TranslatedBlock {
                    id,
                    text,
                    error: None,
                },
                Err(error) => TranslatedBlock {
                    id,
                    text: String::new(),
                    error: Some(error),
                },
            };
            (source, translated)
        }
    });
    let results: Vec<_> = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err("已取消图片翻译".into()),
        results = stream::iter(jobs).buffer_unordered(3).collect() => results,
    };
    if cancel.is_cancelled() {
        return Err("已取消图片翻译".into());
    }
    if cache.len() + results.len() > 512 {
        cache.clear();
    }
    let mut translated = Vec::with_capacity(results.len());
    for (source, block) in results {
        if block.error.is_none() {
            cache.insert(source, block.text.clone());
        }
        translated.push(block);
    }
    translated.sort_by_key(|block| block.id);
    Ok(PictureTranslation {
        blocks: translated,
        target_lang: target.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ProviderError, TranslateResult};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Provider(AtomicUsize);
    #[async_trait::async_trait]
    impl Translator for Provider {
        async fn translate(
            &self,
            query: &TranslateQuery,
        ) -> Result<TranslateResult, ProviderError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            if query.text == "slow" {
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
            if query.text == "fail" {
                return Err(ProviderError::Execution("offline".into()));
            }
            Ok(TranslateResult {
                original_text: query.text.clone(),
                translated_text: format!("译:{}", query.text),
                detected_source_lang: None,
                target_lang: "zh-CN".into(),
                details: None,
                provider_name: "test".into(),
            })
        }
    }
    fn block(id: u32, text: &str) -> OcrBlock {
        OcrBlock {
            id,
            text: text.into(),
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 20.0,
        }
    }

    #[test]
    fn picture_clips_geometry_and_roundtrips_original_pixels() {
        let rgba = [20, 30, 40, 255].repeat(400);
        let picture = OcrPicture::new(
            OcrImage {
                width: 20,
                height: 20,
                rgba: &rgba,
            },
            vec![block(5, "text")],
        )
        .unwrap();
        assert_eq!(picture.blocks[0].width, 20.0);
        assert_eq!(picture.blocks[0].id, 0);
        assert_eq!(decode_png(&picture.data_url).unwrap().as_raw(), &rgba);
        let mut bad = block(0, "bad");
        bad.x = f32::NAN;
        assert!(bad.clipped(20, 20).is_none());
    }

    #[tokio::test]
    async fn failures_keep_block_identity_and_only_successes_are_cached() {
        let provider = Arc::new(Provider(AtomicUsize::new(0)));
        let blocks = vec![block(0, "one"), block(1, "fail"), block(2, "two")];
        let mut cache = TranslationCache::new();
        let result = translate_blocks(
            &blocks,
            provider.clone(),
            "zh-CN",
            &mut cache,
            &Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(
            result.blocks.iter().map(|b| b.id).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(result.blocks[1].error.is_some());
        assert!(!result.succeeded());
        translate_blocks(
            &blocks,
            provider.clone(),
            "zh-CN",
            &mut cache,
            &Cancellation::default(),
        )
        .await
        .unwrap();
        assert_eq!(provider.0.load(Ordering::Relaxed), 4);
    }

    #[tokio::test]
    async fn cancelling_interrupts_an_in_flight_provider_request() {
        let cancel = Cancellation::default();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            trigger.cancel();
        });
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            translate_blocks(
                &[block(0, "slow")],
                Arc::new(Provider(AtomicUsize::new(0))),
                "zh-CN",
                &mut TranslationCache::new(),
                &cancel,
            ),
        )
        .await
        .unwrap();
        assert!(result.is_err());
    }
}
