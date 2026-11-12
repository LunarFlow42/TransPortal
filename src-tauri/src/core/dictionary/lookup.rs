//! Dictionary orchestration: an ordered, user-configurable list of lookup
//! sources (online by default, with an optional local ECCEDICT store).
//!
//! Every source is toggleable and reorderable from the settings UI. The
//! default order is Youdao → Google → Local ECCEDICT; disabled or unavailable
//! sources are skipped automatically, so the app works out of the box without
//! any local data.

use crate::core::dictionary::{
    DictEnrichmentInfo, DictEntry, DictSource, DictStore, ExampleSentence, LocalSupplement,
    PosDefinition,
};
use crate::core::translation::google::GoogleFreeProvider;
use crate::core::translation::{TranslateQuery, Translator, resolve_target_lang};
use lru::LruCache;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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
    pub enrichment: Option<DictEnrichmentInfo>,
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

type PipelineItem = (SourceKind, Arc<dyn DictSource>, Arc<Mutex<SourceStats>>);

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
        parse_youdao(word, &json)
    }
}

/// Build an entry from a Youdao `jsonapi` response.
///
/// English headwords carry `ec` (English→Chinese) and `ee` sections; Chinese
/// headwords carry `ce` (Chinese→English) instead. Without any definitions
/// the result is not a hit: pinyin or example sentences alone would show a
/// card with no meaning and suppress the machine-translation fallback.
fn parse_youdao(word: &str, json: &Value) -> Option<DictEntry> {
    let ec_word = json.pointer("/ec/word/0");
    let ce_word = json.pointer("/ce/word/0");
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
    if let Some(trs) = ce_word.and_then(|w| w.get("trs")).and_then(Value::as_array) {
        for block in trs {
            let Some(line) = block.pointer("/tr/0/l") else {
                continue;
            };
            let pos = line.get("pos").and_then(Value::as_str).unwrap_or("def");
            push_meaning(&mut definitions, pos, &ce_meaning(line));
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

    if definitions.is_empty() && english_definitions.is_empty() {
        return None;
    }

    let mut examples = Vec::new();
    if let Some(pairs) = json
        .pointer("/blng_sents_part/sentence-pair")
        .and_then(Value::as_array)
    {
        for pair in pairs.iter().take(3) {
            let sentence = pair.get("sentence").and_then(Value::as_str);
            let translation = pair
                .get("sentence-translation")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(sentence) = sentence.filter(|s| !s.trim().is_empty()) else {
                continue;
            };
            // For Chinese headwords the source sentence is the Chinese side.
            let (en, zh) = if has_han(sentence) {
                (translation, sentence)
            } else {
                (sentence, translation)
            };
            examples.push(ExampleSentence {
                en: en.to_string(),
                zh: zh.to_string(),
            });
        }
    }

    Some(DictEntry {
        word: word.to_string(),
        phonetic,
        audio_url: Some(youdao_audio_url(word)),
        definitions,
        english_definitions,
        exchange: exchange_of(ec_word),
        tags: Vec::new(),
        examples,
        source: "youdao".to_string(),
        collins: None,
        oxford: false,
        bnc: None,
        frq: None,
        local_supplement: None,
    })
}

/// Youdao pronunciation; its default English voice is near-silent for Chinese.
fn youdao_audio_url(word: &str) -> String {
    if has_han(word) {
        format!("https://dict.youdao.com/dictvoice?audio={word}&le=zh")
    } else {
        format!("https://dict.youdao.com/dictvoice?audio={word}&type=2")
    }
}

/// Join a `ce` line's text runs, e.g. `["", {"#text":"how"}, " ", {"#text":"do"}]`.
fn ce_meaning(line: &Value) -> String {
    line.get("i")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| {
                    part.as_str()
                        .or_else(|| part.get("#text").and_then(Value::as_str))
                })
                .collect::<String>()
        })
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Append `meaning`, merging it into the previous definition when the part of
/// speech repeats (Youdao lists each `ce` meaning as its own block).
fn push_meaning(definitions: &mut Vec<PosDefinition>, pos: &str, meaning: &str) {
    if meaning.is_empty() {
        return;
    }
    match definitions.last_mut() {
        Some(last) if last.pos == pos => last.meanings.push(meaning.to_string()),
        _ => definitions.push(PosDefinition {
            pos: pos.to_string(),
            meanings: vec![meaning.to_string()],
        }),
    }
}

