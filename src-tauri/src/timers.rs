//! Timers and reminders: kept here (not in the page) so they ring with the panel closed
//! and survive restarts (`timers.json`). Ringing sends "alarm" to the island.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, EventTarget};

use crate::store::{load_json, now_ms, save_json};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Timer {
    pub id: String,
    pub title: String,
    /// unix ms when it rings
    pub at_ms: i64,
    /// "timer" or "reminder"
    pub kind: String,
    /// "" or "daily"
    pub repeat: String,
    /// for timers: the length, so «Ещё раз» can restart it
    pub duration_ms: i64,
}

struct State {
    list: Vec<Timer>,
    path: PathBuf,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn st() -> std::sync::MutexGuard<'static, State> {
    STATE
        .get_or_init(|| Mutex::new(State { list: vec![], path: PathBuf::new() }))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn save(s: &State) {
    // running countdowns don't survive a restart, reminders do
    let keep: Vec<&Timer> = s.list.iter().filter(|t| t.kind == "reminder").collect();
    save_json(&s.path, &keep);
}

fn changed(app: &AppHandle, list: Vec<Timer>) {
    let _ = app.emit_to(EventTarget::webview_window("main"), "timers", list);
}

pub fn list() -> Vec<Timer> {
    let mut v = st().list.clone();
    v.sort_by_key(|t| t.at_ms);
    v
}

pub fn add(app: &AppHandle, t: Timer) -> Vec<Timer> {
    {
        let mut s = st();
        s.list.retain(|x| x.id != t.id);
        s.list.push(t);
        save(&s);
    }
    let l = list();
    changed(app, l.clone());
    l
}

pub fn remove(app: &AppHandle, id: &str) -> Vec<Timer> {
    {
        let mut s = st();
        s.list.retain(|x| x.id != id);
        save(&s);
    }
    let l = list();
    changed(app, l.clone());
    l
}

pub fn spawn(app: AppHandle, path: PathBuf) {
    {
        let mut s = st();
        let loaded: Vec<Timer> = load_json(&path);
        s.list = loaded;
        s.path = path;
    }
    thread::Builder::new()
        .name("timers".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            let now = now_ms();
            let due: Vec<Timer> = {
                let mut s = st();
                let due: Vec<Timer> = s.list.iter().filter(|t| t.at_ms <= now).cloned().collect();
                if due.is_empty() {
                    continue;
                }
                for t in s.list.iter_mut().filter(|t| t.at_ms <= now && t.repeat == "daily") {
                    while t.at_ms <= now {
                        t.at_ms += 86_400_000;
                    }
                }
                s.list.retain(|t| t.at_ms > now);
                save(&s);
                due
            };
            for t in due {
                // reminders missed while the PC was off ring once, late
                let _ = app.emit_to(EventTarget::webview_window("main"), "alarm", t);
            }
            changed(&app, list());
        })
        .ok();
}
