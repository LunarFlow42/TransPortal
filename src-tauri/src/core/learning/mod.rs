//! Explicit, offline usage collection. No capture listeners or review scheduler.

pub mod model;
mod store;

use model::{ContextSource, DefinitionKind, LearningBackup, LearningEntry};
pub use model::{EnrollResult, ExportFormat, ExportResult, LearningDraft, LearningPage};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use store::{LearningStore, SCHEMA_VERSION};

#[derive(Debug, thiserror::Error)]
pub enum LearningError {
    #[error("Learning collection unavailable: {0}")]
    Unavailable(String),
    #[error("{0}")]
    Invalid(String),
    #[error("Learning database error (the existing file was not reset): {0}")]
    Storage(String),
    #[error("Unsupported learning database version {0}; the existing file was not reset")]
    UnsupportedSchema(u64),
}

type LearningResult<T> = Result<T, LearningError>;

/// Always managed, even when initialization failed, so lookup can keep working.
#[derive(Clone)]
pub struct LearningService {
    store: Result<Arc<LearningStore>, String>,
}

fn now() -> LearningResult<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|error| LearningError::Invalid(error.to_string()))
}

impl LearningService {
    pub fn open(path: PathBuf) -> Self {
        let store = (|| {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            LearningStore::open(path)
                .map(Arc::new)
                .map_err(|error| error.to_string())
        })();
        Self { store }
    }

    pub fn unavailable(error: String) -> Self {
        Self { store: Err(error) }
    }

    fn store(&self) -> LearningResult<&LearningStore> {
        self.store
            .as_deref()
            .map_err(|error| LearningError::Unavailable(error.clone()))
    }

    pub fn enroll(&self, draft: LearningDraft) -> LearningResult<EnrollResult> {
        self.store()?.enroll(draft, now()?)
    }

    pub fn list(&self, query: &str, offset: u64, limit: u32) -> LearningResult<LearningPage> {
        self.store()?.list(query, offset, limit)
    }

    pub fn delete(&self, id: &str) -> LearningResult<bool> {
        self.store()?.delete(id)
    }

    /// Export only to a new user-selected file. Never truncate existing data.
    pub fn export(&self, path: &Path, format: ExportFormat) -> LearningResult<ExportResult> {
        let extension = match format {
            ExportFormat::Json => "json",
            ExportFormat::Tsv => "tsv",
        };
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        {
            return Err(LearningError::Invalid(format!(
                "Please choose a new .{extension} file"
            )));
        }
        let entries = self.store()?.snapshot()?;
        let entry_count = entries.len() as u64;
        let bytes = export_bytes(entries, format, now()?)?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                LearningError::Invalid(format!(
                    "Could not create export; existing files are never overwritten: {error}"
                ))
            })?;
        if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
            drop(file);
            // This is only the fresh file owned by this export, never user data.
            let _ = std::fs::remove_file(path);
            return Err(LearningError::Invalid(format!("Export failed: {error}")));
        }
        Ok(ExportResult { entry_count })
    }
}

