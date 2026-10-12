use super::types::{OcrError, Point, ScaleParam, TextBox};
use super::utils::OcrUtils;
use geo_clipper::{Clipper, EndType, JoinType};
use geo_types::{Coord, LineString, Polygon};
use ort::inputs;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::session::{Session, SessionOutputs};
use ort::value::Tensor;
use std::cmp::Ordering;

const MEAN_VALUES: [f32; 3] = [0.485 * 255.0, 0.456 * 255.0, 0.406 * 255.0];
const NORM_VALUES: [f32; 3] = [
    1.0 / (0.229 * 255.0),
    1.0 / (0.224 * 255.0),
    1.0 / (0.225 * 255.0),
];

#[derive(Debug)]
pub struct DbNet {
    session: Option<Session>,
    input_names: Vec<String>,
}

impl Default for DbNet {
    fn default() -> Self {
        Self::new()
    }
}

impl DbNet {
    pub fn new() -> Self {
        Self {
            session: None,
            input_names: Vec::new(),
        }
    }

    pub fn init_model(
        &mut self,
        path: &str,
        num_thread: usize,
        builder_fn: Option<fn(SessionBuilder) -> ort::Result<SessionBuilder>>,
    ) -> Result<(), OcrError> {
        let builder = Session::builder()?;
        let builder = match builder_fn {
            Some(custom) => custom(builder)?,
            None => builder
                .with_optimization_level(GraphOptimizationLevel::Level2)?
                .with_intra_threads(num_thread)?
                .with_inter_threads(num_thread)?,
        };
        let session = builder.commit_from_file(path)?;
        self.input_names = session
            .inputs
            .iter()
            .map(|input| input.name.clone())
            .collect();
        self.session = Some(session);
        Ok(())
    }

    pub fn get_text_boxes(
        &mut self,
        img_src: &image::RgbImage,
        scale: &ScaleParam,
        box_score_thresh: f32,
        box_thresh: f32,
        un_clip_ratio: f32,
    ) -> Result<Vec<TextBox>, OcrError> {
        let session = self
            .session
            .as_mut()
            .ok_or(OcrError::SessionNotInitialized)?;
        if self.input_names.is_empty() {
            return Err(OcrError::SessionNotInitialized);
        }

        let src_resize = image::imageops::resize(
            img_src,
            scale.dst_width,
            scale.dst_height,
            image::imageops::FilterType::Triangle,
        );

        let input_tensors =
            OcrUtils::subtract_mean_normalize(&src_resize, &MEAN_VALUES, &NORM_VALUES);
        let tensor = Tensor::from_array(input_tensors)?;

        let outputs = session.run(inputs![self.input_names[0].as_str() => tensor])?;

        let text_boxes = Self::get_text_boxes_core(
            &outputs,
            src_resize.height(),
            src_resize.width(),
            scale,
            box_score_thresh,
            box_thresh,
            un_clip_ratio,
        )?;

        Ok(text_boxes)
    }

