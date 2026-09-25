use crate::platform::desktop::{DesktopCapture, ScreenRect};
use crate::platform::imaging::is_all_black;
use std::sync::Mutex;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW,
    HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

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

    pub(crate) fn capture_gdi(region: ScreenRect) -> Result<image::RgbaImage, String> {
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
            let blt_res = BitBlt(
                dc.memory,
                0,
                0,
                region.width as i32,
                region.height as i32,
                dc.screen,
                region.x,
                region.y,
                SRCCOPY | CAPTUREBLT,
            );
            if blt_res.is_err() {
                BitBlt(
                    dc.memory,
                    0,
                    0,
                    region.width as i32,
                    region.height as i32,
                    dc.screen,
                    region.x,
                    region.y,
                    SRCCOPY,
                )
                .map_err(|error| format!("屏幕截图失败：{error}"))?;
            }
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
                return Err("读取屏幕位图数据失败。".into());
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

    fn capture(&self, region: ScreenRect) -> Result<image::RgbaImage, String> {
        region.validate()?;

        // Primary path: GDI BitBlt captures desktop composition and borderless fullscreen games reliably
        let gdi_result = Self::capture_gdi(region);
        if let Ok(ref image) = gdi_result
            && !is_all_black(image)
        {
            return gdi_result;
        }

        let gdi_was_black = match &gdi_result {
            Ok(img) => is_all_black(img),
            Err(_) => false,
        };

        // Fallback to DXGI only if GDI failed or returned an all-black frame
        let dxgi_capture = {
            let mut guard = self.dxgi.lock().unwrap();
            if guard.is_none() {
                *guard = super::dxgi::DxgiDesktop::new().ok();
            }
            guard.as_ref().map(|d| d.capture_output(region))
        };

        let dxgi_was_black = match &dxgi_capture {
            Some(Ok(img)) => is_all_black(img),
            _ => false,
        };

        if let Some(Ok(image)) = dxgi_capture
            && !is_all_black(&image)
        {
            return Ok(image);
        }

        if gdi_was_black || dxgi_was_black {
            return Err("未截取到有效画面（画面为纯黑）。通常是因为游戏处于「独占全屏」模式，请在游戏设置中将显示模式更改为「无边框窗口化 (Borderless)」即可正常取词。".into());
        }

        gdi_result
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
