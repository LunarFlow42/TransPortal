//! Offline-first dictionary backed by an embedded [`redb`] key/value store.
//!
//! The store maps `word -> DictEntry` (JSON). On first launch the database is
//! seeded with a small built-in set so the word card works out of the box; a
//! full [ECDICT](https://github.com/skywind3000/ECDICT) CSV can be imported
//! with [`DictStore::import_csv_file`] for real, sub-millisecond lookups.
//!
//! Lookups never fabricate data: an unknown word returns `Ok(None)` and the
//! frontend falls back to the AI provider.

pub mod eccedict;
pub mod enrichment;
pub mod examples;
pub mod lookup;
#[cfg(test)]
mod test_support;

pub use lookup::{DictStatus, LookupService, SourceInfo};

use csv::StringRecord;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;
use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;

/// `word -> serialized DictEntry` table (legacy & seed entries).
const WORDS_TABLE: TableDefinition<&str, &str> = TableDefinition::new("words");

/// `first_word -> compressed serialized Vec<(word, StoredCompactEntry)>` table.
const BLOCKS_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("dict_blocks");

/// Metadata table for total word counts and versioning.
const META_TABLE: TableDefinition<&str, u64> = TableDefinition::new("dict_meta");
const INFO_TABLE: TableDefinition<&str, &str> = TableDefinition::new("dict_info");

/// Number of words per compressed block. 16 words per block achieves ~60-70 MB database file size
/// while keeping single-word decompression under 5 microseconds.
const BLOCK_SIZE: usize = 16;

fn compress_block(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(data)?;
    encoder.finish()
}

fn decompress_block(data: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(data);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out)?;
    Ok(out)
}

/// Number of built-in seed entries. Kept in sync with [`builtin_entries`].
const SEED_ENTRY_COUNT: usize = 6;

