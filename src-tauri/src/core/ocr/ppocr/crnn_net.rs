use super::types::{OcrError, TextLine};
use super::utils::OcrUtils;
use ndarray::{Array4, s};
use ort::environment::Environment;
use ort::inputs;
use ort::session::Session;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::value::Tensor;
use std::collections::HashMap;

const CRNN_DST_HEIGHT: u32 = 48;
const MEAN_VALUES: [f32; 3] = [127.5, 127.5, 127.5];
const NORM_VALUES: [f32; 3] = [1.0 / 127.5, 1.0 / 127.5, 1.0 / 127.5];

#[derive(Debug)]
pub struct CrnnNet {
    session: Option<Session>,
    keys: Vec<String>,
    input_names: Vec<String>,
    dst_height: u32,
    min_width: u32,
    use_bgr: bool,
}

impl Default for CrnnNet {
    fn default() -> Self {
        Self::new()
    }
}

impl CrnnNet {
    pub fn new() -> Self {
        Self {
            session: None,
            keys: Vec::new(),
            input_names: Vec::new(),
            dst_height: CRNN_DST_HEIGHT,
            min_width: 1,
            use_bgr: false,
        }
    }

    pub fn init_model(
        &mut self,
        environment: &Environment,
        path: &str,
        num_thread: usize,
        builder_fn: Option<fn(SessionBuilder) -> ort::Result<SessionBuilder>>,
    ) -> Result<(), OcrError> {
        self.init_model_inner(environment, path, num_thread, builder_fn, None)
    }

    pub fn init_model_with_dictionary(
        &mut self,
        environment: &Environment,
        path: &str,
        character_dict_path: &str,
        builder_fn: fn(SessionBuilder) -> ort::Result<SessionBuilder>,
    ) -> Result<(), OcrError> {
        self.init_model_inner(
            environment,
            path,
            0,
            Some(builder_fn),
            Some(character_dict_path),
        )
    }

    fn init_model_inner(
        &mut self,
        environment: &Environment,
        path: &str,
        num_thread: usize,
        builder_fn: Option<fn(SessionBuilder) -> ort::Result<SessionBuilder>>,
        character_dict_path: Option<&str>,
    ) -> Result<(), OcrError> {
        let builder = Session::builder(environment)?;
        let mut builder = match builder_fn {
            Some(custom) => custom(builder)?,
            None => builder
                .with_optimization_level(GraphOptimizationLevel::Level2)?
                .with_intra_threads(num_thread)?
                .with_inter_threads(num_thread)?,
        };
        let session = builder.commit_from_file(path)?;
        self.input_names = session
            .inputs()
            .iter()
            .map(|input| input.name().to_string())
            .collect();

        // 动态根据模型输入张量高度适配（默认 48）
        let dst_height = if let Some(input) = session.inputs().first() {
            if let ort::value::ValueType::Tensor { shape, .. } = input.dtype() {
                shape.get(2).copied().filter(|&d| d > 0).unwrap_or(48) as u32
            } else {
                CRNN_DST_HEIGHT
            }
        } else {
            CRNN_DST_HEIGHT
        };
        self.dst_height = dst_height;

        let keys = match character_dict_path {
            Some(path) => Self::keys_from_metadata(Some(std::fs::read_to_string(path)?))?,
            None => Self::keys_from_metadata(session.metadata()?.custom("character"))?,
        };
        let output = session.outputs().first().ok_or(OcrError::EmptyOutput)?;
        match output.dtype() {
            ort::value::ValueType::Tensor { shape, .. } if shape.len() == 3 => {
                if shape[2] > 0 {
                    Self::validate_dictionary_size(shape[2] as usize, keys.len())?;
                }
            }
            _ => return Err(OcrError::InvalidModel("识别输出必须是 N×T×C 张量".into())),
        }
        self.min_width = if character_dict_path.is_some() {
            320
        } else {
            1
        };
        self.use_bgr = character_dict_path.is_some();
        self.keys = keys;
        self.session = Some(session);
        Ok(())
    }

