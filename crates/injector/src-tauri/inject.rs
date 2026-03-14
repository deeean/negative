use std::collections::HashSet;
use std::ffi::CString;
use std::io::{Error, ErrorKind};
use std::process::Command;

use windows::core::PCSTR;
use windows::Win32::Foundation::{CloseHandle, LUID};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueA, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_PRIVILEGES, TOKEN_QUERY,
};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::Win32::System::Memory::{MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx};
#[cfg(target_arch = "x86_64")]
use windows::Win32::System::Threading::PROCESS_QUERY_INFORMATION;
use windows::Win32::System::Threading::{
    CreateRemoteThread, GetCurrentProcess, INFINITE, LPTHREAD_START_ROUTINE, OpenProcess,
    OpenProcessToken, PROCESS_ALL_ACCESS, WaitForSingleObject,
};

#[cfg(target_arch = "x86_64")]
mod ffi {
    extern "system" {
        pub fn IsWow64Process(h_process: isize, wow64_process: *mut i32) -> i32;
    }
}

#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub name: String,
}

fn trim_null_i8(bytes: &[i8]) -> String {
    let as_u8: &[u8] =
        unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const u8, bytes.len()) };
    let end = as_u8.iter().position(|&b| b == 0).unwrap_or(as_u8.len());
    String::from_utf8_lossy(&as_u8[..end]).to_string()
}

pub fn get_processes() -> Result<Vec<Process>, Error> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|e| Error::new(ErrorKind::Other, format!("CreateToolhelp32Snapshot: {e}")))?;

    let mut entry = PROCESSENTRY32 {
        dwSize: std::mem::size_of::<PROCESSENTRY32>() as u32,
        ..Default::default()
    };

    let mut processes = Vec::new();

    if unsafe { Process32First(snapshot, &mut entry) }.is_ok() {
        loop {
            processes.push(Process {
                pid: entry.th32ProcessID,
                name: trim_null_i8(&entry.szExeFile),
            });
            if unsafe { Process32Next(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }

    let _ = unsafe { CloseHandle(snapshot) };
    Ok(processes)
}

fn is_process_32bit(pid: u32) -> bool {
    #[cfg(target_arch = "x86")]
    {
        let _ = pid;
        return true;
    }

    #[cfg(target_arch = "x86_64")]
    {
        let handle = match unsafe { OpenProcess(PROCESS_QUERY_INFORMATION, false, pid) } {
            Ok(h) => h,
            Err(_) => return false,
        };

        let mut is_wow64: i32 = 0;
        let ok =
            unsafe { ffi::IsWow64Process(std::mem::transmute(handle), &mut is_wow64) };
        let _ = unsafe { CloseHandle(handle) };

        ok != 0 && is_wow64 != 0
    }
}

fn inject_dll_direct(pid: u32, dll_path: &str) -> Result<(), Error> {
    let handle = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, pid) }
        .map_err(|_| Error::last_os_error())?;

    let c_path = CString::new(dll_path).map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;
    let size = c_path.as_bytes_with_nul().len();

    let alloc = unsafe {
        VirtualAllocEx(
            handle,
            Some(std::ptr::null()),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };

    if alloc.is_null() {
        let _ = unsafe { CloseHandle(handle) };
        return Err(Error::new(ErrorKind::Other, "VirtualAllocEx failed"));
    }

    let write_result =
        unsafe { WriteProcessMemory(handle, alloc, c_path.as_ptr().cast(), size, None) };

    if write_result.is_err() {
        let _ = unsafe { CloseHandle(handle) };
        return Err(Error::last_os_error());
    }

    let kernel32 = unsafe { GetModuleHandleA(PCSTR(b"kernel32.dll\0".as_ptr())) }
        .map_err(|_| Error::last_os_error())?;

    let load_library = unsafe { GetProcAddress(kernel32, PCSTR(b"LoadLibraryA\0".as_ptr())) }
        .ok_or_else(Error::last_os_error)?;

    let start: LPTHREAD_START_ROUTINE = unsafe { std::mem::transmute(load_library) };

    let thread =
        match unsafe { CreateRemoteThread(handle, None, 0, start, Some(alloc), 0, None) } {
            Ok(t) => t,
            Err(_) => {
                let _ = unsafe { CloseHandle(handle) };
                return Err(Error::last_os_error());
            }
        };

    unsafe { WaitForSingleObject(thread, INFINITE) };

    // Check LoadLibraryA return value (thread exit code = HMODULE)
    let mut exit_code: u32 = 0;
    let _ = unsafe {
        windows::Win32::System::Threading::GetExitCodeThread(thread, &mut exit_code)
    };

    let _ = unsafe { VirtualFreeEx(handle, alloc, 0, MEM_RELEASE) };
    let _ = unsafe { CloseHandle(thread) };
    let _ = unsafe { CloseHandle(handle) };

    if exit_code == 0 {
        return Err(Error::new(ErrorKind::Other, "LoadLibraryA returned NULL (DLL load failed)"));
    }

    Ok(())
}

fn inject_dll_via_helper(pid: u32, dll_path: &str, helper_path: &str) -> Result<(), Error> {
    let output = Command::new(helper_path)
        .args([&pid.to_string(), dll_path])
        .output()
        .map_err(|e| Error::new(ErrorKind::Other, format!("Failed to run helper32: {e}")))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(Error::new(
            ErrorKind::Other,
            format!("helper32: {}", stderr.trim()),
        ))
    }
}

