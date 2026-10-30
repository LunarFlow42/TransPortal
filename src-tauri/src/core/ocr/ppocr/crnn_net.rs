use super::types::{OcrError, TextLine};
use super::utils::OcrUtils;
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
        }
    }

    pub fn init_model(
        &mut self,
        environment: &Environment,
        path: &str,
        num_thread: usize,
        builder_fn: Option<fn(SessionBuilder) -> ort::Result<SessionBuilder>>,
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

        self.session = Some(session);
        self.keys = self.get_keys()?;
        Ok(())
    }

    fn get_keys(&mut self) -> Result<Vec<String>, OcrError> {
        let session = self
            .session
            .as_ref()
            .ok_or(OcrError::SessionNotInitialized)?;
        let metadata = session.metadata()?;
        Self::keys_from_metadata(metadata.custom("character"))
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
        keys.push(" ".to_string());
        Ok(keys)
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
        let session = self
            .session
            .as_mut()
            .ok_or(OcrError::SessionNotInitialized)?;
        if self.input_names.is_empty() {
            return Err(OcrError::SessionNotInitialized);
        }

        let scale = self.dst_height as f32 / (img_src.height() as f32).max(1.0);
        let dst_width = ((img_src.width() as f32 * scale) as u32).max(1);

        let src_resize = image::imageops::resize(
            img_src,
            dst_width,
            self.dst_height,
            image::imageops::FilterType::Triangle,
        );

        let input_tensors =
            OcrUtils::subtract_mean_normalize(&src_resize, &MEAN_VALUES, &NORM_VALUES);
        let input_tensor = Tensor::from_array(input_tensors)?;

        let outputs = session.run(inputs![self.input_names[0].as_str() => input_tensor])?;

        let (_, red_data) = outputs.iter().next().ok_or(OcrError::EmptyOutput)?;
        let (shape, src_slice) = red_data.try_extract_tensor::<f32>()?;
        let height = shape[1] as usize;
        let width = shape[2] as usize;
        let src_data: Vec<f32> = src_slice.to_vec();

        Self::score_to_text_line(&src_data, height, width, &self.keys)
    }

    fn score_to_text_line(
        output_data: &[f32],
        height: usize,
        width: usize,
        keys: &[String],
    ) -> Result<TextLine, OcrError> {
        let mut text_line = TextLine::default();
        let mut last_index = 0usize;

        let mut text_score_sum = 0.0f32;
        let mut text_score_count = 0usize;

        for i in 0..height {
            let start = i * width;
            let stop = (i + 1) * width;
            if start >= output_data.len() {
                break;
            }
            let slice = &output_data[start..stop.min(output_data.len())];

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
}
