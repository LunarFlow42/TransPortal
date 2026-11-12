//! A separate, versioned redb database. Never resets or renames user data on error.

use super::model::{EnrollResult, LearningContext, LearningDraft, LearningEntry, LearningPage};
use super::{LearningError, LearningResult};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use std::path::Path;

pub(super) const SCHEMA_VERSION: u64 = 1;
const ENTRIES: TableDefinition<u64, &str> = TableDefinition::new("learning_entries");
const DEDUP: TableDefinition<&str, u64> = TableDefinition::new("learning_dedup");
const META: TableDefinition<&str, u64> = TableDefinition::new("learning_meta");

pub struct LearningStore {
    db: Database,
}

fn storage_error(error: impl std::fmt::Display) -> LearningError {
    LearningError::Storage(error.to_string())
}

fn text(value: &str, field: &str, max: usize, required: bool) -> LearningResult<String> {
    let value = value.trim();
    if (required && value.is_empty())
        || value.chars().count() > max
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(LearningError::Invalid(format!("Invalid {field}")));
    }
    Ok(value.to_string())
}

fn optional_text(value: Option<String>, field: &str, max: usize) -> LearningResult<Option<String>> {
    value
        .map(|value| text(&value, field, max, false))
        .transpose()
        .map(|value| value.filter(|value| !value.is_empty()))
}

fn normalize(mut draft: LearningDraft) -> LearningResult<LearningDraft> {
    draft.language = draft.language.trim().to_lowercase();
    if draft.language != "en" {
        return Err(LearningError::Invalid(
            "Only English word usages are supported at this stage".into(),
        ));
    }
    draft.lemma = text(&draft.lemma, "word", 128, true)?;
    if !draft.lemma.chars().any(|c| c.is_ascii_alphabetic())
        || !draft
            .lemma
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '\'' | '’' | '.'))
    {
        return Err(LearningError::Invalid("Invalid English word".into()));
    }
    draft.surface = text(&draft.surface, "surface form", 128, true)?;
    draft.pos = text(&draft.pos, "part of speech", 64, false)?;
    draft.meaning = text(&draft.meaning, "definition", 2_000, true)?;
    draft.definition_source = text(&draft.definition_source, "definition source", 128, true)?;
    draft.phonetic = optional_text(draft.phonetic, "phonetic", 256)?;
    draft.context = draft
        .context
        .map(|context| {
            Ok(LearningContext {
                text: text(&context.text, "example", 2_000, true)?,
                translation: optional_text(context.translation, "example translation", 2_000)?,
                source: context.source,
            })
        })
        .transpose()?;
    Ok(draft)
}

/// Exact-sense deduplication; sources and contexts do not change the identity.
fn dedup_key(draft: &LearningDraft) -> LearningResult<String> {
    let compact = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    serde_json::to_string(&(
        &draft.language,
        compact(&draft.lemma).to_lowercase(),
        compact(&draft.pos).to_lowercase(),
        compact(&draft.meaning),
    ))
    .map_err(storage_error)
}

fn decode(id: u64, json: &str) -> LearningResult<LearningEntry> {
    let entry: LearningEntry = serde_json::from_str(json).map_err(storage_error)?;
    if entry.id != id.to_string() {
        return Err(LearningError::Storage("Learning entry ID mismatch".into()));
    }
    Ok(entry)
}

