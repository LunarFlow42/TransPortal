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
    #[serde(alias = "apiKey")]
    pub api_key: String,
    #[serde(alias = "baseUrl")]
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    #[serde(alias = "systemPrompt")]
    pub system_prompt: Option<String>,
    #[serde(alias = "promptTemplate")]
    pub prompt_template: Option<String>,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: Some("https://api.openai.com/v1".to_string()),
            model: Some("gpt-4o-mini".to_string()),
            temperature: Some(0.3),
            system_prompt: None,
            prompt_template: None,
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
            .trim()
            .trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    fn build_system_prompt(&self, target_lang: &str) -> String {
        if let Some(prompt) = self
            .config
            .system_prompt
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            return prompt.to_string();
        }
        match self.config.prompt_template.as_deref() {
            Some("academic") => format!(
                "You are a professional academic translator and editor. Translate or polish the following text into formal, natural academic {target_lang} with scholarly vocabulary. Provide only the translation, with no conversational remarks."
            ),
            Some("code") => format!(
                "You are an expert developer translator. Translate the following text into concise, technical {target_lang} suitable for code comments, commit messages, or variable naming conventions. Provide only the pure translation, with no explanation."
            ),
            Some("casual") => format!(
                "You are a native conversational translator. Translate the following text into casual, everyday colloquial {target_lang}. Provide only the translation, with no conversational remarks."
            ),
            _ => format!(
                "You are a high-speed professional translator and dictionary assistant. Translate the following user input accurately and naturally into {target_lang}. Provide only the pure translation or explanation, with no preamble or conversational remarks."
            ),
        }
    }

    /// Non-streaming chat completion with an explicit system prompt.
    ///
    /// Used by the LLM example-sentence enhancement (not the translator flow).
    pub async fn chat(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, ProviderError> {
        let model = self.config.model.as_deref().unwrap_or("gpt-4o-mini");
        let url = self.endpoint_url();

        let payload = json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": self.config.temperature.unwrap_or(0.3),
            "stream": false
        });

        let mut request = self.client.post(&url).json(&payload);
        if !self.config.api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Execution(format!(
                "chat request failed ({status}): {body}"
            )));
        }

        let json_val: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse chat response: {e}")))?;

        let content = json_val["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .trim()
            .to_string();

        if content.is_empty() {
            return Err(ProviderError::Parse("empty chat response".to_string()));
        }

        Ok(content)
    }

    /// Fetches and returns the available model list from the OpenAI-compatible `/models` endpoint.
    pub async fn fetch_models(&self) -> Result<Vec<String>, ProviderError> {
        let url = normalize_models_url(self.config.base_url.as_deref());
        let mut request = self.client.get(&url);
        let trimmed_key = self.config.api_key.trim();
        if !trimmed_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {trimmed_key}"));
        }

        let response = request
            .send()
            .await
            .map_err(|e| ProviderError::Execution(format!("请求模型列表网络异常: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 401 {
                return Err(ProviderError::Auth(format!(
                    "API Key 认证失败 (401)，请检查 API Key: {body}"
                )));
            }
            if status.as_u16() == 404 {
                return Err(ProviderError::Execution(format!(
                    "接口返回 404，请确认 Base URL 是否正确 (通常需包含 /v1): {url}"
                )));
            }
            return Err(ProviderError::Execution(format!(
                "获取模型列表失败 ({status}): {body}"
            )));
        }

        let json_val: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("解析模型列表 JSON 响应失败: {e}")))?;

        parse_and_sort_models(&json_val)
    }
}

/// Normalizes the base URL into a `/models` endpoint URL.
pub fn normalize_models_url(base_url: Option<&str>) -> String {
    let raw = base_url.unwrap_or("https://api.openai.com/v1").trim();
    let trimmed = raw.trim_end_matches('/');
    if let Some(stripped) = trimmed.strip_suffix("/chat/completions") {
        let base = stripped.trim_end_matches('/');
        format!("{base}/models")
    } else if trimmed.ends_with("/models") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/models")
    }
}

fn model_priority_tier(name: &str) -> u8 {
    let lower = name.to_lowercase();
    // Non-text models get Tier 3 (bottom)
    let is_non_text = lower.contains("embed")
        || lower.contains("whisper")
        || lower.contains("tts")
        || lower.contains("dall-e")
        || lower.contains("moderation")
        || lower.contains("realtime")
        || lower.contains("audio")
        || lower.contains("babbage")
        || lower.contains("davinci");

    if is_non_text {
        return 3;
    }

    // Mainstream chat / translation models get Tier 1 (top)
    let is_popular_chat = lower.contains("gpt-4")
        || lower.contains("gpt-3.5")
        || lower.contains("o1")
        || lower.contains("o3")
        || lower.contains("o4")
        || lower.contains("deepseek")
        || lower.contains("claude")
        || lower.contains("gemini")
        || lower.contains("qwen")
        || lower.contains("llama")
        || lower.contains("mistral")
        || lower.contains("glm")
        || lower.contains("moonshot")
        || lower.contains("kimi")
        || lower.contains("gemma")
        || lower.contains("chat");

    if is_popular_chat { 1 } else { 2 }
}

