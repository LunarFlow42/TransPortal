use crate::core::ocr::{
    ClipboardOrigin, OcrEngine, OcrEngineInfo, OcrImage, RecognizedText, ScreenCapture,
    ScreenEvent, ScreenObserver,
};
use std::sync::mpsc::SyncSender;

pub struct UnavailableOcr;
const UNAVAILABLE: &str = "当前平台尚未提供本地 OCR；Windows 10/11 可使用系统 OCR。";

impl OcrEngine for UnavailableOcr {
    fn id(&self) -> &'static str {
        "unavailable"
    }

    fn info(&self) -> Result<OcrEngineInfo, String> {
        Ok(OcrEngineInfo {
            id: self.id().into(),
            label: "本地 OCR".into(),
            languages: vec![],
            available: false,
            error: Some(UNAVAILABLE.into()),
        })
    }

    fn recognize(&self, _: OcrImage<'_>, _: Option<&str>) -> Result<RecognizedText, String> {
        Err(UNAVAILABLE.into())
    }
}

impl ScreenCapture for UnavailableOcr {
    fn supported(&self) -> bool {
        false
    }
    fn clipboard_revision(&self) -> u32 {
        0
    }
    fn clipboard_origin(&self) -> ClipboardOrigin {
        ClipboardOrigin::Unavailable
    }
    fn observe(&self, _: SyncSender<ScreenEvent>) -> Result<Box<dyn ScreenObserver>, String> {
        Err(UNAVAILABLE.into())
    }
}
