//! Physical screen coordinates and capture; no platform APIs escape this trait.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl ScreenRect {
    pub fn validate(&self) -> Result<(), String> {
        if self.width < 2
            || self.height < 2
            || self.width > 16000
            || self.height > 16000
            || u64::from(self.width) * u64::from(self.height) > 32_000_000
        {
            return Err("选区无效或过大，请重新选择较小的区域。".into());
        }
        Ok(())
    }
    pub fn contains(&self, other: &Self) -> bool {
        i64::from(other.x) >= i64::from(self.x)
            && i64::from(other.y) >= i64::from(self.y)
            && i64::from(other.x) + i64::from(other.width)
                <= i64::from(self.x) + i64::from(self.width)
            && i64::from(other.y) + i64::from(other.height)
                <= i64::from(self.y) + i64::from(self.height)
    }
    pub fn intersects(&self, other: &Self) -> bool {
        let left = self.x.max(other.x);
        let top = self.y.max(other.y);
        let right = (self.x + self.width as i32).min(other.x + other.width as i32);
        let bottom = (self.y + self.height as i32).min(other.y + other.height as i32);
        right > left && bottom > top
    }
}

pub trait DesktopCapture: Send + Sync {
    fn bounds(&self) -> Result<ScreenRect, String>;
    fn capture(&self, region: ScreenRect) -> Result<image::RgbaImage, String>;
    /// Keep translated output out of later screenshots to avoid feedback loops.
    fn exclude(&self, window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowProbeInfo {
    pub hwnd: usize,
    pub title: String,
    pub class_name: String,
    pub process_id: u32,
    pub process_name: String,
    pub rect: ScreenRect,
    pub is_visible: bool,
    pub is_iconic: bool,
    pub contains_target: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TierLog {
    pub tier: String,
    pub attempted: bool,
    pub success: bool,
    pub frame0_latency_ms: Option<u64>,
    pub frame1_latency_ms: Option<u64>,
    pub frames_drained: u32,
    pub staging_copy_ms: Option<u64>,
    pub is_all_black: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureDiagnostics {
    pub timestamp: String,
    pub target_region: ScreenRect,
    pub foreground_window: Option<WindowProbeInfo>,
    pub monitor_rect: Option<ScreenRect>,
    pub tiers: Vec<TierLog>,
    pub final_backend: String,
    pub total_duration_ms: u64,
    pub saved_image_path: Option<String>,
}

static LAST_CAPTURE_DIAGNOSTICS: std::sync::Mutex<Option<CaptureDiagnostics>> =
    std::sync::Mutex::new(None);

pub fn record_last_diagnostics(diag: CaptureDiagnostics) {
    if let Ok(mut lock) = LAST_CAPTURE_DIAGNOSTICS.lock() {
        *lock = Some(diag);
    }
}

pub fn get_last_diagnostics() -> Option<CaptureDiagnostics> {
    LAST_CAPTURE_DIAGNOSTICS
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

pub fn format_diagnostics_markdown(diag: &CaptureDiagnostics) -> String {
    let mut out = String::new();
    out.push_str("### 📸 截图管线诊断报告 (Capture Diagnostics)\n\n");
    out.push_str(&format!("- **时间戳**: `{}`\n", diag.timestamp));
    out.push_str(&format!("- **最终采用后端**: `{}`\n", diag.final_backend));
    out.push_str(&format!(
        "- **捕获总耗时**: `{} ms`\n",
        diag.total_duration_ms
    ));
    out.push_str(&format!(
        "- **请求选区**: `({}, {}) {}×{}`\n",
        diag.target_region.x,
        diag.target_region.y,
        diag.target_region.width,
        diag.target_region.height
    ));
    if let Some(ref mon) = diag.monitor_rect {
        out.push_str(&format!(
            "- **目标显示器**: `({}, {}) {}×{}`\n",
            mon.x, mon.y, mon.width, mon.height
        ));
    }
    if let Some(ref path) = diag.saved_image_path {
        out.push_str(&format!("- **调试图片落盘**: `{}`\n", path));
    }
    out.push_str("\n#### 🎯 前台窗口探针 (Foreground Window)\n");
    if let Some(ref fg) = diag.foreground_window {
        out.push_str(&format!("- **句柄 (HWND)**: `0x{:08X}`\n", fg.hwnd));
        out.push_str(&format!(
            "- **进程名 (EXE)**: `{}` (PID: `{}`)\n",
            fg.process_name, fg.process_id
        ));
        out.push_str(&format!("- **窗口标题**: `{}`\n", fg.title));
        out.push_str(&format!("- **窗口类名**: `{}`\n", fg.class_name));
        out.push_str(&format!(
            "- **窗口边界**: `({}, {}) {}×{}`\n",
            fg.rect.x, fg.rect.y, fg.rect.width, fg.rect.height
        ));
        out.push_str(&format!(
            "- **状态**: Visible: `{}`, Iconic (最小化): `{}`, 覆盖选区: `{}`\n",
            fg.is_visible, fg.is_iconic, fg.contains_target
        ));
    } else {
        out.push_str("- *未探测到有效前台窗口（或属于 TransPortal 自身进程）*\n");
    }
    out.push_str("\n#### ⚙️ 分层管线执行追踪 (Pipeline Tiers)\n");
    for tier in &diag.tiers {
        let status = if tier.success {
            "✅ 成功"
        } else if tier.attempted {
            "❌ 失败/跳过"
        } else {
            "⚪ 未尝试"
        };
        out.push_str(&format!("\n##### `{}` ({})\n", tier.tier, status));
        out.push_str(&format!("- 耗时: `{} ms`\n", tier.duration_ms));
        if let Some(f0) = tier.frame0_latency_ms {
            out.push_str(&format!("- Frame 0 抵达时间: `{} ms`\n", f0));
        }
        if let Some(f1) = tier.frame1_latency_ms {
            out.push_str(&format!("- Frame 1 探测耗时: `{} ms`\n", f1));
        }
        if tier.frames_drained > 0 {
            out.push_str(&format!("- 排空缓冲帧数: `{}` 帧\n", tier.frames_drained));
        }
        if let Some(st) = tier.staging_copy_ms {
            out.push_str(&format!("- Staging 纹理拷贝与映射: `{} ms`\n", st));
        }
        if tier.attempted {
            out.push_str(&format!("- 是否纯黑画面: `{}`\n", tier.is_all_black));
        }
        if let Some(ref err) = tier.error {
            out.push_str(&format!("- 错误信息: `{}`\n", err));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_regions_handle_negative_monitor_origins_without_overflow() {
        let desktop = ScreenRect {
            x: -1920,
            y: -200,
            width: 4480,
            height: 1640,
        };
        assert!(desktop.contains(&ScreenRect {
            x: -1800,
            y: -100,
            width: 600,
            height: 200
        }));
        assert!(!desktop.contains(&ScreenRect {
            x: i32::MAX,
            y: 0,
            width: u32::MAX,
            height: 20
        }));
        assert!(
            ScreenRect {
                x: 0,
                y: 0,
                width: u32::MAX,
                height: u32::MAX
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn test_desktop_capture_pipeline() {
        let p = crate::platform::desktop();
        if let Ok(bounds) = p.bounds() {
            assert!(bounds.width > 0 && bounds.height > 0);
            let test_region = ScreenRect {
                x: bounds.x,
                y: bounds.y,
                width: 50,
                height: 50,
            };
            assert!(test_region.validate().is_ok());
            let result = p.capture(test_region);
            if let Ok(img) = result {
                assert_eq!(img.width(), 50);
                assert_eq!(img.height(), 50);
            }
        }
    }
}
