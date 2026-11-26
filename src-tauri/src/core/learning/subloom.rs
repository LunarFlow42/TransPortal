//! User-initiated import of SubLoom's versioned offline exchange file.

use super::model::{ContextSource, DefinitionKind, LearningContext, LearningDraft, VideoSource};
use super::{LearningError, LearningResult};
use serde::Deserialize;
use std::io::Read;
use std::path::Path;

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ENTRIES: usize = 2_000;

#[derive(Deserialize)]
struct Exchange {
    format: String,
    version: u64,
    entries: Vec<Row>,
}

#[derive(Deserialize)]
struct Row {
    word: String,
    meaning: String,
    phonetic: Option<String>,
    pos: String,
    definition_source: String,
    sentence: String,
    translation: Option<String>,
    video_url: String,
    timestamp_seconds: f64,
}

fn invalid(message: impl Into<String>) -> LearningError {
    LearningError::Invalid(message.into())
}

pub(super) fn validate_video(video: VideoSource) -> LearningResult<VideoSource> {
    let url = reqwest::Url::parse(&video.url).map_err(|_| invalid("Invalid video URL"))?;
    if video.url.len() > 4096
        || video.timestamp_ms >= 1_000_000_000_000
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid("Invalid video source"));
    }
    Ok(VideoSource {
        url: url.into(),
        ..video
    })
}

pub(super) fn read(path: &Path) -> LearningResult<Vec<LearningDraft>> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        return Err(invalid("Select a SubLoom exchange .json file"));
    }
    if !std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_BYTES)
    {
        return Err(invalid("SubLoom import requires a file of at most 8 MiB"));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| invalid(format!("Could not read SubLoom import: {error}")))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("SubLoom import exceeds 8 MiB"));
    }
    let exchange: Exchange = serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("Invalid SubLoom JSON: {error}")))?;
    if exchange.format != "subloom-transportal"
        || exchange.version != 1
        || exchange.entries.is_empty()
        || exchange.entries.len() > MAX_ENTRIES
    {
        return Err(invalid(
            "Unsupported SubLoom exchange format or entry count",
        ));
    }
    exchange
        .entries
        .into_iter()
        .map(|row| {
            if !row.timestamp_seconds.is_finite() || !(0.0..1e9).contains(&row.timestamp_seconds) {
                return Err(invalid("Invalid video timestamp"));
            }
            let (definition_kind, definition_source) = match row.definition_source.as_str() {
                "ai" => (DefinitionKind::Translation, "SubLoom · AI"),
                "dictionary" => (DefinitionKind::Dictionary, "SubLoom · Dictionary"),
                _ => return Err(invalid("Invalid SubLoom definition source")),
            };
            Ok(LearningDraft {
                language: "en".into(),
                lemma: row.word.to_ascii_lowercase(),
                surface: row.word,
                phonetic: row.phonetic,
                pos: row.pos,
                meaning: row.meaning,
                definition_kind,
                definition_source: definition_source.into(),
                context: Some(LearningContext {
                    text: row.sentence,
                    translation: row.translation,
                    source: ContextSource::Video,
                    video: Some(VideoSource {
                        url: row.video_url,
                        timestamp_ms: (row.timestamp_seconds * 1_000.0).round() as u64,
                    }),
                }),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::learning::{LearningService, tests::TestDirectory};
    use serde_json::json;

    fn row() -> serde_json::Value {
        json!({
            "word": "Hello", "meaning": "你好", "phonetic": "/həˈləʊ/", "pos": "interjection",
            "definition_source": "ai", "sentence": "Hello world.", "translation": "你好，世界。",
            "video_url": "https://www.youtube.com/watch?v=video001&t=12s", "timestamp_seconds": 12.125
        })
    }

    fn fixture(path: &Path, version: u64, rows: Vec<serde_json::Value>) {
        std::fs::write(
            path,
            serde_json::to_vec(&json!({
                "format": "subloom-transportal", "version": version, "entries": rows
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn imports_subtitle_snapshot_and_reimport_is_idempotent() {
        let dir = TestDirectory::new();
        let file = dir.path.join("subloom-transportal-v1.json");
        let service = LearningService::open(dir.path.join("learning.redb"));
        fixture(&file, 1, vec![row()]);
        let first = service.import_subloom(&file).unwrap();
        assert_eq!((first.created, first.duplicates), (1, 0));
        let second = service.import_subloom(&file).unwrap();
        assert_eq!((second.created, second.duplicates), (0, 1));
        let page = service.list("hello", 0, 20).unwrap();
        let context = page.entries[0].usage.context.as_ref().unwrap();
        assert_eq!(context.source, ContextSource::Video);
        assert_eq!(context.text, "Hello world.");
        assert_eq!(context.translation.as_deref(), Some("你好，世界。"));
        let video = context.video.as_ref().unwrap();
        assert_eq!(video.timestamp_ms, 12_125);
        assert_eq!(video.url, "https://www.youtube.com/watch?v=video001&t=12s");
    }

    #[test]
    fn rejects_invalid_urls_invalid_words_and_unknown_versions_without_partial_writes() {
        let dir = TestDirectory::new();
        let file = dir.path.join("subloom.json");
        let service = LearningService::open(dir.path.join("learning.redb"));
        fixture(&file, 2, vec![row()]);
        assert!(service.import_subloom(&file).is_err());
        let mut bad = row();
        bad["video_url"] = json!("javascript:alert(1)");
        fixture(&file, 1, vec![row(), bad]);
        assert!(service.import_subloom(&file).is_err());
        let mut bad = row();
        bad["word"] = json!("你好");
        fixture(&file, 1, vec![row(), bad]);
        assert!(service.import_subloom(&file).is_err());
        assert_eq!(service.list("", 0, 20).unwrap().total, 0);
    }
}
