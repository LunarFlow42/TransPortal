use super::{ProviderError, StreamChunk, TranslateQuery, TranslateResult, Translator};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub api_key: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub system_prompt: Option<String>,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            temperature: Some(0.3),
            system_prompt: None,
        }
    }
}

pub struct OpenAiCompatibleProvider {
    client: Client,
    config: OpenAiConfig,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: OpenAiConfig) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { client, config }
    }

    fn endpoint_url(&self) -> String {
        let base = self
            .config
            .base_url
            .as_deref()
            .unwrap_or("https://api.openai.com/v1")
            .trim_end_matches('/');
        format!("{base}/chat/completions")
    }

    fn build_system_prompt(&self, target_lang: &str) -> String {
        if let Some(ref prompt) = self.config.system_prompt {
            if !prompt.trim().is_empty() {
                return prompt.clone();
            }
        }
        format!(
            "You are a high-speed professional translator and dictionary assistant. Translate the following user input accurately and naturally into {target_lang}. Provide only the pure translation or explanation, with no preamble or conversational remarks."
        )
    }
}

#[async_trait]
impl Translator for OpenAiCompatibleProvider {
    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError> {
        let target_lang = query.target_lang.as_deref().unwrap_or("Chinese");
        let system_prompt = self.build_system_prompt(target_lang);
        let model = self.config.model.as_deref().unwrap_or("gpt-4o-mini");
        let url = self.endpoint_url();

        let payload = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": query.text }
            ],
            "temperature": self.config.temperature.unwrap_or(0.3),
            "stream": false
        });

        let mut req = self.client.post(&url).json(&payload);
        if !self.config.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 401 {
                return Err(ProviderError::Auth(format!("Invalid OpenAI API Key: {body}")));
            }
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimit(format!("OpenAI Rate Limit reached: {body}")));
            }
            return Err(ProviderError::Execution(format!(
                "OpenAI request failed ({status}): {body}"
            )));
        }

        let json_val: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse OpenAI JSON: {e}")))?;

        let translated = json_val["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        if translated.is_empty() {
            return Err(ProviderError::Parse("Received empty translation from OpenAI".into()));
        }

        Ok(TranslateResult {
            original_text: query.text.clone(),
            translated_text: translated,
            detected_source_lang: query.source_lang.clone(),
            target_lang: target_lang.to_string(),
            details: None,
            provider_name: format!("OpenAI({})", model),
        })
    }

    async fn translate_stream(
        &self,
        query: &TranslateQuery,
        channel: tauri::ipc::Channel<StreamChunk>,
    ) -> Result<(), ProviderError> {
        let target_lang = query.target_lang.as_deref().unwrap_or("Chinese");
        let system_prompt = self.build_system_prompt(target_lang);
        let model = self.config.model.as_deref().unwrap_or("gpt-4o-mini");
        let url = self.endpoint_url();

        let payload = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": query.text }
            ],
            "temperature": self.config.temperature.unwrap_or(0.3),
            "stream": true
        });

        let mut req = self.client.post(&url).json(&payload);
        if !self.config.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        let response = match req.send().await {
            Ok(resp) => resp,
            Err(e) => {
                let _ = channel.send(StreamChunk {
                    text: String::new(),
                    is_done: true,
                    error: Some(e.to_string()),
                });
                return Err(ProviderError::Network(e));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let err_msg = format!("OpenAI API error ({status}): {body}");
            let _ = channel.send(StreamChunk {
                text: String::new(),
                is_done: true,
                error: Some(err_msg.clone()),
            });
            return Err(ProviderError::Execution(err_msg));
        }

        let mut stream = response.bytes_stream().eventsource();

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    let data = event.data.trim();
                    if data == "[DONE]" {
                        let _ = channel.send(StreamChunk {
                            text: String::new(),
                            is_done: true,
                            error: None,
                        });
                        return Ok(());
                    }

                    if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(content) = chunk_json["choices"][0]["delta"]["content"].as_str() {
                            if !content.is_empty() {
                                channel
                                    .send(StreamChunk {
                                        text: content.to_string(),
                                        is_done: false,
                                        error: None,
                                    })
                                    .map_err(|e| ProviderError::Execution(e.to_string()))?;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = channel.send(StreamChunk {
                        text: String::new(),
                        is_done: true,
                        error: Some(e.to_string()),
                    });
                    return Err(ProviderError::Execution(format!("SSE Stream error: {e}")));
                }
            }
        }

        // Signal completion if not done already
        let _ = channel.send(StreamChunk {
            text: String::new(),
            is_done: true,
            error: None,
        });

        Ok(())
    }
}
