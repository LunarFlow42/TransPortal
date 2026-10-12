use super::types::{OcrError, Point, TextBox};
use image::imageops;
use imageproc::geometric_transformations::{Interpolation, Projection};
use ndarray::{Array, Array4};

pub struct OcrUtils;

impl OcrUtils {
    /// 归一化输入图像为 NCHW (1, 3, H, W) 浮点张量
    pub fn subtract_mean_normalize(
        img_src: &image::RgbImage,
        mean_vals: &[f32],
        norm_vals: &[f32],
    ) -> Array4<f32> {
        let cols = img_src.width() as usize;
        let rows = img_src.height() as usize;
        let channels = 3usize;

        let mut input_tensor = Array::zeros((1, channels, rows, cols));
        let raw = img_src.as_raw();

        for r in 0..rows {
            let row_offset = r * cols * channels;
            for c in 0..cols {
                let pixel_offset = row_offset + c * channels;
                for ch in 0..channels {
                    let val = f32::from(raw[pixel_offset + ch]);
                    input_tensor[[0, ch, r, c]] = (val - mean_vals[ch]) * norm_vals[ch];
                }
            }
        }

        input_tensor
    }

    /// 在原图像四周添加空白白色内边距（提升边界文本检测精度）
    pub fn make_padding(
        img_src: &image::RgbImage,
        padding: u32,
    ) -> Result<image::RgbImage, OcrError> {
        if padding == 0 {
            return Ok(img_src.clone());
        }

        let width = img_src.width();
        let height = img_src.height();
        let mut padding_src = image::RgbImage::new(width + 2 * padding, height + 2 * padding);

        imageproc::drawing::draw_filled_rect_mut(
            &mut padding_src,
            imageproc::rect::Rect::at(0, 0).of_size(width + 2 * padding, height + 2 * padding),
            image::Rgb([255, 255, 255]),
        );

        image::imageops::replace(
            &mut padding_src,
            img_src,
            i64::from(padding),
            i64::from(padding),
        );
        Ok(padding_src)
    }

    /// 根据文本检测框切分裁剪各文本区域
    pub fn get_part_images(
        img_src: &image::RgbImage,
        text_boxes: &[TextBox],
    ) -> Vec<image::RgbImage> {
        text_boxes
            .iter()
            .map(|text_box| Self::get_rotate_crop_image(img_src, &text_box.points))
            .collect()
    }

    /// 基于四顶点透视变换裁剪旋转文本行
    pub fn get_rotate_crop_image(
        img_src: &image::RgbImage,
        box_points: &[Point],
    ) -> image::RgbImage {
        if box_points.len() < 4 {
            return image::RgbImage::new(1, 1);
        }

        let mut points = box_points.to_vec();

        let (min_x, min_y, max_x, max_y) = points.iter().fold(
            (u32::MAX, u32::MAX, 0u32, 0u32),
            |(min_x, min_y, max_x, max_y), point| {
                (
                    min_x.min(point.x),
                    min_y.min(point.y),
                    max_x.max(point.x),
                    max_y.max(point.y),
                )
            },
        );

        let crop_w = (max_x.saturating_sub(min_x)).max(1);
        let crop_h = (max_y.saturating_sub(min_y)).max(1);

        let img_crop = imageops::crop_imm(img_src, min_x, min_y, crop_w, crop_h).to_image();

        for point in &mut points {
            point.x = point.x.saturating_sub(min_x);
            point.y = point.y.saturating_sub(min_y);
        }

        let width_diff_x = points[0].x as f32 - points[1].x as f32;
        let width_diff_y = points[0].y as f32 - points[1].y as f32;
        let img_crop_width =
            ((width_diff_x * width_diff_x + width_diff_y * width_diff_y).sqrt() as u32).max(1);

        let height_diff_x = points[0].x as f32 - points[3].x as f32;
        let height_diff_y = points[0].y as f32 - points[3].y as f32;
        let img_crop_height =
            ((height_diff_x * height_diff_x + height_diff_y * height_diff_y).sqrt() as u32).max(1);

        let src_points = [
            (points[0].x as f32, points[0].y as f32),
            (points[1].x as f32, points[1].y as f32),
            (points[2].x as f32, points[2].y as f32),
            (points[3].x as f32, points[3].y as f32),
        ];

        let dst_points = [
            (0.0, 0.0),
            (img_crop_width as f32, 0.0),
            (img_crop_width as f32, img_crop_height as f32),
            (0.0, img_crop_height as f32),
        ];

        let projection = match Projection::from_control_points(src_points, dst_points) {
            Some(p) => p,
            None => return img_crop,
        };

        let mut part_img = image::RgbImage::new(img_crop_width, img_crop_height);
        imageproc::geometric_transformations::warp_into(
            &img_crop,
            &projection,
            Interpolation::Nearest,
            image::Rgb([255, 255, 255]),
            &mut part_img,
        );

        if part_img.height() >= part_img.width() * 3 / 2 {
            let mut rotated = image::RgbImage::new(part_img.height(), part_img.width());
            for (x, y, pixel) in part_img.enumerate_pixels() {
                rotated.put_pixel(
                    y,
                    part_img.width().saturating_sub(1).saturating_sub(x),
                    *pixel,
                );
            }
            rotated
        } else {
            part_img
        }
    }

    /// 顺时针旋转 180 度
    pub fn mat_rotate_clock_wise_180(src: &mut image::RgbImage) {
        imageops::rotate180_in_place(src);
    }

    /// 计算掩码区域内的预测概率均值
    pub fn calculate_mean_with_mask(
        img: &image::ImageBuffer<image::Luma<f32>, Vec<f32>>,
        mask: &image::ImageBuffer<image::Luma<u8>, Vec<u8>>,
    ) -> f32 {
        if img.width() != mask.width() || img.height() != mask.height() {
            return 0.0;
        }

        let mut sum: f32 = 0.0;
        let mut mask_count = 0usize;

        for y in 0..img.height() {
            for x in 0..img.width() {
                if mask.get_pixel(x, y)[0] > 0 {
                    sum += img.get_pixel(x, y)[0];
                    mask_count += 1;
                }
            }
        }

        if mask_count == 0 {
            0.0
        } else {
            sum / mask_count as f32
        }
    }
}
