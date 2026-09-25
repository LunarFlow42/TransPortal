//! Minimal AnkiConnect JSON-RPC client.
//!
//! AnkiConnect (Anki add-on `2055492159`) exposes a JSON-RPC-ish HTTP endpoint
//! on `http://127.0.0.1:8765`. Every request has the shape
//! `{ "action", "version": 6, "params": {...} }` and every response is
//! `{ "result": ..., "error": null | "..." }`.

use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// Parameters for adding a single note, sent from the frontend.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnkiNoteRequest {
    pub endpoint: String,
    pub deck_name: String,
    pub model_name: String,
    pub front: String,
    pub back: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub allow_duplicate: bool,
}

/// One JSON-RPC round trip. Returns the `result` field or the Anki error.
async fn call(endpoint: &str, action: &str, params: Value) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let payload = json!({ "action": action, "version": 6, "params": params });
    let response = client
        .post(endpoint)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("cannot reach AnkiConnect: {e}"))?;

    let body: Value = response
        .json()
        .await
        .map_err(|e| format!("invalid AnkiConnect response: {e}"))?;

    if let Some(error) = body.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }

    body.get("result")
        .cloned()
        .ok_or_else(|| "missing `result` in AnkiConnect response".to_string())
}

/// AnkiConnect API version, used as a connectivity probe.
pub async fn version(endpoint: &str) -> Result<i64, String> {
    let result = call(endpoint, "version", json!({})).await?;
    result
        .as_i64()
        .ok_or_else(|| "unexpected version response".to_string())
}

/// All available deck names.
pub async fn deck_names(endpoint: &str) -> Result<Vec<String>, String> {
    let result = call(endpoint, "deckNames", json!({})).await?;
    Ok(result
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default())
}

/// Add a note and return its Anki note id.
pub async fn add_note(request: &AnkiNoteRequest) -> Result<i64, String> {
    let params = json!({
        "note": {
            "deckName": request.deck_name,
            "modelName": request.model_name,
            "fields": {
                "Front": request.front,
                "Back": request.back,
            },
            "options": {
                "allowDuplicate": request.allow_duplicate,
            },
            "tags": request.tags,
        }
    });

    let result = call(&request.endpoint, "addNote", params).await?;
    result
        .as_i64()
        .ok_or_else(|| "AnkiConnect did not return a note id (duplicate?)".to_string())
}
