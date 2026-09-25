//! Materialize clipboard data before copying/pasting. An IDataObject obtained
//! from OleGetClipboard can be lazy and no longer usable after ownership changes.

use std::time::Duration;
use windows::Win32::Foundation::{
    ERROR_SUCCESS, GetLastError, HANDLE, HGLOBAL, HWND, SetLastError,
};
use windows::Win32::System::Com::{
    STGMEDIUM, STGMEDIUM_0, TYMED_ENHMF, TYMED_GDI, TYMED_HGLOBAL, TYMED_MFPICT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::GMEM_MOVEABLE;
use windows::Win32::System::Ole::{CLIPBOARD_FORMAT, OleDuplicateData, ReleaseStgMedium};

struct ClipboardLock;
impl ClipboardLock {
    fn open(owner: HWND) -> Result<Self, String> {
        for _ in 0..20 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        Err("剪贴板正被其他程序占用，请稍后重试。".into())
    }
}
impl Drop for ClipboardLock {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

struct FormatCopy {
    format: u32,
    medium: STGMEDIUM,
    transferred: bool,
}
impl Drop for FormatCopy {
    fn drop(&mut self) {
        if !self.transferred {
            unsafe {
                ReleaseStgMedium(&mut self.medium);
            }
        }
    }
}

pub(super) struct ClipboardRestore {
    owner: HWND,
    formats: Vec<FormatCopy>,
    pub owned_revision: Option<u32>,
}

impl ClipboardRestore {
    pub fn snapshot(owner: HWND) -> Result<Self, String> {
        let _lock = ClipboardLock::open(owner)?;
        let mut snapshot = Self {
            owner,
            formats: Vec::new(),
            owned_revision: None,
        };
        let mut format = 0;
        loop {
            unsafe {
                SetLastError(ERROR_SUCCESS);
            }
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                if unsafe { GetLastError() } != ERROR_SUCCESS {
                    return Err("无法枚举原剪贴板内容。".into());
                }
                break;
            }
            // OLE ownership metadata contains pointers to the previous owner;
            // copying those bytes would restore stale proxies, not the data.
            let mut name = [0u16; 128];
            let length = unsafe { GetClipboardFormatNameW(format, &mut name) }.max(0) as usize;
            let name = String::from_utf16_lossy(&name[..length]);
            if format == 128
                || matches!(
                    name.as_str(),
                    "Ole Private Data"
                        | "DataObject"
                        | "DataObjectAttributes"
                        | "DataObjectAttributesRequiringElevation"
                )
            {
                continue;
            }
            if snapshot.formats.len() >= 512 {
                return Err("剪贴板格式过多，无法安全保留，请手动复制译文。".into());
            }
            let source = unsafe { GetClipboardData(format) }
                .map_err(|_| "无法读取完整的原剪贴板，已取消替换。")?;
            let storage_format = match format {
                130 => 2,
                131 => 3,
                142 => 14,
                _ => format,
            };
            let copied = unsafe {
                OleDuplicateData(
                    source,
                    CLIPBOARD_FORMAT(storage_format as u16),
                    GMEM_MOVEABLE,
                )
            };
            if copied.is_invalid() {
                return Err(format!("无法保留剪贴板格式 {format}，已取消替换。"));
            }
            let kind = match storage_format {
                2 | 9 => TYMED_GDI,
                3 => TYMED_MFPICT,
                14 => TYMED_ENHMF,
                _ => TYMED_HGLOBAL,
            };
            snapshot.formats.push(FormatCopy {
                format,
                transferred: false,
                medium: STGMEDIUM {
                    tymed: kind.0 as u32,
                    u: STGMEDIUM_0 {
                        hGlobal: HGLOBAL(copied.0),
                    },
                    ..Default::default()
                },
            });
        }
        Ok(snapshot)
    }

    pub fn restore(&mut self) -> Result<(), String> {
        let Some(revision) = self.owned_revision else {
            return Ok(());
        };
        let _lock = ClipboardLock::open(self.owner)?;
        if unsafe { GetClipboardSequenceNumber() } != revision {
            self.owned_revision = None;
            return Ok(()); // Preserve a later user copy.
        }
        unsafe { EmptyClipboard() }.map_err(|error| format!("无法恢复原剪贴板：{error}"))?;
        self.owned_revision = None;
        for format in &mut self.formats {
            // All STGMEDIUM handle union members share the same handle bits.
            let handle = unsafe { HANDLE(format.medium.u.hGlobal.0) };
            unsafe { SetClipboardData(format.format, handle) }
                .map_err(|error| format!("无法恢复剪贴板格式 {}：{error}", format.format))?;
            format.transferred = true;
        }
        Ok(())
    }
}

impl Drop for ClipboardRestore {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("[selection] {error}");
        }
    }
}
