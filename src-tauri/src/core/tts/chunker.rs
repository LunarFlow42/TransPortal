//! Sentence chunking, text defense filtering, and phonemization for Kokoro TTS.

use super::tokenizer::Tokenizer;
use misaki_rs::{G2P, Language};
use std::sync::OnceLock;

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("文本包含中文字符或无有效可发音英文字符，已触发优雅降级")]
    UnsupportedLanguageOrEmpty,

    #[error("TTS 模型资源未就绪或未安装：{0}")]
    AssetNotReady(String),

    #[error("ONNX 推理引擎失败：{0}")]
    Inference(String),

    #[error("音频打包失败：{0}")]
    AudioFormat(String),

    #[error("未知音色：{0}")]
    UnknownVoice(String),
}

pub struct SentenceChunker {
    g2p_us: G2P,
    g2p_gb: G2P,
}

impl SentenceChunker {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<SentenceChunker> = OnceLock::new();
        INSTANCE.get_or_init(|| SentenceChunker {
            g2p_us: G2P::new(Language::EnglishUS),
            g2p_gb: G2P::new(Language::EnglishGB),
        })
    }

    /// Process input text into phonemized, tokenized chunks of size <= 510.
    ///
    /// # Arguments
    /// * `text` - The input sentence or paragraph
    /// * `is_british` - Whether to use British English (en-gb) or American English (en-us)
    ///
    /// # Returns
    /// Vector of (token_ids, raw_phoneme_count), where raw_phoneme_count is in 1..=510.
    pub fn chunk_and_tokenize(
        &self,
        text: &str,
        is_british: bool,
    ) -> Result<Vec<(Vec<i64>, usize)>, TtsError> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(TtsError::UnsupportedLanguageOrEmpty);
        }

        // Defense filter: If text contains any CJK Unified Ideographs, immediately
        // trigger fallback to Web Speech to prevent swallowing Chinese characters.
        if trimmed
            .chars()
            .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
        {
            return Err(TtsError::UnsupportedLanguageOrEmpty);
        }

        // 1. Split text by natural sentence-ending punctuation.
        let sentences = split_sentences(trimmed);
        if sentences.is_empty() {
            return Err(TtsError::UnsupportedLanguageOrEmpty);
        }

        let g2p = if is_british {
            &self.g2p_gb
        } else {
            &self.g2p_us
        };
        let tokenizer = Tokenizer::global();

        let mut chunks = Vec::new();

        for sentence in sentences {
            let sentence = sentence.trim();
            if sentence.is_empty() {
                continue;
            }

            // 2. Run G2P on the sentence and resolve any spelled-out compound words (e.g. wargames).
            let (phonemes, tokens) = match g2p.g2p(sentence) {
                Ok(res) => res,
                Err(_) => continue,
            };
            let phonemes = resolve_sentence_phonemes(g2p, &phonemes, &tokens);
            let phonemes = phonemes.trim();
            if phonemes.is_empty() {
                continue;
            }

            // 3. Tokenize into IDs and check length.
            let (ids, count) = tokenizer.encode(phonemes);
            if count == 0 {
                continue;
            }

            // 4. If count <= 510, directly accept. If > 510, sub-chunk by word spaces.
            if count <= 510 {
                chunks.push((ids, count));
            } else {
                let sub_chunks = split_long_phonemes(phonemes, tokenizer);
                chunks.extend(sub_chunks);
            }
        }

        if chunks.is_empty() {
            return Err(TtsError::UnsupportedLanguageOrEmpty);
        }

        Ok(chunks)
    }
}

/// Splits text on common English sentence boundaries (`.`, `!`, `?`, `;`, `\n`).
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if ch == '.' || ch == '!' || ch == '?' || ch == ';' || ch == '\n' {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                sentences.push(trimmed.to_string());
            }
            current.clear();
        }
    }

    let remainder = current.trim();
    if !remainder.is_empty() {
        sentences.push(remainder.to_string());
    }

    sentences
}

