//! Verified build-time overrides applied with Rust, not runtime DuckDB/Python.

use super::eccedict::TempFileGuard;
use csv::{ReaderBuilder, StringRecord, WriterBuilder};
use flate2::read::GzDecoder;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::io::{Read, Write};
use std::path::Path;

const OVERRIDES: &[u8] = include_bytes!("../../../resources/dictionary/enrichment.csv.gz");
const MANIFEST: &str = include_str!("../../../resources/dictionary/manifest.json");
/// The notice is also installed by tauri.conf.json alongside binary distributions.
pub const UPSTREAM_LICENSE: &str = include_str!("../../../resources/dictionary/LICENSE.upstream");
pub const CSV_HEADER: [&str; 13] = [
    "word",
    "phonetic",
    "definition",
    "translation",
    "pos",
    "collins",
    "oxford",
    "tag",
    "bnc",
    "frq",
    "exchange",
    "detail",
    "audio",
];

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub schema_version: u64,
    pub revision: String,
    pub base: BaseAsset,
    pub enrichment: EnrichmentAsset,
    pub entries: usize,
    pub enriched_phonetics: usize,
    pub enriched_tags: usize,
}

#[derive(Debug, Deserialize)]
pub struct BaseAsset {
    pub bytes: u64,
    pub sha256: String,
    pub urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct EnrichmentAsset {
    pub bytes: usize,
    pub sha256: String,
    pub rows: usize,
}

#[derive(Debug, Clone, Default)]
pub struct EnrichmentSummary {
    pub entries: usize,
    pub phonetics: usize,
    pub tags: usize,
}

#[derive(Debug)]
struct Override {
    phonetic: String,
    tag: String,
}

pub fn manifest() -> Result<Manifest, String> {
    let manifest: Manifest = serde_json::from_str(MANIFEST).map_err(|error| error.to_string())?;
    if manifest.schema_version != 1
        || manifest.base.urls.is_empty()
        || manifest
            .base
            .urls
            .iter()
            .any(|url| !url.starts_with("https://") || !url.contains(&manifest.revision))
    {
        return Err("Invalid pinned dictionary manifest".into());
    }
    Ok(manifest)
}

pub fn verify_base(path: &Path, manifest: &Manifest) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if file.metadata().map_err(|error| error.to_string())?.len() != manifest.base.bytes {
        return Err("Dictionary download length does not match the pinned manifest".into());
    }
    let mut reader = std::io::BufReader::new(file);
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if format!("{:x}", digest.finalize()) != manifest.base.sha256 {
        return Err("Dictionary download checksum does not match the pinned manifest".into());
    }
    Ok(())
}

fn overrides(manifest: &Manifest) -> Result<HashMap<String, Override>, String> {
    if OVERRIDES.len() != manifest.enrichment.bytes
        || format!("{:x}", Sha256::digest(OVERRIDES)) != manifest.enrichment.sha256
    {
        return Err("Bundled dictionary enrichment failed integrity verification".into());
    }
    let mut reader = ReaderBuilder::new().from_reader(GzDecoder::new(OVERRIDES));
    if reader.headers().map_err(|error| error.to_string())?
        != &StringRecord::from(vec!["word", "phonetic", "tag"])
    {
        return Err("Invalid enrichment CSV header".into());
    }
    let mut result = HashMap::with_capacity(manifest.enrichment.rows);
    for record in reader.records() {
        let record = record.map_err(|error| error.to_string())?;
        let word = record[0].trim().to_lowercase();
        if word.is_empty() || result.contains_key(&word) {
            return Err("Empty or duplicate word in enrichment asset".into());
        }
        result.insert(
            word,
            Override {
                phonetic: record[1].to_string(),
                tag: record[2].to_string(),
            },
        );
    }
    if result.len() != manifest.enrichment.rows {
        return Err("Enrichment row count mismatch".into());
    }
    Ok(result)
}

