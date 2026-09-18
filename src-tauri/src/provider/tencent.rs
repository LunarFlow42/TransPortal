//! Tencent TranSmart free sentence translation (no API key required).
//!
//! Uses Tencent's public web endpoint (`transmart.qq.com/api/imt`), which
//! supports automatic source-language detection and batched plain-text input.

use super::{ProviderError, TranslateQuery, TranslateResult, Translator};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;

const ENDPOINT: &str = "https://transmart.qq.com/api/imt";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub struct TencentFreeProvider {
    client: Client,
}

impl TencentFreeProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self { client }
    }
}

impl Default for TencentFreeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Map a BCP-47-ish tag onto TranSmart's language codes.
fn tencent_lang(tag: Option<&str>, target: bool) -> String {
    let fallback = if target { "zh" } else { "auto" };
    let Some(tag) = tag.map(str::trim).filter(|tag| !tag.is_empty()) else {
        return fallback.to_string();
    };

    let base = tag.split(['-', '_']).next().unwrap_or(tag).to_ascii_lowercase();
    match (base.as_str(), tag.to_ascii_lowercase().as_str()) {
        ("auto", _) => "auto".to_string(),
        ("zh", "zh-tw") | ("zh", "zh-hant") => "zh-TW".to_string(),
        ("zh", _) => "zh".to_string(),
        ("ja", _) => "jp".to_string(),
        ("ko", _) => "kr".to_string(),
        (other, _) => other.to_string(),
    }
}

#[async_trait]
impl Translator for TencentFreeProvider {
    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError> {
        let source_lang = tencent_lang(query.source_lang.as_deref(), false);
        let target_lang = tencent_lang(query.target_lang.as_deref(), true);

        let payload = json!({
            "header": {
                "fn": "auto_translation",
                "client_key": "browser-chrome-110.0.0",
                "client_version": "1.0.0"
            },
            "type": "plain",
            "model_category": "normal",
            "source": { "lang": source_lang, "text_list": [query.text] },
            "target": { "lang": target_lang }
        });

        let response = self
            .client
            .post(ENDPOINT)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Execution(format!(
                "Tencent request failed with status {status}: {body}"
            )));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Tencent JSON: {e}")))?;

        let ret_code = json["header"]["ret_code"].as_str().unwrap_or("");
        if ret_code != "succ" {
            return Err(ProviderError::Execution(format!(
                "Tencent translation failed: {ret_code}"
            )));
        }

        let translated: String = json["auto_translation"]
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
                "Empty translation result from Tencent".to_string(),
            ));
        }

        Ok(TranslateResult {
            original_text: query.text.clone(),
            translated_text: translated,
            detected_source_lang: json["src_lang"].as_str().map(str::to_string),
            target_lang: json["tgt_lang"]
                .as_str()
                .unwrap_or(target_lang.as_str())
                .to_string(),
            details: None,
            provider_name: "TencentFree".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::tencent_lang;

    #[test]
    fn maps_tencent_language_codes() {
        assert_eq!(tencent_lang(None, false), "auto");
        assert_eq!(tencent_lang(None, true), "zh");
        assert_eq!(tencent_lang(Some("auto"), false), "auto");
        assert_eq!(tencent_lang(Some("zh-CN"), false), "zh");
        assert_eq!(tencent_lang(Some("zh-TW"), true), "zh-TW");
        assert_eq!(tencent_lang(Some("en-US"), true), "en");
        assert_eq!(tencent_lang(Some("ja"), true), "jp");
        assert_eq!(tencent_lang(Some("ko"), true), "kr");
    }
}
