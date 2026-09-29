//! Bing free sentence translation (no API key required).
//!
//! Connects directly to Bing's high-speed web endpoint (`cn.bing.com/ttranslatev3`),
//! which is accessible in China without a proxy and powered by modern language models.
//! Automatically retrieves and caches session tokens (`IG`, `IID`, `token`, `key`).

use super::{ProviderError, TranslateQuery, TranslateResult, Translator};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const BASE_URL: &str = "https://cn.bing.com";
const TRANSLATOR_URL: &str = "https://cn.bing.com/translator";
const TRANSLATE_API: &str = "https://cn.bing.com/ttranslatev3?isVertical=1";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36";

#[derive(Clone, Debug)]
struct BingSession {
    ig: String,
    iid: String,
    key: u64,
    token: String,
    expires_at: Instant,
}

pub struct BingFreeProvider {
    client: Client,
    session: RwLock<Option<BingSession>>,
}

impl BingFreeProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(12))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            client,
            session: RwLock::new(None),
        }
    }

    /// Extract `(IG, IID, key, token)` from the Bing translator page HTML.
    fn parse_page_credentials(html: &str) -> Option<(String, String, u64, String, u64)> {
        // 1. IG:"A49FE2C8134E41E8AEF806922D747F4E"
        let ig = html
            .split("IG:\"")
            .nth(1)?
            .split('"')
            .next()?
            .trim()
            .to_string();
        if ig.is_empty() {
            return None;
        }

        // 2. data-iid="translator.5028"
        let iid = html
            .split("data-iid=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("translator.5028")
            .trim()
            .to_string();

        // 3. params_AbusePreventionHelper = [1790681808621,"wIuczqQySh9WnQaXd-saRB5cVm4BLtkd",3600000];
        let helper_part = html.split("params_AbusePreventionHelper").nth(1)?;
        let array_content = helper_part.split('[').nth(1)?.split(']').next()?;
        let mut items = array_content.split(',');
        let key = items.next()?.trim().parse::<u64>().ok()?;
        let token = items
            .next()?
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .to_string();
        let expiry = items
            .next()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(3_600_000);

        if token.is_empty() {
            return None;
        }

        Some((ig, iid, key, token, expiry))
    }

    /// Ensure valid session credentials, refreshing if needed.
    async fn get_session(&self, force_refresh: bool) -> Result<BingSession, ProviderError> {
        if !force_refresh {
            let read_lock = self.session.read().await;
            if let Some(ref session) = *read_lock
                && Instant::now() < session.expires_at
            {
                return Ok(session.clone());
            }
        }

        let mut write_lock = self.session.write().await;
        // Double-check after acquiring write lock
        if !force_refresh
            && let Some(ref session) = *write_lock
            && Instant::now() < session.expires_at
        {
            return Ok(session.clone());
        }

        let page_response = self
            .client
            .get(TRANSLATOR_URL)
            .header("Referer", BASE_URL)
            .send()
            .await?;

        if !page_response.status().is_success() {
            return Err(ProviderError::Execution(format!(
                "Failed to access Bing translator page (HTTP {})",
                page_response.status()
            )));
        }

        let html = page_response.text().await?;
        let (ig, iid, key, token, expiry_ms) =
            Self::parse_page_credentials(&html).ok_or_else(|| {
                ProviderError::Parse(
                    "Failed to extract Bing authentication credentials".to_string(),
                )
            })?;

        // Expire slightly earlier than server interval (default ~50 mins)
        let valid_duration = Duration::from_millis(expiry_ms.saturating_sub(600_000).max(60_000));
        let session = BingSession {
            ig,
            iid,
            key,
            token,
            expires_at: Instant::now() + valid_duration,
        };

        *write_lock = Some(session.clone());
        Ok(session)
    }

    async fn execute_translate(
        &self,
        text: &str,
        from_lang: &str,
        to_lang: &str,
        session: &BingSession,
    ) -> Result<TranslateResult, ProviderError> {
        let url = format!("{TRANSLATE_API}&IG={}&IID={}.1", session.ig, session.iid);
        let key_str = session.key.to_string();
        let form_data = [
            ("text", text),
            ("fromLang", from_lang),
            ("to", to_lang),
            ("token", session.token.as_str()),
            ("key", key_str.as_str()),
            ("tryFetchingGenderDebiasedTranslations", "true"),
        ];

        let response = self
            .client
            .post(&url)
            .header("Referer", TRANSLATOR_URL)
            .form(&form_data)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::Execution(format!(
                "Bing translation request failed ({status}): {body}"
            )));
        }

        let json: Value = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Bing JSON: {e}")))?;

        // Check for Bing error status code
        if let Some(status_code) = json.get("statusCode").and_then(Value::as_i64)
            && status_code != 200
        {
            let msg = json["errorMessage"].as_str().unwrap_or("unknown error");
            return Err(ProviderError::Execution(format!(
                "Bing returned error {status_code}: {msg}"
            )));
        }

        // Bing response structure:
        // [ { "detectedLanguage": { "language": "en" }, "translations": [ { "text": "...", "to": "zh-Hans" } ] } ]
        let first_item = json
            .as_array()
            .and_then(|arr| arr.first())
            .ok_or_else(|| ProviderError::Parse("Empty response array from Bing".into()))?;

        let translated = first_item["translations"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|t| t["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        if translated.trim().is_empty() {
            return Err(ProviderError::Parse(
                "Empty translation content from Bing".into(),
            ));
        }

        let detected = first_item["detectedLanguage"]["language"]
            .as_str()
            .map(str::to_string);

        Ok(TranslateResult {
            original_text: text.to_string(),
            translated_text: translated,
            detected_source_lang: detected,
            target_lang: to_lang.to_string(),
            details: None,
            provider_name: "BingFree".to_string(),
        })
    }
}

