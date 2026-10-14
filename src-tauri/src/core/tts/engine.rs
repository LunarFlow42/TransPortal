//! Kokoro ONNX inference engine and WAV audio packaging.

use super::assets::TtsAssets;
use super::chunker::TtsError;
use ndarray::{Array1, Array2};
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use std::collections::HashMap;
use std::ffi::CStr;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const SAMPLE_RATE: u32 = 24000;
const PAUSE_SAMPLES_COUNT: usize = 3600; // 150ms pause at 24kHz

pub struct KokoroEngine {
    assets: Arc<TtsAssets>,
    session: Mutex<Option<Session>>,
    voices: Mutex<HashMap<String, Vec<f32>>>,
}

impl KokoroEngine {
    pub fn new(assets: Arc<TtsAssets>) -> Self {
        Self {
            assets,
            session: Mutex::new(None),
            voices: Mutex::new(HashMap::new()),
        }
    }

    /// Releases loaded session and voice caches to free memory and file locks.
    pub fn release(&self) {
        if let Ok(mut session) = self.session.lock() {
            *session = None;
        }
        if let Ok(mut voices) = self.voices.lock() {
            voices.clear();
        }
    }

    fn ensure_session(&self) -> Result<(), TtsError> {
        let mut session_guard = self
            .session
            .lock()
            .map_err(|_| TtsError::Inference("TTS 会话锁不可用，请重启应用。".into()))?;

        if session_guard.is_some() {
            return Ok(());
        }

        let model_path = self
            .assets
            .resolve_model()
            .map_err(TtsError::AssetNotReady)?;
        let runtime_path = self
            .assets
            .resolve_runtime()
            .map_err(TtsError::AssetNotReady)?;

        init_runtime(&runtime_path).map_err(TtsError::Inference)?;

        let threads = std::thread::available_parallelism()
            .map(|n| (n.get() / 2).clamp(2, 4))
            .unwrap_or(2);

        let builder = Session::builder()
            .map_err(|e| TtsError::Inference(format!("创建会话构建器失败: {e}")))?;
        let builder = builder
            .with_execution_providers([CPUExecutionProvider::default()
                .with_arena_allocator(false)
                .build()])
            .map_err(|e| TtsError::Inference(format!("配置 CPU Provider 失败: {e}")))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| TtsError::Inference(format!("配置优化等级失败: {e}")))?
            .with_intra_threads(threads as usize)
            .map_err(|e| TtsError::Inference(format!("配置线程失败: {e}")))?
            .with_inter_threads(1)
            .map_err(|e| TtsError::Inference(format!("配置线程失败: {e}")))?
            .with_parallel_execution(false)
            .map_err(|e| TtsError::Inference(format!("配置线程失败: {e}")))?
            .with_memory_pattern(false)
            .map_err(|e| TtsError::Inference(format!("配置内存模式失败: {e}")))?
            .with_config_entry("session.intra_op.allow_spinning", "0")
            .map_err(|e| TtsError::Inference(format!("配置自旋失败: {e}")))?
            .with_config_entry("session.inter_op.allow_spinning", "0")
            .map_err(|e| TtsError::Inference(format!("配置自旋失败: {e}")))?;

        let session = builder
            .commit_from_file(model_path)
            .map_err(|e| TtsError::Inference(format!("ONNX 模型加载失败: {e}")))?;