impl LearningStore {
    pub fn open(path: impl AsRef<Path>) -> LearningResult<Self> {
        let path = path.as_ref();
        let existing = path.try_exists().map_err(storage_error)?;
        let db = if existing {
            Database::open(path).map_err(storage_error)?
        } else {
            Database::create(path).map_err(storage_error)?
        };

        if existing {
            // Check before starting a write transaction. Unknown/missing versions
            // need an explicit migration, never dictionary-style recreation.
            let read = db.begin_read().map_err(storage_error)?;
            let meta = read.open_table(META).map_err(storage_error)?;
            let version = meta
                .get("schema_version")
                .map_err(storage_error)?
                .ok_or_else(|| LearningError::Storage("Missing learning schema version".into()))?
                .value();
            if version != SCHEMA_VERSION {
                return Err(LearningError::UnsupportedSchema(version));
            }
            read.open_table(ENTRIES).map_err(storage_error)?;
            read.open_table(DEDUP).map_err(storage_error)?;
        } else {
            let write = db.begin_write().map_err(storage_error)?;
            {
                write.open_table(ENTRIES).map_err(storage_error)?;
                write.open_table(DEDUP).map_err(storage_error)?;
                let mut meta = write.open_table(META).map_err(storage_error)?;
                meta.insert("schema_version", SCHEMA_VERSION)
                    .map_err(storage_error)?;
                meta.insert("next_id", 0).map_err(storage_error)?;
            }
            write.commit().map_err(storage_error)?;
        }
        Ok(Self { db })
    }

    pub fn enroll(&self, draft: LearningDraft, now: u64) -> LearningResult<EnrollResult> {
        let draft = normalize(draft)?;
        let key = dedup_key(&draft)?;
        let write = self.db.begin_write().map_err(storage_error)?;
        let result = {
            let mut entries = write.open_table(ENTRIES).map_err(storage_error)?;
            let mut dedup = write.open_table(DEDUP).map_err(storage_error)?;
            let existing_id = dedup
                .get(key.as_str())
                .map_err(storage_error)?
                .map(|v| v.value());

            if let Some(id) = existing_id {
                let mut entry = {
                    let raw = entries.get(id).map_err(storage_error)?.ok_or_else(|| {
                        LearningError::Storage("Learning dedup index is inconsistent".into())
                    })?;
                    decode(id, raw.value())?
                };
                // Preserve the first saved sense/context and creation time.
                // A later explicit save may only fill previously missing data.
                let mut changed = false;
                if entry.usage.context.is_none() && draft.context.is_some() {
                    entry.usage.context = draft.context;
                    changed = true;
                }
                if entry.usage.phonetic.is_none() && draft.phonetic.is_some() {
                    entry.usage.phonetic = draft.phonetic;
                    changed = true;
                }
                if changed {
                    entry.updated_at = now.max(entry.updated_at);
                    let json = serde_json::to_string(&entry).map_err(storage_error)?;
                    entries.insert(id, json.as_str()).map_err(storage_error)?;
                }
                EnrollResult {
                    entry,
                    created: false,
                }
            } else {
                let mut meta = write.open_table(META).map_err(storage_error)?;
                let next = meta
                    .get("next_id")
                    .map_err(storage_error)?
                    .ok_or_else(|| LearningError::Storage("Missing learning ID sequence".into()))?
                    .value()
                    .checked_add(1)
                    .ok_or_else(|| {
                        LearningError::Storage("Learning ID sequence exhausted".into())
                    })?;
                let entry = LearningEntry {
                    id: next.to_string(),
                    usage: draft,
                    created_at: now,
                    updated_at: now,
                };
                let json = serde_json::to_string(&entry).map_err(storage_error)?;
                entries.insert(next, json.as_str()).map_err(storage_error)?;
                dedup.insert(key.as_str(), next).map_err(storage_error)?;
                meta.insert("next_id", next).map_err(storage_error)?;
                EnrollResult {
                    entry,
                    created: true,
                }
            }
        };
        write.commit().map_err(storage_error)?;
        Ok(result)
    }

    pub fn list(&self, query: &str, offset: u64, limit: u32) -> LearningResult<LearningPage> {
        let query = text(query, "search query", 256, false)?.to_lowercase();
        let limit = limit.clamp(1, 100) as usize;
        let read = self.db.begin_read().map_err(storage_error)?;
        let table = read.open_table(ENTRIES).map_err(storage_error)?;
        let mut total = 0;
        let mut entries = Vec::new();
        for item in table.iter().map_err(storage_error)?.rev() {
            let (id, raw) = item.map_err(storage_error)?;
            let entry = decode(id.value(), raw.value())?;
            let usage = &entry.usage;
            let matches = query.is_empty()
                || [&usage.lemma, &usage.surface, &usage.meaning]
                    .iter()
                    .any(|value| value.to_lowercase().contains(&query))
                || usage.context.as_ref().is_some_and(|context| {
                    context.text.to_lowercase().contains(&query)
                        || context
                            .translation
                            .as_ref()
                            .is_some_and(|value| value.to_lowercase().contains(&query))
                });
            if matches {
                if total >= offset && entries.len() < limit {
                    entries.push(entry);
                }
                total += 1;
            }
        }
        Ok(LearningPage { entries, total })
    }