fn has_han(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF))
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
            source_lang: None,
            // Chinese headwords look up English meanings, everything else Chinese.
            target_lang: Some(resolve_target_lang(word, Some("zh-CN"))),
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
            audio_url: Some(youdao_audio_url(word)),
            definitions,
            english_definitions: Vec::new(),
            exchange: None,
            tags: Vec::new(),
            examples: Vec::new(),
            source: "google".to_string(),
            collins: None,
            oxford: false,
            bnc: None,
            frq: None,
            local_supplement: None,
        }))
    }
}

/// Lookup orchestrator over the ordered, toggleable source list.
pub struct LookupService {
    entries: RwLock<Vec<SourceEntry>>,
    local: Arc<DictStore>,
    local_path: PathBuf,
    /// In-memory LRU of resolved entries, keyed by lowercased word.
    cache: Arc<Mutex<LruCache<String, DictEntry>>>,
    /// Prevent an in-flight lookup from repopulating a cache invalidated by an
    /// import or source setting change.
    revision: Arc<AtomicU64>,
    import_operation: Arc<tokio::sync::Mutex<()>>,
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
                description: "Enriched offline dictionary; also supplements online results when enabled.",
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
            cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(CACHE_CAPACITY).expect("non-zero cache capacity"),
            ))),
            revision: Arc::new(AtomicU64::new(0)),
            import_operation: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn local(&self) -> &DictStore {
        self.local.as_ref()
    }

    /// Shared handle to the local store, for blocking import work.
    pub fn local_handle(&self) -> Arc<DictStore> {
        Arc::clone(&self.local)
    }

    pub fn begin_import(&self) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
        Arc::clone(&self.import_operation)
            .try_lock_owned()
            .map_err(|_| "A dictionary import is already in progress".to_string())
    }

    /// Drop all cached lookups (e.g. after importing/changing sources).
    pub fn invalidate_cache(&self) {
        self.cache_invalidator()();
    }

    /// A worker owns this callback so a committed import still invalidates the
    /// reader cache even if its IPC caller went away before the reply.
    pub(super) fn cache_invalidator(&self) -> impl FnOnce() + Send + 'static {
        let cache = Arc::clone(&self.cache);
        let revision = Arc::clone(&self.revision);
        move || {
            let mut cache = cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            revision.fetch_add(1, Ordering::AcqRel);
            cache.clear();
        }
    }

    /// Ordered source list with state, for the settings UI.
    pub fn sources(&self) -> Vec<SourceInfo> {
        let entries = self
            .entries
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries
            .iter()
            .map(|entry| {
                let stats = entry
                    .stats
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            let mut entries = self
                .entries
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            let mut entries = self
                .entries
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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
            let mut entries = self
                .entries
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
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

    /// Source order selects the primary entry. Enabled local data can enrich an
    /// online hit without triggering another online provider or replacing its
    /// meanings, examples, headword or pronunciation audio.
    pub async fn lookup(&self, word: &str) -> Option<DictEntry> {
        let key = word.trim().to_lowercase();
        if key.is_empty() {
            return None;
        }

        let revision = {
            let mut cache = self
                .cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(cached) = cache.get(&key).cloned() {
                return Some(cached);
            }
            self.revision.load(Ordering::Acquire)
        };

        // Snapshot (source, stats) so no lock guard is held across `.await`.
        let pipeline: Vec<PipelineItem> = {
            let entries = self
                .entries
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            entries
                .iter()
                .filter(|entry| entry.enabled && entry.source.is_available())
                .map(|entry| {
                    (
                        entry.kind,
                        Arc::clone(&entry.source),
                        Arc::clone(&entry.stats),
                    )
                })
                .collect()
        };

        let local_stats = pipeline
            .iter()
            .find(|(kind, _, _)| *kind == SourceKind::Local)
            .map(|(_, _, stats)| Arc::clone(stats));
        for (kind, source, stats) in pipeline {
            let started = Instant::now();
            let result = source.query(word).await.ok().flatten();
            let elapsed_ms = started.elapsed().as_millis() as u64;
            {
                let mut stats = stats
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                stats.last_latency_ms = Some(elapsed_ms);
                stats.last_ok = Some(result.is_some());
            }

            if let Some(mut entry) = result {
                if kind == SourceKind::Online
                    && !has_han(word)
                    && let Some(stats) = &local_stats
                {
                    let local = self.local_handle();
                    let query = word.to_string();
                    let started = Instant::now();
                    let supplement =
                        tauri::async_runtime::spawn_blocking(move || local.lookup(&query))
                            .await
                            .ok()
                            .and_then(Result::ok)
                            .flatten();
                    {
                        let mut stats = stats
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        stats.last_ok = Some(supplement.is_some());
                        stats.last_latency_ms = Some(started.elapsed().as_millis() as u64);
                    }
                    if let Some(supplement) = supplement
                        && self.revision.load(Ordering::Acquire) == revision
                    {
                        entry = supplement_online(entry, supplement);
                    }
                }
                let mut cache = self
                    .cache
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if self.revision.load(Ordering::Acquire) == revision {
                    cache.put(key, entry.clone());
                }
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
            enrichment: self.local.enrichment_info().ok().flatten(),
        }
    }
}

fn canonical_forms(exchange: &str) -> Vec<(&str, &str)> {
    exchange
        .split('/')
        .filter_map(|part| {
            let (code, word) = part.split_once(':')?;
            let code = code.trim();
            let word = word.trim();
            (!word.is_empty()
                && matches!(code, "p" | "d" | "i" | "3" | "r" | "t" | "s" | "0" | "1"))
            .then_some((code, word))
        })
        .collect()
}

fn distinct_definitions(local: &[PosDefinition], online: &[PosDefinition]) -> Vec<PosDefinition> {
    let normalize = |value: &str| value.split_whitespace().collect::<Vec<_>>().join(" ");
    local
        .iter()
        .filter_map(|definition| {
            let meanings: Vec<String> = definition
                .meanings
                .iter()
                .filter(|meaning| {
                    !meaning.trim().is_empty()
                        && !online.iter().any(|existing| {
                            existing.pos.trim() == definition.pos.trim()
                                && existing
                                    .meanings
                                    .iter()
                                    .any(|value| normalize(value) == normalize(meaning))
                        })
                })
                .cloned()
                .collect();
            (!meanings.is_empty()).then(|| PosDefinition {
                pos: definition.pos.clone(),
                meanings,
            })
        })
        .collect()
}

/// Local meanings stay in a separately attributed section: dictionary senses
/// cannot be concatenated blindly with a provider's context-specific answer.
fn supplement_online(mut online: DictEntry, local: DictEntry) -> DictEntry {
    let same_word = online.word.trim().eq_ignore_ascii_case(local.word.trim());
    if !same_word
        && !local.exchange.as_deref().is_some_and(|exchange| {
            canonical_forms(exchange).iter().any(|(code, form)| {
                matches!(*code, "p" | "d" | "i" | "3" | "r" | "t" | "s")
                    && form.eq_ignore_ascii_case(online.word.trim())
            })
        })
    {
        // A suffix heuristic alone is not proof that two words have the same
        // meaning (e.g. hardly -> hard). Never enrich unrelated fallback hits.
        return online;
    }

    let mut supplement = LocalSupplement {
        word: local.word.clone(),
        phonetic: local.phonetic.clone(),
        ..Default::default()
    };
    let mut mark = |field: &str| supplement.fields.push(field.to_string());
    if same_word
        && let Some(phonetic) = local
            .phonetic
            .as_ref()
            .filter(|value| !value.trim().is_empty())
    {
        let current = online.phonetic.as_deref().unwrap_or_default();
        let paired = |value: &str| value.contains('英') && value.contains('美');
        if current.trim().is_empty() || (!paired(current) && paired(phonetic)) {
            online.phonetic = Some(phonetic.clone());
            mark("phonetic");
        }
    }
    if same_word
        && online
            .audio_url
            .as_ref()
            .is_none_or(|value| value.trim().is_empty())
        && let Some(audio) = local
            .audio_url
            .as_ref()
            .filter(|value| !value.trim().is_empty())
    {
        online.audio_url = Some(audio.clone());
        mark("audio_url");
    }
    let mut added_tags = false;
    for tag in &local.tags {
        let tag = tag.trim();
        if !tag.is_empty()
            && !online
                .tags
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(tag))
        {
            online.tags.push(tag.to_string());
            added_tags = true;
        }
    }
    if added_tags {
        mark("tags");
    }
    if let Some(local_exchange) = &local.exchange {
        let mut forms = canonical_forms(online.exchange.as_deref().unwrap_or_default());
        let mut added = false;
        for (code, form) in canonical_forms(local_exchange) {
            if !forms.iter().any(|(existing, _)| *existing == code) {
                forms.push((code, form));
                added = true;
            }
        }
        if added {
            online.exchange = Some(
                forms
                    .iter()
                    .map(|(code, form)| format!("{code}:{form}"))
                    .collect::<Vec<_>>()
                    .join("/"),
            );
            mark("exchange");
        }
    }
    if online.collins.is_none() && local.collins.is_some() {
        online.collins = local.collins;
        mark("collins");
    }
    if !online.oxford && local.oxford {
        online.oxford = true;
        mark("oxford");
    }
    if online.bnc.is_none() && local.bnc.is_some() {
        online.bnc = local.bnc;
        mark("bnc");
    }
    if online.frq.is_none() && local.frq.is_some() {
        online.frq = local.frq;
        mark("frq");
    }
    // End the mutable fields borrow before assigning the supplemental content.
    supplement.definitions = distinct_definitions(&local.definitions, &online.definitions);
    supplement.english_definitions =
        distinct_definitions(&local.english_definitions, &online.english_definitions);
    supplement.examples = local
        .examples
        .into_iter()
        .filter(|example| {
            !example.en.trim().is_empty()
                && !online.examples.iter().any(|existing| {
                    existing.en.trim() == example.en.trim()
                        && existing.zh.trim() == example.zh.trim()
                })
        })
        .collect();
    if !supplement.definitions.is_empty() {
        supplement.fields.push("definitions".into());
    }
    if !supplement.english_definitions.is_empty() {
        supplement.fields.push("english_definitions".into());
    }
    if !supplement.examples.is_empty() {
        supplement.fields.push("examples".into());
    }
    if !supplement.fields.is_empty() {
        online.local_supplement = Some(supplement);
    }
    online
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
                Some(youdao_form(name, value))
            })
            .collect(),
        Value::Object(_) => {
            let name = wfs.pointer("/wf/name").and_then(Value::as_str);
            let value = wfs.pointer("/wf/value").and_then(Value::as_str);
            match (name, value) {
                (Some(name), Some(value)) => vec![youdao_form(name, value)],
                _ => Vec::new(),
            }
        }
        _ => Vec::new(),
    };

    if forms.is_empty() {
        None
    } else {
        Some(forms.join("/"))
    }
}

