//! Pinned, verified OCR artifacts. Network access occurs only during installation.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

const MANIFEST_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/ocr/manifest.json"
));
const NOTICE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/ocr/NOTICE.md"
));
const LICENSES: &[(&str, &str)] = &[
    (
        "LICENSE-ort-MIT.txt",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/ocr/licenses/LICENSE-ort-MIT.txt"
        )),
    ),
    (
        "LICENSE-libloading.txt",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/ocr/licenses/LICENSE-libloading.txt"
        )),
    ),
    (
        "LICENSE-PaddleOCR.txt",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/ocr/licenses/LICENSE-PaddleOCR.txt"
        )),
    ),
    (
        "LICENSE-RapidOCR.txt",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/ocr/licenses/LICENSE-RapidOCR.txt"
        )),
    ),
    (
        "LICENSE-paddle-ocr-rs.txt",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/ocr/licenses/LICENSE-paddle-ocr-rs.txt"
        )),
    ),
];

#[derive(Clone, Debug, Deserialize)]
pub struct Artifact {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub urls: Vec<String>,
    pub member: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RuntimeArtifact {
    pub platform: String,
    pub library: String,
    pub archive: String,
    pub format: String,
    pub bytes: u64,
    pub sha256: String,
    pub urls: Vec<String>,
    pub files: Vec<Artifact>,
}

#[derive(Debug, Deserialize)]
pub struct ModelManifest {
    pub id: String,
    pub model_family: String,
    pub precision: String,
    pub runtime_version: String,
    pub models: Vec<Artifact>,
    pub runtimes: Vec<RuntimeArtifact>,
}

pub fn manifest() -> &'static ModelManifest {
    static MANIFEST: OnceLock<ModelManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(MANIFEST_JSON).expect("built-in OCR manifest is valid JSON")
    })
}

pub fn current_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

pub fn runtime_for(platform: &str) -> Result<&'static RuntimeArtifact, String> {
    manifest()
        .runtimes
        .iter()
        .find(|runtime| runtime.platform == platform)
        .ok_or_else(|| format!("暂未提供 {platform} 的 ONNX Runtime 安装包。"))
}

#[derive(Clone, Debug, Serialize)]
pub struct OnnxInstallStatus {
    pub installed: bool,
    pub installing: bool,
    pub path: Option<String>,
    pub download_bytes: u64,
    pub installed_bytes: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstallProgress {
    pub stage: &'static str,
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
}

/// Both roots contain `<pack id>/<os-arch>/`. Installed app resources are
/// read-only; repairs/downloads go into the application data directory instead.
pub struct OnnxAssets {
    bundled_root: Option<PathBuf>,
    install_root: PathBuf,
    cache_root: PathBuf,
    platform: String,
    installing: AtomicBool,
}

impl OnnxAssets {
    pub fn new(bundled_root: Option<PathBuf>, install_root: PathBuf, cache_root: PathBuf) -> Self {
        Self::for_platform(bundled_root, install_root, cache_root, current_platform())
    }

    pub fn for_platform(
        bundled_root: Option<PathBuf>,
        install_root: PathBuf,
        cache_root: PathBuf,
        platform: String,
    ) -> Self {
        Self {
            bundled_root,
            install_root,
            cache_root,
            platform,
            installing: AtomicBool::new(false),
        }
    }

    pub fn runtime(&self) -> Result<&'static RuntimeArtifact, String> {
        runtime_for(&self.platform)
    }

    pub fn install_dir(&self) -> PathBuf {
        self.install_root.join(&manifest().id).join(&self.platform)
    }

    pub fn resolve(&self) -> Result<PathBuf, String> {
        let runtime = self.runtime()?;
        let installed = self.install_dir();
        let bundled = self
            .bundled_root
            .as_ref()
            .map(|root| root.join(&manifest().id).join(&self.platform));
        let mut last_error = None;
        for directory in bundled.into_iter().chain(std::iter::once(installed)) {
            match verify_pack(&directory, runtime) {
                Ok(()) => return Ok(directory),
                Err(error) => last_error = Some(error),
            }
        }
        Err(format!(
            "ONNX 资源尚未安装或校验失败，请在 OCR 设置中安装/修复。{}",
            last_error
                .map(|error| format!(" ({error})"))
                .unwrap_or_default()
        ))
    }

