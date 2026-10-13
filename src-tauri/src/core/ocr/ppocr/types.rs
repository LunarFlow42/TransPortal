use std::fmt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum OcrError {
    #[error("ONNX Runtime 错误：{0}")]
    Ort(#[from] ort::Error),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("图像处理错误：{0}")]
    Image(#[from] image::ImageError),
    #[error("ONNX 会话未初始化")]
    SessionNotInitialized,
    #[error("模型元数据缺失：{0}")]
    MetadataNotFound(String),
    #[error("模型输出为空")]
    EmptyOutput,
    #[error("图像数据无效：{0}")]
    InvalidImage(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone)]
pub struct TextBox {
    pub points: Vec<Point>,
    pub score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Angle {
    pub index: i32,
    pub score: f32,
}

#[derive(Debug, Clone, Default)]
pub struct TextLine {
    pub text: String,
    pub text_score: f32,
}

#[derive(Debug, Clone)]
pub struct TextBlock {
    pub box_points: Vec<Point>,
    pub box_score: f32,
    pub angle_index: i32,
    pub angle_score: f32,
    pub text: String,
    pub text_score: f32,
}

#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text_blocks: Vec<TextBlock>,
}

#[derive(Debug, Clone, Default)]
pub struct ScaleParam {
    pub src_width: u32,
    pub src_height: u32,
    pub dst_width: u32,
    pub dst_height: u32,
    pub scale_width: f32,
    pub scale_height: f32,
}

impl ScaleParam {
    pub fn new(
        src_width: u32,
        src_height: u32,
        dst_width: u32,
        dst_height: u32,
        scale_width: f32,
        scale_height: f32,
    ) -> Self {
        Self {
            src_width,
            src_height,
            dst_width,
            dst_height,
            scale_width,
            scale_height,
        }
    }

    pub fn get_scale_param(src: &image::RgbImage, target_size: u32) -> Self {
        let src_width = src.width();
        let src_height = src.height();

        if src_width == 0 || src_height == 0 {
            return Self::default();
        }

        let ratio = if src_width > src_height {
            target_size as f32 / src_width as f32
        } else {
            target_size as f32 / src_height as f32
        };

        let mut dst_width = (src_width as f32 * ratio) as u32;
        let mut dst_height = (src_height as f32 * ratio) as u32;

        dst_width = ((dst_width / 32) * 32).max(32);
        dst_height = ((dst_height / 32) * 32).max(32);

        let scale_width = dst_width as f32 / src_width as f32;
        let scale_height = dst_height as f32 / src_height as f32;

        Self::new(
            src_width,
            src_height,
            dst_width,
            dst_height,
            scale_width,
            scale_height,
        )
    }
}

impl fmt::Display for ScaleParam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ScaleParam(src: {}x{}, dst: {}x{}, scale: {:.3}x{:.3})",
            self.src_width,
            self.src_height,
            self.dst_width,
            self.dst_height,
            self.scale_width,
            self.scale_height
        )
    }
}