/// The same deterministic union policy as the development-time CSV builder.
fn merge_tags(base: &str, additional: &str) -> String {
    base.split_whitespace()
        .chain(additional.split_whitespace())
        .map(str::to_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn build_csv(
    source: &Path,
    output: &Path,
) -> Result<(TempFileGuard, EnrichmentSummary), String> {
    let manifest = manifest()?;
    verify_base(source, &manifest)?;
    let overrides = overrides(&manifest)?;
    let source = std::fs::File::open(source).map_err(|error| error.to_string())?;
    let target = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|error| error.to_string())?;
    let guard = TempFileGuard(output.to_path_buf());
    let summary = apply_csv(source, target, &overrides)?;
    if summary.entries != manifest.entries
        || summary.phonetics != manifest.enriched_phonetics
        || summary.tags != manifest.enriched_tags
    {
        return Err("Enriched dictionary coverage does not match the pinned manifest".into());
    }
    Ok((guard, summary))
}

fn apply_csv(
    source: impl Read,
    target: impl Write,
    overrides: &HashMap<String, Override>,
) -> Result<EnrichmentSummary, String> {
    let mut reader = ReaderBuilder::new().from_reader(source);
    if reader.headers().map_err(|error| error.to_string())?
        != &StringRecord::from(CSV_HEADER.to_vec())
    {
        return Err("Invalid dictionary CSV header".into());
    }
    let mut writer = WriterBuilder::new().from_writer(target);
    writer
        .write_record(CSV_HEADER)
        .map_err(|error| error.to_string())?;
    let mut summary = EnrichmentSummary::default();
    for record in reader.records() {
        let record = record.map_err(|error| error.to_string())?;
        let mut fields: Vec<String> = record.iter().map(str::to_string).collect();
        if let Some(additional) = overrides.get(&record[0].trim().to_lowercase()) {
            if !additional.phonetic.is_empty() {
                fields[1] = additional.phonetic.clone();
                summary.phonetics += 1;
            }
            if !additional.tag.is_empty() {
                fields[7] = merge_tags(&fields[7], &additional.tag);
                summary.tags += 1;
            }
        }
        writer
            .write_record(fields)
            .map_err(|error| error.to_string())?;
        summary.entries += 1;
    }
    writer.flush().map_err(|error| error.to_string())?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_overrides_are_verified_and_contain_real_uk_us_phonetics_and_tags() {
        let manifest = manifest().unwrap();
        let data = overrides(&manifest).unwrap();
        assert!(data.len() > 100_000);
        let word = data.get("hello").unwrap();
        assert!(word.phonetic.contains("英 /"));
        assert!(word.phonetic.contains("美 /"));
        assert!(data.values().any(|entry| entry.tag.contains("cet4")));
    }

    #[test]
    fn enrichment_preserves_existing_tags_and_deduplicates_case_insensitively() {
        assert_eq!(
            merge_tags("CET4 oxford ielts", "cet4 ky"),
            "cet4 ielts ky oxford"
        );
    }

    #[test]
    fn rust_enrichment_changes_only_phonetics_and_unioned_tags() {
        let original = crate::core::dictionary::test_support::row("Hello", "n. 问候\n招呼");
        let mut source = Vec::new();
        {
            let mut writer = WriterBuilder::new().from_writer(&mut source);
            writer.write_record(CSV_HEADER).unwrap();
            writer.write_record(&original).unwrap();
            writer.flush().unwrap();
        }
        let data = HashMap::from([(
            "hello".into(),
            Override {
                phonetic: "英 /uk/ 美 /us/".into(),
                tag: "ky cet4".into(),
            },
        )]);
        let mut output = Vec::new();
        let summary = apply_csv(source.as_slice(), &mut output, &data).unwrap();
        let record = ReaderBuilder::new()
            .from_reader(output.as_slice())
            .records()
            .next()
            .unwrap()
            .unwrap();
        for index in 0..13 {
            if index != 1 && index != 7 {
                assert_eq!(&record[index], &original[index]);
            }
        }
        assert_eq!(&record[1], "英 /uk/ 美 /us/");
        assert_eq!(&record[7], "cet4 ielts ky");
        assert_eq!(
            (summary.entries, summary.phonetics, summary.tags),
            (1, 1, 1)
        );
    }

    #[test]
    fn base_integrity_check_rejects_corruption_and_partial_files() {
        let dir = crate::core::dictionary::test_support::TestDirectory::new();
        let path = dir.0.join("base.csv");
        let mut manifest = manifest().unwrap();
        manifest.base.bytes = 5;
        manifest.base.sha256 = format!("{:x}", Sha256::digest(b"hello"));
        std::fs::write(&path, b"hello").unwrap();
        assert!(verify_base(&path, &manifest).is_ok());
        std::fs::write(&path, b"world").unwrap();
        assert!(verify_base(&path, &manifest).is_err());
        std::fs::write(&path, b"hel").unwrap();
        assert!(verify_base(&path, &manifest).is_err());
    }

    #[test]
    fn default_manifest_and_optional_full_dataset_import() {
        let manifest = manifest().unwrap();
        assert!(manifest.entries > 700_000);
        assert!(manifest.enriched_phonetics > 100_000);
        assert!(manifest.base.urls[0].starts_with("https://gh-proxy.com/"));
        assert!(UPSTREAM_LICENSE.contains("MIT License"));
        // Full downloaded-fixture verification is opt-in so ordinary unit tests
        // remain offline and fast. Always verify the manifest above.
        let Ok(base) = std::env::var("TRANSPORTAL_ECCEDICT_BASE") else {
            return;
        };
        let dir = crate::core::dictionary::test_support::TestDirectory::new();
        let output = dir.0.join("ecdict.enriched.csv");
        let started = std::time::Instant::now();
        let (_guard, summary) = build_csv(Path::new(&base), &output).unwrap();
        if let Ok(expected) = std::env::var("TRANSPORTAL_ECCEDICT_ENRICHED") {
            let mut actual_reader = ReaderBuilder::new().from_path(&output).unwrap();
            let mut expected_reader = ReaderBuilder::new().from_path(expected).unwrap();
            let mut actual = actual_reader.records();
            let mut expected = expected_reader.records();
            loop {
                match (actual.next(), expected.next()) {
                    (Some(actual), Some(expected)) => {
                        assert_eq!(actual.unwrap(), expected.unwrap())
                    }
                    (None, None) => break,
                    _ => panic!("Rust and Python enrichment produced different entry counts"),
                }
            }
        }
        let store = crate::core::dictionary::DictStore::open(dir.0.join("test.redb")).unwrap();
        let count = store
            .import_enriched_csv_file(
                &output,
                crate::core::dictionary::DictEnrichmentInfo {
                    revision: manifest.revision.clone(),
                    phonetics: summary.phonetics,
                    tags: summary.tags,
                },
            )
            .unwrap();
        assert_eq!(count, manifest.entries);
        let hello = store.lookup("hello").unwrap().unwrap();
        assert!(hello.phonetic.as_ref().unwrap().contains("英 /"));
        assert!(hello.phonetic.as_ref().unwrap().contains("美 /"));
        assert_eq!(hello.collins, Some(3));
        assert!(hello.oxford);
        assert_eq!(hello.bnc, Some(2319));
        assert_eq!(hello.frq, Some(2238));
        for word in ["ephemeral", "mitigate", "word", "run"] {
            assert!(!store.lookup(word).unwrap().unwrap().definitions.is_empty());
        }
        assert_eq!(
            store.enrichment_info().unwrap().unwrap().revision,
            manifest.revision
        );
        eprintln!(
            "Full enriched import: {count} entries, {} phonetics, {} tags in {:?}",
            summary.phonetics,
            summary.tags,
            started.elapsed()
        );
    }
}
