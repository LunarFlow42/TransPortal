//! Dictionary orchestration: an ordered, user-configurable list of lookup
//! sources (online by default, with an optional local ECCEDICT store).
//!
//! Every source is toggleable and reorderable from the settings UI. The
//! default order is Youdao → Google → Local ECCEDICT; disabled or unavailable
//! sources are skipped automatically, so the app works out of the box without
//! any local data.

use crate::dict::{DictEntry, DictSource, DictStore, ExampleSentence, PosDefinition};
use crate::provider::google::GoogleFreeProvider;
use crate::provider::{TranslateQuery, Translator};
use lru::LruCache;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

/// Maximum number of online lookup results kept in the in-memory LRU cache.
const CACHE_CAPACITY: usize = 256;

/// Whether a source needs the network or uses local data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Online,
    Local,
}

/// Source metadata + state for the settings UI.
#[derive(Debug, Clone, Serialize)]
pub struct SourceInfo {
    pub id: String,
    pub label: String,
    pub description: String,
    pub kind: SourceKind,
    pub enabled: bool,
    /// False for the local source when nothing has been imported.
    pub available: bool,
    /// Result of the most recent query (`None` = never queried this session).
    pub last_ok: Option<bool>,
    pub last_latency_ms: Option<u64>,
}

/// Runtime health of a single source.
#[derive(Debug, Clone, Default)]
struct SourceStats {
    last_ok: Option<bool>,
    last_latency_ms: Option<u64>,
}

/// Snapshot of dictionary availability, surfaced to the settings UI.
#[derive(Debug, Clone, Serialize)]
pub struct DictStatus {
    pub installed: bool,
    pub entry_count: usize,
    pub local_path: String,
}

/// One entry in the ordered source pipeline.
struct SourceEntry {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    kind: SourceKind,
    enabled: bool,
    source: Arc<dyn DictSource>,
    stats: Arc<Mutex<SourceStats>>,
}

/// Online dictionary source backed by Youdao's JSON endpoint.
pub struct YoudaoSource {
    client: Client,
}

impl YoudaoSource {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .build()
            .unwrap_or_default();
        Self { client }
    }

    async fn fetch(&self, word: &str) -> Option<DictEntry> {
        let response = self
            .client
            .get("https://dict.youdao.com/jsonapi")
            .query(&[("q", word)])
            .send()
            .await
            .ok()?;

        if !response.status().is_success() {
            return None;
        }
        let json: Value = response.json().await.ok()?;

        let ec_word = json.pointer("/ec/word/0");
        let simple_word = json.pointer("/simple/word/0");
        let ee_word = json.pointer("/ee/word/0");

        let phonetic = phone_of(ec_word)
            .or_else(|| phone_of(simple_word))
            .map(|phone| format!("/{phone}/"));

        let mut definitions = Vec::new();
        if let Some(trs) = ec_word.and_then(|w| w.get("trs")).and_then(Value::as_array) {
            for block in trs {
                let text = block
                    .pointer("/tr/0/l/i/0")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                push_definition(&mut definitions, text);
            }
        }

        let mut english_definitions = Vec::new();
        if let Some(trs) = ee_word.and_then(|w| w.get("trs")).and_then(Value::as_array) {
            for entry in trs {
                let pos = entry
                    .get("pos")
                    .and_then(Value::as_str)
                    .unwrap_or("def")
                    .to_string();
                let meanings: Vec<String> = entry
                    .get("tr")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|item| item.pointer("/l/i").and_then(Value::as_str))
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                if !meanings.is_empty() {
                    english_definitions.push(PosDefinition { pos, meanings });
                }
            }
        }

        let mut examples = Vec::new();
        if let Some(pairs) = json
            .pointer("/blng_sents_part/sentence-pair")
            .and_then(Value::as_array)
        {
            for pair in pairs.iter().take(3) {
                let en = pair.get("sentence").and_then(Value::as_str);
                let zh = pair
                    .get("sentence-translation")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if let Some(en) = en.filter(|s| !s.trim().is_empty()) {
                    examples.push(ExampleSentence {
                        en: en.to_string(),
                        zh: zh.to_string(),
                    });
                }
            }
        }

        if definitions.is_empty()
            && english_definitions.is_empty()
            && phonetic.is_none()
            && examples.is_empty()
        {
            return None;
        }

        Some(DictEntry {
            word: word.to_string(),
            phonetic,
            audio_url: Some(format!(
                "https://dict.youdao.com/dictvoice?audio={word}&type=2"
            )),
            definitions,
            english_definitions,
            exchange: exchange_of(ec_word),
            tags: Vec::new(),
            examples,
            source: "youdao".to_string(),
        })
    }
}