    pub fn status(&self) -> OnnxInstallStatus {
        let result = self.resolve();
        let (download_bytes, installed_bytes) = self
            .runtime()
            .map(|runtime| {
                let models = manifest()
                    .models
                    .iter()
                    .map(|model| model.bytes)
                    .sum::<u64>();
                (
                    models + runtime.bytes,
                    models + runtime.files.iter().map(|file| file.bytes).sum::<u64>(),
                )
            })
            .unwrap_or_default();
        OnnxInstallStatus {
            installed: result.is_ok(),
            installing: self.installing.load(Ordering::Acquire),
            path: result.as_ref().ok().map(|path| path.display().to_string()),
            error: result.err(),
            download_bytes,
            installed_bytes,
        }
    }

    pub async fn install(
        &self,
        progress: &(dyn Fn(InstallProgress) + Send + Sync),
    ) -> Result<PathBuf, String> {
        if self
            .installing
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("ONNX 资源正在安装，请等待当前安装完成。".into());
        }
        struct Installing<'a>(&'a AtomicBool);
        impl Drop for Installing<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _installing = Installing(&self.installing);
        let runtime = self.runtime()?;
        let total = runtime.bytes
            + manifest()
                .models
                .iter()
                .map(|model| model.bytes)
                .sum::<u64>();
        if let Ok(directory) = self.resolve() {
            progress(InstallProgress {
                stage: "ready",
                file: String::new(),
                downloaded: total,
                total,
            });
            return Ok(directory);
        }
        let directory = self.install_dir();
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.cache_root).map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(600))
            .user_agent("TransPortal/0.1 OCR asset installer")
            .build()
            .map_err(|e| e.to_string())?;
        let mut completed = 0;
        for model in &manifest().models {
            let destination = directory.join(&model.name);
            if verify_file(&destination, model).is_err() {
                let downloaded =
                    download(&client, &self.cache_root, model, completed, total, progress).await?;
                copy_verified(&downloaded, &destination, model)?;
            }
            completed += model.bytes;
        }

        if runtime
            .files
            .iter()
            .any(|file| verify_file(&directory.join(&file.name), file).is_err())
        {
            let archive = Artifact {
                name: runtime.archive.clone(),
                bytes: runtime.bytes,
                sha256: runtime.sha256.clone(),
                urls: runtime.urls.clone(),
                member: None,
            };
            let archive_path = download(
                &client,
                &self.cache_root,
                &archive,
                completed,
                total,
                progress,
            )
            .await?;
            progress(InstallProgress {
                stage: "verifying",
                file: runtime.archive.clone(),
                downloaded: total,
                total,
            });
            // Only explicitly listed regular files are extracted, never archive
            // paths, links, debug symbols, headers or executables.
            extract_runtime(&archive_path, &directory, runtime)?;
        }
        fs::write(directory.join("manifest.json"), MANIFEST_JSON).map_err(|e| e.to_string())?;
        fs::write(directory.join("NOTICE.md"), NOTICE).map_err(|e| e.to_string())?;
        let licenses = directory.join("licenses");
        fs::create_dir_all(&licenses).map_err(|e| e.to_string())?;
        for (name, content) in LICENSES {
            fs::write(licenses.join(name), content).map_err(|e| e.to_string())?;
        }
        verify_pack(&directory, runtime)?;
        progress(InstallProgress {
            stage: "ready",
            file: String::new(),
            downloaded: total,
            total,
        });
        Ok(directory)
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).components().count() == 1
        && matches!(
            Path::new(name).components().next(),
            Some(Component::Normal(_))
        )
        && !name.contains(['/', '\\', ':'])
}

pub fn verify_file(path: &Path, artifact: &Artifact) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| format!("缺少 {}", artifact.name))?;
    if !metadata.is_file() || metadata.len() != artifact.bytes {
        return Err(format!("{} 大小不符", artifact.name));
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:x}", hasher.finalize()) != artifact.sha256 {
        return Err(format!("{} SHA-256 校验失败", artifact.name));
    }
    Ok(())
}