/// Convenience alias; callers stringify at the command boundary.
pub type DictResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PosDefinition {
    pub pos: String,
    pub meanings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExampleSentence {
    pub en: String,
    pub zh: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictEntry {
    pub word: String,
    pub phonetic: Option<String>,
    pub audio_url: Option<String>,
    /// Primary (Chinese) meanings, one entry per part of speech.
    pub definitions: Vec<PosDefinition>,
    /// Secondary English definitions (online Wiktionary source).
    #[serde(default)]
    pub english_definitions: Vec<PosDefinition>,
    /// ECDICT exchange forms, e.g. `p:transported/d:transported/3:transports`.
    pub exchange: Option<String>,
    pub tags: Vec<String>,
    pub examples: Vec<ExampleSentence>,
    /// Which source produced this entry, e.g. `online` or `local_eccedict`.
    pub source: String,
    #[serde(default)]
    pub collins: Option<u8>,
    #[serde(default)]
    pub oxford: bool,
    #[serde(default)]
    pub bnc: Option<u32>,
    #[serde(default)]
    pub frq: Option<u32>,
    /// Local additions never replace the identity/source of the primary entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_supplement: Option<LocalSupplement>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalSupplement {
    pub word: String,
    pub phonetic: Option<String>,
    pub fields: Vec<String>,
    pub definitions: Vec<PosDefinition>,
    pub english_definitions: Vec<PosDefinition>,
    pub examples: Vec<ExampleSentence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictEnrichmentInfo {
    pub revision: String,
    pub phonetics: usize,
    pub tags: usize,
}

/// Compact on-disk representation of a dictionary entry.
///
/// Strips redundant keys, omits default audio URLs, and stores raw strings
/// to shrink database size by ~85% (from >500 MB down to 60-80 MB).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCompactEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub d: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub o: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defs: Option<Vec<PosDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edefs: Option<Vec<PosDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<Vec<ExampleSentence>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub c: Option<u8>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ox: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub f: Option<u32>,
}

impl StoredCompactEntry {
    pub fn into_dict_entry(self, word: &str) -> DictEntry {
        let pos = self
            .o
            .as_deref()
            .map(primary_pos)
            .unwrap_or_else(|| "def".to_string());

        let definitions = if let Some(t) = self.t {
            let mut defs = Vec::new();
            for line in split_lines(&t) {
                push_definition(&mut defs, &line);
            }
            if defs.is_empty() {
                let meanings = split_lines(&t);
                if !meanings.is_empty() {
                    defs.push(PosDefinition {
                        pos: pos.clone(),
                        meanings,
                    });
                }
            }
            defs
        } else {
            self.defs.unwrap_or_default()
        };

        let english_definitions = if let Some(d) = self.d {
            let meanings = split_lines(&d);
            if meanings.is_empty() {
                Vec::new()
            } else {
                vec![PosDefinition { pos, meanings }]
            }
        } else {
            self.edefs.unwrap_or_default()
        };

        let tags = self
            .g
            .map(|g| {
                g.split_whitespace()
                    .map(|tag| tag.to_uppercase())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let audio_url = self.a.or_else(|| {
            Some(format!(
                "https://dict.youdao.com/dictvoice?audio={word}&type=2"
            ))
        });

        DictEntry {
            word: word.to_string(),
            phonetic: self.p,
            audio_url,
            definitions,
            english_definitions,
            exchange: self.x,
            tags,
            examples: self.e.unwrap_or_default(),
            source: "local_eccedict".to_string(),
            collins: self.c,
            oxford: self.ox,
            bnc: self.b,
            frq: self.f,
            local_supplement: None,
        }
    }

    pub fn from_dict_entry(entry: DictEntry) -> Self {
        let g = if entry.tags.is_empty() {
            None
        } else {
            Some(entry.tags.join(" "))
        };
        let e = if entry.examples.is_empty() {
            None
        } else {
            Some(entry.examples)
        };
        let defs = if entry.definitions.is_empty() {
            None
        } else {
            Some(entry.definitions)
        };
        let edefs = if entry.english_definitions.is_empty() {
            None
        } else {
            Some(entry.english_definitions)
        };

        let default_audio = format!(
            "https://dict.youdao.com/dictvoice?audio={}&type=2",
            entry.word
        );
        let a = entry.audio_url.filter(|url| url != &default_audio);

        StoredCompactEntry {
            p: entry.phonetic,
            t: None,
            d: None,
            o: None,
            defs,
            edefs,
            x: entry.exchange,
            g,
            e,
            a,
            c: entry.collins,
            ox: entry.oxford,
            b: entry.bnc,
            f: entry.frq,
        }
    }
}

/// Handle to the embedded dictionary database.
pub struct DictStore {
    db: Database,
}

impl DictStore {
    /// Open (creating if needed) the database at `path` and seed built-ins.
    pub fn open(path: impl AsRef<Path>) -> DictResult<Self> {
        let p = path.as_ref();
        let _ = Self::maybe_compact_database(p);
        let db = match Database::create(p) {
            Ok(db) => db,
            Err(e) => {
                // If the database was created with an incompatible older version of redb (or corrupted),
                // backup the outdated file and create a fresh one to prevent application launch crash.
                eprintln!(
                    "[DictStore] Failed to open database at {}: {e}. Resetting for compatibility...",
                    p.display()
                );
                let backup = p.with_extension("redb.old");
                let _ = std::fs::rename(p, &backup);
                Database::create(p)?
            }
        };
        let store = Self { db };
        store.seed_if_empty()?;
        Ok(store)
    }

    /// Check if database needs compaction and compact it in-place.
    ///
    /// If the file size is larger than 100 MB (indicating legacy uncompressed/uncompacted records),
    /// this function migrates all entries to compressed blocks in `BLOCKS_TABLE` in a fresh database file
    /// and atomically swaps it in, reducing disk usage from >500 MB down to 60~80 MB.
    pub fn maybe_compact_database(db_path: impl AsRef<Path>) -> DictResult<bool> {
        let path = db_path.as_ref();
        if !path.exists() {
            return Ok(false);
        }
        let Ok(metadata) = std::fs::metadata(path) else {
            return Ok(false);
        };
        // Only compact if file size exceeds threshold (100 MB)
        if metadata.len() < 100 * 1024 * 1024 {
            return Ok(false);
        }

        Self::compact_database_force(path)
    }

    /// Migrate all entries in `WORDS_TABLE` into compressed blocks in `BLOCKS_TABLE` in a new database file
    /// and atomically swap it in.
    pub fn compact_database_force(db_path: impl AsRef<Path>) -> DictResult<bool> {
        let path = db_path.as_ref();
        if !path.exists() {
            return Ok(false);
        }

        let temp_path = path.with_extension("compact.tmp");
        if temp_path.exists() {
            let _ = std::fs::remove_file(&temp_path);
        }

        {
            let old_db = Database::open(path)?;
            let old_read = old_db.begin_read()?;
            if let Ok(blocks) = old_read.open_table(BLOCKS_TABLE)
                && !blocks.is_empty()?
            {
                // This migration only understands legacy individual entries.
                // Never replace a current full dictionary with its seed table.
                return Ok(false);
            }
            let Ok(old_table) = old_read.open_table(WORDS_TABLE) else {
                return Ok(false);
            };

            let count = old_table.len()?;
            if count == 0 {
                return Ok(false);
            }

            let new_db = Database::create(&temp_path)?;
            let new_write = new_db.begin_write()?;
            let mut total_count = 0usize;
            {
                let mut blocks_table = new_write.open_table(BLOCKS_TABLE)?;
                let mut meta_table = new_write.open_table(META_TABLE)?;
                let mut current_block: Vec<(String, StoredCompactEntry)> =
                    Vec::with_capacity(BLOCK_SIZE);

                for item in old_table.iter()? {
                    let (k, v) = item?;
                    let word = k.value().to_string();
                    let raw = v.value();

                    let compact = if raw.starts_with("{\"word\":") {
                        if let Ok(entry) = serde_json::from_str::<DictEntry>(raw) {
                            StoredCompactEntry::from_dict_entry(entry)
                        } else {
                            continue;
                        }
                    } else if let Ok(compact) = serde_json::from_str::<StoredCompactEntry>(raw) {
                        compact
                    } else {
                        continue;
                    };

                    current_block.push((word, compact));
                    total_count += 1;

                    if current_block.len() >= BLOCK_SIZE {
                        let first_word = current_block[0].0.clone();
                        let raw_bytes = serde_json::to_vec(&current_block)?;
                        let compressed = compress_block(&raw_bytes)?;
                        blocks_table.insert(first_word.as_str(), compressed.as_slice())?;
                        current_block.clear();
                    }
                }

                if !current_block.is_empty() {
                    let first_word = current_block[0].0.clone();
                    let raw_bytes = serde_json::to_vec(&current_block)?;
                    let compressed = compress_block(&raw_bytes)?;
                    blocks_table.insert(first_word.as_str(), compressed.as_slice())?;
                }

                meta_table.insert("total_words", total_count as u64)?;
            }
            new_write.commit()?;
        }

        let bak_path = path.with_extension("redb.bak");
        if bak_path.exists() {
            let _ = std::fs::remove_file(&bak_path);
        }
        std::fs::rename(path, &bak_path)?;
        if let Err(e) = std::fs::rename(&temp_path, path) {
            let _ = std::fs::rename(&bak_path, path);
            return Err(Box::new(e));
        }
        let _ = std::fs::remove_file(&bak_path);

        Ok(true)
    }

    /// Seed the built-in entries only when the database is empty.
    fn seed_if_empty(&self) -> DictResult<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(WORDS_TABLE)?;
            if table.len()? == 0 {
                for (word, entry) in builtin_entries() {
                    let compact = StoredCompactEntry::from_dict_entry(entry);
                    let json = serde_json::to_string(&compact)?;
                    table.insert(word, json.as_str())?;
                }
            }
        }
        write.commit()?;
        Ok(())
    }

    /// Look up a word with a lightweight inflection fallback.
    pub fn lookup(&self, word: &str) -> DictResult<Option<DictEntry>> {
        let clean = word.trim().to_lowercase();
        if clean.is_empty() {
            return Ok(None);
        }

        let read = self.db.begin_read()?;

        // 1. Try compressed blocks table first (for full dictionary)
        if let Ok(blocks) = read.open_table(BLOCKS_TABLE)
            && blocks.len()? > 0
        {
            for candidate in lookup_candidates(&clean) {
                if let Some(guard) = blocks.range(..=candidate.as_str())?.next_back() {
                    let (_k, compressed) = guard?;
                    let raw = decompress_block(compressed.value())?;
                    let items: Vec<(String, StoredCompactEntry)> = serde_json::from_slice(&raw)?;
                    for (w, compact) in items {
                        if w == candidate {
                            return Ok(Some(compact.into_dict_entry(&candidate)));
                        }
                    }
                }
            }
            return Ok(None);
        }

        // 2. Fall back to individual words table (seed entries or legacy)
        if let Ok(table) = read.open_table(WORDS_TABLE) {
            for candidate in lookup_candidates(&clean) {
                if let Some(value) = table.get(candidate.as_str())? {
                    let raw = value.value();
                    let entry = if raw.starts_with("{\"word\":") {
                        serde_json::from_str::<DictEntry>(raw)?
                    } else if let Ok(compact) = serde_json::from_str::<StoredCompactEntry>(raw) {
                        compact.into_dict_entry(&candidate)
                    } else {
                        serde_json::from_str::<DictEntry>(raw)?
                    };
                    return Ok(Some(entry));
                }
            }
        }

        Ok(None)
    }

    /// Total number of entries currently stored (seed + imported).
    pub fn count(&self) -> DictResult<usize> {
        let read = self.db.begin_read()?;
        if let Ok(meta) = read.open_table(META_TABLE)
            && let Some(total) = meta.get("total_words")?
        {
            return Ok(total.value() as usize);
        }
        if let Ok(table) = read.open_table(WORDS_TABLE) {
            return Ok(table.len()? as usize);
        }
        Ok(0)
    }

    /// Whether the user has imported a real dictionary beyond the seed set.
    pub fn is_installed(&self) -> bool {
        self.count()
            .map(|count| count > SEED_ENTRY_COUNT)
            .unwrap_or(false)
    }

    /// Bulk-import an ECDICT-format CSV file into compressed blocks. Returns the number of entries.
    pub fn import_csv_file(&self, path: impl AsRef<Path>) -> DictResult<usize> {
        self.import_csv(path.as_ref(), None)
    }

    pub fn import_enriched_csv_file(
        &self,
        path: &Path,
        info: DictEnrichmentInfo,
    ) -> DictResult<usize> {
        self.import_csv(path, Some(info))
    }

    pub fn enrichment_info(&self) -> DictResult<Option<DictEnrichmentInfo>> {
        let read = self.db.begin_read()?;
        let Ok(table) = read.open_table(INFO_TABLE) else {
            return Ok(None);
        };
        table
            .get("enrichment")?
            .map(|value| serde_json::from_str(value.value()))
            .transpose()
            .map_err(Into::into)
    }

    fn import_csv(&self, path: &Path, info: Option<DictEnrichmentInfo>) -> DictResult<usize> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_path(path)?;

        // Block range lookup requires normalized lexical order. Read/validate
        // before touching the current database, and sort even custom CSV files.
        let mut sorted = BTreeMap::new();
        for record in reader.records() {
            let record = record?;
            let word = record.get(0).unwrap_or_default().trim().to_lowercase();
            if word.is_empty() || (word == "word" && record.get(1) == Some("phonetic")) {
                continue;
            }
            if record.len() < 11 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Dictionary row is missing required ECDICT columns",
                )
                .into());
            }
            sorted.insert(word, compact_entry_from_ecdict(&record));
        }
        if sorted.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Dictionary CSV contains no entries",
            )
            .into());
        }

        let write = self.db.begin_write()?;
        let mut total_count = 0usize;
        {
            let mut blocks_table = write.open_table(BLOCKS_TABLE)?;
            // Replacement, not append: repeated imports must not retain stale
            // blocks or words from a previous dictionary.
            blocks_table.retain(|_, _| false)?;
            let mut meta_table = write.open_table(META_TABLE)?;
            let mut current_block: Vec<(String, StoredCompactEntry)> =
                Vec::with_capacity(BLOCK_SIZE);

            for (word, compact) in sorted {
                current_block.push((word, compact));
                total_count += 1;

                if current_block.len() >= BLOCK_SIZE {
                    let first_word = current_block[0].0.clone();
                    let raw = serde_json::to_vec(&current_block)?;
                    let compressed = compress_block(&raw)?;
                    blocks_table.insert(first_word.as_str(), compressed.as_slice())?;
                    current_block.clear();
                }
            }

            if !current_block.is_empty() {
                let first_word = current_block[0].0.clone();
                let raw = serde_json::to_vec(&current_block)?;
                let compressed = compress_block(&raw)?;
                blocks_table.insert(first_word.as_str(), compressed.as_slice())?;
            }

            meta_table.insert("total_words", total_count as u64)?;
            let mut info_table = write.open_table(INFO_TABLE)?;
            if let Some(info) = info {
                let json = serde_json::to_string(&info)?;
                info_table.insert("enrichment", json.as_str())?;
            } else {
                info_table.remove("enrichment")?;
            }
        }
        write.commit()?;
        Ok(total_count)
    }
}