    pub fn delete(&self, id: &str) -> LearningResult<bool> {
        let id = id
            .parse::<u64>()
            .map_err(|_| LearningError::Invalid("Invalid learning entry ID".into()))?;
        let write = self.db.begin_write().map_err(storage_error)?;
        let deleted = {
            let mut entries = write.open_table(ENTRIES).map_err(storage_error)?;
            let entry = entries
                .get(id)
                .map_err(storage_error)?
                .map(|raw| decode(id, raw.value()))
                .transpose()?;
            if let Some(entry) = entry {
                let key = dedup_key(&entry.usage)?;
                write
                    .open_table(DEDUP)
                    .map_err(storage_error)?
                    .remove(key.as_str())
                    .map_err(storage_error)?;
                entries.remove(id).map_err(storage_error)?;
                true
            } else {
                false
            }
        };
        write.commit().map_err(storage_error)?;
        Ok(deleted)
    }

    pub fn snapshot(&self) -> LearningResult<Vec<LearningEntry>> {
        let read = self.db.begin_read().map_err(storage_error)?;
        let table = read.open_table(ENTRIES).map_err(storage_error)?;
        table
            .iter()
            .map_err(storage_error)?
            .map(|item| {
                let (id, raw) = item.map_err(storage_error)?;
                decode(id.value(), raw.value())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::learning::model::{ContextSource, DefinitionKind};
    use crate::core::learning::tests::TestDirectory;

    fn draft() -> LearningDraft {
        LearningDraft {
            language: "en".into(),
            lemma: "mitigate".into(),
            surface: "mitigated".into(),
            phonetic: Some("/ˈmɪtɪɡeɪt/".into()),
            pos: "v.".into(),
            meaning: "减轻、缓解".into(),
            definition_kind: DefinitionKind::Dictionary,
            definition_source: "local_eccedict".into(),
            context: Some(LearningContext {
                text: "Caching can mitigate the impact of network latency.".into(),
                translation: Some("缓存可以减轻网络延迟的影响。".into()),
                source: ContextSource::Dictionary,
            }),
        }
    }

    #[test]
    fn saves_and_reopens_exact_snapshot() {
        let dir = TestDirectory::new();
        let path = dir.path.join("learning.redb");
        let saved = {
            let store = LearningStore::open(&path).unwrap();
            assert!(store.list("", 0, 20).unwrap().entries.is_empty());
            store.enroll(draft(), 100).unwrap()
        };
        assert!(saved.created);
        let reopened = LearningStore::open(&path).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), vec![saved.entry]);
    }

    #[test]
    fn duplicate_preserves_first_context_source_and_timestamps() {
        let dir = TestDirectory::new();
        let store = LearningStore::open(dir.path.join("learning.redb")).unwrap();
        let first = store.enroll(draft(), 100).unwrap();
        let mut duplicate = draft();
        duplicate.lemma = "  MITIGATE  ".into();
        duplicate.definition_source = "youdao".into();
        duplicate.context.as_mut().unwrap().text = "A different example.".into();
        let second = store.enroll(duplicate, 200).unwrap();
        assert!(!second.created);
        assert_eq!(second.entry, first.entry);
        assert_eq!(store.list("", 0, 20).unwrap().total, 1);
    }

    #[test]
    fn duplicate_can_fill_a_missing_example_without_recreating_the_entry() {
        let dir = TestDirectory::new();
        let store = LearningStore::open(dir.path.join("learning.redb")).unwrap();
        let mut without_context = draft();
        without_context.context = None;
        without_context.phonetic = None;
        let first = store.enroll(without_context, 100).unwrap();
        let filled = store.enroll(draft(), 200).unwrap();
        assert!(!filled.created);
        assert_eq!(filled.entry.id, first.entry.id);
        assert_eq!(filled.entry.created_at, 100);
        assert_eq!(filled.entry.updated_at, 200);
        assert_eq!(filled.entry.usage, draft());
    }

    #[test]
    fn different_senses_remain_separate_and_list_is_searchable_and_paged() {
        let dir = TestDirectory::new();
        let store = LearningStore::open(dir.path.join("learning.redb")).unwrap();
        let first = store.enroll(draft(), 100).unwrap();
        let mut other = draft();
        other.meaning = "使温和".into();
        other.context = None;
        let second = store.enroll(other, 200).unwrap();
        let page = store.list("MITIGATE", 0, 1).unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.entries, vec![second.entry]);
        assert_eq!(store.list("", 1, 1).unwrap().entries, vec![first.entry]);
        assert_eq!(store.list("network", 0, 20).unwrap().total, 1);
        assert_eq!(store.list("缓存", 0, 20).unwrap().total, 1);
        assert!(store.list("", 100, 20).unwrap().entries.is_empty());
    }

