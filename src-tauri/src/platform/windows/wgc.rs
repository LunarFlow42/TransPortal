//! Direct3D 11 Windows.Graphics.Capture (WGC) modern capture pipeline.
//!
//! Provides zero-flicker, real-time screen capture directly from the Windows Compositor
//! and Direct3D swapchain. This eliminates the ~16ms stale frame delay and taskbar
//! flicker associated with legacy GDI / PrintWindow captures in borderless fullscreen games
//! (Independent Flip / MPO), and fully supports hybrid laptops and virtual displays (IDD).

use crate::platform::desktop::ScreenRect;
use crate::platform::imaging::crop_monitor_frame;
use std::sync::mpsc::channel;
use std::time::Duration;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::core::Interface;

pub struct WgcDesktop {
    d3d11_device: ID3D11Device,
    d3d11_context: ID3D11DeviceContext,
    winrt_device: IDirect3DDevice,
}

unsafe impl Send for WgcDesktop {}
unsafe impl Sync for WgcDesktop {}

#[derive(Clone, Debug, Default)]
pub struct WgcMetrics {
    pub frame0_latency_ms: u64,
    pub frame1_latency_ms: Option<u64>,
    pub frames_drained: u32,
    pub staging_copy_ms: u64,
    pub total_ms: u64,
}

impl WgcDesktop {
    /// Initialize Direct3D 11 device and WinRT interop bridge.
    pub fn new() -> Result<Self, String> {
        unsafe {
            if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
                return Err("当前系统不支持 Windows.Graphics.Capture".into());
            }

            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut feature_level = D3D_FEATURE_LEVEL_11_0;
            let levels = [D3D_FEATURE_LEVEL_11_0];
            let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;

            // Attempt hardware adapter first, fallback to WARP software rasterizer if needed
            let hr = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                HMODULE::default(),
                flags,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            );

            let (device, context) = if hr.is_ok() {
                (
                    device.ok_or("D3D11 hardware device is None")?,
                    context.ok_or("D3D11 hardware context is None")?,
                )
            } else {
                let mut warp_dev: Option<ID3D11Device> = None;
                let mut warp_ctx: Option<ID3D11DeviceContext> = None;
                D3D11CreateDevice(
                    None,
                    D3D_DRIVER_TYPE_WARP,
                    HMODULE::default(),
                    flags,
                    Some(&levels),
                    D3D11_SDK_VERSION,
                    Some(&mut warp_dev),
                    Some(&mut feature_level),
                    Some(&mut warp_ctx),
                )
                .map_err(|e| format!("无法创建 D3D11 设备: {e}"))?;

                (
                    warp_dev.ok_or("D3D11 WARP device is None")?,
                    warp_ctx.ok_or("D3D11 WARP context is None")?,
                )
            };

            let dxgi_device: IDXGIDevice = device
                .cast()
                .map_err(|e| format!("Failed to query IDXGIDevice: {e}"))?;