/// A pluggable word-lookup backend.
///
/// Implementations may be online (network) or local (redb). The
/// [`crate::core::dictionary::LookupService`] orchestrates them by priority.
#[async_trait::async_trait]
pub trait DictSource: Send + Sync {
    /// Stable identifier surfaced to the frontend, e.g. `local_eccedict`.
    fn id(&self) -> &'static str;

    /// Whether this source can be queried right now.
    fn is_available(&self) -> bool;

    /// Query a single word. `Ok(None)` means "not found here", not an error.
    async fn query(&self, word: &str) -> Result<Option<DictEntry>, String>;
}

#[async_trait::async_trait]
impl DictSource for DictStore {
    fn id(&self) -> &'static str {
        "local_eccedict"
    }

    fn is_available(&self) -> bool {
        self.is_installed()
    }

    async fn query(&self, word: &str) -> Result<Option<DictEntry>, String> {
        self.lookup(word).map_err(|e| e.to_string())
    }
}

/// Ordered candidate forms to try for a query.
fn lookup_candidates(word: &str) -> Vec<String> {
    let mut candidates = vec![word.to_string()];

    let mut push = |candidate: String| {
        if candidate.len() > 1 {
            candidates.push(candidate);
        }
    };

    if let Some(base) = word.strip_suffix("'s") {
        push(base.to_string());
    }
    if let Some(base) = word.strip_suffix("ies") {
        push(format!("{base}y"));
    }
    if let Some(base) = word.strip_suffix("es") {
        push(base.to_string());
    }
    if let Some(base) = word.strip_suffix('s') {
        push(base.to_string());
    }
    if let Some(base) = word.strip_suffix("ing") {
        push(base.to_string());
        push(format!("{base}e"));
    }
    if let Some(base) = word.strip_suffix("ed") {
        push(base.to_string());
        push(format!("{base}e"));
    }
    if let Some(base) = word.strip_suffix("ly") {
        push(base.to_string());
    }

    candidates.dedup();
    candidates
}

