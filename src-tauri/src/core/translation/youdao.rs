//! Youdao free sentence translation (no API key required).
//!
//! Uses Youdao's public demo endpoint (`aidemo.youdao.com/trans`), which is the
//! same family of endpoint the dictionary side already relies on. It supports
//! automatic source-language detection and returns a TTS URL for the result.

use super::{ProviderError, TranslateQuery, TranslateResult, Translator};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

const ENDPOINT: &str = "https://aidemo.youdao.com/trans";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub struct YoudaoFreeProvider {
    client: Client,
}

impl YoudaoFreeProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

impl Default for YoudaoFreeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a BCP-47-ish tag onto the codes Youdao expects (`zh-CHS` / `zh-CHT`).
fn youdao_lang(tag: Option<&str>, fallback: &str) -> String {
    let Some(tag) = tag.map(str::trim).filter(|tag| !tag.is_empty()) else {
        return fallback.to_string();
    };
    match tag.to_ascii_lowercase().as_str() {
        "auto" => "auto".to_string(),
        "zh" | "zh-cn" | "zh-hans" | "zhchs" => "zh-CHS".to_string(),
        "zh-tw" | "zh-hant" | "zhcht" => "zh-CHT".to_string(),
        other => other.to_string(),
    }
}

#[async_trait]
impl Translator for YoudaoFreeProvider {
    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError> {
        let from = youdao_lang(query.source_lang.as_deref(), "auto");
        let to = youdao_lang(query.target_lang.as_deref(), "zh-CHS");

        let response = self
            .client
            .post(ENDPOINT)
            .form(&[("q", query.text.as_str()), ("from", &from), ("to", &to)])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Execution(format!(
                "Youdao request failed with status {status}: {body}"
            )));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Youdao JSON: {e}")))?;

        let error_code = json["errorCode"].as_str().unwrap_or("");
        if !error_code.is_empty() && error_code != "0" {
            return Err(ProviderError::Execution(format!(
                "Youdao error code {error_code}"
            )));
        }

        // `translation` is an array of sentence fragments that concatenate into
        // the full result.
        let translated: String = json["translation"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        if translated.trim().is_empty() {
            return Err(ProviderError::Parse(
                "Empty translation result from Youdao".to_string(),
            ));
        }

        // `l` looks like `en2zh-CHS` (or `zh-CHS2en`) — the prefix is the
        // detected source language.
        let detected = json["l"]
            .as_str()
            .and_then(|pair| pair.split('2').next())
            .filter(|lang| !lang.is_empty())
            .map(str::to_string);

        Ok(TranslateResult {
            original_text: query.text.clone(),
            translated_text: translated,
            detected_source_lang: detected,
            target_lang: to,
            details: None,
            provider_name: "YoudaoFree".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::youdao_lang;

    #[test]
    fn maps_youdao_language_codes() {
        assert_eq!(youdao_lang(None, "auto"), "auto");
        assert_eq!(youdao_lang(Some("  "), "zh-CHS"), "zh-CHS");
        assert_eq!(youdao_lang(Some("auto"), "zh-CHS"), "auto");
        assert_eq!(youdao_lang(Some("zh-CN"), "zh-CHS"), "zh-CHS");
        assert_eq!(youdao_lang(Some("zh-TW"), "zh-CHS"), "zh-CHT");
        assert_eq!(youdao_lang(Some("en"), "zh-CHS"), "en");
    }
}
