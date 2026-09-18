use super::ScreenRect;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIOutput1,
    IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

pub struct DxgiDesktop {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
}

impl DxgiDesktop {
    pub fn new() -> Result<Self, String> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        let mut feature_level = D3D_FEATURE_LEVEL_11_0;

        let levels = [D3D_FEATURE_LEVEL_11_0];
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;

        unsafe {
            // First attempt: Hardware driver
            let res = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                flags,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            );

            // Fallback: WARP software rasterizer if hardware unavailable
            if res.is_err() {
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    None,
                    flags,
                    Some(&levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    Some(&mut feature_level),
                    Some(&mut context),
                )
                .map_err(|e| format!("无法创建 D3D11 设备：{e}"))?;
            }
        }

        let device = device.ok_or("D3D11 设备指针为空")?;
        let context = context.ok_or("D3D11 上下文指针为空")?;

        Ok(Self { device, context })
    }

    /// Capture an output monitor that intersects `region`.
    pub fn capture_output(&self, region: ScreenRect) -> Result<image::RgbaImage, String> {
        unsafe {
            let dxgi_device: IDXGIDevice = self.device.cast().map_err(|e| e.to_string())?;
            let adapter = dxgi_device.GetAdapter().map_err(|e| e.to_string())?;

            let mut cursor_pt = POINT::default();
            let has_cursor = GetCursorPos(&mut cursor_pt).is_ok();

            let mut target_output: Option<(IDXGIOutput1, ScreenRect)> = None;
            let mut fallback_output: Option<(IDXGIOutput1, ScreenRect)> = None;
            let mut i = 0;
            while let Ok(output) = adapter.EnumOutputs(i) {
                if let Ok(desc) = output.GetDesc() {
                    let rect = ScreenRect {
                        x: desc.DesktopCoordinates.left,
                        y: desc.DesktopCoordinates.top,
                        width: (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).max(0) as u32,
                        height: (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).max(0) as u32,
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

                    if contains_cursor {
                        if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                            target_output = Some((output1, rect));
                            break;
                        }
                    } else if overlaps
                        && fallback_output.is_none()
                        && let Ok(output1) = output.cast::<IDXGIOutput1>()
                    {
                        fallback_output = Some((output1, rect));
                    }
                }
                i += 1;
            }

            let (output1, monitor_rect) = target_output
                .or(fallback_output)
                .ok_or("未找到匹配当前选区的显示器输出")?;

            // Acquire duplicate output interface
            let dupl = output1
                .DuplicateOutput(&self.device)
                .map_err(|e| format!("无法初始化 DXGI 桌面复制：{e}"))?;

            // Try acquiring a frame up to 5 times (short timeout for games/MPO)
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut resource: Option<IDXGIResource> = None;
            let mut acquired = false;

            for _ in 0..5 {
                let status = dupl.AcquireNextFrame(50, &mut frame_info, &mut resource);
                if status.is_ok() {
                    acquired = true;
                    break;
                } else if status == Err(DXGI_ERROR_WAIT_TIMEOUT.into()) {
                    continue;
                } else {
                    return Err(format!("DXGI 帧获取失败：{status:?}"));
                }
            }

            if !acquired || resource.is_none() {
                return Err("DXGI 获取屏幕帧超时".into());
            }

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
            self.device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .map_err(|e| format!("创建暂存纹理失败：{e}"))?;
            let staging = staging.unwrap();

            self.context.CopyResource(&staging, &texture);

            // Release the frame from duplication pipeline as soon as GPU copy is dispatched
            let _ = dupl.ReleaseFrame();

            // Map CPU staging buffer
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| format!("映射显存数据失败：{e}"))?;

            let width = tex_desc.Width as usize;
            let height = tex_desc.Height as usize;
            let row_pitch = mapped.RowPitch as usize;
            let src_slice = std::slice::from_raw_parts(mapped.pData as *const u8, row_pitch * height);

            // Read BGRA and convert to RGBA
            let mut rgba_bytes = vec![0u8; width * height * 4];
            for row in 0..height {
                let src_row = &src_slice[row * row_pitch..row * row_pitch + width * 4];
                let dst_row = &mut rgba_bytes[row * width * 4..(row + 1) * width * 4];
                for (src_px, dst_px) in src_row.as_chunks::<4>().0.iter().zip(dst_row.as_chunks_mut::<4>().0.iter_mut()) {
                    dst_px[0] = src_px[2]; // R
                    dst_px[1] = src_px[1]; // G
                    dst_px[2] = src_px[0]; // B
                    dst_px[3] = 255;       // A
                }
            }

            self.context.Unmap(&staging, 0);

            let monitor_image = image::RgbaImage::from_raw(width as u32, height as u32, rgba_bytes)
                .ok_or_else(|| "DXGI 帧尺寸无效".to_string())?;

            // If requested region is within the monitor, crop it
            let rel_x = region.x - monitor_rect.x;
            let rel_y = region.y - monitor_rect.y;

            if rel_x >= 0
                && rel_y >= 0
                && rel_x as u32 + region.width <= monitor_rect.width
                && rel_y as u32 + region.height <= monitor_rect.height
            {
                Ok(image::imageops::crop_imm(
                    &monitor_image,
                    rel_x as u32,
                    rel_y as u32,
                    region.width,
                    region.height,
                )
                .to_image())
            } else {
                Ok(monitor_image)
            }
        }
    }
}