pub fn sort_models(models: &mut [String]) {
    models.sort_by(|a, b| {
        let tier_a = model_priority_tier(a);
        let tier_b = model_priority_tier(b);
        if tier_a != tier_b {
            tier_a.cmp(&tier_b)
        } else {
            a.to_lowercase().cmp(&b.to_lowercase())
        }
    });
}

/// Parses multi-format JSON responses from OpenAI or compatible endpoints,
/// deduplicates the model IDs, and sorts them with smart priority.
pub fn parse_and_sort_models(json_val: &serde_json::Value) -> Result<Vec<String>, ProviderError> {
    let mut raw_models = Vec::new();

    let mut extract_item = |item: &serde_json::Value| {
        if let Some(s) = item.as_str() {
            let s = s.trim();
            if !s.is_empty() {
                raw_models.push(s.to_string());
            }
        } else if let Some(obj) = item.as_object() {
            if let Some(id) = obj.get("id").and_then(|v| v.as_str()) {
                let id = id.trim();
                if !id.is_empty() {
                    raw_models.push(id.to_string());
                    return;
                }
            }
            if let Some(name) = obj.get("name").and_then(|v| v.as_str()) {
                let name = name.trim();
                if !name.is_empty() {
                    raw_models.push(name.to_string());
                }
            }
        }
    };

    if let Some(arr) = json_val.get("data").and_then(|v| v.as_array()) {
        for item in arr {
            extract_item(item);
        }
    } else if let Some(arr) = json_val.get("models").and_then(|v| v.as_array()) {
        for item in arr {
            extract_item(item);
        }
    } else if let Some(arr) = json_val.as_array() {
        for item in arr {
            extract_item(item);
        }
    }

    if raw_models.is_empty() {
        return Err(ProviderError::Parse("未找到任何可用模型 ID".to_string()));
    }

    let mut seen = std::collections::HashSet::new();
    let mut unique_models = Vec::new();
    for m in raw_models {
        if seen.insert(m.clone()) {
            unique_models.push(m);
        }
    }

    sort_models(&mut unique_models);
    Ok(unique_models)
}

fn natural_lang_name(tag: &str) -> &str {
    match tag {
        "zh-CN" | "zh-Hans" | "zh" => "Simplified Chinese (简体中文)",
        "zh-TW" | "zh-Hant" | "zh-HK" => "Traditional Chinese (繁體中文)",
        "en" => "English",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "ru" => "Russian",
        other => other,
    }
}

#[async_trait]
impl Translator for OpenAiCompatibleProvider {
    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError> {
        let target_tag = query.target_lang.as_deref().unwrap_or("zh-CN");
        let target_lang = natural_lang_name(target_tag);
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
                return Err(ProviderError::Auth(format!(
                    "Invalid OpenAI API Key: {body}"
                )));
            }
            if status.as_u16() == 429 {
                return Err(ProviderError::RateLimit(format!(
                    "OpenAI Rate Limit reached: {body}"
                )));
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
            return Err(ProviderError::Parse(
                "Received empty translation from OpenAI".into(),
            ));
        }

