//! Pinned, verified Kokoro TTS artifacts and voice packs.

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;

const MANIFEST_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/resources/tts/manifest.json"
));

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Artifact {
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub urls: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VoiceArtifact {
    pub id: String,
    pub name: String,
    pub label: String,
    pub language: String,
    pub gender: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default)]
    pub urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TtsManifest {
    pub id: String,
    pub model_family: String,
    pub precision: String,
    pub runtime_version: String,
    pub model: Artifact,
    pub voices: Vec<VoiceArtifact>,
}

pub fn manifest() -> &'static TtsManifest {
    static MANIFEST: OnceLock<TtsManifest> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        serde_json::from_str(MANIFEST_JSON).expect("built-in TTS manifest is valid JSON")
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceInfo {
    pub id: String,
    pub name: String,
    pub label: String,
    pub language: String,
    pub gender: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct TtsInstallStatus {
    pub installed: bool,
    pub installing: bool,
    pub path: Option<String>,
    pub download_bytes: u64,
    pub installed_bytes: u64,
    pub error: Option<String>,
    pub voices: Vec<VoiceInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct InstallProgress {
    pub stage: &'static str,
    pub file: String,
    pub downloaded: u64,
    pub total: u64,
}

pub struct TtsAssets {
    install_root: PathBuf,
    cache_root: PathBuf,
    ocr_runtime_dir: Option<PathBuf>,
    installing: AtomicBool,
}

impl TtsAssets {
    pub fn new(
        install_root: PathBuf,
        cache_root: PathBuf,
        ocr_runtime_dir: Option<PathBuf>,
    ) -> Self {
        Self {
            install_root,
            cache_root,
            ocr_runtime_dir,
            installing: AtomicBool::new(false),
        }
    }

    pub fn install_dir(&self) -> PathBuf {
        self.install_root.join(&manifest().id)
    }

    pub fn resolve_model(&self) -> Result<PathBuf, String> {
        let dir = self.install_dir();
        let path = dir.join(&manifest().model.name);
        verify_file(&path, &manifest().model)?;
        Ok(path)
    }

    pub fn resolve_voice(&self, voice_id: &str) -> Result<PathBuf, String> {
        let manifest = manifest();
        let voice = manifest
            .voices
            .iter()
            .find(|v| v.id == voice_id)
            .or_else(|| manifest.voices.first())
            .ok_or_else(|| format!("未找到可用音色：{voice_id}"))?;

        let dir = self.install_dir().join("voices");
        let path = dir.join(&voice.name);
        let artifact = Artifact {
            name: voice.name.clone(),
            bytes: voice.bytes,
            sha256: voice.sha256.clone(),
            urls: voice.urls.clone(),
        };
        verify_file(&path, &artifact)?;
        Ok(path)
    }

    /// Resolves the shared ONNX Runtime dynamic library (`onnxruntime.dll` / `.so`).
    pub fn resolve_runtime(&self) -> Result<PathBuf, String> {
        // 1. Check OCR runtime directory first if configured
        if let Some(ocr_dir) = &self.ocr_runtime_dir {
            let lib_name = match std::env::consts::OS {
                "windows" => "onnxruntime.dll",
                "linux" => "libonnxruntime.so.1.22.0",
                "macos" => "libonnxruntime.1.22.0.dylib",
                _ => "onnxruntime.dll",
            };

            let path = ocr_dir.join(lib_name);
            if path.exists() {
                return Ok(path);
            }
        }

        // 2. Fall back to current executable's directory
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            let lib_name = match std::env::consts::OS {
                "windows" => "onnxruntime.dll",
                "linux" => "libonnxruntime.so",
                "macos" => "libonnxruntime.dylib",
                _ => "onnxruntime.dll",
            };

            let path = dir.join(lib_name);
            if path.exists() {
                return Ok(path);
            }
        }

        Err("未找到共享 ONNX Runtime 运行库，请在 OCR 设置中安装 ONNX 运行环境。".into())
    }

    pub fn status(&self) -> TtsInstallStatus {
        let manifest = manifest();
        let model_ok = self.resolve_model();
        let runtime_ok = self.resolve_runtime();

        let total_bytes =
            manifest.model.bytes + manifest.voices.iter().map(|v| v.bytes).sum::<u64>();

        let voices = manifest
            .voices
            .iter()
            .map(|v| VoiceInfo {
                id: v.id.clone(),
                name: v.name.clone(),
                label: v.label.clone(),
                language: v.language.clone(),
                gender: v.gender.clone(),
            })
            .collect();

        let installed = model_ok.is_ok() && runtime_ok.is_ok();
        let error = if let Err(e) = model_ok {
            Some(e)
        } else {
            runtime_ok.err()
        };

        TtsInstallStatus {
            installed,
            installing: self.installing.load(Ordering::Acquire),
            path: if installed {
                Some(self.install_dir().display().to_string())
            } else {
                None
            },
            download_bytes: total_bytes,
            installed_bytes: if installed { total_bytes } else { 0 },
            error,
            voices,
        }
    }

    pub fn clean_cache(&self) {
        if self.cache_root.exists() {
            let _ = fs::remove_dir_all(&self.cache_root);
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
            return Err("TTS 资源正在安装中，请等待完成。".into());
        }

        struct Installing<'a>(&'a AtomicBool);
        impl Drop for Installing<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _installing = Installing(&self.installing);

        let manifest = manifest();
        let total = manifest.model.bytes + manifest.voices.iter().map(|v| v.bytes).sum::<u64>();

        let install_dir = self.install_dir();
        let voices_dir = install_dir.join("voices");
        fs::create_dir_all(&install_dir).map_err(|e| e.to_string())?;
        fs::create_dir_all(&voices_dir).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.cache_root).map_err(|e| e.to_string())?;
        clean_stale_partial_files(&self.cache_root);

        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(600))
            .user_agent("TransPortal/0.1 TTS asset installer")
            .build()
            .map_err(|e| e.to_string())?;

        let mut completed = 0;

        // 1. Download & verify model
        let model_dest = install_dir.join(&manifest.model.name);
        if verify_file(&model_dest, &manifest.model).is_err() {
            let downloaded = download(
                &client,
                &self.cache_root,
                &manifest.model,
                completed,
                total,
                progress,
            )
            .await?;
            copy_verified(&downloaded, &model_dest, &manifest.model)?;
        }
        completed += manifest.model.bytes;

        // 2. Download & verify default voices
        for voice in &manifest.voices {
            let voice_dest = voices_dir.join(&voice.name);
            let artifact = Artifact {
                name: voice.name.clone(),
                bytes: voice.bytes,
                sha256: voice.sha256.clone(),
                urls: voice.urls.clone(),
            };
            if verify_file(&voice_dest, &artifact).is_err() {
                let downloaded = download(
                    &client,
                    &self.cache_root,
                    &artifact,
                    completed,
                    total,
                    progress,
                )
                .await?;
                copy_verified(&downloaded, &voice_dest, &artifact)?;
            }
            completed += voice.bytes;
        }

        self.clean_cache();

        progress(InstallProgress {
            stage: "ready",
            file: String::new(),
            downloaded: total,
            total,
        });

        Ok(install_dir)
    }

    pub fn delete(&self) -> Result<(), String> {
        let dir = self.install_dir();
        if dir.exists() {
            fs::remove_dir_all(&dir).map_err(|e| format!("删除 TTS 资源失败：{e}"))?;
        }
        self.clean_cache();
        Ok(())
    }
}

