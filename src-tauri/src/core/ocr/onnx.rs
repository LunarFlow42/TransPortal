//! RapidOCR's PaddleOCR FP32 models executed by ONNX Runtime's CPU provider.

use super::assets::{OnnxAssets, manifest};
use super::picture::OcrBlock;
use super::{OcrEngine, OcrEngineInfo, OcrImage, OcrLanguage, RecognizedText};
use ort::execution_providers::CPUExecutionProvider;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use paddle_ocr_rs::ocr_lite::OcrLite;
use paddle_ocr_rs::ocr_result::TextBlock;
use std::ffi::CStr;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const ENGINE_ID: &str = "onnx";
pub const MODEL_LANGUAGE: &str = "zh-Hans-en";

pub struct OnnxOcrEngine {
    assets: Arc<OnnxAssets>,
    session: Mutex<Option<OcrLite>>,
}

impl OnnxOcrEngine {
    pub fn new(assets: Arc<OnnxAssets>) -> Self {
        Self {
            assets,
            session: Mutex::new(None),
        }
    }

    fn load(&self) -> Result<OcrLite, String> {
        let directory = self.assets.resolve()?;
        let runtime = self.assets.runtime()?;
        init_runtime(&directory.join(&runtime.library))?;
        let models = &manifest().models;
        let paths: Vec<String> = models
            .iter()
            .map(|model| directory.join(&model.name))
            .map(|path| {
                path.to_str()
                    .map(String::from)
                    .ok_or_else(|| "OCR 资源路径无法编码为 Unicode".to_string())
            })
            .collect::<Result<_, _>>()?;
        let mut ocr = OcrLite::new();
        ocr.init_models_custom(&paths[0], &paths[1], &paths[2], cpu_session)
            .map_err(|e| format!("ONNX 模型初始化失败：{e}"))?;
        Ok(ocr)
    }
}

impl OcrEngine for OnnxOcrEngine {
    fn id(&self) -> &'static str {
        ENGINE_ID
    }

    fn info(&self) -> Result<OcrEngineInfo, String> {
        let error = self.assets.resolve().err();
        Ok(OcrEngineInfo {
            id: ENGINE_ID.into(),
            label: "ONNX · PP-OCRv4 FP32".into(),
            languages: vec![OcrLanguage {
                tag: MODEL_LANGUAGE.into(),
                label: "简体中文 + English".into(),
            }],
            available: error.is_none(),
            error,
        })
    }

    fn prepare(&self) -> Result<(), String> {
        let mut session = self
            .session
            .lock()
            .map_err(|_| "ONNX 会话不可用，请重启应用。")?;
        if session.is_none() {
            *session = Some(self.load()?);
        }
        Ok(())
    }

    fn release(&self) {
        if let Ok(mut session) = self.session.lock() {
            *session = None;
        }
    }

    fn recognize(
        &self,
        image: OcrImage<'_>,
        language: Option<&str>,
    ) -> Result<RecognizedText, String> {
        image.validate()?;
        if language.is_some_and(|language| language != MODEL_LANGUAGE) {
            return Err("当前 ONNX 模型支持简体中文和英文，请使用中英混合识别。".into());
        }
        self.prepare()?;
        let rgb = screen_rgb(image)?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| "ONNX 会话不可用，请重启应用。")?;
        // `release` can run between prepare() and this lock on explicit engine
        // changes. Loading under the same mutex keeps this public API race-safe.
        if session.is_none() {
            *session = Some(self.load()?);
        }
        let ocr = session.as_mut().ok_or("ONNX 模型未初始化")?;
        let max_dimension = image.width.max(image.height);
        let max_side_len = max_dimension.clamp(320, 1536);
        let result = ocr
            .detect_angle_rollback(&rgb, 16, max_side_len, 0.5, 0.3, 1.6, false, false, 0.75)
            .map_err(|e| format!("ONNX 识别失败：{e}"))?;
        let (text, blocks) = ordered_result(result.text_blocks);
        Ok(RecognizedText {
            text,
            language: MODEL_LANGUAGE.into(),
            blocks,
        })
    }
}

