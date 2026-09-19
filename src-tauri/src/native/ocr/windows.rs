use crate::ocr::{OcrEngine, OcrEngineInfo, OcrImage, OcrLanguage, RecognizedText, fit_dimensions};
use crate::ocr::picture::OcrBlock;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap,
};
use windows::Media::Ocr::OcrEngine as NativeOcrEngine;
use windows::Win32::System::WinRT::{
    IMemoryBufferByteAccess, RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize,
};
use windows::core::{HSTRING, Interface};

use std::collections::HashMap;
use std::sync::Mutex;

pub struct WindowsOcrEngine {
    cache: Mutex<HashMap<String, NativeOcrEngine>>,
}

impl Default for WindowsOcrEngine {
    fn default() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }
}

impl WindowsOcrEngine {
    fn get_engine(&self, language: Option<&str>) -> Result<NativeOcrEngine, String> {
        let key = language.unwrap_or("").to_string();
        let mut cache = self.cache.lock().map_err(|e| e.to_string())?;
        if let Some(engine) = cache.get(&key) {
            return Ok(engine.clone());
        }
        let engine = create_engine(language)?;
        cache.insert(key, engine.clone());
        Ok(engine)
    }
}

// These objects must be dropped before balancing this thread's WinRT init.
struct Apartment;
impl Apartment {
    fn new() -> Result<Self, String> {
        // SAFETY: OCR runs on a worker, never on the webview's STA thread.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(|e| format!("无法初始化 Windows OCR：{e}"))?;
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: balances this guard's successful RoInitialize on this thread.
        unsafe { RoUninitialize() };
    }
}

const LANGUAGE_HELP: &str = "Windows OCR 语言包不可用。请在 Windows 设置 → 时间和语言 → 语言和区域中添加语言并安装光学字符识别功能，然后刷新 OCR 设置。";

fn normalize_chinese_alias(lower: &str) -> Option<&'static str> {
    match lower {
        "zh" | "zh-cn" | "zh-hans" | "zh-hans-cn" => Some("zh-Hans-CN"),
        "zh-tw" | "zh-hant" | "zh-hant-tw" => Some("zh-Hant-TW"),
        "zh-hk" | "zh-hant-hk" => Some("zh-Hant-HK"),
        _ => None,
    }
}

fn find_matching_language(
    available: &windows::Foundation::Collections::IVectorView<Language>,
    requested: &str,
) -> Option<Language> {
    let clean = requested.trim().replace('_', "-");
    let lower = clean.to_ascii_lowercase();

    let mut items = Vec::new();
    for lang in available {
        if let Ok(tag_hstring) = lang.LanguageTag() {
            let tag = tag_hstring.to_string();
            items.push((tag, lang));
        }
    }

    // 1. Exact match (case-insensitive)
    for (tag, lang) in &items {
        if tag.eq_ignore_ascii_case(&clean) {
            return Some(lang.clone());
        }
    }

    // 2. Chinese alias match against available languages
    if let Some(target) = normalize_chinese_alias(&lower) {
        if let Some((_, lang)) = items.iter().find(|(tag, _)| tag.eq_ignore_ascii_case(target)) {
            return Some(lang.clone());
        }
        let prefix = if target.contains("Hans") { "zh-hans" } else { "zh-hant" };
        if let Some((_, lang)) = items.iter().find(|(tag, _)| tag.to_ascii_lowercase().starts_with(prefix)) {
            return Some(lang.clone());
        }
        if let Some((_, lang)) = items.iter().find(|(tag, _)| tag.to_ascii_lowercase().starts_with("zh")) {
            return Some(lang.clone());
        }
    }

    // 3. Prefix match: e.g. "en" matches "en-US", or "en-us" matches "en"
    for (tag, lang) in &items {
        let tag_lower = tag.to_ascii_lowercase();
        if tag_lower.starts_with(&format!("{lower}-")) || lower.starts_with(&format!("{tag_lower}-")) {
            return Some(lang.clone());
        }
    }

    None
}

