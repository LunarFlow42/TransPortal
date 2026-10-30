//! Opt-in real inference using existing assets; never downloads or modifies them.
//! Set TRANSPORTAL_ONNX_SMOKE_DATA_DIR to the app data directory containing ocr/
//! and tts/. Set TRANSPORTAL_ONNX_SMOKE_TTS_FIRST=1 to test TTS-first startup.

use std::path::PathBuf;
use std::sync::Arc;
use transportal_lib::core::ocr::assets::{OnnxAssets, current_platform, manifest};
use transportal_lib::core::ocr::onnx::OnnxOcrEngine;
use transportal_lib::core::ocr::{OcrEngine, OcrImage};
use transportal_lib::core::tts::assets::TtsAssets;
use transportal_lib::core::tts::chunker::SentenceChunker;
use transportal_lib::core::tts::engine::{KokoroEngine, SAMPLE_RATE};

#[test]
fn installed_engines_share_runtime_and_reload_sessions() {
    let Some(data_dir) = std::env::var_os("TRANSPORTAL_ONNX_SMOKE_DATA_DIR") else {
        eprintln!("Set TRANSPORTAL_ONNX_SMOKE_DATA_DIR to run real OCR/TTS inference.");
        return;
    };
    let data_dir = PathBuf::from(data_dir);
    let ocr_root = data_dir.join("ocr");
    let ocr = OnnxOcrEngine::new(Arc::new(OnnxAssets::new(
        None,
        ocr_root.clone(),
        ocr_root.join("downloads"),
    )));
    let tts = KokoroEngine::new(Arc::new(TtsAssets::new(
        data_dir.join("tts"),
        data_dir.join("tts/downloads"),
        Some(ocr_root.join(&manifest().id).join(current_platform())),
    )));
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr/english-light.png");
    let image = image::open(fixture).unwrap().into_rgba8();

    let recognize = || {
        let result = ocr
            .recognize(
                OcrImage {
                    width: image.width(),
                    height: image.height(),
                    rgba: image.as_raw(),
                },
                None,
            )
            .expect("real OCR inference failed");
        let compact: String = result.text.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(compact, "LocalOCRsmoketest.Translatethisscreenshot.");
        assert!(!result.blocks.is_empty());
    };
    let synthesize = || {
        let chunks = SentenceChunker::global()
            .chunk_and_tokenize("Local ONNX smoke test.", false)
            .unwrap();
        let wav = tts
            .synthesize_chunks(chunks, "af_heart", 1.0)
            .expect("real TTS inference failed");
        assert!(wav.len() > 44);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[24..28], &SAMPLE_RATE.to_le_bytes());
        assert!(wav[44..].iter().any(|&byte| byte != 0));
    };

    for _ in 0..2 {
        if std::env::var_os("TRANSPORTAL_ONNX_SMOKE_TTS_FIRST").is_some() {
            synthesize();
            recognize();
        } else {
            recognize();
            synthesize();
        }
        // Both sessions are dropped before the next cycle; the shared
        // environment must remain alive so `ort` can create new sessions.
        ocr.release();
        tts.release();
    }
}