fn screen_rgb(image: OcrImage<'_>) -> Result<image::RgbImage, String> {
    // Composite transparent PNG pixels onto white before estimating polarity.
    let mut pixels = Vec::with_capacity(image.width as usize * image.height as usize * 3);
    for rgba in image.rgba.as_chunks::<4>().0 {
        let alpha = u32::from(rgba[3]);
        for channel in &rgba[..3] {
            pixels.push(((u32::from(*channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8);
        }
    }
    let mut rgb =
        image::RgbImage::from_raw(image.width, image.height, pixels).ok_or("OCR 图片数据无效")?;
    // Consistently dark screen backgrounds are decoded more accurately after
    // inversion, notably tiny punctuation. Border sampling avoids counting text
    // ink or invisible RGB values in a transparent image as the background.
    let mut dark = 0usize;
    let mut samples = 0usize;
    let mut sample = |x, y| {
        let pixel = rgb.get_pixel(x, y);
        let luminance =
            (77 * u32::from(pixel[0]) + 150 * u32::from(pixel[1]) + 29 * u32::from(pixel[2])) / 256;
        dark += usize::from(luminance < 96);
        samples += 1;
    };
    for x in (0..image.width).step_by((image.width / 128).max(1) as usize) {
        sample(x, 0);
        sample(x, image.height - 1);
    }
    for y in (0..image.height).step_by((image.height / 128).max(1) as usize) {
        sample(0, y);
        sample(image.width - 1, y);
    }
    if dark * 4 >= samples * 3 {
        image::imageops::invert(&mut rgb);
    }
    Ok(rgb)
}

fn cpu_session(builder: SessionBuilder) -> ort::Result<SessionBuilder> {
    let threads = std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(2, 8))
        .unwrap_or(4);
    builder
        .with_execution_providers([CPUExecutionProvider::default()
            .with_arena_allocator(false)
            .build()])?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(threads as usize)?
        .with_inter_threads(1)?
        .with_parallel_execution(false)?
        .with_memory_pattern(false)?
        .with_config_entry("session.intra_op.allow_spinning", "0")?
        .with_config_entry("session.inter_op.allow_spinning", "0")
}

/// Keep the preflight library loaded. `ort`'s dynamic loader panics on a missing
/// DLL or incompatible API, so validate both through libloading first and return
/// a normal settings error, even in release builds using panic=abort.
fn init_runtime(path: &Path) -> Result<(), String> {
    static RUNTIME: Mutex<Option<libloading::Library>> = Mutex::new(None);
    let mut runtime = RUNTIME
        .lock()
        .map_err(|_| "ONNX Runtime 状态不可用，请重启应用。")?;
    if runtime.is_some() {
        return Ok(());
    }
    let path = path
        .canonicalize()
        .map_err(|e| format!("ONNX Runtime 路径无效：{e}"))?;
    // SAFETY: the artifact's exact size/SHA-256 and platform were checked by
    // OnnxAssets before loading this official, pinned runtime library.
    let library = unsafe { libloading::Library::new(&path) }
        .map_err(|e| format!("无法加载 ONNX Runtime，请检查系统运行库或重新安装 OCR 资源：{e}"))?;
    // SAFETY: OrtGetApiBase is ONNX Runtime's documented C entry point. Verify
    // the returned pointers and ABI version before any ort API can be called.
    unsafe {
        let getter: libloading::Symbol<unsafe extern "C" fn() -> *const ort::sys::OrtApiBase> =
            library
                .get(b"OrtGetApiBase\0")
                .map_err(|e| format!("ONNX Runtime API 不可用：{e}"))?;
        let base = getter();
        if base.is_null() {
            return Err("ONNX Runtime 返回了空的 API 表".into());
        }
        let version = ((*base).GetVersionString)();
        if version.is_null() {
            return Err("无法读取 ONNX Runtime 版本".into());
        }
        let version = CStr::from_ptr(version)
            .to_str()
            .map_err(|e| e.to_string())?;
        if version != manifest().runtime_version
            || ((*base).GetApi)(ort::sys::ORT_API_VERSION).is_null()
        {
            return Err(format!(
                "ONNX Runtime 版本不匹配：需要 {}，实际 {version}",
                manifest().runtime_version
            ));
        }
    }
    let path = path.to_str().ok_or("ONNX Runtime 路径无法编码为 Unicode")?;
    ort::init_from(path)
        .with_name("TransPortal OCR")
        .with_telemetry(false)
        .commit()
        .map_err(|e| format!("ONNX Runtime 初始化失败：{e}"))?;
    *runtime = Some(library);
    Ok(())
}

/// Detector contour order is not reading order. Sort rows top-to-bottom and
/// fragments left-to-right; preserve a newline between actual text rows.
fn ordered_result(blocks: Vec<TextBlock>) -> (String, Vec<OcrBlock>) {
    struct Line {
        top: u32,
        bottom: u32,
        blocks: Vec<TextBlock>,
    }
    let bounds = |block: &TextBlock| {
        let top = block
            .box_points
            .iter()
            .map(|point| point.y)
            .min()
            .unwrap_or(0);
        let bottom = block
            .box_points
            .iter()
            .map(|point| point.y)
            .max()
            .unwrap_or(top);
        (top, bottom)
    };
    let mut blocks: Vec<_> = blocks
        .into_iter()
        .filter(|block| {
            block.text_score.is_finite() && block.text_score >= 0.5 && !block.text.trim().is_empty()
        })
        .collect();
    blocks.sort_by_key(|block| bounds(block).0);
    let mut lines: Vec<Line> = Vec::new();
    for block in blocks {
        let (top, bottom) = bounds(&block);
        let row = lines.last_mut().filter(|line| {
            let overlap = bottom.min(line.bottom).saturating_sub(top.max(line.top));
            overlap * 2
                >= bottom
                    .saturating_sub(top)
                    .min(line.bottom.saturating_sub(line.top))
                    .max(1)
        });
        if let Some(line) = row {
            line.top = line.top.min(top);
            line.bottom = line.bottom.max(bottom);
            line.blocks.push(block);
        } else {
            lines.push(Line {
                top,
                bottom,
                blocks: vec![block],
            });
        }
    }
    let mut regions = Vec::new();
    let text = lines
        .into_iter()
        .map(|mut line| {
            line.blocks.sort_by_key(|block| {
                block
                    .box_points
                    .iter()
                    .map(|point| point.x)
                    .min()
                    .unwrap_or(0)
            });
            for block in &line.blocks {
                let left = block.box_points.iter().map(|p| p.x).min().unwrap_or(0);
                let right = block.box_points.iter().map(|p| p.x).max().unwrap_or(left);
                let (top, bottom) = bounds(block);
                regions.push(OcrBlock {
                    id: regions.len() as u32,
                    text: block.text.trim().into(),
                    x: left as f32,
                    y: top as f32,
                    width: right.saturating_sub(left) as f32,
                    height: bottom.saturating_sub(top) as f32,
                });
            }
            line.blocks
                .iter()
                .map(|block| block.text.trim())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    (text, regions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddle_ocr_rs::ocr_result::Point;

    #[test]
    fn screen_preprocessing_normalizes_dark_mode_and_preserves_transparent_text() {
        let mut rgba = [20, 20, 20, 255].repeat(25);
        rgba[12 * 4..13 * 4].copy_from_slice(&[255, 255, 255, 255]);
        let rgb = screen_rgb(OcrImage {
            width: 5,
            height: 5,
            rgba: &rgba,
        })
        .unwrap();
        assert_eq!(rgb.get_pixel(0, 0).0, [235, 235, 235]);
        assert_eq!(rgb.get_pixel(2, 2).0, [0, 0, 0]);
        let mut rgba = [0, 0, 0, 0].repeat(25);
        rgba[12 * 4..13 * 4].copy_from_slice(&[0, 0, 0, 255]);
        let rgb = screen_rgb(OcrImage {
            width: 5,
            height: 5,
            rgba: &rgba,
        })
        .unwrap();
        assert_eq!(rgb.get_pixel(0, 0).0, [255, 255, 255]);
        assert_eq!(rgb.get_pixel(2, 2).0, [0, 0, 0]);
    }

    #[test]
    fn contour_order_does_not_reverse_lines_or_join_unrelated_rows() {
        let block = |text: &str, x, y, score| TextBlock {
            box_points: vec![
                Point { x, y },
                Point {
                    x: x + 60,
                    y: y + 20,
                },
            ],
            box_score: 0.99,
            angle_index: 0,
            angle_score: 0.99,
            text: text.into(),
            text_score: score,
        };
        assert_eq!(
            ordered_result(vec![
                block("Second row.", 10, 60, 0.99),
                block("world", 90, 12, 0.99),
                block("Hello", 10, 10, 0.99),
                block("noise", 10, 90, 0.1),
            ])
            .0,
            "Hello world\nSecond row."
        );
    }
}
