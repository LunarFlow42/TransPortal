//! Small synthetic fixtures; never downloads or opens a user's dictionary.

use super::enrichment::CSV_HEADER;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) struct TestDirectory(pub PathBuf);

impl TestDirectory {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "transportal-dictionary-{}-{time}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    pub fn csv(&self, name: &str, rows: &[Vec<String>]) -> PathBuf {
        let path = self.0.join(name);
        write_csv(&path, rows);
        path
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(super) fn row(word: &str, meaning: &str) -> Vec<String> {
    vec![
        word.into(),
        "英 /həˈləʊ/ 美 /həˈloʊ/".into(),
        "a greeting".into(),
        meaning.into(),
        "n:100".into(),
        "3".into(),
        "1".into(),
        "cet4 ielts".into(),
        "100".into(),
        "200".into(),
        format!("s:{word}s"),
        String::new(),
        String::new(),
    ]
}

pub(super) fn write_csv(path: &Path, rows: &[Vec<String>]) {
    let mut writer = csv::Writer::from_path(path).unwrap();
    writer.write_record(CSV_HEADER).unwrap();
    for row in rows {
        writer.write_record(row).unwrap();
    }
    writer.flush().unwrap();
}
