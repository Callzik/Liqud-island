//! A tiny error journal: `%APPDATA%\dev.callzik.island\logs\island.log`.
//! Panics, failed background work and front-end errors end up here, so a user
//! can send the file instead of describing what went wrong.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Above this the journal is moved to `island.old.log` and started anew.
const MAX_BYTES: u64 = 1024 * 1024;

static FILE: OnceLock<PathBuf> = OnceLock::new();
static WRITE: Mutex<()> = Mutex::new(());

pub fn path() -> Option<&'static Path> {
    FILE.get().map(|p| p.as_path())
}

/// Opens the journal and installs the panic hook. Call once, at start-up.
pub fn init(app_dir: &Path) {
    let dir = app_dir.join("logs");
    let _ = fs::create_dir_all(&dir);
    let file = dir.join("island.log");
    if fs::metadata(&file).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
        let _ = fs::rename(&file, dir.join("island.old.log"));
    }
    let _ = FILE.set(file);

    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let at = info.location().map(|l| format!(" ({}:{})", l.file(), l.line())).unwrap_or_default();
        write("PANIC", &format!("поток «{name}»: {msg}{at}"));
        default(info);
    }));
    write("INFO", &format!("запуск Island {}", env!("CARGO_PKG_VERSION")));
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}

pub fn warn(msg: &str) {
    write("WARN", msg);
}

pub fn write(level: &str, msg: &str) {
    let Some(file) = FILE.get() else { return };
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{} {level:<5} {}", now(), msg.replace('\n', " ⏎ "));
    }
}

/// The last `max` bytes of the journal (for "Скопировать журнал").
pub fn tail(max: usize) -> String {
    let Some(file) = FILE.get() else { return String::new() };
    let Ok(bytes) = fs::read(file) else { return String::new() };
    let mut start = bytes.len().saturating_sub(max);
    // start on a line boundary
    if start > 0 {
        if let Some(nl) = bytes[start..].iter().position(|&b| b == b'\n') {
            start += nl + 1;
        }
    }
    String::from_utf8_lossy(&bytes[start..]).into_owned()
}

#[cfg(windows)]
fn now() -> String {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    )
}

#[cfg(not(windows))]
fn now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}
