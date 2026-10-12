use crate::platform::desktop::{
    CaptureDiagnostics, DesktopCapture, ScreenRect, TierLog, WindowProbeInfo,
    format_diagnostics_markdown, record_last_diagnostics,
};
use crate::platform::imaging::is_all_black;
use std::sync::Mutex;
use windows::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap,
    CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW,
    HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, ReleaseDC,
    SRCCOPY, SelectObject,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
};

pub struct WindowsDesktop {
    wgc: Mutex<Option<super::wgc::WgcDesktop>>,
    dxgi: Mutex<Option<super::dxgi::DxgiDesktop>>,
}

impl Default for WindowsDesktop {
    fn default() -> Self {
        Self {
            wgc: Mutex::new(match super::wgc::WgcDesktop::new() {
                Ok(wgc) => Some(wgc),
                Err(err) => {
                    eprintln!("[desktop] WGC initialization failed: {err}");
                    None
                }
            }),
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
                let _ = DeleteObject(self.bitmap.into());
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

    fn get_current_timestamp() -> String {
        unsafe {
            let st = GetLocalTime();
            format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
            )
        }
    }

    fn save_debug_frame(image: &image::RgbaImage) -> Option<String> {
        let mut temp = std::env::temp_dir();
        temp.push("transportal_last_capture.png");
        let path_str = temp.to_string_lossy().into_owned();
        let img_clone = image.clone();
        std::thread::spawn(move || {
            let _ = img_clone.save(&temp);
        });
        Some(path_str)
    }

    fn probe_foreground_window(target_region: &ScreenRect) -> Option<(HWND, WindowProbeInfo)> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() || !IsWindow(Some(hwnd)).as_bool() {
                return None;
            }
            let is_visible = IsWindowVisible(hwnd).as_bool();
            let is_iconic = IsIconic(hwnd).as_bool();

            let mut pid = 0;
            let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == GetCurrentProcessId() {
                return None;
            }

            let mut title_buf = [0u16; 512];
            let title_len = GetWindowTextW(hwnd, &mut title_buf);
            let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);