/// Compact ECDICT row mapper storing raw fields to avoid redundancy.
fn compact_entry_from_ecdict(record: &StringRecord) -> StoredCompactEntry {
    let phonetic = non_empty(record.get(1).map(str::to_string));
    let definition = non_empty(record.get(2).map(str::to_string));
    let translation = non_empty(record.get(3).map(str::to_string));
    let pos_field = non_empty(record.get(4).map(str::to_string));
    let tags = non_empty(record.get(7).map(str::to_string));
    let exchange = non_empty(record.get(10).map(str::to_string));
    let detail = record.get(11).unwrap_or_default();
    let examples = {
        let ex = examples_from_detail(detail);
        if ex.is_empty() { None } else { Some(ex) }
    };
    let audio = non_empty(record.get(12).map(str::to_string));

    StoredCompactEntry {
        p: phonetic,
        t: translation,
        d: definition,
        o: pos_field,
        defs: None,
        edefs: None,
        x: exchange,
        g: tags,
        e: examples,
        a: audio,
        c: record
            .get(5)
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|value| *value <= 5),
        ox: record.get(6) == Some("1"),
        b: record
            .get(8)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0),
        f: record
            .get(9)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0),
    }
}

#[cfg(test)]
fn dict_entry_from_ecdict(record: &StringRecord) -> DictEntry {
    let word = record.get(0).unwrap_or_default().trim().to_string();
    compact_entry_from_ecdict(record).into_dict_entry(&word)
}

