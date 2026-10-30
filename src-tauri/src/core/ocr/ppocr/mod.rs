pub mod angle_net;
pub mod crnn_net;
pub mod db_net;
pub mod types;
pub mod utils;

pub use types::{Angle, OcrError, OcrResult, Point, ScaleParam, TextBlock, TextBox, TextLine};

use angle_net::AngleNet;
use crnn_net::CrnnNet;
use db_net::DbNet;
use ort::environment::Environment;
use ort::session::builder::SessionBuilder;
use std::collections::HashMap;
use utils::OcrUtils;

#[derive(Debug)]
pub struct PpOcrEngine {
    db_net: DbNet,
    angle_net: AngleNet,
    crnn_net: CrnnNet,
}

impl Default for PpOcrEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PpOcrEngine {
    pub fn new() -> Self {
        Self {
            db_net: DbNet::new(),
            angle_net: AngleNet::new(),
            crnn_net: CrnnNet::new(),
        }
    }

    pub fn init_models(
        &mut self,
        environment: &Environment,
        det_path: &str,
        cls_path: &str,
        rec_path: &str,
        num_thread: usize,
    ) -> Result<(), OcrError> {
        self.db_net
            .init_model(environment, det_path, num_thread, None)?;
        self.angle_net
            .init_model(environment, cls_path, num_thread, None)?;
        self.crnn_net
            .init_model(environment, rec_path, num_thread, None)?;
        Ok(())
    }

