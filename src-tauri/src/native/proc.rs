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

/// Every process: (pid, parent pid, exe file name).
fn processes() -> Vec<(u32, u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                out.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..n])));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// This process's parents, nearest first (bash → claude → shell → terminal / app).
pub fn ancestors() -> Vec<u32> {
    let parent: std::collections::HashMap<u32, u32> = processes().into_iter().map(|(p, pp, _)| (p, pp)).collect();
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

/// Running processes with one of these exe names (lowercase, e.g. "code.exe").
pub fn pids_by_exe(names: &[&str]) -> Vec<u32> {
    processes()
        .into_iter()
        .filter(|(_, _, n)| names.contains(&n.to_lowercase().as_str()))
        .map(|(p, _, _)| p)
        .collect()
}

/// The live direct parents of the given processes.
pub fn parent_pids(pids: &[u32]) -> Vec<u32> {
    processes()
        .into_iter()
        .filter_map(|(pid, parent, _)| pids.contains(&pid).then_some(parent))
        .collect()
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
    unique: bool,
    ambiguous: bool,
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
        if f.unique && f.best.is_some() {
            f.ambiguous = true;
            return BOOL(0);
        }
        if f.best.map(|(b, _)| i < b).unwrap_or(true) {
            f.best = Some((i, h.0 as isize));
        }
    }
    BOOL(1)
}

/// Top-level window of the nearest process in `pids` that has one.
pub fn window_for_pids(pids: &[u32]) -> isize {
    find_window(pids, false)
}

/// A fallback by executable is safe only when there is one candidate window.
pub fn unique_window_for_pids(pids: &[u32]) -> isize {
    find_window(pids, true)
}

fn find_window(pids: &[u32], unique: bool) -> isize {
    if pids.is_empty() {
        return 0;
    }
    let mut f = Find { pids: pids.to_vec(), unique, ambiguous: false, best: None };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut f as *mut Find as isize));
    }
    if f.ambiguous { 0 } else { f.best.map(|(_, h)| h).unwrap_or(0) }
}