    fn get_text_boxes_core(
        output_tensor: &SessionOutputs,
        rows: u32,
        cols: u32,
        s: &ScaleParam,
        box_score_thresh: f32,
        box_thresh: f32,
        un_clip_ratio: f32,
    ) -> Result<Vec<TextBox>, OcrError> {
        let max_side_thresh = 3.0f32;
        let mut rs_boxes = Vec::new();

        let (_, red_data) = output_tensor.iter().next().ok_or(OcrError::EmptyOutput)?;
        let (_, pred_slice) = red_data.try_extract_tensor::<f32>()?;
        let pred_data: Vec<f32> = pred_slice.to_vec();

        let cbuf_data: Vec<u8> = pred_data
            .iter()
            .map(|pixel| (pixel * 255.0).clamp(0.0, 255.0) as u8)
            .collect();

        let pred_img =
            image::ImageBuffer::<image::Luma<f32>, Vec<f32>>::from_vec(cols, rows, pred_data)
                .ok_or_else(|| OcrError::InvalidImage("无法构建概率图缓存".into()))?;

        let cbuf_img = image::GrayImage::from_vec(cols, rows, cbuf_data)
            .ok_or_else(|| OcrError::InvalidImage("无法构建灰度图缓存".into()))?;

        let threshold_img = imageproc::contrast::threshold(
            &cbuf_img,
            (box_thresh * 255.0).clamp(0.0, 255.0) as u8,
            imageproc::contrast::ThresholdType::Binary,
        );

        let dilate_img = imageproc::morphology::dilate(
            &threshold_img,
            imageproc::distance_transform::Norm::LInf,
            1,
        );

        let img_contours: Vec<imageproc::contours::Contour<i32>> =
            imageproc::contours::find_contours(&dilate_img);

        for contour in img_contours {
            if contour.points.len() <= 2 {
                continue;
            }

            let mut max_side = 0.0f32;
            let min_box = Self::get_mini_box(&contour.points, &mut max_side)?;
            if max_side < max_side_thresh {
                continue;
            }

            let score = Self::get_score(&contour, &pred_img)?;
            if score < box_score_thresh {
                continue;
            }

            let clip_box = Self::unclip(&min_box, un_clip_ratio)?;
            if clip_box.is_empty() {
                continue;
            }

            let mut clip_contour = Vec::with_capacity(clip_box.len());
            for point in &clip_box {
                clip_contour.push(*point);
            }

            let mut max_side_clip = 0.0f32;
            let clip_min_box = Self::get_mini_box(&clip_contour, &mut max_side_clip)?;
            if max_side_clip < max_side_thresh + 2.0 {
                continue;
            }

            let mut final_points = Vec::with_capacity(clip_min_box.len());
            for item in clip_min_box {
                let x = (item.x / s.scale_width) as u32;
                let ptx = x.min(s.src_width);

                let y = (item.y / s.scale_height) as u32;
                let pty = y.min(s.src_height);

                final_points.push(Point { x: ptx, y: pty });
            }

            rs_boxes.push(TextBox {
                score,
                points: final_points,
            });
        }

        Ok(rs_boxes)
    }

    fn get_mini_box(
        contour_points: &[imageproc::point::Point<i32>],
        min_edge_size: &mut f32,
    ) -> Result<Vec<imageproc::point::Point<f32>>, OcrError> {
        let rect = imageproc::geometry::min_area_rect(contour_points);

        let mut rect_points: Vec<imageproc::point::Point<f32>> = rect
            .iter()
            .map(|p| imageproc::point::Point::new(p.x as f32, p.y as f32))
            .collect();

        let width = ((rect_points[0].x - rect_points[1].x).powi(2)
            + (rect_points[0].y - rect_points[1].y).powi(2))
        .sqrt();
        let height = ((rect_points[1].x - rect_points[2].x).powi(2)
            + (rect_points[1].y - rect_points[2].y).powi(2))
        .sqrt();

        *min_edge_size = width.min(height);

        rect_points.sort_by(|a, b| {
            if a.x > b.x {
                Ordering::Greater
            } else if a.x < b.x {
                Ordering::Less
            } else {
                Ordering::Equal
            }
        });

        let (index_1, index_4) = if rect_points[1].y > rect_points[0].y {
            (0, 1)
        } else {
            (1, 0)
        };

        let (index_2, index_3) = if rect_points[3].y > rect_points[2].y {
            (2, 3)
        } else {
            (3, 2)
        };

        let box_points = vec![
            rect_points[index_1],
            rect_points[index_2],
            rect_points[index_3],
            rect_points[index_4],
        ];

        Ok(box_points)
    }

