use super::{DesktopCapture, ScreenRect};
use std::sync::Mutex;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW,
    HBITMAP, HDC, HGDIOBJ, MONITORINFO, MonitorFromPoint, ReleaseDC, SRCCOPY, SelectObject,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};
use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};

pub struct WindowsDesktop {
    dxgi: Mutex<Option<super::dxgi::DxgiDesktop>>,
}

impl Default for WindowsDesktop {
    fn default() -> Self {
        Self {
            dxgi: Mutex::new(super::dxgi::DxgiDesktop::new().ok()),
        }
    }
}

/// Every GDI resource is released on all allocation/copy/error paths.
struct CaptureDc {
    screen: HDC,
    memory: HDC,
    bitmap: HBITMAP,
    previous: HGDIOBJ,
}
impl Drop for CaptureDc {
    fn drop(&mut self) {
        unsafe {
            if !self.previous.is_invalid() {
                SelectObject(self.memory, self.previous);
            }
            if !self.bitmap.is_invalid() {
                let _ = DeleteObject(self.bitmap);
            }
            if !self.memory.is_invalid() {
                let _ = DeleteDC(self.memory);
            }
            if !self.screen.is_invalid() {
                ReleaseDC(None, self.screen);
            }
        }
    }
}

impl WindowsDesktop {
    fn active_monitor_bounds() -> Result<ScreenRect, String> {
        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let monitor = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
            if !monitor.is_invalid() {
                let mut info = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if GetMonitorInfoW(monitor, &mut info).as_bool() {
                    let width = (info.rcMonitor.right - info.rcMonitor.left).max(0) as u32;
                    let height = (info.rcMonitor.bottom - info.rcMonitor.top).max(0) as u32;
                    let rect = ScreenRect {
                        x: info.rcMonitor.left,
                        y: info.rcMonitor.top,
                        width,
                        height,
                    };
                    if rect.validate().is_ok() {
                        return Ok(rect);
                    }
                }
            }
            Self::virtual_bounds()
        }
    }

    fn virtual_bounds() -> Result<ScreenRect, String> {
        let bounds = unsafe {
            ScreenRect {
                x: GetSystemMetrics(SM_XVIRTUALSCREEN),
                y: GetSystemMetrics(SM_YVIRTUALSCREEN),
                width: GetSystemMetrics(SM_CXVIRTUALSCREEN).max(0) as u32,
                height: GetSystemMetrics(SM_CYVIRTUALSCREEN).max(0) as u32,
            }
        };
        bounds.validate()?;
        Ok(bounds)
    }

    fn capture_gdi(region: ScreenRect) -> Result<image::RgbaImage, String> {
        unsafe {
            let mut dc = CaptureDc {
                screen: GetDC(None),
                memory: HDC::default(),
                bitmap: HBITMAP::default(),
                previous: HGDIOBJ::default(),
            };
            if dc.screen.is_invalid() {
                return Err("无法访问当前桌面。".into());
            }
            dc.memory = CreateCompatibleDC(dc.screen);
            dc.bitmap =
                CreateCompatibleBitmap(dc.screen, region.width as i32, region.height as i32);
            if dc.memory.is_invalid() || dc.bitmap.is_invalid() {
                return Err("无法创建屏幕截图缓冲区。".into());
            }
            dc.previous = SelectObject(dc.memory, dc.bitmap);
            BitBlt(
                dc.memory,
                0,
                0,
                region.width as i32,
                region.height as i32,
                dc.screen,
                region.x,
                region.y,
                SRCCOPY | CAPTUREBLT,
            )
            .map_err(|error| format!("屏幕截图失败：{error}"))?;
            SelectObject(dc.memory, dc.previous);
            dc.previous = HGDIOBJ::default();
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: region.width as i32,
                    biHeight: -(region.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bytes = vec![0u8; region.width as usize * region.height as usize * 4];
            let rows = GetDIBits(
                dc.memory,
                dc.bitmap,
                0,
                region.height,
                Some(bytes.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            );
            if rows != region.height as i32 {
                return Err("屏幕截图像素不完整。".into());
            }
            for pixel in bytes.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
                pixel[3] = 255;
            }
            image::RgbaImage::from_raw(region.width, region.height, bytes)
                .ok_or_else(|| "截图尺寸无效。".into())
        }
    }
}

impl DesktopCapture for WindowsDesktop {
    fn bounds(&self) -> Result<ScreenRect, String> {
        Self::active_monitor_bounds()
    }

    fn is_exclusive_fullscreen(&self) -> bool {
        unsafe {
            SHQueryUserNotificationState() == Ok(QUNS_RUNNING_D3D_FULL_SCREEN)
        }
    }

    fn capture(&self, region: ScreenRect) -> Result<image::RgbaImage, String> {
        region.validate()?;

        // Fast path: DXGI Desktop Duplication (GPU physical scanout, handles DirectX/Vulkan/MPO without bleed-through)
        let dxgi_capture = {
            let mut guard = self.dxgi.lock().unwrap();
            if guard.is_none() {
                *guard = super::dxgi::DxgiDesktop::new().ok();
            }
            guard.as_ref().map(|d| d.capture_output(region))
        };

        if let Some(Ok(image)) = dxgi_capture {
            return Ok(image);
        }

        // Resilient fallback: GDI BitBlt
        Self::capture_gdi(region)
    }

    fn exclude(&self, window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
        let handle = window.hwnd().map_err(|error| error.to_string())?;
        unsafe {
            SetWindowDisplayAffinity(
                HWND(handle.0),
                if enabled {
                    WDA_EXCLUDEFROMCAPTURE
                } else {
                    WDA_NONE
                },
            )
        }
        .map_err(|error| {
            format!("无法从屏幕捕获中排除译文窗口（需要 Windows 10 2004 或更新版本）：{error}")
        })
    }
}