    fn keys_from_metadata(character_list: Option<String>) -> Result<Vec<String>, OcrError> {
        let character_list =
            character_list.ok_or_else(|| OcrError::MetadataNotFound("character".into()))?;

        let mut keys = Vec::with_capacity(character_list.len() / 3);
        keys.push("#".to_string());
        keys.extend(
            character_list
                .lines()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('\r').to_string()),
        );
        if keys.len() == 1 {
            return Err(OcrError::InvalidModel("识别字典为空".into()));
        }
        keys.push(" ".to_string());
        Ok(keys)
    }

    fn validate_dictionary_size(classes: usize, key_count: usize) -> Result<(), OcrError> {
        if classes != key_count {
            return Err(OcrError::InvalidModel(format!(
                "识别模型有 {classes} 个类别，字典含 blank/space 共 {key_count} 项"
            )));
        }
        Ok(())
    }

    pub fn get_text_lines(
        &mut self,
        part_imgs: &[image::RgbImage],
        angle_rollback_records: &HashMap<usize, image::RgbImage>,
        angle_rollback_threshold: f32,
    ) -> Result<Vec<TextLine>, OcrError> {
        let mut text_lines = Vec::with_capacity(part_imgs.len());

        for (index, img) in part_imgs.iter().enumerate() {
            let mut text_line = self.get_text_line(img)?;

            if (text_line.text_score.is_nan() || text_line.text_score < angle_rollback_threshold)
                && let Some(rollback_img) = angle_rollback_records.get(&index)
            {
                text_line = self.get_text_line(rollback_img)?;
            }

            text_lines.push(text_line);
        }

        Ok(text_lines)
    }

    fn get_text_line(&mut self, img_src: &image::RgbImage) -> Result<TextLine, OcrError> {
        let input_tensors =
            Self::preprocess(img_src, self.dst_height, self.min_width, self.use_bgr);
        let session = self
            .session
            .as_mut()
            .ok_or(OcrError::SessionNotInitialized)?;
        if self.input_names.is_empty() {
            return Err(OcrError::SessionNotInitialized);
        }

        let input_tensor = Tensor::from_array(input_tensors)?;

        let outputs = session.run(inputs![self.input_names[0].as_str() => input_tensor])?;

        let (_, red_data) = outputs.iter().next().ok_or(OcrError::EmptyOutput)?;
        let (shape, src_slice) = red_data.try_extract_tensor::<f32>()?;
        if shape.len() != 3 || shape[0] != 1 || shape[1] <= 0 || shape[2] <= 0 {
            return Err(OcrError::InvalidModel("识别输出必须是 1×T×C 张量".into()));
        }
        let height = shape[1] as usize;
        let width = shape[2] as usize;

        Self::score_to_text_line(src_slice, height, width, &self.keys)
    }

    fn preprocess(
        img: &image::RgbImage,
        dst_height: u32,
        min_width: u32,
        use_bgr: bool,
    ) -> Array4<f32> {
        let scale = dst_height as f32 / (img.height() as f32).max(1.0);
        let dst_width = ((img.width() as f32 * scale).ceil() as u32).max(1);
        let resized = image::imageops::resize(
            img,
            dst_width,
            dst_height,
            image::imageops::FilterType::Triangle,
        );
        let normalized = if use_bgr {
            OcrUtils::subtract_mean_normalize_bgr(&resized, &MEAN_VALUES, &NORM_VALUES)
        } else {
            OcrUtils::subtract_mean_normalize(&resized, &MEAN_VALUES, &NORM_VALUES)
        };
        if dst_width >= min_width {
            return normalized;
        }
        // PaddleOCR pads after normalization: padding is zero, not white (+1).
        let mut padded = Array4::zeros((1, 3, dst_height as usize, min_width as usize));
        padded
            .slice_mut(s![.., .., .., ..dst_width as usize])
            .assign(&normalized);
        padded
    }

    fn score_to_text_line(
        output_data: &[f32],
        height: usize,
        width: usize,
        keys: &[String],
    ) -> Result<TextLine, OcrError> {
        Self::validate_dictionary_size(width, keys.len())?;
        if width == 0 || height == 0 || height.checked_mul(width) != Some(output_data.len()) {
            return Err(OcrError::InvalidModel("识别输出尺寸与数据长度不符".into()));
        }
        let mut text_line = TextLine::default();
        let mut last_index = 0usize;

        let mut text_score_sum = 0.0f32;
        let mut text_score_count = 0usize;

        for (i, slice) in output_data.chunks_exact(width).enumerate() {
            let (max_index, max_value) = slice.iter().enumerate().fold(
                (0usize, f32::MIN),
                |(max_idx, max_val), (idx, &val)| {
                    if val > max_val {
                        (idx, val)
                    } else {
                        (max_idx, max_val)
                    }
                },
            );

            if !max_value.is_finite() {
                return Err(OcrError::InvalidModel("识别输出包含无效置信度".into()));
            }
            if max_index > 0 && max_index < keys.len() && !(i > 0 && max_index == last_index) {
                text_line.text.push_str(&keys[max_index]);
                text_score_sum += max_value;
                text_score_count += 1;
            }
            last_index = max_index;
        }

        text_line.text_score = if text_score_count > 0 {
            text_score_sum / text_score_count as f32
        } else {
            0.0
        };
        Ok(text_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_character_metadata_returns_ocr_error() {
        assert!(matches!(
            CrnnNet::keys_from_metadata(None),
            Err(OcrError::MetadataNotFound(key)) if key == "character"
        ));
    }

    #[test]
    fn character_metadata_preserves_blank_and_space_tokens() {
        let keys = CrnnNet::keys_from_metadata(Some("中\r\n\nA\r\n文\n".into())).unwrap();
        assert_eq!(keys, ["#", "中", "A", "文", " "]);
    }

    #[test]
    fn v6_dictionary_preserves_unicode_and_matches_ctc_class_count() {
        let keys = CrnnNet::keys_from_metadata(Some("!\n繁\nあ\né\n　\n🛅\n".into())).unwrap();
        assert_eq!(keys, ["#", "!", "繁", "あ", "é", "　", "🛅", " "]);
        assert!(CrnnNet::validate_dictionary_size(8, keys.len()).is_ok());
        assert!(CrnnNet::validate_dictionary_size(7, keys.len()).is_err());
        assert!(CrnnNet::keys_from_metadata(Some("\n\r\n".into())).is_err());
    }

    #[test]
    fn v6_preprocessing_uses_bgr_48px_height_and_zero_padding_to_320px() {
        let img = image::RgbImage::from_pixel(10, 24, image::Rgb([255, 0, 0]));
        let tensor = CrnnNet::preprocess(&img, 48, 320, true);
        assert_eq!(tensor.shape(), [1, 3, 48, 320]);
        assert_eq!(tensor[[0, 0, 0, 0]], -1.0);
        assert_eq!(tensor[[0, 2, 0, 0]], 1.0);
        assert_eq!(tensor[[0, 0, 0, 19]], -1.0);
        assert_eq!(tensor[[0, 0, 0, 20]], 0.0);
        assert_eq!(tensor[[0, 2, 47, 319]], 0.0);
        let wide = image::RgbImage::new(401, 48);
        assert_eq!(
            CrnnNet::preprocess(&wide, 48, 320, true).shape(),
            [1, 3, 48, 401]
        );
    }

    #[test]
    fn ctc_decoding_keeps_duplicates_separated_by_blank_and_spaces() {
        let keys = CrnnNet::keys_from_metadata(Some("a\n中\n".into())).unwrap();
        let data: Vec<f32> = [1, 1, 0, 1, 3, 2, 2]
            .into_iter()
            .flat_map(|index| (0..4).map(move |i| if i == index { 0.9 } else { 0.01 }))
            .collect();
        let line = CrnnNet::score_to_text_line(&data, 7, 4, &keys).unwrap();
        assert_eq!(line.text, "aa 中");
        assert!((line.text_score - 0.9).abs() < 1e-6);
    }

    #[test]
    fn malformed_ctc_outputs_fail_instead_of_truncating_or_panicking() {
        let keys = CrnnNet::keys_from_metadata(Some("a\n".into())).unwrap();
        assert!(CrnnNet::score_to_text_line(&[0.0; 2], 1, 2, &keys).is_err());
        assert!(CrnnNet::score_to_text_line(&[0.0; 2], 1, 3, &keys).is_err());
        assert!(CrnnNet::score_to_text_line(&[], 0, 3, &keys).is_err());
        assert!(CrnnNet::score_to_text_line(&[f32::INFINITY; 3], 1, 3, &keys).is_err());
    }
}
