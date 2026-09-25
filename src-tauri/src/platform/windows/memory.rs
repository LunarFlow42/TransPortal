//! Process tree memory introspection and working set trimming.
//!
//! Provides deep physical memory compression by flushing cold pages
//! from both the main `transportal.exe` process and all child processes
//! (WebView2 renderer, GPU, and utility processes).

use crate::platform::system::{MemoryReport, TrimResult};
use std::collections::HashSet;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE};
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcess, PROCESS_QUERY_INFORMATION,
    PROCESS_SET_QUOTA, SetProcessWorkingSetSize,
};
use windows::core::s;

#[repr(C)]
struct PROCESSENTRY32W {
    dw_size: u32,
    cnt_usage: u32,
    th32_process_id: u32,
    th32_default_heap_id: usize,
    th32_module_id: u32,
    cnt_threads: u32,
    th32_parent_process_id: u32,
    pc_pri_class_base: i32,
    dw_flags: u32,
    sz_exe_file: [u16; 260],
}

#[repr(C)]
#[derive(Default, Debug)]
struct PROCESS_MEMORY_COUNTERS {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

type CreateToolhelp32SnapshotFn = unsafe extern "system" fn(u32, u32) -> HANDLE;
type Process32FirstWFn = unsafe extern "system" fn(HANDLE, *mut PROCESSENTRY32W) -> i32;
type Process32NextWFn = unsafe extern "system" fn(HANDLE, *mut PROCESSENTRY32W) -> i32;
type K32GetProcessMemoryInfoFn =
    unsafe extern "system" fn(HANDLE, *mut PROCESS_MEMORY_COUNTERS, u32) -> i32;
type K32EmptyWorkingSetFn = unsafe extern "system" fn(HANDLE) -> i32;

struct Kernel32MemoryApi {
    create_toolhelp32_snapshot: CreateToolhelp32SnapshotFn,
    process32_first_w: Process32FirstWFn,
    process32_next_w: Process32NextWFn,
    k32_get_process_memory_info: K32GetProcessMemoryInfoFn,
    k32_empty_working_set: Option<K32EmptyWorkingSetFn>,
}

impl Kernel32MemoryApi {
    #[allow(clippy::missing_transmute_annotations)]
    fn load() -> Option<Self> {
        unsafe {
            let h_kernel32: HMODULE = GetModuleHandleA(s!("kernel32.dll")).ok()?;
            let p_snapshot = GetProcAddress(h_kernel32, s!("CreateToolhelp32Snapshot"))?;
            let p_first = GetProcAddress(h_kernel32, s!("Process32FirstW"))?;
            let p_next = GetProcAddress(h_kernel32, s!("Process32NextW"))?;
            let p_mem_info = GetProcAddress(h_kernel32, s!("K32GetProcessMemoryInfo"))?;
            let p_empty_ws = GetProcAddress(h_kernel32, s!("K32EmptyWorkingSet"));

            Some(Self {
                create_toolhelp32_snapshot: std::mem::transmute(p_snapshot),
                process32_first_w: std::mem::transmute(p_first),
                process32_next_w: std::mem::transmute(p_next),
                k32_get_process_memory_info: std::mem::transmute(p_mem_info),
                k32_empty_working_set: p_empty_ws.map(|p| std::mem::transmute(p)),
            })
        }
    }
}

/// Enumerate current process ID and all its child/grandchild process IDs (e.g. WebView2).
fn collect_process_tree_ids(api: &Kernel32MemoryApi) -> Vec<u32> {
    let current_pid = unsafe { GetCurrentProcessId() };
    let mut result = vec![current_pid];

    // TH32CS_SNAPPROCESS = 0x00000002
    let snapshot = unsafe { (api.create_toolhelp32_snapshot)(0x00000002, 0) };
    if snapshot.is_invalid() {
        return result;
    }

    let mut entry = PROCESSENTRY32W {
        dw_size: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        cnt_usage: 0,
        th32_process_id: 0,
        th32_default_heap_id: 0,
        th32_module_id: 0,
        cnt_threads: 0,
        th32_parent_process_id: 0,
        pc_pri_class_base: 0,
        dw_flags: 0,
        sz_exe_file: [0; 260],
    };

    let mut parent_to_children: Vec<(u32, u32)> = Vec::new();
    unsafe {
        if (api.process32_first_w)(snapshot, &mut entry) != 0 {
            loop {
                parent_to_children.push((entry.th32_parent_process_id, entry.th32_process_id));
                if (api.process32_next_w)(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }

    // Recursively find all descendants of current_pid
    let mut known_pids = HashSet::new();
    known_pids.insert(current_pid);

    let mut added = true;
    while added {
        added = false;
        for &(parent, child) in &parent_to_children {
            if known_pids.contains(&parent) && !known_pids.contains(&child) {
                known_pids.insert(child);
                result.push(child);
                added = true;
            }
        }
    }

    result
}

fn read_process_memory(api: &Kernel32MemoryApi, handle: HANDLE) -> Option<(usize, usize)> {
    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    unsafe {
        let ok = (api.k32_get_process_memory_info)(
            handle,
            &mut counters,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        );
        if ok != 0 {
            Some((counters.working_set_size, counters.pagefile_usage))
        } else {
            None
        }
    }
}

pub fn get_memory_usage() -> Result<MemoryReport, String> {
    let api = Kernel32MemoryApi::load().ok_or("Failed to load kernel32 memory APIs")?;
    let pids = collect_process_tree_ids(&api);
    let current_pid = unsafe { GetCurrentProcessId() };

    let mut total_working_set: u64 = 0;
    let mut total_private: u64 = 0;
    let mut main_working_set: u64 = 0;

    for &pid in &pids {
        if pid == current_pid {
            let handle = unsafe { GetCurrentProcess() };
            if let Some((ws, pf)) = read_process_memory(&api, handle) {
                total_working_set += ws as u64;
                total_private += pf as u64;
                main_working_set = ws as u64;
            }
        } else {
            let handle = unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) };
            if let Ok(h) = handle
                && !h.is_invalid()
            {
                if let Some((ws, pf)) = read_process_memory(&api, h) {
                    total_working_set += ws as u64;
                    total_private += pf as u64;
                }
                let _ = unsafe { CloseHandle(h) };
            }
        }
    }

    Ok(MemoryReport {
        total_working_set_bytes: total_working_set,
        total_private_bytes: total_private,
        main_working_set_bytes: main_working_set,
        process_count: pids.len(),
    })
}

pub fn trim_memory() -> Result<TrimResult, String> {
    let before = get_memory_usage()?;
    let api = Kernel32MemoryApi::load().ok_or("Failed to load kernel32 memory APIs")?;
    let pids = collect_process_tree_ids(&api);
    let current_pid = unsafe { GetCurrentProcessId() };

    for &pid in &pids {
        if pid == current_pid {
            let handle = unsafe { GetCurrentProcess() };
            unsafe {
                let _ = SetProcessWorkingSetSize(handle, usize::MAX, usize::MAX);
                if let Some(empty_fn) = api.k32_empty_working_set {
                    let _ = (empty_fn)(handle);
                }
            }
        } else {
            // Open child process with PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION
            let handle =
                unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_QUERY_INFORMATION, false, pid) };
            if let Ok(h) = handle
                && !h.is_invalid()
            {
                unsafe {
                    let _ = SetProcessWorkingSetSize(h, usize::MAX, usize::MAX);
                    if let Some(empty_fn) = api.k32_empty_working_set {
                        let _ = (empty_fn)(h);
                    }
                    let _ = CloseHandle(h);
                }
            }
        }
    }

    // Give the OS paging manager a brief moment (30ms) to unmap cold pages
    std::thread::sleep(std::time::Duration::from_millis(30));

    let after = get_memory_usage().unwrap_or_else(|_| before.clone());
    let trimmed = before
        .total_working_set_bytes
        .saturating_sub(after.total_working_set_bytes);

    Ok(TrimResult {
        before_bytes: before.total_working_set_bytes,
        after_bytes: after.total_working_set_bytes,
        trimmed_bytes: trimmed,
        process_count: pids.len(),
    })
}
