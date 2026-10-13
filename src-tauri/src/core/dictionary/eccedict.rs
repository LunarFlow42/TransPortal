//! Import of the full ECCEDICT CSV into the local store, from a file or a
//! mirror download.

use super::LookupService;
use futures_util::StreamExt;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

/// Progress payload for the full-dictionary download + import pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct DictProgress {
    /// `downloading` | `importing` | `done`
    pub stage: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub imported: Option<usize>,
}

/// Full ECCEDICT CSV mirrors, tried in order (jsdelivr first for CN networks).
const ECCEDICT_CSV_URLS: [&str; 2] = [
    "https://cdn.jsdelivr.net/gh/H1DDENADM1N/ECCEDICT@master/ecdict.csv",
    "https://raw.githubusercontent.com/H1DDENADM1N/ECCEDICT/master/ecdict.csv",
];

/// Import an ECCEDICT-format CSV file into the local redb store.
pub async fn import_file(service: &LookupService, path: String) -> Result<usize, String> {
    let store = service.local_handle();
    let count = tauri::async_runtime::spawn_blocking(move || store.import_csv_file(&path))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    service.invalidate_cache();
    Ok(count)
}

/// RAII guard ensuring temporary CSV download file is cleaned up upon completion, error, or drop.
pub(crate) struct TempFileGuard(pub(crate) std::path::PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Download the full (~63 MB) ECCEDICT CSV with progress, then import it.
pub async fn download_and_import(
    service: &LookupService,
    on_progress: impl Fn(DictProgress),
) -> Result<usize, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let mut response = None;
    let mut last_error = String::new();
    for url in ECCEDICT_CSV_URLS {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                response = Some(resp);
                break;
            }
            Ok(resp) => last_error = format!("{url} -> HTTP {}", resp.status()),
            Err(err) => last_error = format!("{url} -> {err}"),
        }
    }
    let response = response.ok_or_else(|| format!("all mirrors failed: {last_error}"))?;

    let total = response.content_length();
    let path = std::env::temp_dir().join("transportal-eccedict.csv");
    let _cleanup_guard = TempFileGuard(path.clone());

    let mut file = tokio::fs::File::create(&path)
        .await
        .map_err(|e| format!("failed to create temp file: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download stream error: {e}"))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("failed to write download: {e}"))?;
        downloaded += chunk.len() as u64;
        on_progress(DictProgress {
            stage: "downloading".to_string(),
            downloaded,
            total,
            imported: None,
        });
    }
    drop(file);

    on_progress(DictProgress {
        stage: "importing".to_string(),
        downloaded,
        total,
        imported: None,
    });

    let count = import_file(service, path.to_string_lossy().to_string()).await?;

    on_progress(DictProgress {
        stage: "done".to_string(),
        downloaded,
        total,
        imported: Some(count),
    });

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_temp_file_guard_cleans_up_on_drop() {
        let temp_path = std::env::temp_dir().join(format!(
            "transportal-test-{}.csv",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&temp_path, "dummy,data\n").expect("write test file");
        assert!(temp_path.exists());

        {
            let _guard = TempFileGuard(temp_path.clone());
            assert!(temp_path.exists());
        }

        assert!(!temp_path.exists());
    }
}