impl Default for BingFreeProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Map language codes to Bing's expected codes.
fn bing_lang(tag: Option<&str>, default: &str) -> String {
    let Some(tag) = tag.map(str::trim).filter(|s| !s.is_empty()) else {
        return default.to_string();
    };

    match tag.to_ascii_lowercase().as_str() {
        "auto" | "auto-detect" => "auto-detect".to_string(),
        "zh" | "zh-cn" | "zh-hans" | "zhchs" => "zh-Hans".to_string(),
        "zh-tw" | "zh-hk" | "zh-hant" | "zhcht" => "zh-Hant".to_string(),
        other => other.to_string(),
    }
}

#[async_trait]
impl Translator for BingFreeProvider {
    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError> {
        if query.text.trim().is_empty() {
            return Ok(TranslateResult {
                original_text: query.text.clone(),
                translated_text: String::new(),
                detected_source_lang: None,
                target_lang: query
                    .target_lang
                    .clone()
                    .unwrap_or_else(|| "zh-Hans".into()),
                details: None,
                provider_name: "BingFree".to_string(),
            });
        }

        let from = bing_lang(query.source_lang.as_deref(), "auto-detect");
        let to = bing_lang(query.target_lang.as_deref(), "zh-Hans");

        let session = self.get_session(false).await?;
        match self
            .execute_translate(&query.text, &from, &to, &session)
            .await
        {
            Ok(result) => Ok(result),
            Err(_) => {
                // If failed (token might have expired on server), refresh and retry once
                let fresh_session = self.get_session(true).await?;
                self.execute_translate(&query.text, &from, &to, &fresh_session)
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bing_lang_mapping() {
        assert_eq!(bing_lang(None, "zh-Hans"), "zh-Hans");
        assert_eq!(bing_lang(Some("auto"), "zh-Hans"), "auto-detect");
        assert_eq!(bing_lang(Some("zh-CN"), "auto-detect"), "zh-Hans");
        assert_eq!(bing_lang(Some("zh-TW"), "auto-detect"), "zh-Hant");
        assert_eq!(bing_lang(Some("en"), "zh-Hans"), "en");
        assert_eq!(bing_lang(Some("ja"), "zh-Hans"), "ja");
    }

    #[test]
    fn test_parse_page_credentials() {
        let sample_html = r#"
            <html>
            <script>
            var params_AbusePreventionHelper = [1790681808621,"test_token_abc_123",3600000];
            var IG:"A49FE2C8134E41E8AEF806922D747F4E";
            </script>
            <div data-iid="translator.5028"></div>
            </html>
        "#;
        let parsed = BingFreeProvider::parse_page_credentials(sample_html);
        assert!(parsed.is_some());
        let (ig, iid, key, token, expiry) = parsed.unwrap();
        assert_eq!(ig, "A49FE2C8134E41E8AEF806922D747F4E");
        assert_eq!(iid, "translator.5028");
        assert_eq!(key, 1790681808621);
        assert_eq!(token, "test_token_abc_123");
        assert_eq!(expiry, 3600000);
    }
}
