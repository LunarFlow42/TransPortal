//! Offline-first dictionary backed by an embedded [`redb`] key/value store.
//!
//! The store maps `word -> DictEntry` (JSON). On first launch the database is
//! seeded with a small built-in set so the word card works out of the box; a
//! full [ECDICT](https://github.com/skywind3000/ECDICT) CSV can be imported
//! with [`DictStore::import_csv_file`] for real, sub-millisecond lookups.
//!
//! Lookups never fabricate data: an unknown word returns `Ok(None)` and the
//! frontend falls back to the AI provider.

use csv::StringRecord;
use redb::{Database, ReadableTableMetadata, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `word -> serialized DictEntry` table.
const WORDS_TABLE: TableDefinition<'static, &str, &str> = TableDefinition::new("words");

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
}

/// Handle to the embedded dictionary database.
pub struct DictStore {
    db: Database,
}

impl DictStore {
    /// Open (creating if needed) the database at `path` and seed built-ins.
    pub fn open(path: impl AsRef<Path>) -> DictResult<Self> {
        let db = Database::create(path)?;
        let store = Self { db };
        store.seed_if_empty()?;
        Ok(store)
    }

    /// Seed the built-in entries only when the database is empty.
    fn seed_if_empty(&self) -> DictResult<()> {
        let write = self.db.begin_write()?;
        {
            let mut table = write.open_table(WORDS_TABLE)?;
            if table.len()? == 0 {
                for (word, entry) in builtin_entries() {
                    let json = serde_json::to_string(&entry)?;
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
        let table = read.open_table(WORDS_TABLE)?;

        for candidate in lookup_candidates(&clean) {
            if let Some(value) = table.get(candidate.as_str())? {
                let entry: DictEntry = serde_json::from_str(value.value())?;
                return Ok(Some(entry));
            }
        }

        Ok(None)
    }

    /// Total number of entries currently stored (seed + imported).
    pub fn count(&self) -> DictResult<usize> {
        let read = self.db.begin_read()?;
        let table = read.open_table(WORDS_TABLE)?;
        Ok(table.len()? as usize)
    }

    /// Whether the user has imported a real dictionary beyond the seed set.
    pub fn is_installed(&self) -> bool {
        self.count()
            .map(|count| count > SEED_ENTRY_COUNT)
            .unwrap_or(false)
    }

    /// Bulk-import an ECDICT-format CSV file. Returns the number of entries.
    pub fn import_csv_file(&self, path: impl AsRef<Path>) -> DictResult<usize> {
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_path(path)?;

        let write = self.db.begin_write()?;
        let mut count = 0usize;
        {
            let mut table = write.open_table(WORDS_TABLE)?;
            for record in reader.records() {
                let record = record?;
                let word = record.get(0).unwrap_or_default().trim().to_lowercase();
                if word.is_empty() {
                    continue;
                }
                let entry = dict_entry_from_ecdict(&record);
                let json = serde_json::to_string(&entry)?;
                table.insert(word.as_str(), json.as_str())?;
                count += 1;
            }
        }
        write.commit()?;
        Ok(count)
    }
}

/// A pluggable word-lookup backend.
///
/// Implementations may be online (network) or local (redb). The
/// [`crate::dictionary::LookupService`] orchestrates them by priority.
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

/// Minimal ECDICT row mapper.
///
/// Column layout: `word, phonetic, definition, translation, pos, collins,
/// oxford, tag, bnc, frq, exchange, detail, audio`.
fn dict_entry_from_ecdict(record: &StringRecord) -> DictEntry {
    let word = record.get(0).unwrap_or_default().trim().to_string();
    let phonetic = non_empty(record.get(1).map(str::to_string));
    let definition = record.get(2).unwrap_or_default();
    let translation = record.get(3).unwrap_or_default();
    let pos_field = record.get(4).unwrap_or_default();
    let exchange = non_empty(record.get(10).map(str::to_string));

    let pos = primary_pos(pos_field);

    let tags = record
        .get(7)
        .map(|raw| {
            raw.split_whitespace()
                .map(|tag| tag.to_uppercase())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Each ECDICT translation line usually carries its own POS prefix
    // ("n. 大门", "v. 跑"); split them so the card shows one clean badge per POS.
    let mut definitions = Vec::new();
    for line in split_lines(translation) {
        push_definition(&mut definitions, &line);
    }
    if definitions.is_empty() {
        let meanings = split_lines(translation);
        if !meanings.is_empty() {
            definitions.push(PosDefinition {
                pos: pos.clone(),
                meanings,
            });
        }
    }

    let english_meanings = split_lines(definition);
    let english_definitions = if english_meanings.is_empty() {
        Vec::new()
    } else {
        vec![PosDefinition {
            pos,
            meanings: english_meanings,
        }]
    };

    let examples = examples_from_detail(record.get(11).unwrap_or_default());

    let audio_url = non_empty(record.get(12).map(str::to_string)).or_else(|| {
        Some(format!("https://dict.youdao.com/dictvoice?audio={word}&type=2"))
    });

    DictEntry {
        word,
        phonetic,
        audio_url,
        definitions,
        english_definitions,
        exchange,
        tags,
        examples,
        source: "local_eccedict".to_string(),
    }
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
        audio_url: Some(format!("https://dict.youdao.com/dictvoice?audio={word}&type=2")),
        definitions,
        english_definitions: Vec::new(),
        exchange: exchange.map(str::to_string),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        examples,
        source: "local_eccedict".to_string(),
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
                vec![pos(
                    "adj.",
                    &["透明的，清澈的", "显然的，易懂的，坦率的"],
                )],
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
        let entry = store.lookup("portal").unwrap().expect("portal should exist");
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
}