            let inspectable = CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)
                .map_err(|e| format!("Failed to create WinRT Direct3D device from DXGI: {e}"))?;

            let winrt_device: IDirect3DDevice = inspectable
                .cast()
                .map_err(|e| format!("Failed to cast to IDirect3DDevice: {e}"))?;

            Ok(Self {
                d3d11_device: device,
                d3d11_context: context,
                winrt_device,
            })
        }
    }

    /// Captures a GraphicsCaptureItem (monitor or window) and crops to `region`.
    pub fn capture_item(
        &self,
        item: GraphicsCaptureItem,
        item_rect: ScreenRect,
        region: ScreenRect,
        drain_newer: bool,
    ) -> Result<(image::RgbaImage, WgcMetrics), String> {
        let total_start = std::time::Instant::now();
        let size = item
            .Size()
            .map_err(|e| format!("获取屏幕分辨率失败: {e}"))?;
        if size.Width <= 0 || size.Height <= 0 {
            return Err("屏幕分辨率无效".into());
        }

        // Use 3 buffers for true triple-buffering to avoid stalling during iFlip swapchain handoff
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &self.winrt_device,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            3,
            size,
        )
        .map_err(|e| format!("Failed to create Direct3D11CaptureFramePool: {e}"))?;

        let session = frame_pool
            .CreateCaptureSession(&item)
            .map_err(|e| format!("Failed to create GraphicsCaptureSession: {e}"))?;

        // Disable capture border (yellow outline) and mouse cursor
        let _ = session.SetIsBorderRequired(false);
        let _ = session.SetIsCursorCaptureEnabled(false);

        let (tx, rx) = channel();
        let _token = frame_pool
            .FrameArrived(&windows::Foundation::TypedEventHandler::<
                Direct3D11CaptureFramePool,
                windows::core::IInspectable,
            >::new(move |pool, _| {
                if let Ok(pool) = pool.ok()
                    && let Ok(frame) = pool.TryGetNextFrame()
                {
                    let _ = tx.send(frame);
                }
                Ok(())
            }))
            .map_err(|e| format!("Failed to register FrameArrived handler: {e}"))?;

        struct WgcSessionGuard {
            session: GraphicsCaptureSession,
            frame_pool: Direct3D11CaptureFramePool,
        }

        impl Drop for WgcSessionGuard {
            fn drop(&mut self) {
                let _ = self.session.Close();
                let _ = self.frame_pool.Close();
            }
        }

        let guard = WgcSessionGuard {
            session: session.clone(),
            frame_pool: frame_pool.clone(),
        };

        session
            .StartCapture()
            .map_err(|e| format!("Failed to start GraphicsCaptureSession: {e}"))?;
        let capture_started = std::time::Instant::now();

        // Wait up to 300ms for the initial frame (frame 0) to arrive
        let mut frame = rx
            .recv_timeout(Duration::from_millis(300))
            .map_err(|e| format!("等待 WGC 画面就绪超时: {e}"))?;
        let frame0_latency_ms = capture_started.elapsed().as_millis() as u64;

        let frame1_start = std::time::Instant::now();
        let mut frame1_latency_ms = None;
        let mut frames_drained = 0;

        // Smart drain:
        // For monitor capture during iFlip transitions, wait up to 120ms for Frame 1.
        // For window capture, Frame 0 is already the direct swapchain frame, so return immediately.
        if drain_newer && let Ok(newer_frame) = rx.recv_timeout(Duration::from_millis(120)) {
            frame1_latency_ms = Some(frame1_start.elapsed().as_millis() as u64);
            frame = newer_frame;
            while let Ok(latest) = rx.try_recv() {
                frame = latest;
                frames_drained += 1;
            }
        }

        // Stop session and frame pool promptly
        drop(guard);

        let staging_start = std::time::Instant::now();
        let surface = frame
            .Surface()
            .map_err(|e| format!("Failed to get frame Surface: {e}"))?;

        let access: IDirect3DDxgiInterfaceAccess = surface
            .cast()
            .map_err(|e| format!("Failed to query IDirect3DDxgiInterfaceAccess: {e}"))?;

        let texture: ID3D11Texture2D = unsafe {
            access
                .GetInterface()
                .map_err(|e| format!("Failed to extract ID3D11Texture2D from surface: {e}"))?
        };

        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };

        let staging_desc = D3D11_TEXTURE2D_DESC {
            Width: desc.Width,
            Height: desc.Height,
            MipLevels: 1,
            ArraySize: 1,
            Format: desc.Format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };

        let mut staging: Option<ID3D11Texture2D> = None;
        unsafe {
            self.d3d11_device
                .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                .map_err(|e| format!("Failed to create staging texture: {e}"))?;
        }
        let staging = staging.ok_or_else(|| "Staging texture is None".to_string())?;

        unsafe {
            self.d3d11_context.CopyResource(&staging, &texture);
        }

        let width = desc.Width as usize;
        let height = desc.Height as usize;
        let mut rgba_bytes = vec![0u8; width * height * 4];

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.d3d11_context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| format!("Failed to map staging texture: {e}"))?;
        }

        struct StagingMapGuard<'a> {
            context: &'a ID3D11DeviceContext,
            staging: &'a ID3D11Texture2D,
            subresource: u32,
        }

        impl Drop for StagingMapGuard<'_> {
            fn drop(&mut self) {
                unsafe {
                    self.context.Unmap(self.staging, self.subresource);
                }
            }
        }

        let _map_guard = StagingMapGuard {
            context: &self.d3d11_context,
            staging: &staging,
            subresource: 0,
        };

        let row_pitch = mapped.RowPitch as usize;
        convert_bgra_to_rgba(
            mapped.pData as *const u8,
            width,
            height,
            row_pitch,
            &mut rgba_bytes,
        );
        drop(_map_guard);
        let staging_copy_ms = staging_start.elapsed().as_millis() as u64;

        let full_image = image::RgbaImage::from_raw(desc.Width, desc.Height, rgba_bytes)
            .ok_or_else(|| "Failed to construct RgbaImage from WGC buffer".to_string())?;

        let cropped = crop_monitor_frame(&full_image, item_rect, region)?;
        let total_ms = total_start.elapsed().as_millis() as u64;

        let metrics = WgcMetrics {
            frame0_latency_ms,
            frame1_latency_ms,
            frames_drained,
            staging_copy_ms,
            total_ms,
        };

        Ok((cropped, metrics))
    }

    /// Captures the specified monitor using Windows.Graphics.Capture and crops to `region`.
    pub fn capture_monitor(
        &self,
        monitor: HMONITOR,
        monitor_rect: ScreenRect,
        region: ScreenRect,
    ) -> Result<(image::RgbaImage, WgcMetrics), String> {
        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| format!("Failed to get IGraphicsCaptureItemInterop factory: {e}"))?;

        let item: GraphicsCaptureItem = unsafe {
            interop
                .CreateForMonitor(monitor)
                .map_err(|e| format!("Failed to create GraphicsCaptureItem for monitor: {e}"))?
        };

        self.capture_item(item, monitor_rect, region, true)
    }

    /// Captures the specified window using Windows.Graphics.Capture and crops to `region`.
    pub fn capture_window(
        &self,
        window: HWND,
        window_rect: ScreenRect,
        region: ScreenRect,
    ) -> Result<(image::RgbaImage, WgcMetrics), String> {
        let interop: IGraphicsCaptureItemInterop =
            windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
                .map_err(|e| format!("Failed to get IGraphicsCaptureItemInterop factory: {e}"))?;

        let item: GraphicsCaptureItem = unsafe {
            interop
                .CreateForWindow(window)
                .map_err(|e| format!("Failed to create GraphicsCaptureItem for window: {e}"))?
        };

        // Window capture targets the window swapchain directly: Frame 0 is already live, drain_newer is false
        self.capture_item(item, window_rect, region, false)
    }
}

