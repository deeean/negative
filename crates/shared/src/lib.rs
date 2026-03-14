use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{iter, ptr};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::System::Memory::*;

const SHM_NAME: &str = "Global\\NegativeState";
const SHM_SIZE: usize = 65536;
const HEADER_SIZE: usize = 8;

extern "system" {
    fn InitializeSecurityDescriptor(
        psecuritydescriptor: *mut c_void,
        dwrevision: u32,
    ) -> i32;
    fn SetSecurityDescriptorDacl(
        psecuritydescriptor: *mut c_void,
        bdaclpresent: i32,
        pdacl: *const c_void,
        bdacldefaulted: i32,
    ) -> i32;
}

#[derive(Clone, Debug)]
pub struct Message {
    pub hide: Vec<String>,
}

fn shm_name_wide() -> Vec<u16> {
    SHM_NAME.encode_utf16().chain(iter::once(0)).collect()
}

fn encode(msg: &Message) -> Vec<u8> {
    let mut buf = Vec::new();
    for name in &msg.hide {
        let bytes = name.as_bytes();
        let len = bytes.len() as u16;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(bytes);
    }
    buf
}

fn decode(data: &[u8]) -> Option<Message> {
    let mut offset = 0;
    let mut hide = Vec::new();
    while offset + 2 <= data.len() {
        let len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        if offset + len > data.len() {
            break;
        }
        if let Ok(s) = std::str::from_utf8(&data[offset..offset + len]) {
            hide.push(s.to_string());
        }
        offset += len;
    }
    Some(Message { hide })
}

fn create_allow_all_sa() -> Option<(SECURITY_ATTRIBUTES, Vec<u8>)> {
    // SECURITY_DESCRIPTOR_MIN_LENGTH = 20 on x86, 40 on x64
    let sd_size = 64; // enough for both architectures
    let mut sd_buf = vec![0u8; sd_size];
    let sd_ptr = sd_buf.as_mut_ptr() as *mut c_void;

    unsafe {
        // SECURITY_DESCRIPTOR_REVISION = 1
        if InitializeSecurityDescriptor(sd_ptr, 1) == 0 {
            return None;
        }
        // NULL DACL = grant all access to everyone
        if SetSecurityDescriptorDacl(sd_ptr, 1, ptr::null(), 0) == 0 {
            return None;
        }
    }

    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd_ptr,
        bInheritHandle: false.into(),
    };

    Some((sa, sd_buf))
}

pub struct SharedMemoryWriter {
    _handle: HANDLE,
    ptr: *mut u8,
}

unsafe impl Send for SharedMemoryWriter {}
unsafe impl Sync for SharedMemoryWriter {}

impl SharedMemoryWriter {
    pub fn create() -> Option<Self> {
        unsafe {
            let name = shm_name_wide();
            let (_sa, _sd_buf) = create_allow_all_sa()?;

            let handle = CreateFileMappingW(
                HANDLE(-1isize as *mut c_void),
                Some(&_sa),
                PAGE_READWRITE,
                0,
                SHM_SIZE as u32,
                PCWSTR(name.as_ptr()),
            )
            .ok()?;

            let view = MapViewOfFile(handle, FILE_MAP_WRITE, 0, 0, SHM_SIZE);
            if view.Value.is_null() {
                let _ = CloseHandle(handle);
                return None;
            }

            let p = view.Value as *mut u8;
            ptr::write_bytes(p, 0, SHM_SIZE);

            Some(Self {
                _handle: handle,
                ptr: p,
            })
        }
    }

    pub fn write(&self, msg: &Message) {
        let data = encode(msg);
        if data.len() + HEADER_SIZE > SHM_SIZE {
            return;
        }

        unsafe {
            let version_ptr = self.ptr as *const AtomicU32;
            let version = (*version_ptr).load(Ordering::Relaxed);

            let len_ptr = self.ptr.add(4) as *mut u32;
            ptr::write_volatile(len_ptr, data.len() as u32);

            ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(HEADER_SIZE), data.len());

            (*version_ptr).store(version.wrapping_add(1), Ordering::Release);
        }
    }
}

pub struct SharedMemoryReader {
    _handle: HANDLE,
    ptr: *const u8,
    last_version: u32,
}

unsafe impl Send for SharedMemoryReader {}
unsafe impl Sync for SharedMemoryReader {}

impl SharedMemoryReader {
    pub fn open() -> Option<Self> {
        unsafe {
            let name = shm_name_wide();
            let handle = OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name.as_ptr())).ok()?;

            let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
            if view.Value.is_null() {
                let _ = CloseHandle(handle);
                return None;
            }

            Some(Self {
                _handle: handle,
                ptr: view.Value as *const u8,
                last_version: u32::MAX,
            })
        }
    }

    pub fn read_if_changed(&mut self) -> Option<Message> {
        unsafe {
            let version_ptr = self.ptr as *const AtomicU32;
            let version = (*version_ptr).load(Ordering::Acquire);

            if version == self.last_version {
                return None;
            }
            self.last_version = version;

            let len = ptr::read_volatile(self.ptr.add(4) as *const u32) as usize;
            if len == 0 || len + HEADER_SIZE > SHM_SIZE {
                return None;
            }

            let mut data = vec![0u8; len];
            ptr::copy_nonoverlapping(self.ptr.add(HEADER_SIZE), data.as_mut_ptr(), len);

            decode(&data)
        }
    }
}
