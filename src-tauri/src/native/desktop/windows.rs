use super::{DesktopCapture, ScreenRect};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HBITMAP, HDC,
    HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

pub struct WindowsDesktop;

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

impl DesktopCapture for WindowsDesktop {
    fn bounds(&self) -> Result<ScreenRect, String> {
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

    fn capture(&self, region: ScreenRect) -> Result<image::RgbaImage, String> {
        region.validate()?;
        if !self.bounds()?.contains(&region) {
            return Err("屏幕布局已变化，请重新选择区域。".into());
        }
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
            // GetDIBits requires the bitmap to be deselected first.
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
