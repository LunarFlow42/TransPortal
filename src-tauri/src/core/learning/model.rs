//! Snapshots explicitly saved by the user, not evidence of successful recall.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefinitionKind {
    Dictionary,
    Translation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    Lookup,
    Dictionary,
    Generated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningContext {
    pub text: String,
    pub translation: Option<String>,
    pub source: ContextSource,
}

/// One chosen definition and, optionally, an example already on the word card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningDraft {
    pub language: String,
    pub lemma: String,
    pub surface: String,
    pub phonetic: Option<String>,
    pub pos: String,
    pub meaning: String,
    pub definition_kind: DefinitionKind,
    pub definition_source: String,
    pub context: Option<LearningContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningEntry {
    /// Decimal string so IDs remain lossless across the JavaScript IPC boundary.
    pub id: String,
    pub usage: LearningDraft,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollResult {
    pub entry: LearningEntry,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningPage {
    pub entries: Vec<LearningEntry>,
    pub total: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Json,
    Tsv,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LearningBackup {
    pub schema_version: u64,
    pub exported_at: u64,
    pub entries: Vec<LearningEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportResult {
    pub entry_count: u64,
}
