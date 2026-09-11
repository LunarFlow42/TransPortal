use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub mod google;
pub mod openai;

pub use google::GoogleFreeProvider;
pub use openai::{OpenAiCompatibleProvider, OpenAiConfig};

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

    /// Stream translation chunks via Tauri IPC Channel
    async fn translate_stream(
        &self,
        query: &TranslateQuery,
        channel: tauri::ipc::Channel<StreamChunk>,
    ) -> Result<(), ProviderError>;
}