    pub fn init_models_custom(
        &mut self,
        environment: &Environment,
        det_path: &str,
        cls_path: &str,
        rec_path: &str,
        builder_fn: fn(SessionBuilder) -> ort::Result<SessionBuilder>,
    ) -> Result<(), OcrError> {
        self.db_net
            .init_model(environment, det_path, 0, Some(builder_fn))?;
        self.angle_net
            .init_model(environment, cls_path, 0, Some(builder_fn))?;
        self.crnn_net
            .init_model(environment, rec_path, 0, Some(builder_fn))?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn detect_angle_rollback(
        &mut self,
        img_src: &image::RgbImage,
        padding: u32,
        max_side_len: u32,
        box_score_thresh: f32,
        box_thresh: f32,
        un_clip_ratio: f32,
        do_angle: bool,
        most_angle: bool,
        angle_rollback_threshold: f32,
    ) -> Result<OcrResult, OcrError> {
        if img_src.width() == 0 || img_src.height() == 0 {
            return Ok(OcrResult {
                text_blocks: Vec::new(),
            });
        }

        let origin_max_side = img_src.width().max(img_src.height());
        let mut resize = if max_side_len == 0 || max_side_len > origin_max_side {
            origin_max_side
        } else {
            max_side_len
        };
        resize += 2 * padding;

        let padding_src = OcrUtils::make_padding(img_src, padding)?;
        let scale = ScaleParam::get_scale_param(&padding_src, resize);

        self.detect_pipeline(
            &padding_src,
            &scale,
            padding,
            box_score_thresh,
            box_thresh,
            un_clip_ratio,
            do_angle,
            most_angle,
            true,
            angle_rollback_threshold,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn detect_pipeline(
        &mut self,
        img_src: &image::RgbImage,
        scale: &ScaleParam,
        padding: u32,
        box_score_thresh: f32,
        box_thresh: f32,
        un_clip_ratio: f32,
        do_angle: bool,
        most_angle: bool,
        angle_rollback: bool,
        angle_rollback_threshold: f32,
    ) -> Result<OcrResult, OcrError> {
        let text_boxes = self.db_net.get_text_boxes(
            img_src,
            scale,
            box_score_thresh,
            box_thresh,
            un_clip_ratio,
        )?;

        let part_images = OcrUtils::get_part_images(img_src, &text_boxes);
        let angles = self
            .angle_net
            .get_angles(&part_images, do_angle, most_angle)?;

        let mut rotated_images = Vec::with_capacity(part_images.len());
        let mut angle_rollback_records = HashMap::new();

        for (index, (angle, mut part_image)) in angles.iter().zip(part_images).enumerate() {
            if angle.index == 1 {
                if angle_rollback {
                    angle_rollback_records.insert(index, part_image.clone());
                }
                OcrUtils::mat_rotate_clock_wise_180(&mut part_image);
            }
            rotated_images.push(part_image);
        }

        let text_lines = self.crnn_net.get_text_lines(
            &rotated_images,
            &angle_rollback_records,
            angle_rollback_threshold,
        )?;

        let mut text_blocks = Vec::with_capacity(text_lines.len());
        for i in 0..text_lines.len() {
            text_blocks.push(TextBlock {
                box_points: text_boxes[i]
                    .points
                    .iter()
                    .map(|p| Point {
                        x: p.x.saturating_sub(padding),
                        y: p.y.saturating_sub(padding),
                    })
                    .collect(),
                box_score: text_boxes[i].score,
                angle_index: angles[i].index,
                angle_score: angles[i].score,
                text: text_lines[i].text.clone(),
                text_score: text_lines[i].text_score,
            });
        }

        Ok(OcrResult { text_blocks })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_param_aligns_to_32() {
        let img = image::RgbImage::new(100, 200);
        let param = ScaleParam::get_scale_param(&img, 300);
        assert_eq!(param.dst_width % 32, 0);
        assert_eq!(param.dst_height % 32, 0);
        assert!(param.dst_width >= 32);
        assert!(param.dst_height >= 32);
    }

    #[test]
    fn test_scale_param_zero_and_boundary() {
        let zero_img = image::RgbImage::new(0, 0);
        let param_zero = ScaleParam::get_scale_param(&zero_img, 300);
        assert_eq!(param_zero.src_width, 0);
        assert_eq!(param_zero.src_height, 0);
        assert_eq!(param_zero.dst_width, 0);

        let tiny_img = image::RgbImage::new(1, 1);
        let param_tiny = ScaleParam::get_scale_param(&tiny_img, 300);
        assert_eq!(param_tiny.dst_width % 32, 0);
        assert_eq!(param_tiny.dst_height % 32, 0);
        assert!(param_tiny.dst_width >= 32);
        assert!(param_tiny.dst_height >= 32);
    }

    #[test]
    fn test_get_rotate_crop_image_edge_bounds() {
        let img = image::RgbImage::new(100, 100);
        // Points touching or exceeding the image boundary
        let edge_points = vec![
            Point { x: 0, y: 0 },
            Point { x: 100, y: 0 },
            Point { x: 100, y: 100 },
            Point { x: 0, y: 100 },
        ];
        let cropped = utils::OcrUtils::get_rotate_crop_image(&img, &edge_points);
        assert!(cropped.width() > 0);
        assert!(cropped.height() > 0);

        // Degenerate points and empty image
        let empty_img = image::RgbImage::new(0, 0);
        let cropped_empty = utils::OcrUtils::get_rotate_crop_image(&empty_img, &edge_points);
        assert_eq!(cropped_empty.width(), 1);
        assert_eq!(cropped_empty.height(), 1);

        let few_points = vec![Point { x: 0, y: 0 }];
        let cropped_few = utils::OcrUtils::get_rotate_crop_image(&img, &few_points);
        assert_eq!(cropped_few.width(), 1);
        assert_eq!(cropped_few.height(), 1);
    }

    #[test]
    fn test_subtract_mean_normalize_shape_and_values() {
        let mut img = image::RgbImage::new(4, 4);
        for pixel in img.pixels_mut() {
            *pixel = image::Rgb([100, 150, 200]);
        }
        let mean = [100.0, 150.0, 200.0];
        let norm = [1.0, 1.0, 1.0];
        let tensor = utils::OcrUtils::subtract_mean_normalize(&img, &mean, &norm);
        assert_eq!(tensor.shape(), &[1, 3, 4, 4]);
        assert_eq!(tensor[[0, 0, 0, 0]], 0.0);
        assert_eq!(tensor[[0, 1, 0, 0]], 0.0);
        assert_eq!(tensor[[0, 2, 0, 0]], 0.0);
    }

    #[test]
    fn test_padding_and_rotation() {
        let mut img = image::RgbImage::new(10, 10);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        let padded = utils::OcrUtils::make_padding(&img, 5).unwrap();
        assert_eq!(padded.width(), 20);
        assert_eq!(padded.height(), 20);
        assert_eq!(*padded.get_pixel(5, 5), image::Rgb([255, 0, 0]));
        assert_eq!(*padded.get_pixel(0, 0), image::Rgb([255, 255, 255]));

        let mut rot_img = img.clone();
        utils::OcrUtils::mat_rotate_clock_wise_180(&mut rot_img);
        assert_eq!(*rot_img.get_pixel(9, 9), image::Rgb([255, 0, 0]));
    }

    #[test]
    fn test_calculate_mean_with_mask() {
        let mut f_map = image::ImageBuffer::new(4, 4);
        for (i, pixel) in f_map.pixels_mut().enumerate() {
            *pixel = image::Luma([i as f32]);
        }
        let mut mask = image::GrayImage::new(4, 4);
        mask.put_pixel(0, 0, image::Luma([255]));
        mask.put_pixel(1, 0, image::Luma([255]));
        let mean = utils::OcrUtils::calculate_mean_with_mask(&f_map, &mask);
        assert_eq!(mean, 0.5);
    }

    #[test]
    fn test_db_net_degenerate_polygons() {
        let mut max_side = 0.0f32;

        // 0 points
        let empty_pts: Vec<imageproc::point::Point<i32>> = vec![];
        let res_empty = DbNet::get_mini_box(&empty_pts, &mut max_side).unwrap();
        assert!(res_empty.is_empty());

        // 1 point
        let single_pt = vec![imageproc::point::Point::new(10, 10)];
        let res_single = DbNet::get_mini_box(&single_pt, &mut max_side).unwrap();
        assert!(res_single.is_empty());

        // 2 points
        let two_pts = vec![
            imageproc::point::Point::new(10, 10),
            imageproc::point::Point::new(20, 20),
        ];
        let res_two = DbNet::get_mini_box(&two_pts, &mut max_side).unwrap();
        assert!(res_two.is_empty());

        // Unclip with degenerate points
        let unclip_empty = DbNet::unclip(&[], 1.5).unwrap();
        assert!(unclip_empty.is_empty());

        let unclip_few = DbNet::unclip(
            &[
                imageproc::point::Point::new(0.0, 0.0),
                imageproc::point::Point::new(1.0, 1.0),
            ],
            1.5,
        )
        .unwrap();
        assert!(unclip_few.is_empty());
    }

    #[test]
    fn test_zero_dimension_ocr_engine_detect() {
        let mut engine = PpOcrEngine::new();
        let zero_img = image::RgbImage::new(0, 0);
        let res =
            engine.detect_angle_rollback(&zero_img, 10, 300, 0.5, 0.3, 1.5, false, false, 0.5);
        assert!(res.is_ok());
        assert!(res.unwrap().text_blocks.is_empty());
    }
}
