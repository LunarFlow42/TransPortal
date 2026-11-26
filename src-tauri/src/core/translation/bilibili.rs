//! Bilibili's public Index-Translate text endpoint. Its prompt and inference
//! settings follow the upstream Index-Translate `inference/llm/call_api.py`.

use super::{ProviderError, StreamChunk, TranslateQuery, TranslateResult, Translator};
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::{sync::LazyLock, time::Duration};

const ENDPOINT: &str = "https://index-translate.bilibili.com/v1/chat/completions";
const MODEL: &str = "Index-Translate-35B-A3B";
static CLIENT: LazyLock<Result<Client, reqwest::Error>> = LazyLock::new(|| {
    Client::builder()
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("TransPortal")
        .build()
});

pub(super) struct BilibiliProvider;

fn language_name(tag: &str) -> String {
    let tag = tag.trim().to_ascii_lowercase().replace('_', "-");
    match tag.as_str() {
        "zh-tw" | "zh-hk" | "zh-mo" | "zh-cht" => return "繁體中文".into(),
        _ if tag.starts_with("zh-hant") => return "繁體中文".into(),
        _ => {}
    }
    match tag.split('-').next().unwrap_or_default() {
        "zh" => "简体中文",
        "en" => "英语",
        "ja" => "日语",
        "ko" => "韩语",
        "fr" => "法语",
        "de" => "德语",
        "es" => "西班牙语",
        "ru" => "俄语",
        _ => return tag,
    }
    .into()
}

fn target_tag(query: &TranslateQuery) -> &str {
    query
        .target_lang
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty() && !tag.eq_ignore_ascii_case("auto"))
        .unwrap_or("zh-CN")
}

fn payload(query: &TranslateQuery) -> Value {
    let source = query
        .source_lang
        .as_deref()
        .map(str::trim)
        .filter(|tag| !tag.is_empty() && !tag.eq_ignore_ascii_case("auto"))
        .map(language_name)
        .unwrap_or_default();
    let target = language_name(target_tag(query));
    let prompt = format!(
        "请将以下{source}文本翻译为{target}，直接输出翻译结果，不要进行任何解释。\n\n{}",
        query.text.trim()
    );
    json!({
        "model": MODEL,
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0.0,
        "max_tokens": 4096,
        "stream": true,
        "chat_template_kwargs": { "enable_thinking": false }
    })
}

fn parse_error(message: &str) -> ProviderError {
    ProviderError::Parse(format!("Bilibili Index-Translate: {message}"))
}

async fn request(
    client: &Client,
    endpoint: &str,
    query: &TranslateQuery,
    mut on_text: impl FnMut(&str) -> Result<(), ProviderError> + Send,
) -> Result<String, ProviderError> {
    if query.text.trim().is_empty() {
        return Err(ProviderError::Execution(
            "Bilibili Index-Translate requires text".into(),
        ));
    }
    // Request SSE even for callers that want the complete translation (OCR,
    // replacement). This avoids waiting silently on the public gateway.
    let response = client.post(endpoint).json(&payload(query)).send().await?;
    match response.status() {
        StatusCode::TOO_MANY_REQUESTS => {
            return Err(ProviderError::RateLimit(
                "Bilibili Index-Translate (HTTP 429)".into(),
            ));
        }
        status if !status.is_success() => {
            return Err(ProviderError::Execution(format!(
                "Bilibili Index-Translate returned HTTP {status}"
            )));
        }
        _ => {}
    }

    let mut translated = String::new();
    let mut stream = response.bytes_stream().eventsource();
    while let Some(event) = stream.next().await {
        let event = event.map_err(|_| parse_error("invalid or interrupted response stream"))?;
        match event.data.trim() {
            "" => continue,
            "[DONE]" if translated.trim().is_empty() => {
                return Err(parse_error("empty translation"));
            }
            "[DONE]" => return Ok(translated.trim().to_owned()),
            data => {
                let chunk: Value = serde_json::from_str(data)
                    .map_err(|_| parse_error("invalid translation event"))?;
                if !chunk["error"].is_null() {
                    return Err(ProviderError::Execution(
                        "Bilibili Index-Translate reported a stream error".into(),
                    ));
                }
                let choice = &chunk["choices"][0];
                if choice["finish_reason"]
                    .as_str()
                    .is_some_and(|reason| reason != "stop")
                {
                    return Err(parse_error(
                        "translation did not finish normally; try a shorter selection",
                    ));
                }
                if let Some(text) = choice["delta"]["content"]
                    .as_str()
                    .filter(|text| !text.is_empty())
                {
                    if translated.len() + text.len() > 256 * 1024 {
                        return Err(parse_error("translation exceeds the response limit"));
                    }
                    translated.push_str(text);
                    on_text(text)?;
                }
            }
        }
    }
    Err(parse_error(
        "stream closed before the translation completed",
    ))
}

