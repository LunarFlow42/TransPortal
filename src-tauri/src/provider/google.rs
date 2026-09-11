use super::{ProviderError, StreamChunk, TranslateQuery, TranslateResult, TranslationDetail, Translator};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

pub struct GoogleFreeProvider {
    client: Client,
}

impl GoogleFreeProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

impl Default for GoogleFreeProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Translator for GoogleFreeProvider {
    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError> {
        let sl = query.source_lang.as_deref().unwrap_or("auto");
        let tl = query.target_lang.as_deref().unwrap_or("zh-CN");

        let response = self
            .client
            .get("https://translate.googleapis.com/translate_a/single")
            .query(&[
                ("client", "gtx"),
                ("sl", sl),
                ("tl", tl),
                ("dt", "t"),
                ("dt", "bd"),
                ("dt", "rm"),
                ("q", &query.text),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Execution(format!(
                "Google translate failed with status {status}: {body}"
            )));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Google response JSON: {e}")))?;

        // Structure of Google GTX JSON:
        // [
        //   [ [ "translated text", "original text", null, null, ... ], ... ],
        //   [ [ "noun", ["meanings"...] ], ... ], // optional dictionary
        //   "en", // detected language
        //   ...
        // ]
        let mut full_translated = String::new();
        if let Some(sentences) = json.get(0).and_then(|v| v.as_array()) {
            for item in sentences {
                if let Some(trans_segment) = item.get(0).and_then(|s| s.as_str()) {
                    full_translated.push_str(trans_segment);
                }
            }
        }

        if full_translated.is_empty() {
            return Err(ProviderError::Parse(
                "Empty translation result from Google API".to_string(),
            ));
        }

        let mut details: Vec<TranslationDetail> = Vec::new();
        if let Some(dict_items) = json.get(1).and_then(|v| v.as_array()) {
            for item in dict_items {
                if let (Some(pos), Some(terms_arr)) = (
                    item.get(0).and_then(|v| v.as_str()),
                    item.get(1).and_then(|v| v.as_array()),
                ) {
                    let terms: Vec<String> = terms_arr
                        .iter()
                        .filter_map(|t| t.as_str().map(|s| s.to_string()))
                        .collect();
                    if !terms.is_empty() {
                        details.push(TranslationDetail {
                            pos: pos.to_string(),
                            terms,
                        });
                    }
                }
            }
        }

        let detected_lang = json.get(2).and_then(|v| v.as_str()).map(|s| s.to_string());

        Ok(TranslateResult {
            original_text: query.text.clone(),
            translated_text: full_translated,
            detected_source_lang: detected_lang,
            target_lang: tl.to_string(),
            details: if details.is_empty() { None } else { Some(details) },
            provider_name: "GoogleFree".to_string(),
        })
    }

    async fn translate_stream(
        &self,
        query: &TranslateQuery,
        channel: tauri::ipc::Channel<StreamChunk>,
    ) -> Result<(), ProviderError> {
        let result = match self.translate(query).await {
            Ok(res) => res,
            Err(e) => {
                let _ = channel.send(StreamChunk {
                    text: String::new(),
                    is_done: true,
                    error: Some(e.to_string()),
                });
                return Err(e);
            }
        };

        // Stream words or sentence chunks smoothly
        let chars: Vec<char> = result.translated_text.chars().collect();
        let chunk_size = 2.max(chars.len() / 15 + 1);

        for (i, chunk_chars) in chars.chunks(chunk_size).enumerate() {
            let chunk_str: String = chunk_chars.iter().collect();
            let is_last = (i + 1) * chunk_size >= chars.len();

            channel
                .send(StreamChunk {
                    text: chunk_str,
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
}