        Ok(TranslateResult {
            original_text: query.text.clone(),
            translated_text: translated,
            detected_source_lang: query.source_lang.clone(),
            target_lang: target_tag.to_string(),
            details: None,
            provider_name: format!("OpenAI({})", model),
        })
    }

    async fn translate_stream(
        &self,
        query: &TranslateQuery,
        channel: tauri::ipc::Channel<StreamChunk>,
    ) -> Result<(), ProviderError> {
        let target_tag = query.target_lang.as_deref().unwrap_or("zh-CN");
        let target_lang = natural_lang_name(target_tag);
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

        let mut request = self.client.post(&url).json(&payload);
        if !self.config.api_key.is_empty() {
            request = request.header("Authorization", format!("Bearer {}", self.config.api_key));
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if status.as_u16() == 401 {
                return Err(ProviderError::Auth(format!(
                    "OpenAI API 认证失败 (401): {body}"
                )));
            }
            return Err(ProviderError::Execution(format!(
                "OpenAI 流式请求失败 ({status}): {body}"
            )));
        }

        let mut stream = response.bytes_stream().eventsource();

        while let Some(event) = stream.next().await {
            match event {
                Ok(event) => {
                    let data = event.data.trim();
                    if data.is_empty() {
                        continue;
                    }
                    if data == "[DONE]" {
                        let _ = channel.send(StreamChunk {
                            text: String::new(),
                            is_done: true,
                            error: None,
                        });
                        return Ok(());
                    }

                    if let Ok(chunk_json) = serde_json::from_str::<serde_json::Value>(data)
                        && let Some(content) = chunk_json["choices"][0]["delta"]["content"].as_str()
                        && !content.is_empty()
                    {
                        channel
                            .send(StreamChunk {
                                text: content.to_string(),
                                is_done: false,
                                error: None,
                            })
                            .map_err(|e| ProviderError::Execution(e.to_string()))?;
                    }
                }
                Err(e) => {
                    let message = e.to_string();
                    let _ = channel.send(StreamChunk {
                        text: String::new(),
                        is_done: true,
                        error: Some(message.clone()),
                    });
                    return Err(ProviderError::Execution(format!(
                        "SSE stream error: {message}"
                    )));
                }
            }
        }

        // Signal completion if the stream ended without an explicit `[DONE]`.
        let _ = channel.send(StreamChunk {
            text: String::new(),
            is_done: true,
            error: None,
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_url_normalization() {
        let p1 = OpenAiCompatibleProvider::new(OpenAiConfig {
            base_url: Some("https://api.openai.com/v1".into()),
            ..Default::default()
        });
        assert_eq!(
            p1.endpoint_url(),
            "https://api.openai.com/v1/chat/completions"
        );

        let p2 = OpenAiCompatibleProvider::new(OpenAiConfig {
            base_url: Some("https://api.deepseek.com/v1/chat/completions".into()),
            ..Default::default()
        });
        assert_eq!(
            p2.endpoint_url(),
            "https://api.deepseek.com/v1/chat/completions"
        );

        let p3 = OpenAiCompatibleProvider::new(OpenAiConfig {
            base_url: Some("https://api.deepseek.com/v1/chat/completions/".into()),
            ..Default::default()
        });
        assert_eq!(
            p3.endpoint_url(),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_natural_lang_name() {
        assert_eq!(natural_lang_name("zh-CN"), "Simplified Chinese (简体中文)");
        assert_eq!(natural_lang_name("zh-TW"), "Traditional Chinese (繁體中文)");
        assert_eq!(natural_lang_name("en"), "English");
        assert_eq!(natural_lang_name("ja"), "Japanese");
    }

    #[test]
    fn test_normalize_models_url() {
        assert_eq!(
            normalize_models_url(None),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            normalize_models_url(Some("https://api.openai.com/v1")),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            normalize_models_url(Some("https://api.openai.com/v1/")),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            normalize_models_url(Some("https://api.openai.com/v1/chat/completions")),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            normalize_models_url(Some("https://api.openai.com/v1/chat/completions/")),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            normalize_models_url(Some("https://api.deepseek.com")),
            "https://api.deepseek.com/models"
        );
        assert_eq!(
            normalize_models_url(Some("https://openrouter.ai/api/v1")),
            "https://openrouter.ai/api/v1/models"
        );
        assert_eq!(
            normalize_models_url(Some("http://127.0.0.1:11434/v1")),
            "http://127.0.0.1:11434/v1/models"
        );
        assert_eq!(
            normalize_models_url(Some("https://api.openai.com/v1/models")),
            "https://api.openai.com/v1/models"
        );
    }

    #[test]
    fn test_parse_and_sort_models() {
        let json_data = serde_json::json!({
            "object": "list",
            "data": [
                { "id": "text-embedding-3-small" },
                { "id": "whisper-1" },
                { "id": "gpt-4o" },
                { "id": "gpt-4o-mini" },
                { "id": "deepseek-chat" },
                { "id": "custom-domain-model" }
            ]
        });

        let models = parse_and_sort_models(&json_data).expect("should parse successfully");
        assert_eq!(
            models,
            vec![
                "deepseek-chat",
                "gpt-4o",
                "gpt-4o-mini",
                "custom-domain-model",
                "text-embedding-3-small",
                "whisper-1"
            ]
        );

        // Test compatible format with `models` array and `name` field (e.g. Ollama)
        let ollama_json = serde_json::json!({
            "models": [
                { "name": "llama3:latest" },
                { "name": "qwen2.5:7b" },
                { "name": "nomic-embed-text" }
            ]
        });
        let ollama_models =
            parse_and_sort_models(&ollama_json).expect("should parse ollama response");
        assert_eq!(
            ollama_models,
            vec!["llama3:latest", "qwen2.5:7b", "nomic-embed-text"]
        );
    }
}
