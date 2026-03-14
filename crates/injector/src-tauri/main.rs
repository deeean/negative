#![windows_subsystem = "windows"]

mod inject;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::{env, thread, time::Duration};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

const MAX_LOGS: usize = 500;

#[derive(Clone)]
struct SharedState {
    inner: Arc<Mutex<SharedInner>>,
}

struct SharedInner {
    running: bool,
    inject: Vec<String>,
    hide: Vec<String>,
    injected: HashSet<u32>,
    failed: HashSet<u32>,
    logs: Vec<LogEntry>,
    paths: inject::InjectPaths,
    shm: Option<shared::SharedMemoryWriter>,
}

struct LogEntry {
    time: String,
    message: String,
    level: LogLevel,
}

#[derive(Clone, Copy)]
enum LogLevel {
    Info,
    Success,
    Warning,
}

#[derive(Serialize, Clone)]
struct FrontendState {
    running: bool,
    inject: Vec<String>,
    hide: Vec<String>,
    injected_count: usize,
    failed_count: usize,
    logs: Vec<FrontendLog>,
    has_dll: bool,
    has_32bit: bool,
}

#[derive(Serialize, Clone)]
struct FrontendLog {
    time: String,
    message: String,
    level: String,
}

#[derive(Deserialize)]
struct InitConfig {
    inject: Vec<String>,
    hide: Vec<String>,
}

mod ffi_time {
    extern "system" {
        pub fn GetLocalTime(lp_system_time: *mut SYSTEMTIME);
    }
    #[repr(C)]
    pub struct SYSTEMTIME {
        pub w_year: u16,
        pub w_month: u16,
        pub w_day_of_week: u16,
        pub w_day: u16,
        pub w_hour: u16,
        pub w_minute: u16,
        pub w_second: u16,
        pub w_milliseconds: u16,
    }
}

fn now_stamp() -> String {
    let mut st = ffi_time::SYSTEMTIME {
        w_year: 0,
        w_month: 0,
        w_day_of_week: 0,
        w_day: 0,
        w_hour: 0,
        w_minute: 0,
        w_second: 0,
        w_milliseconds: 0,
    };
    unsafe { ffi_time::GetLocalTime(&mut st) };
    format!("{:02}:{:02}:{:02}", st.w_hour, st.w_minute, st.w_second)
}

fn parse_log(msg: String) -> LogEntry {
    let level = if msg.starts_with("[+]") {
        LogLevel::Success
    } else if msg.starts_with("[!]") {
        LogLevel::Warning
    } else {
        LogLevel::Info
    };
    LogEntry {
        time: now_stamp(),
        message: msg,
        level,
    }
}

fn exe_dir() -> std::path::PathBuf {
    env::current_exe()
        .expect("cannot get exe path")
        .parent()
        .expect("exe has no parent dir")
        .to_path_buf()
}

impl SharedState {
    fn new() -> Self {
        let dir = exe_dir();

        let dll64 = dir.join("payload.dll");
        let dll32 = dir.join("payload32.dll");
        let helper32 = dir.join("helper32.exe");

        let mut logs = Vec::new();

        match inject::enable_debug_privilege() {
            Ok(_) => logs.push(parse_log("[*] SeDebugPrivilege: OK".into())),
            Err(e) => logs.push(parse_log(format!("[!] SeDebugPrivilege failed: {e}"))),
        }

        if dll64.exists() {
            logs.push(parse_log("[*] payload.dll: OK".into()));
        } else {
            logs.push(parse_log(
                "[!] payload.dll not found (64-bit injection disabled)".into(),
            ));
        }

        if dll32.exists() && helper32.exists() {
            logs.push(parse_log("[*] payload32.dll + helper32.exe: OK".into()));
        } else {
            logs.push(parse_log("[!] 32-bit support unavailable".into()));
        }

        let paths = inject::InjectPaths {
            dll64_exists: dll64.exists(),
            dll64: dll64.to_string_lossy().to_string(),
            dll32_exists: dll32.exists(),
            dll32: dll32.to_string_lossy().to_string(),
            helper32_exists: helper32.exists(),
            helper32: helper32.to_string_lossy().to_string(),
        };

        let shm = match shared::SharedMemoryWriter::create() {
            Some(w) => {
                logs.push(parse_log("[*] Shared memory: OK".into()));
                Some(w)
            }
            None => {
                logs.push(parse_log("[!] Shared memory failed to create".into()));
                None
            }
        };

        Self {
            inner: Arc::new(Mutex::new(SharedInner {
                running: false,
                inject: Vec::new(),
                hide: Vec::new(),
                injected: HashSet::new(),
                failed: HashSet::new(),
                logs,
                paths,
                shm,
            })),
        }
    }