fn clean_stale_partial_files(dir: &Path) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && let Some(name) = path.file_name().and_then(|s| s.to_str())
                && name.contains(".part-")
            {
                let _ = fs::remove_file(path);
            }
        }
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
    if destination.exists() {
        fs::remove_file(destination).map_err(|e| format!("无法修复资源，请重启后再试：{e}"))?;
    }
    fs::rename(temporary, destination).map_err(|e| format!("无法安装 TTS 资源：{e}"))
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
        return Err("TTS 清单包含无效文件名".into());
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
                    .send()
                    .await
                    .map_err(|e| format!("无法连接：{e}"))?;
                if !response.status().is_success() {
                    return Err(format!("HTTP 状态码 {}", response.status()));
                }
                let mut stream = response.bytes_stream();
                let mut file = tokio::fs::File::create(&temporary.0)
                    .await
                    .map_err(|e| e.to_string())?;
                let mut downloaded = completed;
                let mut last_emit = Instant::now();
                while let Some(chunk) = stream.next().await {
                    let chunk = chunk.map_err(|e| e.to_string())?;
                    file.write_all(&chunk).await.map_err(|e| e.to_string())?;
                    downloaded += chunk.len() as u64;
                    if last_emit.elapsed() >= Duration::from_millis(150) {
                        last_emit = Instant::now();
                        progress(InstallProgress {
                            stage: "downloading",
                            file: artifact.name.clone(),
                            downloaded,
                            total,
                        });
                    }
                }
                file.flush().await.map_err(|e| e.to_string())?;
                drop(file);
                verify_file(&temporary.0, artifact)?;
                publish_file(&temporary.0, &destination)?;
                Ok::<PathBuf, String>(destination.clone())
            }
            .await;
            match result {
                Ok(path) => return Ok(path),
                Err(error) => last_error = error,
            }
        }
    }
    Err(format!("下载 {} 失败：{last_error}", artifact.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_cache_and_stale_partial_files_removes_downloads() {
        let temp_dir =
            std::env::temp_dir().join(format!("transportal-tts-cache-test-{}", std::process::id()));
        let cache_root = temp_dir.join("downloads");
        let install_root = temp_dir.join("install");
        fs::create_dir_all(&cache_root).unwrap();

        // Create a stale partial download and a completed download
        let stale_part = cache_root.join("model.part-8888");
        let complete_file = cache_root.join("model_quantized.onnx");
        fs::write(&stale_part, b"partial data").unwrap();
        fs::write(&complete_file, b"complete data").unwrap();

        // Stale partial files should be cleaned up
        clean_stale_partial_files(&cache_root);
        assert!(!stale_part.exists());
        assert!(complete_file.exists());

        // clean_cache should remove the entire cache_root directory
        let assets = TtsAssets::new(install_root, cache_root.clone(), None);
        assets.clean_cache();
        assert!(!cache_root.exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