fn create_engine(language: Option<&str>) -> Result<NativeOcrEngine, String> {
    let available = NativeOcrEngine::AvailableRecognizerLanguages().ok();

    if let Some(tag) = language {
        if let Some(ref list) = available
            && let Some(lang) = find_matching_language(list, tag)
            && let Ok(engine) = NativeOcrEngine::TryCreateFromLanguage(&lang)
        {
            return Ok(engine);
        }
        // Fall back to direct tag creation with normalization
        let clean = tag.trim().replace('_', "-");
        let target_tag = normalize_chinese_alias(&clean.to_ascii_lowercase()).unwrap_or(&clean);
        let language = Language::CreateLanguage(&HSTRING::from(target_tag))
            .map_err(|e| format!("无效的 OCR 语言 {tag}：{e}"))?;
        return NativeOcrEngine::TryCreateFromLanguage(&language)
            .map_err(|_| format!("{tag} 的 OCR 语言包不可用。{LANGUAGE_HELP}"));
    }

    // When language is None (auto), check AvailableRecognizerLanguages().
    // If any Chinese recognizer is available, prefer it over a Latin-only language like en-US,
    // because Chinese recognizers on Windows are bilingual (recognize both Chinese and English).
    if let Some(ref list) = available {
        let mut items = Vec::new();
        for lang in list {
            if let Ok(tag_hstring) = lang.LanguageTag() {
                items.push((tag_hstring.to_string(), lang));
            }
        }
        let chinese_candidate = items
            .iter()
            .find(|(tag, _)| tag.eq_ignore_ascii_case("zh-Hans-CN"))
            .or_else(|| items.iter().find(|(tag, _)| tag.to_ascii_lowercase().starts_with("zh-hans")))
            .or_else(|| items.iter().find(|(tag, _)| tag.to_ascii_lowercase().starts_with("zh")));

        if let Some((_, lang)) = chinese_candidate
            && let Ok(engine) = NativeOcrEngine::TryCreateFromLanguage(lang)
        {
            return Ok(engine);
        }
    }

    if let Ok(engine) = NativeOcrEngine::TryCreateFromUserProfileLanguages() {
        return Ok(engine);
    }

    // A usable pack may be installed without appearing in the user's preferred
    // language list. Fall back deterministically to the first installed pack.
    let languages = match available {
        Some(list) => list,
        None => NativeOcrEngine::AvailableRecognizerLanguages()
            .map_err(|e| format!("{LANGUAGE_HELP} ({e})"))?,
    };
    for language in &languages {
        if let Ok(engine) = NativeOcrEngine::TryCreateFromLanguage(&language) {
            return Ok(engine);
        }
    }
    Err(LANGUAGE_HELP.into())
}

impl OcrEngine for WindowsOcrEngine {
    fn id(&self) -> &'static str {
        "windows"
    }

    fn release(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    fn info(&self) -> Result<OcrEngineInfo, String> {
        let _apartment = Apartment::new()?;
        let languages = NativeOcrEngine::AvailableRecognizerLanguages()
            .map_err(|e| format!("无法读取 Windows OCR 语言包：{e}"))?;
        let mut installed = Vec::new();
        for language in languages {
            let tag = language
                .LanguageTag()
                .map_err(|e| e.to_string())?
                .to_string();
            let label = language
                .NativeName()
                .map(|name| name.to_string())
                .unwrap_or_else(|_| tag.clone());
            installed.push(OcrLanguage { tag, label });
        }
        let error = create_engine(None).err();
        Ok(OcrEngineInfo {
            id: self.id().into(),
            label: "Windows 自带 OCR".into(),
            languages: installed,
            available: error.is_none(),
            error,
        })
    }

    fn recognize(
        &self,
        image: OcrImage<'_>,
        language: Option<&str>,
    ) -> Result<RecognizedText, String> {
        image.validate()?;
        let _apartment = Apartment::new()?;
        let engine = self.get_engine(language)?;
        let limit = NativeOcrEngine::MaxImageDimension().map_err(|e| e.to_string())?;
        let (width, height) = fit_dimensions(image.width, image.height, limit)?;
        let bitmap =
            to_bitmap(&image, width, height).map_err(|e| format!("无法准备 OCR 图片：{e}"))?;
        let result = engine
            .RecognizeAsync(&bitmap)
            .and_then(|operation| operation.get())
            .map_err(|e| format!("Windows OCR 识别失败：{e}"))?;
        // OcrResult.Text collapses line breaks. Keep line order for paragraphs,
        // code and copied text, rather than guessing at sentence boundaries.
        let lines = result.Lines().map_err(|e| e.to_string())?;
        let mut text = Vec::new();
        let mut blocks = Vec::new();
        let scale_x = image.width as f32 / width as f32;
        let scale_y = image.height as f32 / height as f32;
        for line in lines {
            let content = line.Text().map_err(|e| e.to_string())?.to_string();
            if !content.trim().is_empty() {
                let mut left = f32::INFINITY;
                let mut top = f32::INFINITY;
                let mut right = 0.0_f32;
                let mut bottom = 0.0_f32;
                for word in line.Words().map_err(|e| e.to_string())? {
                    let rect = word.BoundingRect().map_err(|e| e.to_string())?;
                    left = left.min(rect.X);
                    top = top.min(rect.Y);
                    right = right.max(rect.X + rect.Width);
                    bottom = bottom.max(rect.Y + rect.Height);
                }
                blocks.push(OcrBlock { id: blocks.len() as u32, text: content.trim().into(),
                    x: left * scale_x, y: top * scale_y,
                    width: (right - left) * scale_x, height: (bottom - top) * scale_y });
                text.push(content.trim().to_string());
            }
        }
        let language = engine
            .RecognizerLanguage()
            .and_then(|lang| lang.LanguageTag())
            .map_err(|e| e.to_string())?
            .to_string();
        Ok(RecognizedText {
            text: text.join("\n"),
            language,
            blocks,
        })
    }
}

