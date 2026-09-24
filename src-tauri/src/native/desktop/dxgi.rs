use super::ScreenRect;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO, IDXGIAdapter1,
    IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
use windows::core::Interface;

struct DuplFrameGuard<'a> {
    dupl: &'a IDXGIOutputDuplication,
    released: bool,
}

impl<'a> DuplFrameGuard<'a> {
    fn new(dupl: &'a IDXGIOutputDuplication) -> Self {
        Self {
            dupl,
            released: false,
        }
    }

    fn release(&mut self) {
        if !self.released {
            unsafe {
                let _ = self.dupl.ReleaseFrame();
            }
            self.released = true;
        }
    }
}

impl Drop for DuplFrameGuard<'_> {
    fn drop(&mut self) {
        self.release();
    }
}

pub struct DxgiDesktop {
    factory: IDXGIFactory1,
}

impl DxgiDesktop {
    pub fn new() -> Result<Self, String> {
        unsafe {
            let factory: IDXGIFactory1 =
                CreateDXGIFactory1().map_err(|e| format!("无法创建 DXGI 工厂：{e}"))?;
            Ok(Self { factory })
        }
    }

    /// Capture an output monitor that intersects `region`.
    pub fn capture_output(&self, region: ScreenRect) -> Result<image::RgbaImage, String> {
        unsafe {
            let mut cursor_pt = POINT::default();
            let has_cursor = GetCursorPos(&mut cursor_pt).is_ok();

            let mut target_output: Option<(IDXGIAdapter1, IDXGIOutput1, ScreenRect)> = None;
            let mut fallback_output: Option<(IDXGIAdapter1, IDXGIOutput1, ScreenRect)> = None;

            let mut adapter_idx = 0;
            while let Ok(adapter) = self.factory.EnumAdapters1(adapter_idx) {
                let mut output_idx = 0;
                while let Ok(output) = adapter.EnumOutputs(output_idx) {
                    if let Ok(desc) = output.GetDesc() {
                        let rect = ScreenRect {
                            x: desc.DesktopCoordinates.left,
                            y: desc.DesktopCoordinates.top,
                            width: (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left)
                                .max(0) as u32,
                            height: (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top)
                                .max(0) as u32,
                        };

                        let contains_cursor = has_cursor
                            && cursor_pt.x >= rect.x
                            && cursor_pt.x < rect.x + rect.width as i32
                            && cursor_pt.y >= rect.y
                            && cursor_pt.y < rect.y + rect.height as i32;

                        let overlaps = !(region.x + region.width as i32 <= rect.x
                            || region.x >= rect.x + rect.width as i32
                            || region.y + region.height as i32 <= rect.y
                            || region.y >= rect.y + rect.height as i32);

                        if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                            if overlaps {
                                target_output = Some((adapter.clone(), output1, rect));
                                break;
                            } else if contains_cursor && fallback_output.is_none() {
                                fallback_output = Some((adapter.clone(), output1, rect));
                            }
                        }
                    }
                    output_idx += 1;
                }
                if target_output.is_some() {
                    break;
                }
                adapter_idx += 1;
            }

            let (target_adapter, output1, monitor_rect) = target_output
                .or(fallback_output)
                .ok_or("未找到匹配当前选区的显示器输出")?;

            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut feature_level = D3D_FEATURE_LEVEL_11_0;
            let levels = [D3D_FEATURE_LEVEL_11_0];
            let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;

            D3D11CreateDevice(
                &target_adapter,
                windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
                None,
                flags,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            )
            .map_err(|e| format!("无法创建与显示器匹配的 D3D11 设备：{e}"))?;

            let device = device.ok_or("D3D11 设备创建为空")?;
            let context = context.ok_or("D3D11 上下文创建为空")?;

            // Acquire duplicate output interface
            let dupl = output1
                .DuplicateOutput(&device)
                .map_err(|e| format!("无法初始化 DXGI 桌面复制：{e}"))?;

            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let mut frame_guard: Option<DuplFrameGuard> = None;
            for _ in 0..5 {
                let status = dupl.AcquireNextFrame(50, &mut frame_info, &mut resource);
                if status.is_ok() {
                    if frame_info.AccumulatedFrames > 0 && resource.is_some() {
                        frame_guard = Some(DuplFrameGuard::new(&dupl));
                        break;
                    }
                    let _ = dupl.ReleaseFrame();
                } else if status == Err(DXGI_ERROR_WAIT_TIMEOUT.into()) {
                    continue;
                } else {
                    return Err(format!("DXGI 帧获取失败：{status:?}"));
                }
            }

            let mut frame_guard = frame_guard.ok_or_else(|| "DXGI 未捕获到新屏幕帧".to_string())?;

            let texture: ID3D11Texture2D = resource.unwrap().cast().map_err(|e| e.to_string())?;

            let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut tex_desc);

            // Create staging texture for CPU read
            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: tex_desc.Width,
                Height: tex_desc.Height,
                MipLevels: 1,
                ArraySize: 1,
                Format: tex_desc.Format,
                SampleDesc: tex_desc.SampleDesc,
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };

            let mut staging: Option<ID3D11Texture2D> = None;
            device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .map_err(|e| format!("创建暂存纹理失败：{e}"))?;
            let staging = staging.unwrap();

            context.CopyResource(&staging, &texture);

            // Release the frame from duplication pipeline as soon as GPU copy is dispatched
            frame_guard.release();

            // Map CPU staging buffer
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| format!("映射显存数据失败：{e}"))?;

            let width = tex_desc.Width as usize;
            let height = tex_desc.Height as usize;
            let row_pitch = mapped.RowPitch as usize;
            let src_slice =
                std::slice::from_raw_parts(mapped.pData as *const u8, row_pitch * height);

            // Read BGRA/RGBA and convert to RGBA
            let mut rgba_bytes = vec![0u8; width * height * 4];
            let is_bgra = tex_desc.Format
                == windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
            for row in 0..height {
                let src_row = &src_slice[row * row_pitch..row * row_pitch + width * 4];
                let dst_row = &mut rgba_bytes[row * width * 4..(row + 1) * width * 4];
                for (src_px, dst_px) in src_row
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(dst_row.as_chunks_mut::<4>().0.iter_mut())
                {
                    if is_bgra {
                        dst_px[0] = src_px[2]; // R
                        dst_px[1] = src_px[1]; // G
                        dst_px[2] = src_px[0]; // B
                    } else {
                        dst_px[0] = src_px[0];
                        dst_px[1] = src_px[1];
                        dst_px[2] = src_px[2];
                    }
                    dst_px[3] = 255; // A
                }
            }

            context.Unmap(&staging, 0);

            let monitor_image = image::RgbaImage::from_raw(width as u32, height as u32, rgba_bytes)
                .ok_or_else(|| "DXGI 帧尺寸无效".to_string())?;

            if !monitor_image
                .pixels()
                .any(|p| p[0] > 0 || p[1] > 0 || p[2] > 0)
            {
                return Err("DXGI 截图像素全为空白。".into());
            }

            crop_monitor_frame(&monitor_image, monitor_rect, region)
        }
    }
}

pub(crate) fn crop_monitor_frame(
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