impl Default for YoudaoSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DictSource for YoudaoSource {
    fn id(&self) -> &'static str {
        "youdao"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn query(&self, word: &str) -> Result<Option<DictEntry>, String> {
        Ok(self.fetch(word).await)
    }
}

/// Online dictionary source backed by Google's `dt=bd` channel.
pub struct GoogleDictSource {
    google: GoogleFreeProvider,
}

impl GoogleDictSource {
    pub fn new() -> Self {
        Self {
            google: GoogleFreeProvider::new(),
        }
    }
}

impl Default for GoogleDictSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl DictSource for GoogleDictSource {
    fn id(&self) -> &'static str {
        "google"
    }

    fn is_available(&self) -> bool {
        true
    }

    async fn query(&self, word: &str) -> Result<Option<DictEntry>, String> {
        let query = TranslateQuery {
            text: word.to_string(),
            source_lang: Some("en".to_string()),
            target_lang: Some("zh-CN".to_string()),
        };
        let result = match self.google.translate(&query).await {
            Ok(result) => result,
            Err(_) => return Ok(None),
        };

        let mut definitions: Vec<PosDefinition> = result
            .details
            .unwrap_or_default()
            .into_iter()
            .map(|detail| PosDefinition {
                pos: detail.pos,
                meanings: detail.terms,
            })
            .collect();

        if definitions.is_empty() && !result.translated_text.trim().is_empty() {
            definitions.push(PosDefinition {
                pos: "def".to_string(),
                meanings: vec![result.translated_text],
            });
        }

        if definitions.is_empty() {
            return Ok(None);
        }

        Ok(Some(DictEntry {
            word: word.to_string(),
            phonetic: None,
            audio_url: Some(format!(
                "https://dict.youdao.com/dictvoice?audio={word}&type=2"
            )),
            definitions,
            english_definitions: Vec::new(),
            exchange: None,
            tags: Vec::new(),
            examples: Vec::new(),
            source: "google".to_string(),
        }))
    }
}

/// Lookup orchestrator over the ordered, toggleable source list.
pub struct LookupService {
    entries: RwLock<Vec<SourceEntry>>,
    local: Arc<DictStore>,
    local_path: PathBuf,
    /// In-memory LRU of resolved entries, keyed by lowercased word.
    cache: Mutex<LruCache<String, DictEntry>>,
}

impl LookupService {
    pub fn new(local: Arc<DictStore>, local_path: PathBuf) -> Self {
        let local_source: Arc<dyn DictSource> = local.clone();
        let entries = vec![
            SourceEntry {
                id: "youdao",
                label: "Youdao",
                description: "Chinese meanings, IPA, audio and bilingual examples.",
                kind: SourceKind::Online,
                enabled: true,
                source: Arc::new(YoudaoSource::new()),
                stats: Arc::new(Mutex::new(SourceStats::default())),
            },
            SourceEntry {
                id: "google",
                label: "Google",
                description: "Chinese meanings and part of speech via dt=bd.",
                kind: SourceKind::Online,
                enabled: true,
                source: Arc::new(GoogleDictSource::new()),
                stats: Arc::new(Mutex::new(SourceStats::default())),
            },
            SourceEntry {
                id: "local_eccedict",
                label: "Local ECCEDICT",
                description: "Optional offline redb dictionary (import required).",
                kind: SourceKind::Local,
                enabled: true,
                source: local_source,
                stats: Arc::new(Mutex::new(SourceStats::default())),
            },
        ];

        Self {
            entries: RwLock::new(entries),
            local,
            local_path,
            cache: Mutex::new(LruCache::new(
                NonZeroUsize::new(CACHE_CAPACITY).expect("non-zero cache capacity"),
            )),
        }
    }

    pub fn local(&self) -> &DictStore {
        self.local.as_ref()
    }

    /// Shared handle to the local store, for blocking import work.
    pub fn local_handle(&self) -> Arc<DictStore> {
        Arc::clone(&self.local)
    }

    /// Drop all cached lookups (e.g. after importing/changing sources).
    pub fn invalidate_cache(&self) {
        self.cache.lock().expect("lookup cache lock").clear();
    }

    /// Ordered source list with state, for the settings UI.
    pub fn sources(&self) -> Vec<SourceInfo> {
        let entries = self.entries.read().expect("source list lock");
        entries
            .iter()
            .map(|entry| {
                let stats = entry.stats.lock().expect("source stats lock");
                SourceInfo {
                    id: entry.id.to_string(),
                    label: entry.label.to_string(),
                    description: entry.description.to_string(),
                    kind: entry.kind,
                    enabled: entry.enabled,
                    available: entry.source.is_available(),
                    last_ok: stats.last_ok,
                    last_latency_ms: stats.last_latency_ms,
                }
            })
            .collect()
    }