/// Protect spreadsheet imports while retaining tabs, quotes and multiline text.
fn spreadsheet_cell(value: &str) -> String {
    if value.trim_start().starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

fn export_bytes(
    entries: Vec<LearningEntry>,
    format: ExportFormat,
    exported_at: u64,
) -> LearningResult<Vec<u8>> {
    match format {
        ExportFormat::Json => serde_json::to_vec_pretty(&LearningBackup {
            schema_version: SCHEMA_VERSION,
            exported_at,
            entries,
        })
        .map_err(|error| LearningError::Invalid(error.to_string())),
        ExportFormat::Tsv => {
            let mut writer = csv::WriterBuilder::new()
                .delimiter(b'\t')
                .from_writer(Vec::new());
            writer
                .write_record([
                    "Word",
                    "Phonetic",
                    "Definition",
                    "Context",
                    "PartOfSpeech",
                    "Surface",
                    "ContextTranslation",
                    "DefinitionKind",
                    "DefinitionSource",
                    "ContextSource",
                    "CreatedAt",
                    "UpdatedAt",
                ])
                .map_err(|error| LearningError::Invalid(error.to_string()))?;
            for entry in entries {
                let usage = &entry.usage;
                let context_source =
                    usage
                        .context
                        .as_ref()
                        .map_or("", |context| match context.source {
                            ContextSource::Lookup => "lookup",
                            ContextSource::Dictionary => "dictionary",
                            ContextSource::Generated => "generated",
                        });
                let definition_kind = match usage.definition_kind {
                    DefinitionKind::Dictionary => "dictionary",
                    DefinitionKind::Translation => "translation",
                };
                let created_at = entry.created_at.to_string();
                let updated_at = entry.updated_at.to_string();
                let fields = [
                    usage.lemma.as_str(),
                    usage.phonetic.as_deref().unwrap_or_default(),
                    &usage.meaning,
                    usage
                        .context
                        .as_ref()
                        .map_or("", |context| context.text.as_str()),
                    &usage.pos,
                    &usage.surface,
                    usage
                        .context
                        .as_ref()
                        .and_then(|context| context.translation.as_deref())
                        .unwrap_or_default(),
                    definition_kind,
                    &usage.definition_source,
                    context_source,
                    &created_at,
                    &updated_at,
                ];
                writer
                    .write_record(fields.map(spreadsheet_cell))
                    .map_err(|error| LearningError::Invalid(error.to_string()))?;
            }
            writer
                .into_inner()
                .map_err(|error| LearningError::Invalid(error.to_string()))
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    pub(super) struct TestDirectory {
        pub(super) path: PathBuf,
    }

    impl TestDirectory {
        pub(super) fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "transportal-learning-{}-{unique}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self { path }
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn draft() -> LearningDraft {
        LearningDraft {
            language: "en".into(),
            lemma: "portal".into(),
            surface: "portal".into(),
            phonetic: None,
            pos: "n.".into(),
            meaning: "入口\t\"传送门\"\n门户".into(),
            definition_kind: DefinitionKind::Dictionary,
            definition_source: "youdao".into(),
            context: Some(model::LearningContext {
                text: "=HYPERLINK(\"https://example.invalid\")".into(),
                translation: None,
                source: ContextSource::Generated,
            }),
        }
    }

    #[test]
    fn exports_complete_json_and_escaped_spreadsheet_safe_tsv() {
        let dir = TestDirectory::new();
        let service = LearningService::open(dir.path.join("learning.redb"));
        let saved = service.enroll(draft()).unwrap().entry;
        let json_path = dir.path.join("backup.json");
        assert_eq!(
            service
                .export(&json_path, ExportFormat::Json)
                .unwrap()
                .entry_count,
            1
        );
        let backup: LearningBackup =
            serde_json::from_slice(&std::fs::read(json_path).unwrap()).unwrap();
        assert_eq!(backup.schema_version, SCHEMA_VERSION);
        assert_eq!(backup.entries, vec![saved]);

        let tsv_path = dir.path.join("usages.tsv");
        service.export(&tsv_path, ExportFormat::Tsv).unwrap();
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .from_path(tsv_path)
            .unwrap();
        let row = reader.records().next().unwrap().unwrap();
        assert_eq!(row.get(2).unwrap(), draft().meaning);
        assert!(row.get(3).unwrap().starts_with("'=HYPERLINK"));
        assert_eq!(row.get(9).unwrap(), "generated");
    }

    #[test]
    fn exports_never_overwrite_existing_files_or_the_learning_database() {
        let dir = TestDirectory::new();
        let db_path = dir.path.join("learning.redb");
        let service = LearningService::open(db_path.clone());
        service.enroll(draft()).unwrap();
        let existing = dir.path.join("existing.json");
        std::fs::write(&existing, "user data").unwrap();
        assert!(service.export(&existing, ExportFormat::Json).is_err());
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "user data");
        assert!(service.export(&db_path, ExportFormat::Json).is_err());
        assert_eq!(service.list("", 0, 20).unwrap().total, 1);
    }

    #[test]
    fn damaged_database_is_preserved_and_service_fails_without_panicking() {
        let dir = TestDirectory::new();
        let path = dir.path.join("learning.redb");
        std::fs::write(&path, b"not a database").unwrap();
        let service = LearningService::open(path.clone());
        assert!(service.list("", 0, 20).is_err());
        assert!(service.enroll(draft()).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"not a database");
    }
}