/// Sub-chunks a long phoneme string (> 510 tokens) along word boundaries (spaces).
fn split_long_phonemes(phonemes: &str, tokenizer: &Tokenizer) -> Vec<(Vec<i64>, usize)> {
    let mut result = Vec::new();
    let mut current_chunk = String::new();

    for word in phonemes.split_whitespace() {
        let candidate = if current_chunk.is_empty() {
            word.to_string()
        } else {
            format!("{current_chunk} {word}")
        };

        let (_, count) = tokenizer.encode(&candidate);
        if count <= 510 {
            current_chunk = candidate;
        } else {
            if !current_chunk.is_empty() {
                let (ids, c) = tokenizer.encode(&current_chunk);
                if c > 0 {
                    result.push((ids, c));
                }
            }
            // If even a single word exceeds 510 phonemes, hard truncate it.
            let (_word_ids, word_count) = tokenizer.encode(word);
            if word_count <= 510 {
                current_chunk = word.to_string();
            } else {
                // Hard truncate to 510
                let truncated: String = word.chars().take(510).collect();
                let (t_ids, t_count) = tokenizer.encode(&truncated);
                if t_count > 0 {
                    result.push((t_ids, t_count));
                }
                current_chunk.clear();
            }
        }
    }

    if !current_chunk.is_empty() {
        let (ids, c) = tokenizer.encode(&current_chunk);
        if c > 0 {
            result.push((ids, c));
        }
    }

    result
}

/// Detects whether a token is unresolved by `misaki-rs` (either `None`/unknown marker
/// in 0.11+ without espeak, or legacy letter-by-letter spelling separated by `"  "`).
fn is_unresolved_word(word: &str, phonemes: Option<&str>, unk: &str) -> bool {
    let char_count = word.chars().filter(|c| c.is_alphabetic()).count();
    if char_count <= 1 {
        return false;
    }
    match phonemes {
        None => true,
        Some(p) => {
            let trimmed = p.trim();
            if trimmed.is_empty() || (!unk.is_empty() && trimmed.contains(unk)) {
                return true;
            }
            let parts: Vec<&str> = trimmed
                .split("  ")
                .filter(|s| !s.trim().is_empty())
                .collect();
            parts.len() == char_count
        }
    }
}

/// Checks whether a candidate G2P result for a word/fragment resolved cleanly.
fn is_resolved_phoneme(
    word: &str,
    phonemes: &str,
    tokens: &[misaki_rs::MToken],
    unk: &str,
) -> bool {
    let trimmed = phonemes.trim();
    if trimmed.is_empty() || (!unk.is_empty() && trimmed.contains(unk)) {
        return false;
    }
    if tokens
        .iter()
        .any(|t| t.text.chars().any(|c| c.is_alphabetic()) && t.phonemes.is_none())
    {
        return false;
    }
    !is_unresolved_word(word, Some(trimmed), unk)
}

/// Attempts to resolve an unresolved OOV token (like "wargames" or "frontend") by:
/// 1. CamelCase splitting ("WarGames" -> "War Games")
/// 2. Closed compound word splitting ("wargames" -> "war" + "games")
fn resolve_token_phonemes(g2p: &G2P, text: &str, current_phonemes: Option<&str>) -> String {
    let trimmed = text.trim();
    if !is_unresolved_word(trimmed, current_phonemes, &g2p.unk) {
        return current_phonemes.unwrap_or(text).to_string();
    }

    // 1. Try CamelCase split first (e.g. WarGames -> War Games, TypeScript -> Type Script)
    let mut camel_split = String::new();
    let chars: Vec<char> = trimmed.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if i > 0 && c.is_uppercase() && chars[i - 1].is_lowercase() {
            camel_split.push(' ');
        }
        camel_split.push(c);
    }
    if camel_split != trimmed
        && let Ok((res, tokens)) = g2p.g2p(&camel_split)
    {
        let res = res.trim();
        if is_resolved_phoneme(&camel_split, res, &tokens, &g2p.unk) {
            return res.to_string();
        }
    }

    // 2. Try compound word split (e.g. wargames -> war + games, frontend -> front + end)
    let lower = trimmed.to_lowercase();
    let byte_indices: Vec<usize> = lower.char_indices().map(|(idx, _)| idx).collect();
    let char_len = byte_indices.len();
    if char_len >= 4 {
        for &split_idx in &byte_indices[2..=(char_len - 2)] {
            let left = &lower[..split_idx];
            let right = &lower[split_idx..];
            if let (Ok((res_left, left_tokens)), Ok((res_right, right_tokens))) =
                (g2p.g2p(left), g2p.g2p(right))
            {
                let res_left = res_left.trim();
                let res_right = res_right.trim();
                if is_resolved_phoneme(left, res_left, &left_tokens, &g2p.unk)
                    && is_resolved_phoneme(right, res_right, &right_tokens, &g2p.unk)
                {
                    return format!("{res_left} {res_right}");
                }
            }
        }
    }

    // Keep original phonemes if present, or fall back to G2P's unknown marker.
    current_phonemes
        .map(str::to_string)
        .unwrap_or_else(|| g2p.unk.clone())
}

