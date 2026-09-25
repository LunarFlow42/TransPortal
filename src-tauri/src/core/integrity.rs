//! Release-file integrity, not publisher authentication or a malware scanner.
//! The manifest is generated from the final executable while NSIS packages it.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

pub const MANIFEST_NAME: &str = "integrity.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    algorithm: String,
    application_version: String,
    file: String,
    file_size: u64,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    Verified,
    Development,
    Unsupported,
    MissingManifest,
    InvalidManifest,
    Mismatch,
    IoError,
}

#[derive(Clone, Debug, Serialize)]
pub struct IntegrityReport {
    pub status: IntegrityStatus,
    pub version: String,
    pub expected_sha256: Option<String>,
    pub actual_sha256: Option<String>,
    pub error: Option<String>,
}

impl IntegrityReport {
    fn new(status: IntegrityStatus) -> Self {
        Self {
            status,
            version: env!("CARGO_PKG_VERSION").into(),
            expected_sha256: None,
            actual_sha256: None,
            error: None,
        }
    }

    fn failed(status: IntegrityStatus, error: impl Into<String>) -> Self {
        Self {
            error: Some(error.into()),
            ..Self::new(status)
        }
    }

    pub fn blocks_startup(&self) -> bool {
        !matches!(
            self.status,
            IntegrityStatus::Verified | IntegrityStatus::Development | IntegrityStatus::Unsupported
        )
    }
}

pub struct IntegrityState(pub Mutex<IntegrityReport>);

fn sha256_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok((format!("{:x}", digest.finalize()), bytes))
}

/// Hash every executable byte, including Tauri's bundle tag and any signature.
/// Never establish or replace a baseline on an end user's machine.
pub fn verify_file(executable: &Path, manifest_path: &Path, version: &str) -> IntegrityReport {
    let metadata = match fs::symlink_metadata(manifest_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return IntegrityReport::failed(
                IntegrityStatus::MissingManifest,
                "缺少程序校验文件，已停止启动。请使用完整的可信安装包重新安装。",
            );
        }
        Err(error) => {
            return IntegrityReport::failed(
                IntegrityStatus::IoError,
                format!("无法读取程序校验文件：{error}"),
            );
        }
    };
    if !metadata.is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return IntegrityReport::failed(
            IntegrityStatus::InvalidManifest,
            "程序校验文件格式异常，请重新安装。",
        );
    }
    let manifest = (|| -> Result<Manifest, String> {
        let mut bytes = Vec::new();
        File::open(manifest_path)
            .map_err(|error| error.to_string())?
            .take(MAX_MANIFEST_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err("校验文件过大".into());
        }
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())
    })();
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            return IntegrityReport::failed(
                IntegrityStatus::InvalidManifest,
                format!("程序校验文件无法解析：{error}"),
            );
        }
    };
    if manifest.schema != 1
        || manifest.algorithm != "sha256"
        || manifest.application_version != version
        || executable.file_name().and_then(|name| name.to_str()) != Some(manifest.file.as_str())
        || manifest.sha256.len() != 64
        || !manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return IntegrityReport::failed(
            IntegrityStatus::InvalidManifest,
            "程序校验文件与当前版本不匹配，请重新安装。",
        );
    }
    let (actual, size) = match sha256_file(executable) {
        Ok(value) => value,
        Err(error) => {
            return IntegrityReport::failed(
                IntegrityStatus::IoError,
                format!("无法读取程序文件：{error}"),
            );
        }
    };
    let matches = size == manifest.file_size && actual.eq_ignore_ascii_case(&manifest.sha256);
    IntegrityReport {
        status: if matches {
            IntegrityStatus::Verified
        } else {
            IntegrityStatus::Mismatch
        },
        version: version.into(),
        expected_sha256: Some(manifest.sha256.to_ascii_lowercase()),
        actual_sha256: Some(actual),
        error: (!matches).then(|| {
            "程序文件与发布校验值不一致，可能已损坏或被修改。已停止运行，请重新安装可信安装包。"
                .into()
        }),
    }
}

pub fn check_current_executable() -> IntegrityReport {
    match std::env::current_exe() {
        Ok(executable) => match executable.parent() {
            Some(directory) => verify_file(
                &executable,
                &directory.join(MANIFEST_NAME),
                env!("CARGO_PKG_VERSION"),
            ),
            None => IntegrityReport::failed(IntegrityStatus::IoError, "无法确定程序所在目录。"),
        },
        Err(error) => IntegrityReport::failed(
            IntegrityStatus::IoError,
            format!("无法定位程序文件：{error}"),
        ),
    }
}

pub fn startup_report() -> IntegrityReport {
    if cfg!(debug_assertions) {
        IntegrityReport::new(IntegrityStatus::Development)
    } else if crate::native::integrity::platform().enforce_release_hash() {
        check_current_executable()
    } else {
        IntegrityReport::new(IntegrityStatus::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "transportal-integrity-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            fs::write(root.join("transportal.exe"), b"trusted executable fixture").unwrap();
            Self(root)
        }
        fn seal(&self, version: &str) {
            let executable = self.0.join("transportal.exe");
            let (sha256, file_size) = sha256_file(&executable).unwrap();
            fs::write(
                self.0.join(MANIFEST_NAME),
                serde_json::to_vec(&serde_json::json!({
                    "schema": 1, "algorithm": "sha256", "application_version": version,
                    "file": "transportal.exe", "file_size": file_size, "sha256": sha256,
                }))
                .unwrap(),
            )
            .unwrap();
        }
        fn check(&self) -> IntegrityReport {
            verify_file(
                &self.0.join("transportal.exe"),
                &self.0.join(MANIFEST_NAME),
                "0.1.0",
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // Only remove the two fixture files created by this test.
            let _ = fs::remove_file(self.0.join("transportal.exe"));
            let _ = fs::remove_file(self.0.join(MANIFEST_NAME));
            let _ = fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn detects_same_length_changes_and_appended_content() {
        let fixture = Fixture::new();
        fixture.seal("0.1.0");
        assert_eq!(fixture.check().status, IntegrityStatus::Verified);
        let mut bytes = fs::read(fixture.0.join("transportal.exe")).unwrap();
        bytes[0] ^= 1;
        fs::write(fixture.0.join("transportal.exe"), &bytes).unwrap();
        let result = fixture.check();
        assert_eq!(result.status, IntegrityStatus::Mismatch);
        assert_ne!(result.actual_sha256, result.expected_sha256);
        assert!(result.blocks_startup());
        bytes[0] ^= 1;
        bytes.push(0);
        fs::write(fixture.0.join("transportal.exe"), bytes).unwrap();
        assert_eq!(fixture.check().status, IntegrityStatus::Mismatch);
    }

    #[test]
    fn missing_malformed_or_wrong_version_manifest_never_creates_a_baseline() {
        let fixture = Fixture::new();
        assert_eq!(fixture.check().status, IntegrityStatus::MissingManifest);
        assert!(!fixture.0.join(MANIFEST_NAME).exists());
        fs::write(fixture.0.join(MANIFEST_NAME), b"{}").unwrap();
        assert_eq!(fixture.check().status, IntegrityStatus::InvalidManifest);
        fixture.seal("0.0.9");
        assert_eq!(fixture.check().status, IntegrityStatus::InvalidManifest);
        fs::write(
            fixture.0.join(MANIFEST_NAME),
            vec![b' '; MAX_MANIFEST_BYTES as usize + 1],
        )
        .unwrap();
        assert_eq!(fixture.check().status, IntegrityStatus::InvalidManifest);
    }
}