/// Fill the native buffer directly to avoid an additional full-sized pixel copy.
fn to_bitmap(image: &OcrImage<'_>, width: u32, height: u32) -> Result<SoftwareBitmap, String> {
    let bitmap = SoftwareBitmap::CreateWithAlpha(
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
        BitmapAlphaMode::Ignore,
    )
    .map_err(|e| e.to_string())?;
    {
        let buffer = bitmap
            .LockBuffer(BitmapBufferAccessMode::Write)
            .map_err(|e| e.to_string())?;
        let plane = buffer.GetPlaneDescription(0).map_err(|e| e.to_string())?;
        let reference = buffer.CreateReference().map_err(|e| e.to_string())?;
        let access: IMemoryBufferByteAccess = reference.cast().map_err(|e| e.to_string())?;
        let mut pointer = std::ptr::null_mut();
        let mut capacity = 0;
        // SAFETY: the reference holds a write lock for the lifetime of the slice.
        unsafe { access.GetBuffer(&mut pointer, &mut capacity) }.map_err(|e| e.to_string())?;
        let row_bytes = u64::from(width) * 4;
        if pointer.is_null()
            || plane.StartIndex < 0
            || plane.Stride < 0
            || (plane.Stride as u64) < row_bytes
            || plane.StartIndex as u64 + u64::from(height - 1) * plane.Stride as u64 + row_bytes
                > u64::from(capacity)
        {
            return Err("Windows 返回了无效的像素缓冲区。".into());
        }
        // SAFETY: native capacity and every row's extent were checked above;
        // the lock, reference and byte-access interface outlive `pixels`.
        let pixels = unsafe { std::slice::from_raw_parts_mut(pointer, capacity as usize) };
        for y in 0..height {
            let source_y = u64::from(y) * u64::from(image.height) / u64::from(height);
            let row = plane.StartIndex as usize + y as usize * plane.Stride as usize;
            for x in 0..width {
                let source_x = u64::from(x) * u64::from(image.width) / u64::from(width);
                let source = ((source_y * u64::from(image.width) + source_x) * 4) as usize;
                let target = row + x as usize * 4;
                pixels[target..target + 4].copy_from_slice(&[
                    image.rgba[source + 2],
                    image.rgba[source + 1],
                    image.rgba[source],
                    255,
                ]);
            }
        }
    }
    Ok(bitmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_ocr_auto_prefers_chinese_when_installed() {
        let _apartment = Apartment::new().unwrap();
        let engine = create_engine(None);
        assert!(engine.is_ok(), "create_engine(None) should succeed: {:?}", engine.err());
        let engine = engine.unwrap();
        let lang = engine.RecognizerLanguage().unwrap().LanguageTag().unwrap().to_string();
        assert!(lang.to_ascii_lowercase().starts_with("zh"), "Expected Chinese recognizer, got: {lang}");
    }

    #[test]
    fn test_windows_ocr_normalizes_zh_cn() {
        let _apartment = Apartment::new().unwrap();
        let engine = create_engine(Some("zh-CN"));
        assert!(engine.is_ok(), "create_engine(Some(\"zh-CN\")) should succeed: {:?}", engine.err());
        let engine = engine.unwrap();
        let lang = engine.RecognizerLanguage().unwrap().LanguageTag().unwrap().to_string();
        assert_eq!(lang, "zh-Hans-CN");
    }

    #[test]
    fn test_windows_ocr_normalizes_zh() {
        let _apartment = Apartment::new().unwrap();
        let engine = create_engine(Some("zh"));
        assert!(engine.is_ok(), "create_engine(Some(\"zh\")) should succeed: {:?}", engine.err());
        let engine = engine.unwrap();
        let lang = engine.RecognizerLanguage().unwrap().LanguageTag().unwrap().to_string();
        assert_eq!(lang, "zh-Hans-CN");
    }

    #[test]
    fn test_windows_ocr_en_us() {
        let _apartment = Apartment::new().unwrap();
        let engine = create_engine(Some("en-US"));
        assert!(engine.is_ok(), "create_engine(Some(\"en-US\")) should succeed: {:?}", engine.err());
        let engine = engine.unwrap();
        let lang = engine.RecognizerLanguage().unwrap().LanguageTag().unwrap().to_string();
        assert_eq!(lang, "en-US");
    }
}

