//! Process tree and window lookup, so an agent's terminal can be brought back.

use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, GW_OWNER,
};

use super::util::hwnd;

/// This process's parents, nearest first (bash → claude → shell → terminal / editor).
pub fn ancestors() -> Vec<u32> {
    let mut parent = std::collections::HashMap::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return Vec::new() };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                parent.insert(e.th32ProcessID, e.th32ParentProcessID);
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    let mut out = Vec::new();
    let mut pid = std::process::id();
    while let Some(&p) = parent.get(&pid) {
        if p == 0 || out.contains(&p) || out.len() > 16 {
            break;
        }
        out.push(p);
        pid = p;
    }
    out
}

/// Process that owns a window (for a console window: the shell running in it).
pub fn window_pid(h: isize) -> u32 {
    let mut pid = 0u32;
    if h != 0 {
        unsafe { GetWindowThreadProcessId(hwnd(h), Some(&mut pid)) };
    }
    pid
}

struct Find {
    pids: Vec<u32>,
    /// (position in `pids`, window)
    best: Option<(usize, isize)>,
}

unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
    let f = &mut *(l.0 as *mut Find);
    if !IsWindowVisible(h).as_bool() || GetWindowTextLengthW(h) == 0 {
        return BOOL(1);
    }
    if GetWindow(h, GW_OWNER).map(|o| !o.0.is_null()).unwrap_or(false) {
        return BOOL(1);
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if let Some(i) = f.pids.iter().position(|p| *p == pid) {
        if f.best.map(|(b, _)| i < b).unwrap_or(true) {
            f.best = Some((i, h.0 as isize));
        }
    }
    BOOL(1)
}

/// Top-level window of the nearest process in `pids` that has one.
pub fn window_for_pids(pids: &[u32]) -> isize {
    if pids.is_empty() {
        return 0;
    }
    let mut f = Find { pids: pids.to_vec(), best: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut f as *mut Find as isize));
    }
    f.best.map(|(_, h)| h).unwrap_or(0)
}