pub fn enable_debug_privilege() -> Result<(), Error> {
    unsafe {
        let mut token = Default::default();
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        )
        .map_err(|_| Error::last_os_error())?;

        let mut luid = LUID::default();
        LookupPrivilegeValueA(PCSTR::null(), PCSTR(b"SeDebugPrivilege\0".as_ptr()), &mut luid)
            .map_err(|_| Error::last_os_error())?;

        let mut tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            ..Default::default()
        };
        tp.Privileges[0].Luid = luid;
        tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;

        AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None)
            .map_err(|_| Error::last_os_error())?;

        let _ = CloseHandle(token);
    }
    Ok(())
}

#[derive(Clone)]
pub struct InjectPaths {
    pub dll64: String,
    pub dll64_exists: bool,
    pub dll32: String,
    pub dll32_exists: bool,
    pub helper32: String,
    pub helper32_exists: bool,
}

pub fn injection_pass(
    inject: &HashSet<String>,
    paths: &InjectPaths,
    injected: &mut HashSet<u32>,
    failed: &mut HashSet<u32>,
) -> Vec<String> {
    let mut logs = Vec::new();

    let processes = match get_processes() {
        Ok(p) => p,
        Err(e) => {
            logs.push(format!("[!] Process enumeration failed: {e}"));
            return logs;
        }
    };

    let alive_pids: HashSet<u32> = processes.iter().map(|p| p.pid).collect();
    injected.retain(|pid| alive_pids.contains(pid));
    failed.retain(|pid| alive_pids.contains(pid));

    for proc in &processes {
        if injected.contains(&proc.pid) || failed.contains(&proc.pid) {
            continue;
        }
        if !inject.contains(&proc.name.to_lowercase()) {
            continue;
        }

        let is_32bit = is_process_32bit(proc.pid);

        let result = if is_32bit {
            if !paths.helper32_exists || !paths.dll32_exists {
                logs.push(format!(
                    "[!] Skipped {} (PID {}, 32-bit): 32-bit files not found",
                    proc.name, proc.pid
                ));
                failed.insert(proc.pid);
                continue;
            }
            inject_dll_via_helper(proc.pid, &paths.dll32, &paths.helper32)
        } else {
            if !paths.dll64_exists {
                logs.push(format!(
                    "[!] Skipped {} (PID {}, 64-bit): payload.dll not found",
                    proc.name, proc.pid
                ));
                failed.insert(proc.pid);
                continue;
            }
            inject_dll_direct(proc.pid, &paths.dll64)
        };

        let arch = if is_32bit { "32-bit" } else { "64-bit" };
        match result {
            Ok(_) => {
                logs.push(format!(
                    "[+] Injected: {} (PID {}, {arch})",
                    proc.name, proc.pid
                ));
                injected.insert(proc.pid);
            }
            Err(e) => {
                logs.push(format!(
                    "[!] Failed {} (PID {}, {arch}): {e}",
                    proc.name, proc.pid
                ));
                failed.insert(proc.pid);
            }
        }
    }

    logs
}