/// Split an ECDICT multi-line field into trimmed, non-empty lines.
fn split_lines(value: &str) -> Vec<String> {
    value
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// Best-effort parser for the ECDICT `detail` JSON field (may hold examples).
fn examples_from_detail(detail: &str) -> Vec<ExampleSentence> {
    let detail = detail.trim();
    if detail.is_empty() {
        return Vec::new();
    }

    let Ok(value) = serde_json::from_str::<serde_json::Value>(detail) else {
        return Vec::new();
    };

    let items: Vec<&serde_json::Value> = match &value {
        serde_json::Value::Array(items) => items.iter().collect(),
        serde_json::Value::Object(map) => map.values().collect(),
        _ => Vec::new(),
    };

    let mut examples = Vec::new();
    for item in items {
        let en = item
            .get("en")
            .or_else(|| item.get("sentence"))
            .and_then(serde_json::Value::as_str);
        let Some(en) = en else { continue };
        if en.trim().is_empty() {
            continue;
        }
        let zh = item
            .get("zh")
            .or_else(|| item.get("translation"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        examples.push(ExampleSentence {
            en: en.to_string(),
            zh: zh.to_string(),
        });
    }

    examples.truncate(3);
    examples
}

/// Turn an ECDICT pos string such as `n:12/v:3` into `n./v.`.
fn primary_pos(raw: &str) -> String {
    if raw.trim().is_empty() {
        return "def".to_string();
    }

    raw.split('/')
        .filter_map(|part| part.split(':').next())
        .map(|pos| format!("{}.", pos.trim()))
        .collect::<Vec<_>>()
        .join("/")
}

/// Push one ECDICT translation line, extracting its leading POS token.
fn push_definition(definitions: &mut Vec<PosDefinition>, raw: &str) {
    let text = raw.trim();
    if text.is_empty() || text.starts_with("网络释义") {
        return;
    }

    let (pos, body) = split_pos_line(text);
    let mut meanings: Vec<String> = body
        .split(['；', ';'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();

    if meanings.is_empty() {
        meanings.push(body.to_string());
    }

    definitions.push(PosDefinition { pos, meanings });
}

/// Split a leading ASCII part-of-speech token (e.g. `n.`, `adj.`, `vt.`).
fn split_pos_line(text: &str) -> (String, &str) {
    let mut end = 0;
    for (index, ch) in text.char_indices() {
        if ch.is_ascii_alphabetic() || ch == '.' {
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }

    if end == 0 {
        return ("def".to_string(), text);
    }

    let (pos, rest) = text.split_at(end);
    let rest = rest.trim_start();
    if rest.is_empty() {
        ("def".to_string(), text)
    } else {
        (pos.to_string(), rest)
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

fn entry(
    word: &str,
    phonetic: &str,
    definitions: Vec<PosDefinition>,
    exchange: Option<&str>,
    tags: &[&str],
    examples: Vec<ExampleSentence>,
) -> DictEntry {
    DictEntry {
        word: word.to_string(),
        phonetic: Some(phonetic.to_string()),
        audio_url: Some(format!(
            "https://dict.youdao.com/dictvoice?audio={word}&type=2"
        )),
        definitions,
        english_definitions: Vec::new(),
        exchange: exchange.map(str::to_string),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        examples,
        source: "local_eccedict".to_string(),
        collins: None,
        oxford: false,
        bnc: None,
        frq: None,
        local_supplement: None,
    }
}

fn pos(pos: &str, meanings: &[&str]) -> PosDefinition {
    PosDefinition {
        pos: pos.to_string(),
        meanings: meanings.iter().map(|m| m.to_string()).collect(),
    }
}

fn example(en: &str, zh: &str) -> ExampleSentence {
    ExampleSentence {
        en: en.to_string(),
        zh: zh.to_string(),
    }
}

/// Small built-in seed set so the app is useful before ECDICT is imported.
fn builtin_entries() -> Vec<(&'static str, DictEntry)> {
    vec![
        (
            "portal",
            entry(
                "portal",
                "/ˈpɔːrtl/",
                vec![
                    pos("n.", &["大门，入口，传送门", "门户网站，入口网站"]),
                    pos("adj.", &["门静脉的，门静脉系统的"]),
                ],
                Some("s:portals"),
                &["CET6", "IELTS", "TOEFL"],
                vec![
                    example(
                        "TransPortal acts as a cognitive gateway across different languages.",
                        "TransPortal 是跨越不同语言的认知传送门。",
                    ),
                    example(
                        "They passed through an ancient mystical portal into the unknown.",
                        "他们穿过一座古老神秘的传送门，进入未知世界。",
                    ),
                ],
            ),
        ),
        (
            "cognitive",
            entry(
                "cognitive",
                "/ˈkɑːɡnətɪv/",
                vec![pos("adj.", &["认知的，感知的", "认识的，有认识能力的"])],
                None,
                &["CET6", "GRE", "TOEFL"],
                vec![
                    example(
                        "Deep reading expands our cognitive boundaries and perception.",
                        "深度阅读拓展了我们的认知边界与感知力。",
                    ),
                    example(
                        "Studies show bilingualism improves cognitive flexibility.",
                        "研究表明双语能力可以提升认知灵活性。",
                    ),
                ],
            ),
        ),
        (
            "transparent",
            entry(
                "transparent",
                "/trænsˈpærənt/",
                vec![pos("adj.", &["透明的，清澈的", "显然的，易懂的，坦率的"])],
                None,
                &["CET4", "CET6", "IELTS"],
                vec![
                    example(
                        "The window features a transparent frosted glass effect.",
                        "该窗口具有透明的毛玻璃效果。",
                    ),
                    example(
                        "Clear communication requires transparent and authentic intentions.",
                        "清晰的沟通需要透明和真实的心意。",
                    ),
                ],
            ),
        ),
        (
            "morphing",
            entry(
                "morphing",
                "/ˈmɔːrfɪŋ/",
                vec![
                    pos("n.", &["渐变，形态变换，变形动画"]),
                    pos("v.", &["改变，形变（morph 的现在分词）"]),
                ],
                Some("d:morphed/p:morphed/3:morphs"),
                &["Tech", "Graphics"],
                vec![example(
                    "The capsule smoothly morphs into an informative word card.",
                    "胶囊气泡平滑形变为信息丰富的单词卡片。",
                )],
            ),
        ),
        (
            "resilience",
            entry(
                "resilience",
                "/rɪˈzɪliəns/",
                vec![pos("n.", &["韧性，弹性", "恢复力，适应力"])],
                None,
                &["CET6", "GRE", "IELTS"],
                vec![example(
                    "Emotional resilience helps people navigate sudden career transitions.",
                    "心理韧性帮助人们度过突如其来的职业转变。",
                )],
            ),
        ),
        (
            "ephemeral",
            entry(
                "ephemeral",
                "/ɪˈfemərəl/",
                vec![pos("adj.", &["短暂的，瞬息即逝的，朝生暮死的"])],
                None,
                &["GRE", "SAT", "TOEFL"],
                vec![example(
                    "The ghost bubble offers an ephemeral reading companion without clutter.",
                    "幽灵伴读气泡提供了短暂且无干扰的伴读体验。",
                )],
            ),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::test_support::{TestDirectory, row};
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store() -> DictStore {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("transportal-test-{nanos}.redb"));
        let _ = std::fs::remove_file(&path);
        DictStore::open(&path).expect("open dictionary store")
    }

    #[test]
    fn seeds_builtin_entries() {
        assert_eq!(
            builtin_entries().len(),
            SEED_ENTRY_COUNT,
            "SEED_ENTRY_COUNT is out of sync with builtin_entries()"
        );
        let store = temp_store();
        let entry = store
            .lookup("portal")
            .unwrap()
            .expect("portal should exist");
        assert_eq!(entry.word, "portal");
        assert!(entry.phonetic.is_some());
        assert!(!entry.definitions.is_empty());
        assert!(!entry.examples.is_empty());
    }

    #[test]
    fn resolves_inflections() {
        let store = temp_store();
        let entry = store.lookup("portals").unwrap().expect("portals -> portal");
        assert_eq!(entry.word, "portal");
    }

    #[test]
    fn unknown_words_return_none() {
        let store = temp_store();
        assert!(store.lookup("zzzznotaword").unwrap().is_none());
    }

    #[test]
    fn parses_ecdict_row() {
        let mut record = csv::StringRecord::new();
        record.push_field("portal");
        record.push_field("pɔːrtl");
        record.push_field("a doorway\nan entrance");
        record.push_field("n. 入口\n门户");
        record.push_field("n:100");
        record.push_field("");
        record.push_field("");
        record.push_field("cet6 ielts");
        record.push_field("");
        record.push_field("");
        record.push_field("s:portals");
        record.push_field("");
        record.push_field("");

        let entry = dict_entry_from_ecdict(&record);
        assert_eq!(entry.word, "portal");
        assert_eq!(entry.phonetic.as_deref(), Some("pɔːrtl"));
        assert_eq!(entry.source, "local_eccedict");
        assert_eq!(entry.tags, vec!["CET6".to_string(), "IELTS".to_string()]);
        assert!(!entry.definitions.is_empty());
        assert!(!entry.english_definitions.is_empty());
    }

    #[test]
    fn parses_detail_examples() {
        let detail = r#"[{"en":"Hello world","zh":"你好世界"}]"#;
        let examples = examples_from_detail(detail);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].en, "Hello world");
        assert_eq!(examples[0].zh, "你好世界");

        assert!(examples_from_detail("not json").is_empty());
        assert!(examples_from_detail("").is_empty());
    }

    #[test]
    fn imported_metadata_survives_compression_and_reopen() {
        let dir = TestDirectory::new();
        let path = dir.0.join("test.redb");
        let csv = dir.csv("enriched.csv", &[row("hello", "n. 问候")]);
        {
            let store = DictStore::open(&path).unwrap();
            store
                .import_enriched_csv_file(
                    &csv,
                    DictEnrichmentInfo {
                        revision: "fixture".into(),
                        phonetics: 1,
                        tags: 1,
                    },
                )
                .unwrap();
            let entry = store.lookup("hello").unwrap().unwrap();
            assert_eq!(entry.collins, Some(3));
            assert!(entry.oxford);
            assert_eq!(entry.bnc, Some(100));
            assert_eq!(entry.frq, Some(200));
            assert!(entry.phonetic.unwrap().contains("美 /"));
        }
        let reopened = DictStore::open(&path).unwrap();
        assert_eq!(
            reopened.enrichment_info().unwrap().unwrap().revision,
            "fixture"
        );
        assert_eq!(reopened.lookup("hello").unwrap().unwrap().collins, Some(3));
    }

    #[test]
    fn repeated_imports_replace_blocks_without_retaining_stale_words_or_enrichment_flags() {
        let dir = TestDirectory::new();
        let store = DictStore::open(dir.0.join("test.redb")).unwrap();
        let first_rows: Vec<_> = (0..40)
            .map(|index| row(&format!("old{index:02}"), "n. 原词"))
            .collect();
        let first = dir.csv("first.csv", &first_rows);
        store
            .import_enriched_csv_file(
                &first,
                DictEnrichmentInfo {
                    revision: "first".into(),
                    phonetics: 40,
                    tags: 40,
                },
            )
            .unwrap();
        let second = dir.csv("second.csv", &[row("new", "n. 新词")]);
        assert_eq!(store.import_csv_file(second).unwrap(), 1);
        assert!(store.lookup("old39").unwrap().is_none());
        assert!(store.lookup("new").unwrap().is_some());
        assert!(store.enrichment_info().unwrap().is_none());
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn unsorted_case_insensitive_input_is_sorted_and_the_word_word_is_not_a_header() {
        let dir = TestDirectory::new();
        let store = DictStore::open(dir.0.join("test.redb")).unwrap();
        let mut rows: Vec<_> = (0..35)
            .rev()
            .map(|index| row(&format!("word{index:02}"), "n. 词"))
            .collect();
        rows.push(row("word", "n. 单词"));
        rows.push(row("WORD00", "n. 更新"));
        assert_eq!(
            store
                .import_csv_file(dir.csv("unsorted.csv", &rows))
                .unwrap(),
            36
        );
        assert_eq!(
            store.lookup("word").unwrap().unwrap().definitions[0].meanings,
            ["单词"]
        );
        assert_eq!(
            store.lookup("word00").unwrap().unwrap().definitions[0].meanings,
            ["更新"]
        );
        assert!(store.lookup("word34").unwrap().is_some());
    }

    #[test]
    fn invalid_import_preserves_the_working_dictionary_and_metadata() {
        let dir = TestDirectory::new();
        let store = DictStore::open(dir.0.join("test.redb")).unwrap();
        let valid = dir.csv("valid.csv", &[row("hello", "n. 问候")]);
        store
            .import_enriched_csv_file(
                &valid,
                DictEnrichmentInfo {
                    revision: "valid".into(),
                    phonetics: 1,
                    tags: 1,
                },
            )
            .unwrap();
        let bad = dir.0.join("bad.csv");
        std::fs::write(&bad, "word,phonetic\nhello,broken\n").unwrap();
        assert!(store.import_csv_file(&bad).is_err());
        std::fs::write(&bad, "word,phonetic\n").unwrap();
        assert!(store.import_csv_file(&bad).is_err());
        assert!(store.lookup("hello").unwrap().is_some());
        assert_eq!(store.count().unwrap(), 1);
        assert_eq!(store.enrichment_info().unwrap().unwrap().revision, "valid");
    }

    #[test]
    fn legacy_compaction_does_not_replace_a_current_dictionary_with_seed_data() {
        let dir = TestDirectory::new();
        let path = dir.0.join("test.redb");
        {
            let store = DictStore::open(&path).unwrap();
            let csv = dir.csv("current.csv", &[row("hello", "n. 问候")]);
            store.import_csv_file(csv).unwrap();
        }
        assert!(!DictStore::compact_database_force(&path).unwrap());
        let reopened = DictStore::open(&path).unwrap();
        assert!(reopened.lookup("hello").unwrap().is_some());
        assert_eq!(reopened.count().unwrap(), 1);
    }

    #[test]
    fn old_compact_entries_default_new_metadata_and_invalid_ranks_are_not_displayed() {
        let entry: StoredCompactEntry =
            serde_json::from_str(r#"{"p":"/test/","t":"n. 例子"}"#).unwrap();
        let entry = entry.into_dict_entry("test");
        assert_eq!(entry.collins, None);
        assert_eq!(entry.bnc, None);
        assert!(!entry.oxford);
        let mut invalid = row("hello", "n. 问候");
        invalid[5] = "99".into();
        invalid[8] = "0".into();
        invalid[9] = "-10".into();
        let entry =
            compact_entry_from_ecdict(&StringRecord::from(invalid)).into_dict_entry("hello");
        assert_eq!(entry.collins, None);
        assert_eq!(entry.bnc, None);
        assert_eq!(entry.frq, None);
    }

    #[test]
    #[ignore]
    fn test_benchmark_compaction() {
        let real_db = std::path::Path::new(
            r"C:\Users\Administrator\AppData\Roaming\com.transportal.app\transportal.redb",
        );
        if !real_db.exists() {
            println!("Real DB does not exist, skipping.");
            return;
        }

        let before_size = std::fs::metadata(real_db).unwrap().len();
        println!(
            "Before compaction DB size: {} bytes ({:.2} MB)",
            before_size,
            before_size as f64 / 1024.0 / 1024.0
        );

        let start = std::time::Instant::now();
        let compacted = DictStore::maybe_compact_database(real_db).expect("compact real db");
        let duration = start.elapsed();
        println!(
            "Compaction finished (compacted={}) in {:?}",
            compacted, duration
        );

        let after_size = std::fs::metadata(real_db).unwrap().len();
        println!(
            "After compaction DB size: {} bytes ({:.2} MB)",
            after_size,
            after_size as f64 / 1024.0 / 1024.0
        );

        // Open compacted DB and verify lookups
        let store = DictStore::open(real_db).expect("open compacted real db");
        let count = store.count().expect("count entries");
        println!("Total stored words: {}", count);
        assert!(count > 700_000, "count should be > 700k");

        let words_to_test = [
            "portal",
            "resilience",
            "transparent",
            "cognitive",
            "abandon",
            "hello",
            "run",
        ];
        for word in words_to_test {
            let t0 = std::time::Instant::now();
            let entry = store
                .lookup(word)
                .expect("lookup")
                .unwrap_or_else(|| panic!("missing word: {word}"));
            let lat = t0.elapsed();
            println!(
                "Lookup '{}' ({:?}): {} definitions, phonetic: {:?}",
                word,
                lat,
                entry.definitions.len(),
                entry.phonetic
            );
            assert_eq!(entry.word, word);
            assert!(!entry.definitions.is_empty());
        }

        // Test inflections: possessive inflection not in ECDICT falls back to base form
        let inflected = store
            .lookup("portal's")
            .expect("lookup")
            .expect("portal's -> portal");
        assert_eq!(inflected.word, "portal");
    }

    #[test]
    fn compacts_legacy_database_to_blocks() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("transportal-legacy-test-{nanos}.redb"));
        let _ = std::fs::remove_file(&path);

        // 1. Create legacy DB with WORDS_TABLE and uncompacted JSON
        {
            let db = Database::create(&path).expect("create test db");
            let write = db.begin_write().expect("begin write");
            {
                let mut table = write.open_table(WORDS_TABLE).expect("open words table");
                for (word, entry) in builtin_entries() {
                    let json = serde_json::to_string(&entry).expect("serialize legacy");
                    table.insert(word, json.as_str()).expect("insert legacy");
                }
            }
            write.commit().expect("commit");
        }

        // 2. Open as legacy DictStore and verify lookups work on old format
        {
            let store = DictStore {
                db: Database::open(&path).expect("open legacy db"),
            };
            assert_eq!(store.count().unwrap(), SEED_ENTRY_COUNT);
            let entry = store.lookup("portal").unwrap().expect("lookup portal");
            assert_eq!(entry.word, "portal");
        }

        // 3. Compact it in-place
        let compacted = DictStore::compact_database_force(&path).expect("compact legacy db");
        assert!(compacted);

        // 4. Re-open and verify lookups and count on compressed BLOCKS_TABLE
        {
            let store = DictStore::open(&path).expect("open compacted db");
            assert_eq!(store.count().unwrap(), SEED_ENTRY_COUNT);
            let entry = store
                .lookup("portal")
                .unwrap()
                .expect("lookup portal after compact");
            assert_eq!(entry.word, "portal");
            assert!(!entry.definitions.is_empty());
            assert_eq!(entry.source, "local_eccedict");

            let inflected = store.lookup("portals").unwrap().expect("lookup inflection");
            assert_eq!(inflected.word, "portal");
        }

        let _ = std::fs::remove_file(&path);
    }
}