/// Use the same exchange codes as ECCEDICT so both online and supplemented
/// forms can be rendered by the existing word-form controls.
fn youdao_form(name: &str, value: &str) -> String {
    let code = match name.trim() {
        "过去式" => "p",
        "过去分词" => "d",
        "现在分词" => "i",
        "第三人称单数" | "第三人称单数形式" => "3",
        "比较级" => "r",
        "最高级" => "t",
        "复数" => "s",
        "原形" => "0",
        _ => return format!("{name} {value}"),
    };
    format!("{code}:{}", value.trim())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dictionary::test_support::{TestDirectory, row};
    use serde_json::json;

    struct StaticSource {
        entry: Option<DictEntry>,
        calls: Arc<AtomicU64>,
    }

    struct DelayedSource {
        entered: Arc<tokio::sync::Notify>,
        resume: Arc<tokio::sync::Notify>,
        calls: Arc<AtomicU64>,
    }

    #[async_trait::async_trait]
    impl DictSource for DelayedSource {
        fn id(&self) -> &'static str {
            "delayed_fixture"
        }
        fn is_available(&self) -> bool {
            true
        }
        async fn query(&self, _: &str) -> Result<Option<DictEntry>, String> {
            if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            Ok(Some(online("hello")))
        }
    }

    #[async_trait::async_trait]
    impl DictSource for StaticSource {
        fn id(&self) -> &'static str {
            "fixture_online"
        }
        fn is_available(&self) -> bool {
            true
        }
        async fn query(&self, _: &str) -> Result<Option<DictEntry>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(self.entry.clone())
        }
    }

    fn online(word: &str) -> DictEntry {
        DictEntry {
            word: word.into(),
            phonetic: Some("/online/".into()),
            audio_url: Some("https://example.invalid/online.mp3".into()),
            definitions: vec![PosDefinition {
                pos: "n.".into(),
                meanings: vec!["在线释义".into()],
            }],
            english_definitions: vec![],
            exchange: None,
            tags: vec!["IELTS".into()],
            examples: vec![ExampleSentence {
                en: "An online example.".into(),
                zh: "在线例句。".into(),
            }],
            source: "fixture_online".into(),
            collins: None,
            oxford: false,
            bnc: None,
            frq: None,
            local_supplement: None,
        }
    }

    fn service(dir: &TestDirectory, import: bool) -> (LookupService, Arc<AtomicU64>) {
        let path = dir.0.join("test.redb");
        let store = Arc::new(DictStore::open(&path).unwrap());
        if import {
            let rows: Vec<_> = [
                "hello", "world", "word", "alpha", "beta", "gamma", "delta", "portal",
            ]
            .iter()
            .map(|word| row(word, "n. 本地释义"))
            .collect();
            store.import_csv_file(dir.csv("local.csv", &rows)).unwrap();
        }
        let service = LookupService::new(store, path);
        let calls = Arc::new(AtomicU64::new(0));
        {
            let mut sources = service.entries.write().unwrap();
            sources[0].source = Arc::new(StaticSource {
                entry: Some(online("hello")),
                calls: Arc::clone(&calls),
            });
            sources[1].enabled = false;
        }
        (service, calls)
    }

    #[test]
    fn online_primary_content_is_preserved_while_local_metadata_and_separate_meanings_are_added() {
        let primary = online("hello");
        let mut local = primary.clone();
        local.source = "local_eccedict".into();
        local.phonetic = Some("英 /həˈləʊ/ 美 /həˈloʊ/".into());
        local.collins = Some(4);
        local.oxford = true;
        local.bnc = Some(100);
        local.frq = Some(200);
        local.tags = vec!["ielts".into(), "CET4".into()];
        local.exchange = Some("s:hellos".into());
        local.definitions[0].meanings = vec!["本地释义".into()];
        local.examples.push(ExampleSentence {
            en: "A local example.".into(),
            zh: "本地例句。".into(),
        });
        let result = supplement_online(primary.clone(), local);
        assert_eq!(result.word, primary.word);
        assert_eq!(result.source, primary.source);
        assert_eq!(result.audio_url, primary.audio_url);
        assert_eq!(
            result.definitions[0].meanings,
            primary.definitions[0].meanings
        );
        assert_eq!(result.examples.len(), 1);
        assert_eq!(result.examples[0].en, primary.examples[0].en);
        assert!(result.phonetic.unwrap().contains("英 /"));
        assert_eq!(result.tags, ["IELTS", "CET4"]);
        assert_eq!(result.exchange.as_deref(), Some("s:hellos"));
        assert_eq!(result.collins, Some(4));
        assert_eq!(result.bnc, Some(100));
        let supplement = result.local_supplement.unwrap();
        assert_eq!(supplement.definitions[0].meanings, ["本地释义"]);
        assert_eq!(supplement.examples.len(), 1);
        assert!(supplement.fields.contains(&"phonetic".into()));
    }

    #[test]
    fn sparse_local_data_does_not_clear_online_fields_or_duplicate_existing_content() {
        let primary = online("hello");
        let mut local = primary.clone();
        local.phonetic = None;
        local.tags.clear();
        local.examples.clear();
        let result = supplement_online(primary.clone(), local);
        assert_eq!(result.phonetic, primary.phonetic);
        assert_eq!(result.tags, primary.tags);
        assert!(result.local_supplement.is_none());
    }

    #[test]
    fn richer_online_phonetics_and_conflicting_online_word_forms_keep_priority() {
        let mut primary = online("hello");
        primary.phonetic = Some("英 /online-uk/ 美 /online-us/".into());
        primary.exchange = Some("s:onlineplural".into());
        let mut local = primary.clone();
        local.phonetic = Some("英 /local-uk/ 美 /local-us/".into());
        local.exchange = Some("s:localplural/p:localpast".into());
        let result = supplement_online(primary.clone(), local);
        assert_eq!(result.phonetic, primary.phonetic);
        assert_eq!(
            result.exchange.as_deref(),
            Some("s:onlineplural/p:localpast")
        );
    }

    #[test]
    fn verified_inflection_can_use_lemma_metadata_but_not_the_lemmas_pronunciation() {
        let primary = online("portals");
        let mut local = online("portal");
        local.exchange = Some("s:portals".into());
        local.phonetic = Some("英 /portal/ 美 /portal/".into());
        local.tags = vec!["CET6".into()];
        let result = supplement_online(primary.clone(), local);
        assert_eq!(result.phonetic, primary.phonetic);
        assert_eq!(result.word, "portals");
        assert_eq!(result.local_supplement.unwrap().word, "portal");
    }

    #[test]
    fn heuristic_only_fallback_cannot_enrich_an_unrelated_word() {
        let primary = online("hardly");
        let mut local = online("hard");
        local.tags = vec!["CET6".into()];
        local.phonetic = Some("英 /hard/ 美 /hard/".into());
        let result = supplement_online(primary.clone(), local);
        assert_eq!(result.phonetic, primary.phonetic);
        assert_eq!(result.tags, primary.tags);
        assert!(result.local_supplement.is_none());
    }

    #[test]
    fn youdao_word_forms_use_the_renderers_canonical_exchange_codes() {
        let response = json!({ "wfs": [
            { "wf": { "name": "过去式", "value": "went" } },
            { "wf": { "name": "第三人称单数", "value": "goes" } },
            { "wf": { "name": "复数", "value": "words" } }
        ] });
        assert_eq!(
            exchange_of(Some(&response)).as_deref(),
            Some("p:went/3:goes/s:words")
        );
    }

    #[tokio::test]
    async fn online_hits_are_supplemented_cached_and_disabling_local_removes_the_supplement() {
        let dir = TestDirectory::new();
        let (service, calls) = service(&dir, true);
        assert!(
            service
                .lookup("hello")
                .await
                .unwrap()
                .local_supplement
                .is_some()
        );
        assert!(
            service
                .lookup("HELLO")
                .await
                .unwrap()
                .local_supplement
                .is_some()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        service.set_source_enabled("local_eccedict", false).unwrap();
        assert!(
            service
                .lookup("hello")
                .await
                .unwrap()
                .local_supplement
                .is_none()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[tokio::test]
    async fn local_first_keeps_the_existing_offline_short_circuit() {
        let dir = TestDirectory::new();
        let (service, calls) = service(&dir, true);
        service
            .set_source_order(&["local_eccedict".into(), "youdao".into(), "google".into()])
            .unwrap();
        let result = service.lookup("hello").await.unwrap();
        assert_eq!(result.source, "local_eccedict");
        assert!(result.local_supplement.is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn no_installed_local_dictionary_does_not_block_online_lookup() {
        let dir = TestDirectory::new();
        let (service, calls) = service(&dir, false);
        let result = service.lookup("hello").await.unwrap();
        assert_eq!(result.source, "fixture_online");
        assert!(result.local_supplement.is_none());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn in_flight_queries_cannot_repopulate_cache_after_source_settings_change() {
        let dir = TestDirectory::new();
        let (service, _) = service(&dir, true);
        let service = Arc::new(service);
        let entered = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        let calls = Arc::new(AtomicU64::new(0));
        service.entries.write().unwrap()[0].source = Arc::new(DelayedSource {
            entered: Arc::clone(&entered),
            resume: Arc::clone(&resume),
            calls: Arc::clone(&calls),
        });
        let handle = Arc::clone(&service);
        let pending = tokio::spawn(async move { handle.lookup("hello").await });
        entered.notified().await;
        service.set_source_enabled("local_eccedict", false).unwrap();
        resume.notify_one();
        assert!(pending.await.unwrap().unwrap().local_supplement.is_none());
        assert!(service.cache.lock().unwrap().get("hello").is_none());
        assert!(
            service
                .lookup("hello")
                .await
                .unwrap()
                .local_supplement
                .is_none()
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn concurrent_import_attempts_are_rejected_without_changing_the_source_list() {
        let dir = TestDirectory::new();
        let (service, _) = service(&dir, true);
        let operation = service.begin_import().unwrap();
        assert!(service.begin_import().is_err());
        drop(operation);
        assert!(service.begin_import().is_ok());
        assert_eq!(service.sources().len(), 3);
    }

    fn ce_block(pos: Option<&str>, words: &[&str]) -> Value {
        let mut parts = vec![json!("")];
        for (index, word) in words.iter().enumerate() {
            if index > 0 {
                parts.push(json!(" "));
            }
            parts.push(json!({ "#text": word, "@action": "link" }));
        }
        let mut line = json!({ "i": parts, "#tran": "中文释义；" });
        if let Some(pos) = pos {
            line["pos"] = json!(pos);
        }
        json!({ "tr": [{ "l": line }] })
    }

    #[test]
    fn chinese_headwords_use_the_ce_section_and_chinese_audio() {
        let response = json!({
            "ce": { "word": [{ "phone": "nǐ hǎo", "trs": [
                ce_block(None, &["hello"]),
                ce_block(None, &["hi"]),
                ce_block(None, &["how", "do", "you", "do"]),
            ]}]},
            "blng_sents_part": { "sentence-pair": [{
                "sentence": "哈罗，约翰，你好吗？",
                "sentence-translation": "Hello John, how are you?"
            }]}
        });
        let entry = parse_youdao("你好", &response).expect("hit");
        assert_eq!(entry.definitions.len(), 1);
        assert_eq!(entry.definitions[0].pos, "def");
        assert_eq!(
            entry.definitions[0].meanings,
            ["hello", "hi", "how do you do"]
        );
        // The Chinese sentence is the translation side of an English example.
        assert_eq!(entry.examples[0].en, "Hello John, how are you?");
        assert_eq!(entry.examples[0].zh, "哈罗，约翰，你好吗？");
        assert!(entry.audio_url.unwrap().ends_with("&le=zh"));
    }

    #[test]
    fn ce_meanings_group_by_consecutive_part_of_speech() {
        let response = json!({ "ce": { "word": [{ "trs": [
            ce_block(Some("vt."), &["hit"]),
            ce_block(Some("vi."), &["fight"]),
            ce_block(Some("vi."), &["strike"]),
            ce_block(Some("n."), &["dozen"]),
        ]}]}});
        let entry = parse_youdao("打", &response).expect("hit");
        let grouped: Vec<(&str, Vec<&str>)> = entry
            .definitions
            .iter()
            .map(|d| {
                (
                    d.pos.as_str(),
                    d.meanings.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            grouped,
            [
                ("vt.", vec!["hit"]),
                ("vi.", vec!["fight", "strike"]),
                ("n.", vec!["dozen"]),
            ]
        );
    }

    #[test]
    fn responses_without_definitions_fall_back_to_translation() {
        // What Youdao returns for phrases such as 取词 / 今天天气真不错.
        let response = json!({
            "web_trans": { "web-translation": [{ "trans": [{ "value": "Autostemming" }] }] },
            "blng_sents_part": { "sentence-pair": [{
                "sentence": "取词窗口背景色设置。",
                "sentence-translation": "Floating window background color setting."
            }]}
        });
        assert!(parse_youdao("取词", &response).is_none());
    }

    #[test]
    fn english_headwords_keep_the_ec_section() {
        let response = json!({
            "ec": { "word": [{ "usphone": "ˈæpl", "trs": [
                { "tr": [{ "l": { "i": ["n. 苹果；苹果树"] } }] }
            ]}]},
            "blng_sents_part": { "sentence-pair": [{
                "sentence": "An apple a day.",
                "sentence-translation": "一天一苹果。"
            }]}
        });
        let entry = parse_youdao("apple", &response).expect("hit");
        assert_eq!(entry.phonetic.as_deref(), Some("/ˈæpl/"));
        assert_eq!(entry.definitions[0].pos, "n.");
        assert_eq!(entry.definitions[0].meanings, ["苹果", "苹果树"]);
        assert_eq!(entry.examples[0].en, "An apple a day.");
        assert!(entry.audio_url.unwrap().ends_with("&type=2"));
    }

    #[tokio::test]
    async fn test_lookup_service_lock_poisoning_recovery() {
        let temp_dir = std::env::temp_dir().join(format!(
            "lookup_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::create_dir_all(&temp_dir);
        let db_path = temp_dir.join("test.redb");
        let store = Arc::new(DictStore::open(&db_path).unwrap());
        let service = Arc::new(LookupService::new(store, db_path));

        // Poison the cache mutex
        let svc_clone = Arc::clone(&service);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = svc_clone.cache.lock().unwrap();
            panic!("poisoning cache lock intentional");
        }));

        assert!(service.cache.is_poisoned());

        // Verify invalidate_cache recovers from poisoned mutex
        service.invalidate_cache();

        // Poison the entries rwlock
        let svc_clone2 = Arc::clone(&service);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = svc_clone2.entries.write().unwrap();
            panic!("poisoning entries lock intentional");
        }));

        assert!(service.entries.is_poisoned());

        // Verify sources() recovers from poisoned rwlock
        let sources = service.sources();
        assert!(!sources.is_empty());

        // Verify set_source_enabled recovers from poisoned rwlock
        assert!(service.set_source_enabled("youdao", true).is_ok());

        // Verify lookup recovers gracefully from poisoned locks
        let _ = service.lookup("").await;

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
