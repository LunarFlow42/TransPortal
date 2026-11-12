//! Manual CSV imports and verified default enriched ECCEDICT downloads.

use super::{DictEnrichmentInfo, LookupService, enrichment};
use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::AsyncWriteExt;

/// Progress payload for the full-dictionary download + import pipeline.
#[derive(Debug, Clone, Serialize)]
pub struct DictProgress {
    /// `downloading` | `enriching` | `importing` | `done`
    pub stage: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub imported: Option<usize>,
}

/// Import an ECCEDICT-format CSV file into the local redb store.
pub async fn import_file(service: &LookupService, path: String) -> Result<usize, String> {
    let operation = service.begin_import()?;
    let store = service.local_handle();
    let invalidate = service.cache_invalidator();
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let count = store
            .import_csv_file(&path)
            .map_err(|error| error.to_string())?;
        invalidate();
        Ok(count)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// RAII guard ensuring temporary CSV download file is cleaned up upon completion, error, or drop.
pub(crate) struct TempFileGuard(pub(crate) std::path::PathBuf);

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_csv(suffix: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "transportal-eccedict-{}-{time}-{}-{suffix}.csv",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Verify the entire response before importing. Retry mirrors on HTTP, stream,
/// length or checksum failures; never silently downgrade to the plain dataset.
async fn download_base(
    client: &reqwest::Client,
    manifest: &enrichment::Manifest,
    path: &Path,
    on_progress: &impl Fn(DictProgress),
) -> Result<TempFileGuard, String> {
    let mut last_error = String::new();
    for url in &manifest.base.urls {
        let result = async {
            let response = client
                .get(url)
                .send()
                .await
                .map_err(|error| error.to_string())?
                .error_for_status()
                .map_err(|error| error.to_string())?;
            if response
                .content_length()
                .is_some_and(|length| length != manifest.base.bytes)
            {
                return Err("Dictionary mirror returned an unexpected content length".to_string());
            }
            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .await
                .map_err(|error| error.to_string())?;
            let guard = TempFileGuard(path.to_path_buf());
            // Close the handle before the cleanup guard on every failure path,
            // including Windows where an open file cannot be removed.
            let mut file = file;
            let mut downloaded = 0u64;
            let mut hash = Sha256::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|error| error.to_string())?;
                downloaded += chunk.len() as u64;
                if downloaded > manifest.base.bytes {
                    return Err("Dictionary download exceeded the pinned length".into());
                }
                hash.update(&chunk);
                file.write_all(&chunk)
                    .await
                    .map_err(|error| error.to_string())?;
                on_progress(DictProgress {
                    stage: "downloading".into(),
                    downloaded,
                    total: Some(manifest.base.bytes),
                    imported: None,
                });
            }
            file.flush().await.map_err(|error| error.to_string())?;
            drop(file);
            if downloaded != manifest.base.bytes
                || format!("{:x}", hash.finalize()) != manifest.base.sha256
            {
                return Err("Dictionary download failed pinned integrity verification".into());
            }
            Ok(guard)
        }
        .await;
        match result {
            Ok(guard) => return Ok(guard),
            Err(error) => last_error = format!("{url}: {error}"),
        }
    }
    Err(format!(
        "All enriched dictionary base mirrors failed: {last_error}"
    ))
}

/// Download the pinned base, construct the enriched CSV locally using bundled
/// overrides, then atomically replace the dictionary. No Python/DuckDB at runtime.
pub async fn download_and_import(
    service: &LookupService,
    on_progress: impl Fn(DictProgress),
) -> Result<usize, String> {
    let operation = service.begin_import()?;
    let invalidate = service.cache_invalidator();
    let manifest = enrichment::manifest()?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let path = temp_csv("base");
    let base_guard = download_base(&client, &manifest, &path, &on_progress).await?;
    let downloaded = manifest.base.bytes;
    let total = Some(downloaded);
    on_progress(DictProgress {
        stage: "enriching".into(),
        downloaded,
        total,
        imported: None,
    });
    let enriched_path = temp_csv("enriched");
    let enrich_path = enriched_path.clone();
    let (operation, enriched_guard, summary) = tauri::async_runtime::spawn_blocking(move || {
        // Own both paths on the worker: cancelling the IPC future must not
        // remove a file while enrichment is still reading/writing it.
        let _base_guard = base_guard;
        let (guard, summary) = enrichment::build_csv(&path, &enrich_path)?;
        Ok::<_, String>((operation, guard, summary))
    })
    .await
    .map_err(|error| error.to_string())??;

    on_progress(DictProgress {
        stage: "importing".to_string(),
        downloaded,
        total,
        imported: None,
    });

    let store = service.local_handle();
    let count = tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        let _enriched_guard = enriched_guard;
        let count = store
            .import_enriched_csv_file(
                &enriched_path,
                DictEnrichmentInfo {
                    revision: manifest.revision,
                    phonetics: summary.phonetics,
                    tags: summary.tags,
                },
            )
            .map_err(|error| error.to_string())?;
        invalidate();
        Ok::<_, String>(count)
    })
    .await
    .map_err(|error| error.to_string())??;

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
    use crate::core::dictionary::test_support::TestDirectory;
    use tokio::io::AsyncReadExt;

    async fn mirror(body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let mut headers = Vec::new();
            while !headers.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let received = socket.read(&mut request).await.unwrap();
                if received == 0 {
                    return;
                }
                headers.extend_from_slice(&request[..received]);
                assert!(
                    headers.len() <= 16 * 1024,
                    "unexpected oversized test request"
                );
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{address}/ecdict.csv")
    }

    #[tokio::test]
    async fn verified_download_retries_corrupt_mirrors_and_cleans_its_temporary_file() {
        let dir = TestDirectory::new();
        let path = dir.0.join("download.csv");
        let mut manifest = enrichment::manifest().unwrap();
        manifest.base.urls = vec![mirror("wrong").await, mirror("hello").await];
        manifest.base.bytes = 5;
        manifest.base.sha256 = format!("{:x}", Sha256::digest(b"hello"));
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap();
        let guard = download_base(&client, &manifest, &path, &|_| {})
            .await
            .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"hello");
        drop(guard);
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn failed_default_download_does_not_overwrite_an_existing_file() {
        let dir = TestDirectory::new();
        let path = dir.0.join("download.csv");
        std::fs::write(&path, b"user data").unwrap();
        let mut manifest = enrichment::manifest().unwrap();
        manifest.base.urls = vec![mirror("hello").await];
        manifest.base.bytes = 5;
        manifest.base.sha256 = format!("{:x}", Sha256::digest(b"hello"));
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .unwrap();
        assert!(
            download_base(&client, &manifest, &path, &|_| {})
                .await
                .is_err()
        );
        assert_eq!(std::fs::read(path).unwrap(), b"user data");
    }

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