fn client() -> Result<&'static Client, ProviderError> {
    CLIENT.as_ref().map_err(|_| {
        ProviderError::Execution("Could not initialize the Bilibili translation client".into())
    })
}

#[async_trait]
impl Translator for BilibiliProvider {
    fn allows_automatic_fallback(&self) -> bool {
        false
    }

    async fn translate(&self, query: &TranslateQuery) -> Result<TranslateResult, ProviderError> {
        let translated_text = request(client()?, ENDPOINT, query, |_| Ok(())).await?;
        Ok(TranslateResult {
            original_text: query.text.clone(),
            translated_text,
            detected_source_lang: query.source_lang.clone(),
            target_lang: target_tag(query).into(),
            details: None,
            provider_name: format!("Bilibili ({MODEL})"),
        })
    }

    async fn translate_stream(
        &self,
        query: &TranslateQuery,
        channel: tauri::ipc::Channel<StreamChunk>,
    ) -> Result<(), ProviderError> {
        let send = |text: String, is_done, error| {
            channel
                .send(StreamChunk {
                    text,
                    is_done,
                    error,
                })
                .map_err(|error| ProviderError::Execution(error.to_string()))
        };
        match request(client()?, ENDPOINT, query, |text| {
            send(text.into(), false, None)
        })
        .await
        {
            Ok(_) => send(String::new(), true, None),
            Err(error) => {
                let _ = send(String::new(), true, Some(error.to_string()));
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    fn query() -> TranslateQuery {
        TranslateQuery {
            text: "Good morning.\nHow are you?".into(),
            source_lang: Some("auto".into()),
            target_lang: Some("zh-TW".into()),
        }
    }

    async fn mock_request(
        status: &str,
        body: &str,
        mut on_text: impl FnMut(&str) + Send,
    ) -> Result<String, ProviderError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = BufReader::new(socket);
            let mut line = String::new();
            socket.read_line(&mut line).await.unwrap();
            assert_eq!(line, "POST /v1/chat/completions HTTP/1.1\r\n");
            let mut length = None;
            loop {
                line.clear();
                assert_ne!(socket.read_line(&mut line).await.unwrap(), 0);
                if line == "\r\n" {
                    break;
                }
                let line = line.to_ascii_lowercase();
                assert!(!line.starts_with("authorization:"));
                if let Some(value) = line.strip_prefix("content-length:") {
                    length = Some(value.trim().parse::<usize>().unwrap());
                }
            }
            let mut body = vec![0; length.unwrap()];
            socket.read_exact(&mut body).await.unwrap();
            let actual: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(actual["model"], MODEL);
            assert_eq!(actual["stream"], true);
            assert_eq!(actual["chat_template_kwargs"]["enable_thinking"], false);
            assert_eq!(
                actual["messages"],
                json!([{
                    "role": "user",
                    "content": "请将以下文本翻译为繁體中文，直接输出翻译结果，不要进行任何解释。\n\nGood morning.\nHow are you?"
                }])
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();
        let result = request(&client, &endpoint, &query(), |text| {
            on_text(text);
            Ok(())
        })
        .await;
        server.await.unwrap();
        result
    }

    #[tokio::test]
    async fn streams_only_translation_content_and_returns_the_complete_text() {
        let mut chunks = Vec::new();
        let result = mock_request("200 OK", concat!(
            "data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"reasoning_content\":\"ignored\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"早上好\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"。\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"choices\":[]}\n\n",
            "data: [DONE]\n\n"
        ), |text| chunks.push(text.to_owned())).await.unwrap();
        assert_eq!(result, "早上好。");
        assert_eq!(chunks, ["早上好", "。"]);
    }

    #[tokio::test]
    async fn rejects_empty_truncated_failed_and_rate_limited_responses() {
        for body in [
            "data: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":\"length\"}]}\n\ndata: [DONE]\n\n",
        ] {
            assert!(matches!(
                mock_request("200 OK", body, |_| {}).await,
                Err(ProviderError::Parse(_))
            ));
        }
        assert!(matches!(
            mock_request("200 OK", "data: {\"error\":{}}\n\n", |_| {}).await,
            Err(ProviderError::Execution(_))
        ));
        assert!(matches!(
            mock_request("429 Too Many Requests", "", |_| {}).await,
            Err(ProviderError::RateLimit(_))
        ));
    }

    #[test]
    fn provider_is_registered_and_does_not_redirect_failed_translation_to_another_service() {
        assert!(
            super::super::registry()
                .iter()
                .any(|(id, _)| *id == "bilibili_index")
        );
        assert!(!super::super::create("bilibili_index", None).allows_automatic_fallback());
    }
}