    #[test]
    fn delete_removes_snapshot_and_dedup_index() {
        let dir = TestDirectory::new();
        let store = LearningStore::open(dir.path.join("learning.redb")).unwrap();
        let first = store.enroll(draft(), 100).unwrap();
        assert!(store.delete(&first.entry.id).unwrap());
        assert!(!store.delete(&first.entry.id).unwrap());
        assert!(store.snapshot().unwrap().is_empty());
        let resaved = store.enroll(draft(), 200).unwrap();
        assert!(resaved.created);
        assert_ne!(resaved.entry.id, first.entry.id);
    }

    #[test]
    fn invalid_drafts_do_not_write_any_data() {
        let dir = TestDirectory::new();
        let store = LearningStore::open(dir.path.join("learning.redb")).unwrap();
        let mut invalid = draft();
        invalid.meaning = "  ".into();
        assert!(store.enroll(invalid, 100).is_err());
        let mut invalid = draft();
        invalid.language = "ja".into();
        assert!(store.enroll(invalid, 100).is_err());
        let mut invalid = draft();
        invalid.context.as_mut().unwrap().text = "a".repeat(2_001);
        assert!(store.enroll(invalid, 100).is_err());
        assert!(store.snapshot().unwrap().is_empty());
    }

    #[test]
    fn concurrent_explicit_saves_are_deduplicated_transactionally() {
        let dir = TestDirectory::new();
        let store =
            std::sync::Arc::new(LearningStore::open(dir.path.join("learning.redb")).unwrap());
        let tasks: Vec<_> = (0..4)
            .map(|_| {
                let store = store.clone();
                std::thread::spawn(move || store.enroll(draft(), 100).unwrap().created)
            })
            .collect();
        let created = tasks
            .into_iter()
            .map(|task| task.join().unwrap())
            .filter(|created| *created)
            .count();
        assert_eq!(created, 1);
        assert_eq!(store.snapshot().unwrap().len(), 1);
    }

    #[test]
    fn unknown_schema_is_rejected_without_resetting_entries() {
        let dir = TestDirectory::new();
        let path = dir.path.join("learning.redb");
        let saved = {
            let store = LearningStore::open(&path).unwrap();
            let saved = store.enroll(draft(), 100).unwrap().entry;
            let write = store.db.begin_write().unwrap();
            write
                .open_table(META)
                .unwrap()
                .insert("schema_version", 99)
                .unwrap();
            write.commit().unwrap();
            saved
        };
        assert!(matches!(
            LearningStore::open(&path),
            Err(LearningError::UnsupportedSchema(99))
        ));
        let db = Database::open(&path).unwrap();
        let read = db.begin_read().unwrap();
        let table = read.open_table(ENTRIES).unwrap();
        assert_eq!(
            decode(1, table.get(1).unwrap().unwrap().value()).unwrap(),
            saved
        );
    }
}
