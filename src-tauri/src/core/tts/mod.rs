//! Local neural Text-To-Speech (TTS) subsystem powered by Kokoro-82M ONNX.

pub mod assets;
pub mod cache;
pub mod chunker;
pub mod engine;
pub mod tokenizer;

use assets::{InstallProgress, TtsAssets, TtsInstallStatus, VoiceInfo};
use cache::{AudioCacheKey, AudioLruCache};
use chunker::{SentenceChunker, TtsError};
use engine::KokoroEngine;
use std::path::PathBuf;
use std::sync::Arc;

pub struct TtsService {
    pub assets: Arc<TtsAssets>,
    pub engine: Arc<KokoroEngine>,
    pub cache: Arc<AudioLruCache>,
}

impl TtsService {
    pub fn new(assets: Arc<TtsAssets>) -> Self {
        let engine = Arc::new(KokoroEngine::new(assets.clone()));
        let cache = Arc::new(AudioLruCache::default());
        Self {
            assets,
            engine,
            cache,
        }
    }

    pub fn status(&self) -> TtsInstallStatus {
        self.assets.status()
    }

    pub fn voices(&self) -> Vec<VoiceInfo> {
        self.assets.status().voices
    }

    pub async fn install(
        &self,
        progress: &(dyn Fn(InstallProgress) + Send + Sync),
    ) -> Result<PathBuf, String> {
        self.assets.install(progress).await
    }

    pub fn delete(&self) -> Result<(), String> {
        // Release session and clear cache before removing files to ensure Windows handles are freed.
        self.engine.release();
        self.cache.clear();
        self.assets.delete()
    }

    pub async fn synthesize(
        &self,
        text: &str,
        voice: &str,
        speed: f32,
    ) -> Result<Vec<u8>, TtsError> {
        let cache_key = AudioCacheKey::new(text, voice, speed);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached);
        }

        let is_british = voice.starts_with("bf_") || voice.starts_with("bm_");
        let chunker = SentenceChunker::global();
        let chunks = chunker.chunk_and_tokenize(text, is_british)?;

        let engine = self.engine.clone();
        let voice_string = voice.to_string();

        // Run CPU-intensive ONNX inference on a blocking thread pool
        let wav_bytes = tokio::task::spawn_blocking(move || {
            engine.synthesize_chunks(chunks, &voice_string, speed)
        })
        .await
        .map_err(|e| TtsError::Inference(format!("Tokio spawn_blocking 调度失败: {e}")))??;

        self.cache.put(cache_key, wav_bytes.clone());
        Ok(wav_bytes)
    }
}
