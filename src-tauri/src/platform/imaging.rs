//! Portable screenshot pixel helpers shared by capture backends.

use super::desktop::ScreenRect;

/// True when every pixel is black, e.g. an exclusive-fullscreen frame.
pub fn is_all_black(image: &image::RgbaImage) -> bool {
    !image.pixels().any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
}

pub fn crop_monitor_frame(
    monitor_image: &image::RgbaImage,
    monitor_rect: ScreenRect,
    region: ScreenRect,
) -> Result<image::RgbaImage, String> {
    let inter_left = region.x.max(monitor_rect.x);
    let inter_top = region.y.max(monitor_rect.y);
    let inter_right =
        (region.x + region.width as i32).min(monitor_rect.x + monitor_rect.width as i32);
    let inter_bottom =
        (region.y + region.height as i32).min(monitor_rect.y + monitor_rect.height as i32);

    if inter_right <= inter_left || inter_bottom <= inter_top {
        return Err("选区不在显示器有效范围内".into());
    }

    let inter_w = (inter_right - inter_left) as u32;
    let inter_h = (inter_bottom - inter_top) as u32;
    let src_x = (inter_left - monitor_rect.x) as u32;
    let src_y = (inter_top - monitor_rect.y) as u32;

    if inter_w == region.width && inter_h == region.height {
        Ok(
            image::imageops::crop_imm(monitor_image, src_x, src_y, region.width, region.height)
                .to_image(),
        )
    } else {
        let mut canvas = image::RgbaImage::new(region.width, region.height);
        let cropped =
            image::imageops::crop_imm(monitor_image, src_x, src_y, inter_w, inter_h).to_image();
        let dst_x = (inter_left - region.x) as i64;
        let dst_y = (inter_top - region.y) as i64;
        image::imageops::overlay(&mut canvas, &cropped, dst_x, dst_y);
        Ok(canvas)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crop_monitor_frame_fully_contained() {
        let monitor_rect = ScreenRect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let mut monitor_image = image::RgbaImage::new(1920, 1080);
        monitor_image.put_pixel(100, 100, image::Rgba([255, 0, 0, 255]));

        let region = ScreenRect {
            x: 100,
            y: 100,
            width: 50,
            height: 50,
        };

        let cropped = crop_monitor_frame(&monitor_image, monitor_rect, region).unwrap();
        assert_eq!(cropped.width(), 50);
        assert_eq!(cropped.height(), 50);
        assert_eq!(cropped.get_pixel(0, 0), &image::Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn test_crop_monitor_frame_negative_monitor_origin() {
        let monitor_rect = ScreenRect {
            x: -1920,
            y: -200,
            width: 1920,
            height: 1080,
        };
        let mut monitor_image = image::RgbaImage::new(1920, 1080);
        // Put pixel at relative offset (100, 100), corresponding to global (-1820, -100)
        monitor_image.put_pixel(100, 100, image::Rgba([0, 255, 0, 255]));

        let region = ScreenRect {
            x: -1820,
            y: -100,
            width: 60,
            height: 40,
        };

        let cropped = crop_monitor_frame(&monitor_image, monitor_rect, region).unwrap();
        assert_eq!(cropped.width(), 60);
        assert_eq!(cropped.height(), 40);
        assert_eq!(cropped.get_pixel(0, 0), &image::Rgba([0, 255, 0, 255]));
    }

    #[test]
    fn test_crop_monitor_frame_partial_cross_boundary() {
        let monitor_rect = ScreenRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let mut monitor_image = image::RgbaImage::new(100, 100);
        monitor_image.put_pixel(90, 90, image::Rgba([0, 0, 255, 255]));

        // Region extends beyond the monitor right edge
        let region = ScreenRect {
            x: 80,
            y: 80,
            width: 50,
            height: 50,
        };

        let cropped = crop_monitor_frame(&monitor_image, monitor_rect, region).unwrap();
        assert_eq!(cropped.width(), 50);
        assert_eq!(cropped.height(), 50);
        // Intersecting pixel at (90, 90) -> (10, 10) on canvas
        assert_eq!(cropped.get_pixel(10, 10), &image::Rgba([0, 0, 255, 255]));
        // Outside pixel at (30, 30) on canvas (global 110, 110) should be transparent/black
        assert_eq!(cropped.get_pixel(30, 30), &image::Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn test_crop_monitor_frame_completely_outside() {
        let monitor_rect = ScreenRect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        let monitor_image = image::RgbaImage::new(100, 100);

        let region = ScreenRect {
            x: 200,
            y: 200,
            width: 50,
            height: 50,
        };

        assert!(crop_monitor_frame(&monitor_image, monitor_rect, region).is_err());
    }
}