        *session_guard = Some(session);
        Ok(())
    }

    fn get_or_load_voice(&self, voice_id: &str) -> Result<Vec<f32>, TtsError> {
        let mut voices = self
            .voices
            .lock()
            .map_err(|_| TtsError::Inference("音色缓存锁不可用".into()))?;

        if let Some(voice_floats) = voices.get(voice_id) {
            return Ok(voice_floats.clone());
        }

        let voice_path = self
            .assets
            .resolve_voice(voice_id)
            .map_err(TtsError::UnknownVoice)?;

        let mut file = File::open(&voice_path)
            .map_err(|e| TtsError::Inference(format!("无法读取音色文件: {e}")))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .map_err(|e| TtsError::Inference(format!("无法加载音色文件: {e}")))?;

        if buffer.len() != 522240 {
            return Err(TtsError::Inference(format!(
                "音色文件大小异常：期望 522240 字节，实际 {} 字节",
                buffer.len()
            )));
        }

        let floats: Vec<f32> = buffer
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| f32::from_le_bytes(*chunk))
            .collect();

        voices.insert(voice_id.to_string(), floats.clone());
        Ok(floats)
    }

    /// Performs inference across one or more chunks and stitches them into a 24kHz WAV.
    pub fn synthesize_chunks(
        &self,
        chunks: Vec<(Vec<i64>, usize)>,
        voice_id: &str,
        speed: f32,
    ) -> Result<Vec<u8>, TtsError> {
        self.ensure_session()?;
        let voice_floats = self.get_or_load_voice(voice_id)?;

        let mut session_guard = self
            .session
            .lock()
            .map_err(|_| TtsError::Inference("TTS 会话锁不可用".into()))?;
        let session = session_guard
            .as_mut()
            .ok_or_else(|| TtsError::Inference("ONNX 会话未就绪".into()))?;

        let (input_id_name, style_name, speed_name) = {
            let id_name = session
                .inputs
                .iter()
                .find(|i| i.name == "input_ids" || i.name == "tokens")
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "input_ids".into());
            let st_name = session
                .inputs
                .iter()
                .find(|i| i.name == "style")
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "style".into());
            let sp_name = session
                .inputs
                .iter()
                .find(|i| i.name == "speed")
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "speed".into());
            (id_name, st_name, sp_name)
        };

        let mut total_samples: Vec<f32> = Vec::new();

        for (chunk_idx, (token_ids, raw_phoneme_count)) in chunks.into_iter().enumerate() {
            // Style slice formula: raw phoneme length clamped to 1..=510, minus 1
            let style_idx = raw_phoneme_count.clamp(1, 510) - 1;
            let style_slice = &voice_floats[style_idx * 256..(style_idx + 1) * 256];

            let ids_array = Array2::from_shape_vec((1, token_ids.len()), token_ids)
                .map_err(|e| TtsError::Inference(e.to_string()))?;
            let style_array = Array2::from_shape_vec((1, 256), style_slice.to_vec())
                .map_err(|e| TtsError::Inference(e.to_string()))?;
            let speed_array = Array1::from_shape_vec(1, vec![speed])
                .map_err(|e| TtsError::Inference(e.to_string()))?;

            let tensor_ids = Tensor::from_array(ids_array)
                .map_err(|e| TtsError::Inference(format!("输入 Tensor 构建失败: {e}")))?;
            let tensor_style = Tensor::from_array(style_array)
                .map_err(|e| TtsError::Inference(format!("风格 Tensor 构建失败: {e}")))?;
            let tensor_speed = Tensor::from_array(speed_array)
                .map_err(|e| TtsError::Inference(format!("语速 Tensor 构建失败: {e}")))?;

            let outputs = session
                .run(inputs![
                    input_id_name.as_str() => tensor_ids,
                    style_name.as_str() => tensor_style,
                    speed_name.as_str() => tensor_speed
                ])
                .map_err(|e| TtsError::Inference(format!("ONNX 推理失败: {e}")))?;

            let (_, out_val) = outputs
                .iter()
                .next()
                .ok_or_else(|| TtsError::Inference("ONNX 未产生音频输出".into()))?;
            let (_, pred_slice) = out_val
                .try_extract_tensor::<f32>()
                .map_err(|e| TtsError::Inference(format!("提取音频数据失败: {e}")))?;

            if chunk_idx > 0 && !total_samples.is_empty() {
                // Insert 150ms natural pause (3600 zero samples) between sentences
                total_samples.resize(total_samples.len() + PAUSE_SAMPLES_COUNT, 0.0);
            }
            total_samples.extend_from_slice(pred_slice);
        }

        if total_samples.is_empty() {
            return Err(TtsError::Inference("生成的音频流为空".into()));
        }

        Ok(pack_wav(&total_samples, SAMPLE_RATE))
    }
}

/// Encodes raw PCM float samples into standard 16-bit mono PCM WAV bytes (44-byte header).
pub fn pack_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let num_samples = samples.len() as u32;
    let data_len = num_samples * 2; // 16-bit = 2 bytes per sample
    let file_len = 36 + data_len;
    let mut wav = Vec::with_capacity(44 + data_len as usize);

    // 1. RIFF header
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&file_len.to_le_bytes());
    wav.extend_from_slice(b"WAVE");

    // 2. fmt subchunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // subchunk1 size (16 for PCM)
    wav.extend_from_slice(&1u16.to_le_bytes()); // audio format (1 = PCM)
    wav.extend_from_slice(&1u16.to_le_bytes()); // num channels (1 = mono)
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * 2;
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align (channels * bits / 8)
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample (16)

    // 3. data subchunk
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());

    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm = (clamped * 32767.0) as i16;
        wav.extend_from_slice(&pcm.to_le_bytes());
    }

    wav
}

fn init_runtime(path: &Path) -> Result<(), String> {
    static RUNTIME: Mutex<Option<libloading::Library>> = Mutex::new(None);
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "ONNX Runtime 状态锁不可用".to_string())?;

    if runtime.is_some() {
        return Ok(());
    }

    let path = path
        .canonicalize()
        .map_err(|e| format!("ONNX Runtime 路径无效：{e}"))?;

    let library = unsafe { libloading::Library::new(&path) }
        .map_err(|e| format!("无法加载 ONNX Runtime 动态库：{e}"))?;

    unsafe {
        let getter: libloading::Symbol<unsafe extern "C" fn() -> *const ort::sys::OrtApiBase> =
            library
                .get(b"OrtGetApiBase\0")
                .map_err(|e| format!("ONNX Runtime API 符号丢失：{e}"))?;
        let base = getter();
        if base.is_null() {
            return Err("ONNX Runtime 返回了空的 API 表".into());
        }
        let version = ((*base).GetVersionString)();
        if version.is_null() {
            return Err("无法读取 ONNX Runtime 版本".into());
        }
        let _ = CStr::from_ptr(version)
            .to_str()
            .map_err(|e| e.to_string())?;
    }

    let path_str = path
        .to_str()
        .ok_or_else(|| "ONNX Runtime 路径无法编码为 Unicode".to_string())?;

    match ort::init_from(path_str)
        .with_name("TransPortal TTS")
        .with_telemetry(false)
        .commit()
    {
        Ok(_) => {}
        Err(e) if e.to_string().contains("already initialized") => {}
        Err(e) => return Err(format!("ONNX Runtime 初始化失败：{e}")),
    }

    *runtime = Some(library);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wav_packing_header() {
        let samples = vec![0.0f32, 0.5, -0.5, 1.0, -1.0];
        let wav = pack_wav(&samples, 24000);
        assert_eq!(wav.len(), 44 + 10);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
    }
}
