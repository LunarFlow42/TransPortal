//! Kokoro phoneme tokenizer mapping characters to token IDs.

use std::collections::HashMap;
use std::sync::OnceLock;

const TOKENIZER_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/tts/tokenizer.json"
));

#[derive(Debug, serde::Deserialize)]
struct TokenizerModel {
    vocab: HashMap<String, i64>,
}

#[derive(Debug, serde::Deserialize)]
struct TokenizerData {
    model: TokenizerModel,
}

pub struct Tokenizer {
    vocab: HashMap<char, i64>,
}

impl Tokenizer {
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<Tokenizer> = OnceLock::new();
        INSTANCE.get_or_init(|| {
            let data: TokenizerData = serde_json::from_str(TOKENIZER_JSON)
                .expect("built-in Kokoro tokenizer.json must be valid");
            let mut vocab = HashMap::new();
            for (key, id) in data.model.vocab {
                if let Some(ch) = key.chars().next() {
                    vocab.insert(ch, id);
                }
            }
            Tokenizer { vocab }
        })
    }

    /// Tokenizes an IPA phoneme sequence into token IDs.
    /// Returns the sequence of token IDs along with the raw phoneme count (excluding boundary 0s).
    pub fn encode(&self, phonemes: &str) -> (Vec<i64>, usize) {
        let mut ids = Vec::with_capacity(phonemes.len() + 2);
        // Special start token '$' -> 0
        ids.push(0);
        let mut raw_count = 0;
        for ch in phonemes.chars() {
            if let Some(&id) = self.vocab.get(&ch) {
                ids.push(id);
                raw_count += 1;
            }
        }
        // Special end token '$' -> 0
        ids.push(0);
        (ids, raw_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenizer_encoding() {
        let tok = Tokenizer::global();
        let (ids, count) = tok.encode("həlˈoʊ");
        assert_eq!(ids.first(), Some(&0));
        assert_eq!(ids.last(), Some(&0));
        assert_eq!(count, 6);
        assert_eq!(ids.len(), count + 2);
    }
}