            let mut class_buf = [0u16; 256];
            let class_len = GetClassNameW(hwnd, &mut class_buf);
            let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);

            let mut process_name = String::new();
            if let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut exe_buf = [0u16; 1024];
                let mut size = exe_buf.len() as u32;
                if QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_FORMAT(0),
                    windows::core::PWSTR(exe_buf.as_mut_ptr()),
                    &mut size,
                )
                .is_ok()
                {
                    let full_path = String::from_utf16_lossy(&exe_buf[..size as usize]);
                    process_name = std::path::Path::new(&full_path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&full_path)
                        .to_string();
                }
                let _ = CloseHandle(handle);
            }

            let mut rect = RECT::default();
            let hr = DwmGetWindowAttribute(
                hwnd,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut rect as *mut _ as *mut _,
                std::mem::size_of::<RECT>() as u32,
            );
            if hr.is_err() && GetWindowRect(hwnd, &mut rect).is_err() {
                return None;
            }
            let width = (rect.right - rect.left).max(0) as u32;
            let height = (rect.bottom - rect.top).max(0) as u32;
            let screen_rect = ScreenRect {
                x: rect.left,
                y: rect.top,
                width,
                height,
            };

            let contains_target = screen_rect.contains(target_region);

            let info = WindowProbeInfo {
                hwnd: hwnd.0 as usize,
                title,
                class_name,
                process_id: pid,
                process_name,
                rect: screen_rect,
                is_visible,
                is_iconic,
                contains_target,
            };

            Some((hwnd, info))
        }
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
            dc.memory = CreateCompatibleDC(Some(dc.screen));
            dc.bitmap =
                CreateCompatibleBitmap(dc.screen, region.width as i32, region.height as i32);
            if dc.memory.is_invalid() || dc.bitmap.is_invalid() {
                return Err("无法创建屏幕截图缓冲区。".into());
            }
            dc.previous = SelectObject(dc.memory, dc.bitmap.into());
            let _ = windows::Win32::Graphics::Dwm::DwmFlush();
            let blt_res = BitBlt(
                dc.memory,
                0,
                0,
                region.width as i32,
                region.height as i32,
                Some(dc.screen),
                region.x,
                region.y,
                SRCCOPY,
            );
            if blt_res.is_err() {
                BitBlt(
                    dc.memory,
                    0,
                    0,
                    region.width as i32,
                    region.height as i32,
                    Some(dc.screen),
                    region.x,
                    region.y,
                    SRCCOPY | CAPTUREBLT,
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
        let capture_start = std::time::Instant::now();
        region.validate()?;

        let (hwnd_opt, fg_info) = match Self::probe_foreground_window(&region) {
            Some((hwnd, info)) => (Some(hwnd), Some(info)),
            None => (None, None),
        };

        let mut tiers: Vec<TierLog> = Vec::new();
        let mut final_backend = String::from("FAILED");
        let mut captured_image: Option<image::RgbaImage> = None;

        let pt = POINT {
            x: region.x,
            y: region.y,
        };
        let monitor = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
        let mut mon_info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let has_mon_info = unsafe { GetMonitorInfoW(monitor, &mut mon_info) }.as_bool();
        let monitor_rect = if has_mon_info {
            Some(ScreenRect {
                x: mon_info.rcMonitor.left,
                y: mon_info.rcMonitor.top,
                width: (mon_info.rcMonitor.right - mon_info.rcMonitor.left).max(0) as u32,
                height: (mon_info.rcMonitor.bottom - mon_info.rcMonitor.top).max(0) as u32,
            })
        } else {
            None
        };

        // --- Tier 1: Windows.Graphics.Capture ---
        {
            let mut guard = self.wgc.lock().unwrap();
            if guard.is_none() {
                *guard = super::wgc::WgcDesktop::new().ok();
            }
            if let Some(wgc) = guard.as_ref() {
                // Tier 1A: Window capture
                let mut attempted_win = false;
                if let (Some(hwnd), Some(info)) = (hwnd_opt, &fg_info)
                    && info.is_visible
                    && !info.is_iconic
                    && info.contains_target
                {
                    attempted_win = true;
                    let t_start = std::time::Instant::now();
                    match wgc.capture_window(hwnd, info.rect, region) {
                        Ok((img, metrics)) => {
                            let all_black = is_all_black(&img);
                            tiers.push(TierLog {
                                tier: "WGC_WINDOW".into(),
                                attempted: true,
                                success: !all_black,
                                frame0_latency_ms: Some(metrics.frame0_latency_ms),
                                frame1_latency_ms: metrics.frame1_latency_ms,
                                frames_drained: metrics.frames_drained,
                                staging_copy_ms: Some(metrics.staging_copy_ms),
                                is_all_black: all_black,
                                error: if all_black {
                                    Some("画面纯黑".into())
                                } else {
                                    None
                                },
                                duration_ms: metrics.total_ms,
                            });
                            if !all_black {
                                captured_image = Some(img);
                                final_backend = "WGC_WINDOW".into();
                            }
                        }
                        Err(err) => {
                            tiers.push(TierLog {
                                tier: "WGC_WINDOW".into(),
                                attempted: true,
                                success: false,
                                frame0_latency_ms: None,
                                frame1_latency_ms: None,
                                frames_drained: 0,
                                staging_copy_ms: None,
                                is_all_black: false,
                                error: Some(err),
                                duration_ms: t_start.elapsed().as_millis() as u64,
                            });
                        }
                    }
                }

                if !attempted_win {
                    tiers.push(TierLog {
                        tier: "WGC_WINDOW".into(),
                        attempted: false,
                        success: false,
                        frame0_latency_ms: None,
                        frame1_latency_ms: None,
                        frames_drained: 0,
                        staging_copy_ms: None,
                        is_all_black: false,
                        error: Some("前台窗口未包含目标选区或未就绪".into()),
                        duration_ms: 0,
                    });
                }

                // Tier 1B: Monitor capture (if window capture skipped or failed)
                if captured_image.is_none()
                    && let Some(m_rect) = monitor_rect
                {
                    let t_start = std::time::Instant::now();
                    match wgc.capture_monitor(monitor, m_rect, region) {
                        Ok((img, metrics)) => {
                            let all_black = is_all_black(&img);
                            tiers.push(TierLog {
                                tier: "WGC_MONITOR".into(),
                                attempted: true,
                                success: !all_black,
                                frame0_latency_ms: Some(metrics.frame0_latency_ms),
                                frame1_latency_ms: metrics.frame1_latency_ms,
                                frames_drained: metrics.frames_drained,
                                staging_copy_ms: Some(metrics.staging_copy_ms),
                                is_all_black: all_black,
                                error: if all_black {
                                    Some("画面纯黑".into())
                                } else {
                                    None
                                },
                                duration_ms: metrics.total_ms,
                            });
                            if !all_black {
                                captured_image = Some(img);
                                final_backend = "WGC_MONITOR".into();
                            }
                        }
                        Err(err) => {
                            tiers.push(TierLog {
                                tier: "WGC_MONITOR".into(),
                                attempted: true,
                                success: false,
                                frame0_latency_ms: None,
                                frame1_latency_ms: None,
                                frames_drained: 0,
                                staging_copy_ms: None,
                                is_all_black: false,
                                error: Some(err),
                                duration_ms: t_start.elapsed().as_millis() as u64,
                            });
                            *guard = None;
                        }
                    }
                }
            } else {
                tiers.push(TierLog {
                    tier: "WGC".into(),
                    attempted: false,
                    success: false,
                    frame0_latency_ms: None,
                    frame1_latency_ms: None,
                    frames_drained: 0,
                    staging_copy_ms: None,
                    is_all_black: false,
                    error: Some("WGC 初始化失败或不受支持".into()),
                    duration_ms: 0,
                });
            }
        }

        // --- Tier 2: DXGI Desktop Duplication ---
        if captured_image.is_none() {
            let t_start = std::time::Instant::now();
            let mut guard = self.dxgi.lock().unwrap();
            if guard.is_none() {
                *guard = super::dxgi::DxgiDesktop::new().ok();
            }
            if let Some(dxgi) = guard.as_ref() {
                match dxgi.capture_output(region) {
                    Ok(img) => {
                        let all_black = is_all_black(&img);
                        tiers.push(TierLog {
                            tier: "DXGI".into(),
                            attempted: true,
                            success: !all_black,
                            frame0_latency_ms: None,
                            frame1_latency_ms: None,
                            frames_drained: 0,
                            staging_copy_ms: None,
                            is_all_black: all_black,
                            error: if all_black {
                                Some("画面纯黑".into())
                            } else {
                                None
                            },
                            duration_ms: t_start.elapsed().as_millis() as u64,
                        });
                        if !all_black {
                            captured_image = Some(img);
                            final_backend = "DXGI".into();
                        }
                    }
                    Err(err) => {
                        tiers.push(TierLog {
                            tier: "DXGI".into(),
                            attempted: true,
                            success: false,
                            frame0_latency_ms: None,
                            frame1_latency_ms: None,
                            frames_drained: 0,
                            staging_copy_ms: None,
                            is_all_black: false,
                            error: Some(err),
                            duration_ms: t_start.elapsed().as_millis() as u64,
                        });
                        *guard = None;
                    }
                }
            } else {
                tiers.push(TierLog {
                    tier: "DXGI".into(),
                    attempted: false,
                    success: false,
                    frame0_latency_ms: None,
                    frame1_latency_ms: None,
                    frames_drained: 0,
                    staging_copy_ms: None,
                    is_all_black: false,
                    error: Some("DXGI 初始化失败或不支持".into()),
                    duration_ms: 0,
                });
            }
        }

        // --- Tier 3: GDI BitBlt Fallback ---
        if captured_image.is_none() {
            let t_start = std::time::Instant::now();
            match Self::capture_gdi(region) {
                Ok(img) => {
                    let all_black = is_all_black(&img);
                    tiers.push(TierLog {
                        tier: "GDI".into(),
                        attempted: true,
                        success: !all_black,
                        frame0_latency_ms: None,
                        frame1_latency_ms: None,
                        frames_drained: 0,
                        staging_copy_ms: None,
                        is_all_black: all_black,
                        error: if all_black {
                            Some("画面纯黑".into())
                        } else {
                            None
                        },
                        duration_ms: t_start.elapsed().as_millis() as u64,
                    });
                    if !all_black {
                        captured_image = Some(img);
                        final_backend = "GDI".into();
                    }
                }
                Err(err) => {
                    tiers.push(TierLog {
                        tier: "GDI".into(),
                        attempted: true,
                        success: false,
                        frame0_latency_ms: None,
                        frame1_latency_ms: None,
                        frames_drained: 0,
                        staging_copy_ms: None,
                        is_all_black: false,
                        error: Some(err),
                        duration_ms: t_start.elapsed().as_millis() as u64,
                    });
                }
            }
        }

        let saved_image_path = captured_image.as_ref().and_then(Self::save_debug_frame);

        let diag = CaptureDiagnostics {
            timestamp: Self::get_current_timestamp(),
            target_region: region,
            foreground_window: fg_info,
            monitor_rect,
            tiers,
            final_backend,
            total_duration_ms: capture_start.elapsed().as_millis() as u64,
            saved_image_path,
        };

        eprintln!("\n{}", format_diagnostics_markdown(&diag));
        record_last_diagnostics(diag);

        match captured_image {
            Some(img) => Ok(img),
            None => Err("所有截图管道均未截取到有效画面（或画面全黑）。".into()),
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
