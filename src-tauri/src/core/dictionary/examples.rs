//! LLM-generated example sentences for the word card.

use super::ExampleSentence;
use crate::core::translation::openai::{OpenAiCompatibleProvider, OpenAiConfig};

const SYSTEM: &str = "You are an English dictionary assistant. \
Return ONLY a JSON array of 2 to 3 example sentences. Each item must be \
{\"en\": \"<English sentence>\", \"zh\": \"<Chinese translation>\"}. \
No prose, no markdown, no code fences.";

/// Generate example sentences for a word via the configured LLM.
///
/// Returns an empty list when no usable OpenAI-compatible key is configured, so
/// the caller can silently skip enhancement.
pub async fn generate(word: &str, config: OpenAiConfig) -> Result<Vec<ExampleSentence>, String> {
    if config.api_key.trim().is_empty() {
        return Ok(Vec::new());
    }

    let provider = OpenAiCompatibleProvider::new(config);
    let content = provider
        .chat(SYSTEM, &format!("Word: {word}"))
        .await
        .map_err(|e| e.to_string())?;

    Ok(parse_examples(&content))
}

/// Extract a JSON array of `{en, zh}` objects from an LLM reply.
fn parse_examples(content: &str) -> Vec<ExampleSentence> {
    let start = content.find('[');
    let end = content.rfind(']');
    let Some((start, end)) = start.zip(end) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }

    serde_json::from_str::<Vec<ExampleSentence>>(&content[start..=end]).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_examples() {
        let raw = "```json\n[{\"en\":\"A portal opened.\",\"zh\":\"传送门打开了。\"}]\n```";
        let examples = parse_examples(raw);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].en, "A portal opened.");
        assert_eq!(examples[0].zh, "传送门打开了。");

        assert!(parse_examples("no json here").is_empty());
    }
}