    pub fn set_source_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        {
            let mut entries = self.entries.write().expect("source list lock");
            let entry = entries
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or_else(|| format!("unknown dictionary source: {id}"))?;
            entry.enabled = enabled;
        }
        self.invalidate_cache();
        Ok(())
    }

    /// Move a source up (`delta < 0`) or down (`delta > 0`) in the pipeline.
    pub fn move_source(&self, id: &str, delta: i32) -> Result<(), String> {
        {
            let mut entries = self.entries.write().expect("source list lock");
            let index = entries
                .iter()
                .position(|entry| entry.id == id)
                .ok_or_else(|| format!("unknown dictionary source: {id}"))?;
            let target = index as i32 + delta;
            if target < 0 || target >= entries.len() as i32 {
                return Ok(());
            }
            entries.swap(index, target as usize);
        }
        self.invalidate_cache();
        Ok(())
    }

    /// Replace the pipeline order with the given ids (must be complete).
    pub fn set_source_order(&self, ids: &[String]) -> Result<(), String> {
        {
            let mut entries = self.entries.write().expect("source list lock");
            if ids.len() != entries.len() {
                return Err("source order must list every source".to_string());
            }

            let mut ordered = Vec::with_capacity(entries.len());
            for id in ids {
                let index = entries
                    .iter()
                    .position(|entry| entry.id == id)
                    .ok_or_else(|| format!("unknown dictionary source: {id}"))?;
                ordered.push(entries.remove(index));
            }
            *entries = ordered;
        }
        self.invalidate_cache();
        Ok(())
    }

    /// Resolve a word through the enabled sources, in order, with caching.
    pub async fn lookup(&self, word: &str) -> Option<DictEntry> {
        let key = word.trim().to_lowercase();
        if key.is_empty() {
            return None;
        }

        if let Some(cached) = self
            .cache
            .lock()
            .expect("lookup cache lock")
            .get(&key)
            .cloned()
        {
            return Some(cached);
        }

        // Snapshot (source, stats) so no lock guard is held across `.await`.
        let pipeline: Vec<(Arc<dyn DictSource>, Arc<Mutex<SourceStats>>)> = {
            let entries = self.entries.read().expect("source list lock");
            entries
                .iter()
                .filter(|entry| entry.enabled && entry.source.is_available())
                .map(|entry| (Arc::clone(&entry.source), Arc::clone(&entry.stats)))
                .collect()
        };

        for (source, stats) in pipeline {
            let started = Instant::now();
            let result = source.query(word).await.ok().flatten();
            let elapsed_ms = started.elapsed().as_millis() as u64;
            {
                let mut stats = stats.lock().expect("source stats lock");
                stats.last_latency_ms = Some(elapsed_ms);
                stats.last_ok = Some(result.is_some());
            }

            if let Some(entry) = result {
                self.cache
                    .lock()
                    .expect("lookup cache lock")
                    .put(key, entry.clone());
                return Some(entry);
            }
        }

        None
    }

    pub fn status(&self) -> DictStatus {
        DictStatus {
            installed: self.local.is_installed(),
            entry_count: self.local.count().unwrap_or(0),
            local_path: self.local_path.to_string_lossy().to_string(),
        }
    }
}

/// Read a non-empty phone field from a Youdao word object.
fn phone_of(word: Option<&Value>) -> Option<String> {
    let word = word?;
    ["usphone", "ukphone"]
        .iter()
        .filter_map(|key| word.get(key).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

/// Read Youdao `wfs` word forms into a compact `name value / ...` string.
fn exchange_of(word: Option<&Value>) -> Option<String> {
    let wfs = word?.get("wfs")?;
    let forms: Vec<String> = match wfs {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let name = item.get("wf")?.get("name")?.as_str()?;
                let value = item.get("wf")?.get("value")?.as_str()?;
                Some(format!("{name} {value}"))
            })
            .collect(),
        Value::Object(_) => {
            let name = wfs.pointer("/wf/name").and_then(Value::as_str);
            let value = wfs.pointer("/wf/value").and_then(Value::as_str);
            match (name, value) {
                (Some(name), Some(value)) => vec![format!("{name} {value}")],
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    };

    if forms.is_empty() {
        None
    } else {
        Some(forms.join(" / "))
    }
}

/// Split a Youdao definition line into a POS tag and one or more meanings.
fn push_definition(definitions: &mut Vec<PosDefinition>, raw: &str) {
    let text = raw.trim();
    if text.is_empty() || text.starts_with("网络释义") {
        return;
    }

    let (pos, body) = split_pos(text);
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
fn split_pos(text: &str) -> (String, &str) {
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