fn verify_pack(directory: &Path, runtime: &RuntimeArtifact) -> Result<(), String> {
    for artifact in manifest().models.iter().chain(&runtime.files) {
        if !safe_name(&artifact.name) {
            return Err("OCR 清单包含无效文件名".into());
        }
        verify_file(&directory.join(&artifact.name), artifact)?;
    }
    for (name, content) in LICENSES {
        if fs::read(directory.join("licenses").join(name))
            .ok()
            .as_deref()
            != Some(content.as_bytes())
        {
            return Err(format!("缺少或需要更新 {name}"));
        }
    }
    if fs::read(directory.join("NOTICE.md")).ok().as_deref() != Some(NOTICE.as_bytes()) {
        return Err("缺少或需要更新 OCR 许可说明".into());
    }
    Ok(())
}

struct PartialFile(PathBuf);
impl Drop for PartialFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn partial_path(destination: &Path) -> PathBuf {
    destination.with_extension(format!("part-{}", std::process::id()))
}

fn publish_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    // This only replaces a known, failed-hash asset in the selected OCR cache.
    // A loaded Windows DLL will fail cleanly here and can be repaired on restart.
    if destination.exists() {
        fs::remove_file(destination).map_err(|e| format!("无法修复资源，请重启后再试：{e}"))?;
    }
    fs::rename(temporary, destination).map_err(|e| format!("无法安装 OCR 资源：{e}"))
}

fn copy_verified(source: &Path, destination: &Path, artifact: &Artifact) -> Result<(), String> {
    let temporary = PartialFile(partial_path(destination));
    fs::copy(source, &temporary.0).map_err(|e| e.to_string())?;
    verify_file(&temporary.0, artifact)?;
    publish_file(&temporary.0, destination)
}

async fn download(
    client: &reqwest::Client,
    cache: &Path,
    artifact: &Artifact,
    completed: u64,
    total: u64,
    progress: &(dyn Fn(InstallProgress) + Send + Sync),
) -> Result<PathBuf, String> {
    if !safe_name(&artifact.name) {
        return Err("OCR 清单包含无效文件名".into());
    }
    let destination = cache.join(&artifact.name);
    if verify_file(&destination, artifact).is_ok() {
        return Ok(destination);
    }
    let mut last_error = String::from("没有可用的下载地址");
    for url in &artifact.urls {
        for _attempt in 0..2 {
            let temporary = PartialFile(partial_path(&destination));
            let result = async {
                progress(InstallProgress {
                    stage: "downloading",
                    file: artifact.name.clone(),
                    downloaded: completed,
                    total,
                });
                let response = client
                    .get(url)
                    .header("Accept", "application/octet-stream")
                    .send()
                    .await
                    .map_err(|e| format!("连接失败：{e}"))?
                    .error_for_status()
                    .map_err(|e| e.to_string())?;
                let mut file = tokio::fs::File::create(&temporary.0)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut stream = response.bytes_stream();
                let mut received = 0u64;
                let mut hasher = Sha256::new();
                let mut last_report = Instant::now();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| e.to_string())?;
                    received += chunk.len() as u64;
                    if received > artifact.bytes {
                        return Err("下载内容超出已知大小".into());
                    }
                    hasher.update(&chunk);
                    file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                    if last_report.elapsed() >= Duration::from_millis(150)
                        || received == artifact.bytes
                    {
                        progress(InstallProgress {
                            stage: "downloading",
                            file: artifact.name.clone(),
                            downloaded: completed + received,
                            total,
                        });
                        last_report = Instant::now();
                    }
                }
                file.flush().await.map_err(|e| e.to_string())?;
                drop(file);
                if received != artifact.bytes
                    || format!("{:x}", hasher.finalize()) != artifact.sha256
                {
                    return Err("下载文件大小或 SHA-256 校验失败".into());
                }
                publish_file(&temporary.0, &destination)
            }
            .await;
            match result {
                Ok(()) => return Ok(destination),
                Err(error) => last_error = error,
            }
        }
    }
    Err(format!(
        "{} 下载失败：{last_error}。可稍后重试，已验证的文件会被复用。",
        artifact.name
    ))
}

