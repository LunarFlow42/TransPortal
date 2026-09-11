use crate::dict::{lookup_word, DictEntry};
use crate::provider::{
    google::GoogleFreeProvider,
    openai::{OpenAiCompatibleProvider, OpenAiConfig},
    StreamChunk, TranslateQuery, TranslateResult, Translator,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextIntent {
    Word,
    Sentence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentResult {
    pub intent: TextIntent,
    pub word_count: usize,
    pub char_count: usize,
    pub clean_text: String,
    pub is_pure_ascii: bool,
}

/// Fast heuristic text granularity detector (< 1ms)
/// Distinguishes between single words/short phrases (triggers Word Card)
/// and sentences/paragraphs (triggers Ghost Bubble).
#[tauri::command]
pub fn detect_intent(text: String) -> IntentResult {
    let clean = text.trim().to_string();
    let char_count = clean.chars().count();
    let is_pure_ascii = clean.is_ascii();

    // Check for sentence terminal markers or linebreaks
    let has_sentence_punctuation = clean.contains(['.', '?', '!', ';', '。', '？', '！', '；', '\n']);

    // Split by whitespace
    let words: Vec<&str> = clean.split_whitespace().collect();
    let word_count = words.len();

    let intent = if word_count == 0 {
        TextIntent::Word
    } else if has_sentence_punctuation {
        TextIntent::Sentence
    } else if word_count <= 2 && char_count <= 40 {
        // Single word or 2-word phrase without sentence punctuation
        TextIntent::Word
    } else {
        TextIntent::Sentence
    };

    IntentResult {
        intent,
        word_count,
        char_count,
        clean_text: clean,
        is_pure_ascii,
    }
}

/// Full translation command
#[tauri::command]
pub async fn translate_text(
    query: TranslateQuery,
    provider: Option<String>,
    openai_config: Option<OpenAiConfig>,
) -> Result<TranslateResult, String> {
    let provider_name = provider.unwrap_or_else(|| "google_free".to_string());

    let translator: Arc<dyn Translator> = match provider_name.as_str() {
        "openai" => {
            let cfg = openai_config.unwrap_or_default();
            Arc::new(OpenAiCompatibleProvider::new(cfg))
        }
        _ => Arc::new(GoogleFreeProvider::new()),
    };

    translator
        .translate(&query)
        .await
        .map_err(|e| e.to_string())
}

/// Streaming translation command using native Tauri v2 IPC Channel
#[tauri::command]
pub async fn translate_stream(
    query: TranslateQuery,
    provider: Option<String>,
    openai_config: Option<OpenAiConfig>,
    on_chunk: tauri::ipc::Channel<StreamChunk>,
) -> Result<(), String> {
    let provider_name = provider.unwrap_or_else(|| "google_free".to_string());

    let translator: Arc<dyn Translator> = match provider_name.as_str() {
        "openai" => {
            let cfg = openai_config.unwrap_or_default();
            Arc::new(OpenAiCompatibleProvider::new(cfg))
        }
        _ => Arc::new(GoogleFreeProvider::new()),
    };

    translator
        .translate_stream(&query, on_chunk)
        .await
        .map_err(|e| e.to_string())
}

/// High-speed local dictionary lookup (< 5ms response)
#[tauri::command]
pub fn lookup_dict(word: String) -> Result<Option<DictEntry>, String> {
    Ok(lookup_word(&word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_intent_word() {
        let res = detect_intent("portal".to_string());
        assert_eq!(res.intent, TextIntent::Word);
        assert_eq!(res.word_count, 1);

        let res2 = detect_intent("cognitive load".to_string());
        assert_eq!(res2.intent, TextIntent::Word);
        assert_eq!(res2.word_count, 2);
    }

    #[test]
    fn test_detect_intent_sentence() {
        let res = detect_intent("This is a cognitive portal for languages.".to_string());
        assert_eq!(res.intent, TextIntent::Sentence);

        let res2 = detect_intent("Hello world! How are you doing today?".to_string());
        assert_eq!(res2.intent, TextIntent::Sentence);
    }

    #[test]
    fn test_lookup_dict_builtin() {
        let entry = lookup_word("portal").expect("portal should be found");
        assert_eq!(entry.word, "portal");
        assert!(entry.phonetic.is_some());
        assert!(!entry.definitions.is_empty());
        assert!(!entry.examples.is_empty());
    }

    #[test]
    fn test_lookup_dict_inflections() {
        let entry = lookup_word("portals").expect("portals should match portal");
        assert_eq!(entry.word, "portal");
    }
}
