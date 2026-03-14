mod hook;

use std::thread;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::DisableThreadLibraryCalls;
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

#[no_mangle]
pub extern "system" fn DllMain(module: HINSTANCE, reason: u32, _reserved: usize) -> i32 {
    match reason {
        DLL_PROCESS_ATTACH => {
            let _ = unsafe { DisableThreadLibraryCalls(module.into()) };
            let raw = module.0 as usize;
            thread::spawn(move || {
                hook::initialize(HINSTANCE(raw as *mut _));
            });
        }
        DLL_PROCESS_DETACH => {}
        _ => {}
    }

    1
}