fn extract_runtime(
    archive: &Path,
    directory: &Path,
    runtime: &RuntimeArtifact,
) -> Result<(), String> {
    let write_member = |reader: &mut dyn Read, artifact: &Artifact| -> Result<(), String> {
        if !safe_name(&artifact.name) {
            return Err("OCR 清单包含无效文件名".into());
        }
        let destination = directory.join(&artifact.name);
        let temporary = PartialFile(partial_path(&destination));
        let mut file = File::create(&temporary.0).map_err(|e| e.to_string())?;
        let bytes = std::io::copy(&mut reader.take(artifact.bytes + 1), &mut file)
            .map_err(|e| e.to_string())?;
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        if bytes != artifact.bytes {
            return Err(format!("{} 解压大小不符", artifact.name));
        }
        verify_file(&temporary.0, artifact)?;
        publish_file(&temporary.0, &destination)
    };
    let file = File::open(archive).map_err(|e| e.to_string())?;
    if runtime.format == "zip" {
        let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for artifact in &runtime.files {
            let member = artifact.member.as_deref().ok_or("缺少运行库成员路径")?;
            let mut entry = zip.by_name(member).map_err(|e| e.to_string())?;
            if !entry.is_file() || entry.is_symlink() {
                return Err("运行库归档包含无效成员".into());
            }
            write_member(&mut entry, artifact)?;
        }
    } else if runtime.format == "tgz" {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let mut found = 0usize;
        for entry in archive.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let path = entry
                .path()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .to_string();
            if let Some(artifact) = runtime
                .files
                .iter()
                .find(|artifact| artifact.member.as_deref() == Some(path.as_str()))
            {
                if !entry.header().entry_type().is_file() {
                    return Err("运行库归档包含无效成员".into());
                }
                write_member(&mut entry, artifact)?;
                found += 1;
            }
        }
        if found != runtime.files.len() {
            return Err("运行库归档缺少必要文件".into());
        }
    } else {
        return Err("不支持的运行库归档格式".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_pack_has_models_checksums_and_only_safe_file_targets() {
        let manifest = manifest();
        assert_eq!(manifest.precision, "FP32");
        assert_eq!(manifest.models.len(), 3);
        assert_eq!(manifest.runtimes.len(), 6);
        for runtime in &manifest.runtimes {
            assert!(
                runtime
                    .files
                    .iter()
                    .any(|file| file.name == runtime.library)
            );
            assert!(
                runtime
                    .files
                    .iter()
                    .any(|file| file.name == "LICENSE-ONNXRuntime.txt")
            );
            assert!(
                runtime
                    .files
                    .iter()
                    .any(|file| file.name == "ThirdPartyNotices.txt")
            );
            for file in manifest.models.iter().chain(&runtime.files) {
                assert!(safe_name(&file.name));
                assert!(file.bytes > 0);
                assert_eq!(file.sha256.len(), 64);
                assert!(file.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
            }
        }
        for unsafe_name in [
            "../model.onnx",
            "C:\\model.onnx",
            "/model.onnx",
            "a/b",
            "a\\b",
            ".",
            "",
        ] {
            assert!(!safe_name(unsafe_name));
        }
    }

    #[test]
    fn asset_verification_rejects_same_length_corruption_and_partial_downloads() {
        let path = std::env::temp_dir().join(format!("transportal-ocr-sha-{}", std::process::id()));
        let artifact = Artifact {
            name: "fixture".into(),
            bytes: 3,
            sha256: format!("{:x}", Sha256::digest(b"abc")),
            urls: vec![],
            member: None,
        };
        fs::write(&path, b"abc").unwrap();
        assert!(verify_file(&path, &artifact).is_ok());
        fs::write(&path, b"abd").unwrap();
        assert!(
            verify_file(&path, &artifact)
                .unwrap_err()
                .contains("SHA-256")
        );
        fs::write(&path, b"ab").unwrap();
        assert!(verify_file(&path, &artifact).is_err());
        fs::remove_file(path).unwrap();
    }
}
