//! Headless installation / inference probe; no screen or clipboard access.
//! `cargo run --example ocr_probe -- prepare` prepares resources for bundling.
//! `cargo run --example ocr_probe -- --engine onnx --image sample.png --repeat 3`
//! Legacy `image.rgba width height [language]` still probes Windows OCR.

use std::path::PathBuf;
use std::sync::Arc;
use transportal_lib::core::ocr::assets::{OnnxAssets, current_platform, manifest};
use transportal_lib::core::ocr::onnx::OnnxOcrEngine;
use transportal_lib::core::ocr::{OcrEngine, OcrImage};

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let project = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let destination = option(&args, "--assets-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| project.join("resources/ocr"));
    let cache = option(&args, "--cache-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| project.join("target/onnx-downloads"));
    let platform = option(&args, "--platform").unwrap_or_else(current_platform);
    let assets = Arc::new(OnnxAssets::for_platform(None, destination, cache, platform));

    if args.first().map(String::as_str) == Some("prepare") {
        let path = assets
            .install(&|progress| {
                eprintln!(
                    "[ocr-assets] {} {} {}/{}",
                    progress.stage, progress.file, progress.downloaded, progress.total
                );
            })
            .await?;
        println!(
            "{}",
            serde_json::json!({ "path": path, "pack": manifest().id, "installed": true })
        );
        return Ok(());
    }

    let selected = option(&args, "--engine").unwrap_or_else(|| "windows".into());
    let engine: Arc<dyn OcrEngine> = match selected.as_str() {
        "onnx" => Arc::new(OnnxOcrEngine::new(assets)),
        "windows" => transportal_lib::platform::native_ocr()
            .ok_or("system OCR is not available on this platform")?,
        _ => return Err("--engine must be windows or onnx".into()),
    };
    let image_path = option(&args, "--image");
    let legacy = args.first().is_some_and(|arg| !arg.starts_with("--"));
    if image_path.is_none() && !legacy {
        println!(
            "{}",
            serde_json::to_string_pretty(&engine.info()?).map_err(|e| e.to_string())?
        );
        return Ok(());
    }

    let (bytes, width, height, language) = if let Some(path) = image_path {
        let image = image::open(path).map_err(|e| e.to_string())?.into_rgba8();
        let (width, height) = image.dimensions();
        (image.into_raw(), width, height, option(&args, "--language"))
    } else {
        if args.len() < 3 || args.len() > 4 {
            return Err("usage: ocr_probe [image.rgba width height [language]] or --engine onnx --image image.png".into());
        }
        (
            std::fs::read(&args[0]).map_err(|e| e.to_string())?,
            args[1].parse().map_err(|_| "invalid width")?,
            args[2].parse().map_err(|_| "invalid height")?,
            args.get(3).cloned(),
        )
    };
    let repeat = option(&args, "--repeat")
        .unwrap_or_else(|| "1".into())
        .parse::<usize>()
        .map_err(|_| "invalid repeat")?;
    if !(1..=20).contains(&repeat) {
        return Err("repeat must be between 1 and 20".into());
    }
    let mut times = Vec::new();
    let mut last = None;
    for _ in 0..repeat {
        let start = std::time::Instant::now();
        let result = engine.recognize(
            OcrImage {
                width,
                height,
                rgba: &bytes,
            },
            language.as_deref(),
        )?;
        times.push(start.elapsed().as_millis());
        last = Some(result);
    }
    let result = last.ok_or("no recognition result")?;
    println!(
        "{}",
        serde_json::json!({
            "engine": engine.id(), "language": result.language, "text": result.text,
            "blocks": result.blocks,
            "elapsed_ms": times[0], "runs_ms": times, "width": width, "height": height,
        })
    );
    Ok(())
}