/// Reconstructs sentence phonemes, replacing any unresolved compound words with natural pronunciations.
fn resolve_sentence_phonemes(
    g2p: &G2P,
    full_phonemes: &str,
    tokens: &[misaki_rs::MToken],
) -> String {
    let has_unresolved = tokens
        .iter()
        .any(|t| is_unresolved_word(&t.text, t.phonemes.as_deref(), &g2p.unk));

    if !has_unresolved {
        return full_phonemes.to_string();
    }

    tokens
        .iter()
        .map(|t| {
            let p = if is_unresolved_word(&t.text, t.phonemes.as_deref(), &g2p.unk) {
                resolve_token_phonemes(g2p, &t.text, t.phonemes.as_deref())
            } else {
                t.phonemes.clone().unwrap_or_else(|| t.text.clone())
            };
            format!("{}{}", p, t.whitespace)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cjk_defense() {
        let chunker = SentenceChunker::global();
        let res = chunker.chunk_and_tokenize("你好世界", false);
        assert!(matches!(res, Err(TtsError::UnsupportedLanguageOrEmpty)));

        let mixed = chunker.chunk_and_tokenize("Hello 你好", false);
        assert!(matches!(mixed, Err(TtsError::UnsupportedLanguageOrEmpty)));
    }

    #[test]
    fn test_sentence_chunking() {
        let chunker = SentenceChunker::global();
        let res = chunker.chunk_and_tokenize("Hello world. This is Kokoro TTS!", false);
        assert!(res.is_ok());
        let chunks = res.unwrap();
        assert_eq!(chunks.len(), 2);
        for (_, count) in chunks {
            assert!(count > 0 && count <= 510);
        }
    }

    #[test]
    fn test_compound_word_resolution() {
        let chunker = SentenceChunker::global();
        let res = chunker.chunk_and_tokenize("wargames", false);
        assert!(res.is_ok());
        let chunks = res.unwrap();
        assert_eq!(chunks.len(), 1);
        let (ids, count) = &chunks[0];
        // Must contain natural phonemes, not 8 individual spelled-out letters
        assert!(*count > 0);
        assert!(*count < 25);
        assert!(!ids.is_empty());
    }

    #[test]
    fn test_wargames_in_sentence() {
        let g2p = G2P::new(Language::EnglishUS);
        let sentence = "The frontend and backend communicate via API for wargames.";
        let (full_phonemes, tokens) = g2p.g2p(sentence).unwrap();

        let resolved = resolve_sentence_phonemes(&g2p, &full_phonemes, &tokens);
        assert!(
            !resolved.contains(&g2p.unk),
            "unresolved OOV marker remained in: {resolved}"
        );
        assert!(resolved.contains("wˈɔɹ"), "got {resolved}");
        assert!(resolved.contains("ɡˈAmz"), "got {resolved}");
        assert!(resolved.contains("fɹˈʌnt"), "got {resolved}");
        assert!(resolved.contains("ˈɛnd"), "got {resolved}");
    }
}
