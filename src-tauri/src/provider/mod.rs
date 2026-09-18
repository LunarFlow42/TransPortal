use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

pub mod google;
pub mod openai;
pub mod tencent;
pub mod youdao;

pub use google::GoogleFreeProvider;
pub use openai::{OpenAiCompatibleProvider, OpenAiConfig};
pub use tencent::TencentFreeProvider;
pub use youdao::YoudaoFreeProvider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateQuery {
    pub text: String,
    pub source_lang: Option<String>,
    pub target_lang: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationDetail {
    pub pos: String,
    pub terms: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslateResult {
    pub original_text: String,
    pub translated_text: String,
    pub detected_source_lang: Option<String>,
    pub target_lang: String,
    pub details: Option<Vec<TranslationDetail>>,
    pub provider_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    pub text: String,
    pub is_done: bool,
    pub error: Option<String>,
}

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("Network request failed: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Failed to parse provider response: {0}")]
    Parse(String),

    #[error("Authentication failed: {0}")]
    Auth(String),

    #[error("Rate limited by provider: {0}")]
    RateLimit(String),

    #[error("Provider execution error: {0}")]
    Execution(String),
}

/// Unified Trait for Translation Providers
#[async_trait]
pub trait Translator: Send + Sync {
    /// Return full translated result asynchronously
    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError>;

    /// Stream translation chunks via Tauri IPC Channel.
    ///
    /// Providers without a native streaming API get a smooth simulated stream
    /// over the full result, so adding an engine only requires `translate`.
    async fn translate_stream(
        &self,
        query: &TranslateQuery,
        channel: tauri::ipc::Channel<StreamChunk>,
    ) -> Result<(), ProviderError> {
        let result = match self.translate(query).await {
            Ok(result) => result,
            Err(error) => {
                let _ = channel.send(StreamChunk {
                    text: String::new(),
                    is_done: true,
                    error: Some(error.to_string()),
                });
                return Err(error);
            }
        };

        emit_chunked(&result.translated_text, channel).await
    }
}

/// Emit `text` as a series of small chunks so non-streaming providers still
/// animate in the overlay.
pub async fn emit_chunked(
    text: &str,
    channel: tauri::ipc::Channel<StreamChunk>,
) -> Result<(), ProviderError> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        let _ = channel.send(StreamChunk {
            text: String::new(),
            is_done: true,
            error: None,
        });
        return Ok(());
    }

    let chunk_size = 2.max(chars.len() / 15 + 1);
    for (index, chunk) in chars.chunks(chunk_size).enumerate() {
        let is_last = (index + 1) * chunk_size >= chars.len();
        channel
            .send(StreamChunk {
                text: chunk.iter().collect(),
                is_done: is_last,
                error: None,
            })
            .map_err(|e| ProviderError::Execution(e.to_string()))?;

        if !is_last {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    Ok(())
}

/// Factory signature for a registered provider.
pub type ProviderFactory = fn(Option<OpenAiConfig>) -> Arc<dyn Translator>;

/// Built-in provider registry (microkernel). Register new engines here only;
/// the command layer never needs to change.
pub fn registry() -> &'static [(&'static str, ProviderFactory)] {
    &[
        ("google_free", |_config| Arc::new(GoogleFreeProvider::new())),
        ("youdao_free", |_config| Arc::new(YoudaoFreeProvider::new())),
        ("tencent_free", |_config| Arc::new(TencentFreeProvider::new())),
        ("openai", |config| {
            Arc::new(OpenAiCompatibleProvider::new(config.unwrap_or_default()))
        }),
    ]
}

/// Instantiate a registered provider by name; unknown names fall back to the
/// first registered entry.
pub fn create(name: &str, config: Option<OpenAiConfig>) -> Arc<dyn Translator> {
    let registry = registry();
    let factory = registry
        .iter()
        .find(|(id, _)| *id == name)
        .map(|(_, factory)| factory)
        .or_else(|| registry.first().map(|(_, factory)| factory))
        .expect("provider registry must not be empty");
    factory(config)
}
