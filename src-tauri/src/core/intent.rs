//! Word-versus-sentence classification of captured or typed text.

use crate::core::translation::lang::{DetectedLang, detect_language};
use serde::{Deserialize, Serialize};

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

/// Chinese and Japanese are written without spaces, so whitespace cannot
/// separate words there. Dictionary headwords rarely exceed four characters
/// (idioms included); longer runs are phrases or sentences to translate.
const MAX_UNSPACED_WORD_CHARS: usize = 4;

/// Fast heuristic text granularity detector (< 1ms).
///
/// Distinguishes single words / short phrases (word card) from sentences and
/// paragraphs (ghost bubble). Shared by the IPC command and the hotkey path.
pub fn classify_intent(text: &str) -> IntentResult {
    let clean = text.trim().to_string();
    let char_count = clean.chars().count();
    let is_pure_ascii = clean.is_ascii();

    let has_sentence_punctuation =
        clean.contains(['.', '?', '!', ';', '。', '？', '！', '；', '\n']);

    let words: Vec<&str> = clean.split_whitespace().collect();
    let word_count = words.len();
    let unspaced = matches!(
        detect_language(&clean),
        DetectedLang::Chinese | DetectedLang::Japanese
    );

    let intent = if word_count == 0 {
        TextIntent::Word
    } else if has_sentence_punctuation {
        TextIntent::Sentence
    } else if unspaced {
        if word_count == 1 && char_count <= MAX_UNSPACED_WORD_CHARS {
            TextIntent::Word
        } else {
            TextIntent::Sentence
        }
    } else if word_count <= 2 && char_count <= 40 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_intent_word() {
        let res = classify_intent("portal");
        assert_eq!(res.intent, TextIntent::Word);
        assert_eq!(res.word_count, 1);

        let res2 = classify_intent("cognitive load");
        assert_eq!(res2.intent, TextIntent::Word);
        assert_eq!(res2.word_count, 2);
    }

    #[test]
    fn test_detect_intent_sentence() {
        let res = classify_intent("This is a cognitive portal for languages.");
        assert_eq!(res.intent, TextIntent::Sentence);

        let res2 = classify_intent("Hello world! How are you doing today?");
        assert_eq!(res2.intent, TextIntent::Sentence);
    }

    #[test]
    fn test_detect_intent_unspaced_scripts() {
        // Whole unpunctuated Chinese phrases are one whitespace "word".
        for word in ["打", "你好", "取词", "一石二鸟"] {
            assert_eq!(classify_intent(word).intent, TextIntent::Word, "{word}");
        }
        for phrase in [
            "今天天气真不错",
            "计算机科学与技术",
            "你好 世界",
            "これは日本語です",
        ] {
            assert_eq!(
                classify_intent(phrase).intent,
                TextIntent::Sentence,
                "{phrase}"
            );
        }
    }
}