    fn to_frontend(&self) -> FrontendState {
        let s = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        FrontendState {
            running: s.running,
            inject: s.inject.clone(),
            hide: s.hide.clone(),
            injected_count: s.injected.len(),
            failed_count: s.failed.len(),
            logs: s
                .logs
                .iter()
                .map(|e| FrontendLog {
                    time: e.time.clone(),
                    message: e.message.clone(),
                    level: match e.level {
                        LogLevel::Info => "info",
                        LogLevel::Success => "success",
                        LogLevel::Warning => "warning",
                    }
                    .to_string(),
                })
                .collect(),
            has_dll: s.paths.dll64_exists,
            has_32bit: s.paths.helper32_exists && s.paths.dll32_exists,
        }
    }
}

fn write_shm(s: &SharedInner) {
    if let Some(shm) = &s.shm {
        shm.write(&shared::Message {
            hide: s.hide.clone(),
        });
    }
}

fn emit_update(app: &AppHandle, state: &SharedState) {
    let _ = app.emit("state-update", state.to_frontend());
}

fn spawn_injection_thread(state: SharedState, app: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(10));

        let (running, inject_set, paths) = {
            let s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
            if !s.running {
                continue;
            }
            let set: HashSet<String> = s.inject.iter().map(|t| t.to_lowercase()).collect();
            (s.running, set, s.paths.clone())
        };

        if !running || inject_set.is_empty() {
            continue;
        }

        let logs = {
            let mut guard = state.inner.lock().unwrap_or_else(|e| e.into_inner());
            let s = &mut *guard;
            inject::injection_pass(&inject_set, &paths, &mut s.injected, &mut s.failed)
        };

        if !logs.is_empty() {
            let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
            for log in logs {
                s.logs.push(parse_log(log));
            }
            if s.logs.len() > MAX_LOGS {
                let drain = s.logs.len() - MAX_LOGS;
                s.logs.drain(..drain);
            }
            drop(s);
            emit_update(&app, &state);
        }
    });
}

#[tauri::command]
fn get_state(state: State<'_, SharedState>) -> FrontendState {
    state.to_frontend()
}

#[tauri::command]
fn init_config(state: State<'_, SharedState>, app: AppHandle, config: InitConfig) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.inject = config.inject;
        s.hide = config.hide;
        write_shm(&s);
    }
    emit_update(&app, &*state);
}

#[tauri::command]
fn start_injection(state: State<'_, SharedState>, app: AppHandle) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !s.running {
            s.running = true;
            s.injected.clear();
            s.failed.clear();
            s.logs.push(parse_log("[*] Started.".into()));
        }
    }
    emit_update(&app, &*state);
}

#[tauri::command]
fn stop_injection(state: State<'_, SharedState>, app: AppHandle) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        if s.running {
            s.running = false;
            s.logs.push(parse_log("[*] Stopped.".into()));
        }
    }
    emit_update(&app, &*state);
}

#[tauri::command]
fn add_inject(state: State<'_, SharedState>, app: AppHandle, name: String) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !s.inject.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
            s.inject.push(name);
        }
    }
    emit_update(&app, &*state);
}

#[tauri::command]
fn remove_inject(state: State<'_, SharedState>, app: AppHandle, index: usize) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        if index < s.inject.len() {
            s.inject.remove(index);
        }
    }
    emit_update(&app, &*state);
}

#[tauri::command]
fn add_hide(state: State<'_, SharedState>, app: AppHandle, name: String) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !s.hide.iter().any(|h| h.eq_ignore_ascii_case(&name)) {
            s.hide.push(name);
            write_shm(&s);
        }
    }
    emit_update(&app, &*state);
}

#[tauri::command]
fn remove_hide(state: State<'_, SharedState>, app: AppHandle, index: usize) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        if index < s.hide.len() {
            s.hide.remove(index);
            write_shm(&s);
        }
    }
    emit_update(&app, &*state);
}

#[tauri::command]
fn clear_logs(state: State<'_, SharedState>, app: AppHandle) {
    {
        let mut s = state.inner.lock().unwrap_or_else(|e| e.into_inner());
        s.logs.clear();
    }
    emit_update(&app, &*state);
}

fn main() {
    tauri::Builder::default()
        .manage(SharedState::new())
        .invoke_handler(tauri::generate_handler![
            get_state,
            init_config,
            start_injection,
            stop_injection,
            add_inject,
            remove_inject,
            add_hide,
            remove_hide,
            clear_logs,
        ])
        .setup(|app| {
            let state = (*app.state::<SharedState>()).clone();
            let handle = app.handle().clone();
            spawn_injection_thread(state, handle);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
