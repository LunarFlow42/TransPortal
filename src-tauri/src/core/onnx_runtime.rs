//! Shared ONNX Runtime environment for OCR and TTS.

use ort::environment::Environment;
use std::path::Path;
use std::sync::Mutex;

/// `ort` permits only one environment per process. Keep a strong handle even
/// after all inference sessions are released, and serialize OCR/TTS startup.
pub(crate) fn environment(path: &Path) -> Result<Environment, String> {
    static ENVIRONMENT: Mutex<Option<Environment>> = Mutex::new(None);
    let mut environment = ENVIRONMENT
        .lock()
        .map_err(|_| "ONNX Runtime 环境锁不可用，请重启应用。")?;
    if let Some(environment) = environment.as_ref() {
        return Ok(environment.clone());
    }

    let initialized = ort::init_from(path)
        .map_err(|e| format!("ONNX Runtime 动态库加载失败：{e}"))?
        .with_name("TransPortal OCR/TTS")
        .with_telemetry(false)
        .build()
        .map_err(|e| format!("ONNX Runtime 初始化失败：{e}"))?;
    *environment = Some(initialized.clone());
    Ok(initialized)
}

#[cfg(test)]
mod tests {
    #[test]
    fn missing_runtime_returns_an_error_and_can_be_retried() {
        let missing = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join(format!("missing-onnx-runtime-{}.dll", std::process::id()));
        assert!(!missing.exists());
        for _ in 0..2 {
            let error = super::environment(&missing).unwrap_err();
            assert!(error.contains("ONNX Runtime 动态库加载失败"), "{error}");
        }
    }

    #[test]
    fn api_version_matches_ocr_and_tts_resources() {
        let expected = format!("1.{}.0", ort::sys::ORT_API_VERSION);
        assert_eq!(
            crate::core::ocr::assets::manifest().runtime_version,
            expected
        );
        assert_eq!(
            crate::core::tts::assets::manifest().runtime_version,
            expected
        );
    }

    #[test]
    fn ocr_and_tts_arrays_match_ort_tensor_types() {
        fn assert_tensor_data<T: ort::value::OwnedTensorArrayData<E>, E>() {}

        assert_tensor_data::<ndarray::Array4<f32>, f32>();
        assert_tensor_data::<ndarray::Array2<i64>, i64>();
        assert_tensor_data::<ndarray::Array2<f32>, f32>();
        assert_tensor_data::<ndarray::Array1<f32>, f32>();
    }
}