/// Helper to convert mapped BGRA bytes to RGBA with opaque alpha using 32-bit word manipulation.
pub(crate) fn convert_bgra_to_rgba(
    p_data: *const u8,
    width: usize,
    height: usize,
    row_pitch: usize,
    rgba_bytes: &mut [u8],
) {
    let dst_ptr = rgba_bytes.as_mut_ptr() as *mut u32;

    if row_pitch == width * 4 {
        let total_pixels = width * height;
        let src_ptr = p_data as *const u32;
        for i in 0..total_pixels {
            let px = unsafe { std::ptr::read_unaligned(src_ptr.add(i)) };
            // Little-endian BGRA (0xAARRGGBB) to RGBA with opaque alpha (0xFFBBGGRR)
            let dst_px = 0xFF00_0000
                | ((px & 0x00FF_0000) >> 16)
                | (px & 0x0000_FF00)
                | ((px & 0x0000_00FF) << 16);
            unsafe { std::ptr::write_unaligned(dst_ptr.add(i), dst_px) };
        }
    } else {
        for y in 0..height {
            let src_row = unsafe { p_data.add(y * row_pitch) as *const u32 };
            let dst_row = unsafe { dst_ptr.add(y * width) };
            for x in 0..width {
                let px = unsafe { std::ptr::read_unaligned(src_row.add(x)) };
                let dst_px = 0xFF00_0000
                    | ((px & 0x00FF_0000) >> 16)
                    | (px & 0x0000_FF00)
                    | ((px & 0x0000_00FF) << 16);
                unsafe { std::ptr::write_unaligned(dst_row.add(x), dst_px) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wgc_desktop_initialization() {
        if GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            let wgc = WgcDesktop::new();
            assert!(
                wgc.is_ok(),
                "WGC should initialize successfully on supported Windows OS"
            );
        }
    }

    #[test]
    fn test_convert_bgra_to_rgba() {
        let width = 2;
        let height = 2;
        let row_pitch = width * 4;
        let src: Vec<u8> = vec![
            10, 20, 30, 40, // px 0: B=10, G=20, R=30, A=40 -> RGBA: 30, 20, 10, 255
            50, 60, 70, 80, // px 1: B=50, G=60, R=70, A=80 -> RGBA: 70, 60, 50, 255
            90, 100, 110, 120, // px 2: B=90, G=100, R=110, A=120 -> RGBA: 110, 100, 90, 255
            130, 140, 150,
            160, // px 3: B=130, G=140, R=150, A=160 -> RGBA: 150, 140, 130, 255
        ];
        let mut dst = vec![0u8; width * height * 4];
        convert_bgra_to_rgba(src.as_ptr(), width, height, row_pitch, &mut dst);
        assert_eq!(&dst[0..4], &[30, 20, 10, 255]);
        assert_eq!(&dst[4..8], &[70, 60, 50, 255]);
        assert_eq!(&dst[8..12], &[110, 100, 90, 255]);
        assert_eq!(&dst[12..16], &[150, 140, 130, 255]);
    }

    #[test]
    fn test_convert_bgra_to_rgba_strided() {
        let width = 2;
        let height = 2;
        let row_pitch = width * 4 + 8; // padded row pitch
        let mut src = vec![0u8; height * row_pitch];
        // row 0
        src[0..4].copy_from_slice(&[1, 2, 3, 4]);
        src[4..8].copy_from_slice(&[5, 6, 7, 8]);
        // row 1
        src[row_pitch..row_pitch + 4].copy_from_slice(&[9, 10, 11, 12]);
        src[row_pitch + 4..row_pitch + 8].copy_from_slice(&[13, 14, 15, 16]);

        let mut dst = vec![0u8; width * height * 4];
        convert_bgra_to_rgba(src.as_ptr(), width, height, row_pitch, &mut dst);
        assert_eq!(&dst[0..4], &[3, 2, 1, 255]);
        assert_eq!(&dst[4..8], &[7, 6, 5, 255]);
        assert_eq!(&dst[8..12], &[11, 10, 9, 255]);
        assert_eq!(&dst[12..16], &[15, 14, 13, 255]);
    }
}
