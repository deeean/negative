use std::ffi::CString;

use windows::core::PCSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows::Win32::System::Memory::{MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx};
use windows::Win32::System::Threading::{
    CreateRemoteThread, INFINITE, LPTHREAD_START_ROUTINE, OpenProcess, PROCESS_ALL_ACCESS,
    WaitForSingleObject,
};

fn inject(pid: u32, dll_path: &str) -> Result<(), String> {
    let handle = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, pid) }
        .map_err(|e| format!("OpenProcess: {e}"))?;

    let c_path = CString::new(dll_path).map_err(|e| format!("CString: {e}"))?;
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
        return Err("VirtualAllocEx failed".into());
    }

    unsafe { WriteProcessMemory(handle, alloc, c_path.as_ptr().cast(), size, None) }
        .map_err(|e| {
            let _ = unsafe { CloseHandle(handle) };
            format!("WriteProcessMemory: {e}")
        })?;

    let kernel32 = unsafe { GetModuleHandleA(PCSTR(b"kernel32.dll\0".as_ptr())) }
        .map_err(|e| format!("GetModuleHandle: {e}"))?;

    let load_library = unsafe { GetProcAddress(kernel32, PCSTR(b"LoadLibraryA\0".as_ptr())) }
        .ok_or("GetProcAddress(LoadLibraryA) failed")?;

    let start: LPTHREAD_START_ROUTINE = unsafe { std::mem::transmute(load_library) };

    let thread =
        unsafe { CreateRemoteThread(handle, None, 0, start, Some(alloc), 0, None) }.map_err(
            |e| {
                let _ = unsafe { CloseHandle(handle) };
                format!("CreateRemoteThread: {e}")
            },
        )?;

    unsafe { WaitForSingleObject(thread, INFINITE) };
    let _ = unsafe { VirtualFreeEx(handle, alloc, 0, MEM_RELEASE) };
    let _ = unsafe { CloseHandle(thread) };
    let _ = unsafe { CloseHandle(handle) };

    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 3 {
        eprintln!("Usage: helper32.exe <pid> <dll_path>");
        std::process::exit(1);
    }

    let pid: u32 = match args[1].parse() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Invalid PID: {e}");
            std::process::exit(1);
        }
    };

    let dll_path = &args[2];

    match inject(pid, dll_path) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
