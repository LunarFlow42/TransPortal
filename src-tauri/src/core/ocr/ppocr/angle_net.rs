use super::types::{Angle, OcrError};
use super::utils::OcrUtils;
use ort::environment::Environment;
use ort::inputs;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;

const MEAN_VALUES: [f32; 3] = [127.5, 127.5, 127.5];
const NORM_VALUES: [f32; 3] = [1.0 / 127.5, 1.0 / 127.5, 1.0 / 127.5];
const ANGLE_DST_WIDTH: u32 = 192;
const ANGLE_DST_HEIGHT: u32 = 48;
const ANGLE_COLS: usize = 2;

#[derive(Debug)]
pub struct AngleNet {
    session: Option<Session>,
    input_names: Vec<String>,
    dst_width: u32,
    dst_height: u32,
}

impl Default for AngleNet {
    fn default() -> Self {
        Self::new()
    }
}

impl AngleNet {
    pub fn new() -> Self {
        Self {
            session: None,
            input_names: Vec::new(),
            dst_width: ANGLE_DST_WIDTH,
            dst_height: ANGLE_DST_HEIGHT,
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

        // 动态根据模型输入张量形状适配目标宽高（兼容 v4 的 48x192 与 v5 LCNet 的 80x160）
        let (dst_height, dst_width) = if let Some(input) = session.inputs().first() {
            if let ort::value::ValueType::Tensor { shape, .. } = input.dtype() {
                let h = shape.get(2).copied().filter(|&d| d > 0).unwrap_or(48) as u32;
                let w = shape.get(3).copied().filter(|&d| d > 0).unwrap_or(192) as u32;
                (h, w)
            } else {
                (ANGLE_DST_HEIGHT, ANGLE_DST_WIDTH)
            }
        } else {
            (ANGLE_DST_HEIGHT, ANGLE_DST_WIDTH)
        };
        self.dst_height = dst_height;
        self.dst_width = dst_width;

        self.session = Some(session);
        Ok(())
    }

    pub fn get_angles(
        &mut self,
        part_imgs: &[image::RgbImage],
        do_angle: bool,
        most_angle: bool,
    ) -> Result<Vec<Angle>, OcrError> {
        let mut angles = Vec::with_capacity(part_imgs.len());

        if do_angle {
            for img in part_imgs {
                let angle = self.get_angle(img)?;
                angles.push(angle);
            }
        } else {
            angles.extend(part_imgs.iter().map(|_| Angle::default()));
        }

        if do_angle && most_angle && !angles.is_empty() {
            let sum: i32 = angles.iter().map(|x| x.index).sum();
            let half_percent = angles.len() as f32 / 2.0;
            let most_angle_index = if (sum as f32) < half_percent { 0 } else { 1 };

            for angle in &mut angles {
                angle.index = most_angle_index;
            }
        }

        Ok(angles)
    }

    fn get_angle(&mut self, img_src: &image::RgbImage) -> Result<Angle, OcrError> {
        let session = self
            .session
            .as_mut()
            .ok_or(OcrError::SessionNotInitialized)?;
        if self.input_names.is_empty() {
            return Err(OcrError::SessionNotInitialized);
        }

        let angle_img = image::imageops::resize(
            img_src,
            self.dst_width,
            self.dst_height,
            image::imageops::FilterType::Triangle,
        );

        let input_tensors =
            OcrUtils::subtract_mean_normalize(&angle_img, &MEAN_VALUES, &NORM_VALUES);
        let input_tensor = Tensor::from_array(input_tensors)?;

        let outputs = session.run(inputs![self.input_names[0].as_str() => input_tensor])?;
        Self::score_to_angle(&outputs, ANGLE_COLS)
    }

    fn score_to_angle(
        output_tensor: &SessionOutputs,
        angle_cols: usize,
    ) -> Result<Angle, OcrError> {
        let (_, red_data) = output_tensor.iter().next().ok_or(OcrError::EmptyOutput)?;
        let (_, src_data) = red_data.try_extract_tensor::<f32>()?;

        let mut angle = Angle::default();
        let mut max_value = f32::MIN;
        let mut angle_index = 0i32;

        for (i, &value) in src_data.iter().take(angle_cols).enumerate() {
            if i == 0 || value > max_value {
                max_value = value;
                angle_index = i as i32;
            }
        }

        angle.index = angle_index;
        angle.score = max_value;
        Ok(angle)
    }
}