    fn get_score(
        contour: &imageproc::contours::Contour<i32>,
        f_map_mat: &image::ImageBuffer<image::Luma<f32>, Vec<f32>>,
    ) -> Result<f32, OcrError> {
        let mut xmin = i32::MAX;
        let mut xmax = i32::MIN;
        let mut ymin = i32::MAX;
        let mut ymax = i32::MIN;

        for point in &contour.points {
            xmin = xmin.min(point.x);
            xmax = xmax.max(point.x);
            ymin = ymin.min(point.y);
            ymax = ymax.max(point.y);
        }

        let width = f_map_mat.width() as i32;
        let height = f_map_mat.height() as i32;

        xmin = xmin.clamp(0, width - 1);
        xmax = xmax.clamp(0, width - 1);
        ymin = ymin.clamp(0, height - 1);
        ymax = ymax.clamp(0, height - 1);

        let roi_width = xmax - xmin + 1;
        let roi_height = ymax - ymin + 1;

        if roi_width <= 0 || roi_height <= 0 {
            return Ok(0.0);
        }

        let mut mask = image::GrayImage::new(roi_width as u32, roi_height as u32);
        let pts: Vec<imageproc::point::Point<i32>> = contour
            .points
            .iter()
            .map(|p| imageproc::point::Point::new(p.x - xmin, p.y - ymin))
            .collect();

        imageproc::drawing::draw_polygon_mut(&mut mask, &pts, image::Luma([255]));

        let cropped_img = image::imageops::crop_imm(
            f_map_mat,
            xmin as u32,
            ymin as u32,
            roi_width as u32,
            roi_height as u32,
        )
        .to_image();

        let mean = OcrUtils::calculate_mean_with_mask(&cropped_img, &mask);
        Ok(mean)
    }

    fn unclip(
        box_points: &[imageproc::point::Point<f32>],
        unclip_ratio: f32,
    ) -> Result<Vec<imageproc::point::Point<i32>>, OcrError> {
        if box_points.len() < 4 {
            return Ok(Vec::new());
        }

        let clip_rect_width = ((box_points[0].x - box_points[1].x).powi(2)
            + (box_points[0].y - box_points[1].y).powi(2))
        .sqrt();
        let clip_rect_height = ((box_points[1].x - box_points[2].x).powi(2)
            + (box_points[1].y - box_points[2].y).powi(2))
        .sqrt();

        if clip_rect_height < 1.001 && clip_rect_width < 1.001 {
            return Ok(Vec::new());
        }

        let the_clipper_pts: Vec<Coord<f64>> = box_points
            .iter()
            .map(|pt| Coord {
                x: pt.x as f64,
                y: pt.y as f64,
            })
            .collect();

        let area = Self::signed_polygon_area(box_points).abs();
        let length = Self::length_of_points(box_points);
        if length < 1e-4 {
            return Ok(Vec::new());
        }
        let distance = area * unclip_ratio / length as f32;

        let co = Polygon::new(LineString::new(the_clipper_pts), vec![]);
        let solution = co
            .offset(
                distance as f64,
                JoinType::Round(2.0),
                EndType::ClosedPolygon,
                1.0,
            )
            .0;

        if solution.is_empty() {
            return Ok(Vec::new());
        }

        let mut ret_pts = Vec::new();
        if let Some(first) = solution.first() {
            for ip in first.exterior().points() {
                ret_pts.push(imageproc::point::Point::new(ip.x() as i32, ip.y() as i32));
            }
        }

        Ok(ret_pts)
    }

    fn signed_polygon_area(points: &[imageproc::point::Point<f32>]) -> f32 {
        let num_points = points.len();
        if num_points < 3 {
            return 0.0;
        }

        let mut area = 0.0f32;
        for i in 0..num_points {
            let next = (i + 1) % num_points;
            area += (points[next].x - points[i].x) * (points[next].y + points[i].y) / 2.0;
        }
        area
    }

    fn length_of_points(box_points: &[imageproc::point::Point<f32>]) -> f64 {
        if box_points.len() < 2 {
            return 0.0;
        }

        let mut length = 0.0f64;
        let mut x0 = box_points[0].x as f64;
        let mut y0 = box_points[0].y as f64;

        for pt in box_points.iter().skip(1) {
            let x1 = pt.x as f64;
            let y1 = pt.y as f64;
            let dx = x1 - x0;
            let dy = y1 - y0;
            length += (dx * dx + dy * dy).sqrt();
            x0 = x1;
            y0 = y1;
        }

        // 闭合最后一条边
        let dx = box_points[0].x as f64 - x0;
        let dy = box_points[0].y as f64 - y0;
        length += (dx * dx + dy * dy).sqrt();

        length
    }
}
