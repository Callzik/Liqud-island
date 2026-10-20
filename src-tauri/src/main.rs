// Island — a liquid "Dynamic Island" for Windows 10/11.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agents;
mod ai;
mod clips;
mod record;
mod downloads;
mod log;
mod native;
mod spectrum;
mod store;
mod update;
mod voice;
mod weather;
mod ym;
mod yt;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, EventTarget, Manager, PhysicalPosition, PhysicalSize, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as _};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use clips::{ClipEntry, ClipStore, ClipView};
use native::{audio::Audio, capture, clip, img, input, media::Media, shell, util};
use store::{load_json, save_json, Data, Paths, PinItem, Settings, ShelfItem};

/// Logical size of the transparent window the island lives in.
const WIN_W: f64 = 1000.0;
const WIN_H: f64 = 640.0;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Clone, Copy, Default, Debug, PartialEq)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn contains(&self, x: f64, y: f64) -> bool {
        self.w > 0.0 && x >= self.x && x <= self.x + self.w && y >= self.y && y <= self.y + self.h
    }
    fn grow(&self, side: f64, down: f64) -> Rect {
        Rect {
            x: self.x - side,
            y: self.y,
            w: self.w + side * 2.0,
            h: self.h + down,
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Geom {
    x: i32,
    y: i32,
    scale: f64,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct MediaPayload {
    app: String,
    app_id: String,
    title: String,
    artist: String,
    album: String,
    playing: bool,
    position_ms: i64,
    duration_ms: i64,
    updated_ms: i64,
    can_prev: bool,
    can_next: bool,
    /// Changes whenever the cover image changes (see the "cover" event).
    cover_id: u32,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct CoverPayload {
    id: u32,
    url: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
struct VolumePayload {
    level: f32,
    muted: bool,
    device: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CursorPayload {
    x: f64,
    y: f64,
    inside: bool,
    lmb: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct AppItem {
    target: String,
    name: String,
}

enum AudioCmd {
    Volume(f32),
    Mute(bool),
    List(Sender<Vec<native::audio::OutputDevice>>),
    SetDevice(String, Sender<bool>),
}

struct MediaCmd {
    action: String,
    value: f64,
}

type Job = Box<dyn FnOnce() + Send>;

/// A single STA thread for shell work (icons, app list, ShellExecute).
struct ShellWorker {
    tx: Mutex<Sender<Job>>,
}

impl ShellWorker {
    fn spawn() -> Self {
        let (tx, rx) = channel::<Job>();
        thread::Builder::new()
            .name("shell".into())
            .spawn(move || {
                let _com = util::Com::sta();
                for job in rx {
                    job();
                }
            })
            .expect("failed to start shell thread");
        Self { tx: Mutex::new(tx) }
    }

    fn run<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        let (rtx, rrx) = channel();
        let job: Job = Box::new(move || {
            let _ = rtx.send(f());
        });
        lock(&self.tx).send(job).ok()?;
        rrx.recv().ok()
    }
}

struct Shared {
    paths: Paths,
    settings: Mutex<Settings>,
    data: Mutex<Data>,
    clips: Mutex<ClipStore>,
    hit: Mutex<Rect>,
    geom: Mutex<Geom>,
    own_hwnd: AtomicIsize,
    last_fg: AtomicIsize,
    suppressed: AtomicBool,
    launcher_open: AtomicBool,
    media_playing: AtomicBool,
    clip_skip_seq: AtomicU32,
    media: Mutex<Option<MediaPayload>>,
    cover: Mutex<CoverPayload>,
    volume: Mutex<VolumePayload>,
    media_tx: Mutex<Sender<MediaCmd>>,
    audio_tx: Mutex<Sender<AudioCmd>>,
    shell: ShellWorker,
    icons: Mutex<HashMap<String, Option<String>>>,
    apps: Mutex<Option<Vec<AppItem>>>,
    /// action -> registered combo
    hotkeys: Mutex<HashMap<String, String>>,
    hotkey_error: Mutex<Option<String>>,
    /// Frozen screen shown by the capture overlay.
    shot: Mutex<Option<Shot>>,
    shot_seq: AtomicU32,
    capturing: AtomicBool,
    /// Bumped to cancel the reply being streamed.
    ai_gen: AtomicU32,
    /// The reply being streamed: aborted on Stop even while it waits for the
    /// server (a model still loading may not send a byte for minutes).
    ai_task: Mutex<Option<(String, tauri::async_runtime::JoinHandle<()>)>>,
    weather: Mutex<Option<weather::WeatherPayload>>,
    weather_wake: Mutex<Sender<()>>,
    voice_tx: Mutex<Sender<VoiceCmd>>,
    voice_active: AtomicBool,
    recording: Mutex<Option<record::Recording>>,
    ym: ym::Shared,
    /// the hidden YouTube player is the one playing («без цензуры»)
    yt_active: AtomicBool,
    /// track that could not be found on YouTube: Yandex plays it itself
    yt_failed: Mutex<String>,
    /// latest glass backdrop (id, BMP) for the backdrop:// protocol
    backdrop: Mutex<Option<(u32, Arc<Vec<u8>>)>>,
    backdrop_seq: AtomicU32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RecordingState {
    active: bool,
    started_ms: i64,
}

#[derive(Clone, Copy, PartialEq)]
enum VoiceCmd {
    Toggle,
    Start,
    Stop,
    Cancel,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VoiceState {
    /// "idle" | "recording" | "processing"
    state: String,
    started_ms: i64,
}

/// A monitor snapshot (physical pixels, top-down BGRA).
struct Shot {
    id: u32,
    mode: String,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    bgra: Arc<Vec<u8>>,
    bmp: Arc<Vec<u8>>,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ToastAction {
    label: String,
    cmd: String,
    args: serde_json::Value,
}

/// Toast shown by the island (sent to the main window).
#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
struct ToastPayload {
    icon: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    swatch: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    actions: Vec<ToastAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ms: Option<u32>,
}

fn toast(app: &AppHandle, t: ToastPayload) {
    let _ = app.emit_to(EventTarget::webview_window("main"), "toast", t);
}

fn toast_error(app: &AppHandle, title: &str, subtitle: &str) {
    log::error(&format!("{title}: {subtitle}"));
    toast(
        app,
        ToastPayload {
            icon: "error".into(),
            tone: Some("error".into()),
            title: title.into(),
            subtitle: Some(subtitle.into()),
            ..Default::default()
        },
    );
}

type St<'a> = State<'a, Arc<Shared>>;

// ---------------------------------------------------------------------------
// Window placement, launcher, hotkey, autostart
// ---------------------------------------------------------------------------

fn place_window(win: &WebviewWindow, shared: &Shared, force: bool) {
    let Ok(Some(mon)) = win.primary_monitor() else { return };
    let scale = mon.scale_factor();
    let w = (WIN_W * scale).round() as i32;
    let h = (WIN_H * scale).round() as i32;
    let pos = *mon.position();
    let size = *mon.size();
    let geom = Geom {
        x: pos.x + (size.width as i32 - w) / 2,
        y: pos.y,
        scale,
    };
    let changed = *lock(&shared.geom) != geom;
    // also put it back when something else moved it (a monitor dropping out on
    // wake, Win+Shift+arrows while the launcher has focus)
    let misplaced = match (win.outer_position(), win.outer_size()) {
        (Ok(p), Ok(sz)) => {
            (p.x - geom.x).abs() > 2 || (p.y - geom.y).abs() > 2 || (sz.width as i32 - w).abs() > 2 || (sz.height as i32 - h).abs() > 2
        }
        _ => false,
    };
    if force || changed || misplaced {
        // position first: moving onto a monitor with another DPI rescales the
        // window, the size set afterwards is then the right one for that monitor
        let _ = win.set_position(PhysicalPosition::new(geom.x, geom.y));
        let _ = win.set_size(PhysicalSize::new(w as u32, h as u32));
        *lock(&shared.geom) = geom;
    }
}

fn set_launcher(app: &AppHandle, open: bool) {
    let shared = app.state::<Arc<Shared>>();
    shared.launcher_open.store(open, Ordering::Relaxed);
    if open {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.set_focus();
        }
    }
    let _ = app.emit("launcher", open);
}

fn toggle_launcher(app: &AppHandle) {
    let open = !app.state::<Arc<Shared>>().launcher_open.load(Ordering::Relaxed);
    set_launcher(app, open);
}

/// (action, combo) pairs wanted by the settings.
fn hotkey_bindings(s: &Settings) -> Vec<(&'static str, String)> {
    let mut v = vec![("launcher", s.hotkey.clone())];
    if s.capture_enabled {
        v.push(("region", s.hotkey_region.clone()));
        v.push(("ocr", s.hotkey_ocr.clone()));
    }
    if s.voice_enabled {
        v.push(("voice", s.hotkey_voice.clone()));
    }
    v.retain(|(_, c)| !c.trim().is_empty());
    v
}

fn run_hotkey(app: &AppHandle, action: &str) {
    match action {
        "launcher" => toggle_launcher(app),
        "region" | "ocr" => begin_capture(app.clone(), action.to_string()),
        "voice" => {
            let _ = lock(&app.state::<Arc<Shared>>().voice_tx).send(VoiceCmd::Toggle);
        }
        _ => {}
    }
}

/// Re-registers every global shortcut. Must run on the main thread (RegisterHotKey
/// is thread-affine). Returns the shortcuts that could not be registered.
fn apply_hotkeys(app: &AppHandle, shared: &Shared, settings: &Settings) -> Vec<String> {
    let gs = app.global_shortcut();
    let mut cur = lock(&shared.hotkeys);
    for (_, combo) in cur.drain() {
        let _ = gs.unregister(combo.as_str());
    }
    let mut errors = Vec::new();
    for (action, combo) in hotkey_bindings(settings) {
        if cur.values().any(|c| c.eq_ignore_ascii_case(&combo)) {
            errors.push(format!("«{combo}» назначено дважды"));
            continue;
        }
        let act = action.to_string();
        let res = gs.on_shortcut(combo.as_str(), move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                run_hotkey(app, &act);
            }
        });
        match res {
            Ok(()) => {
                cur.insert(action.to_string(), combo);
            }
            Err(e) => errors.push(format!("«{combo}»: {e}")),
        }
    }
    errors
}

fn apply_autostart(app: &AppHandle, enabled: bool) {
    let al = app.autolaunch();
    let _ = if enabled { al.enable() } else { al.disable() };
}

/// Player name shown on the island. Known players by their id; others by the
/// name Start shows for them (looked up once per id on the shell thread);
/// last resort — the meaningful part of the id itself.
fn media_app_name(shared: &Shared, id: &str) -> String {
    static CACHE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(n) = lock(cache).get(id) {
        return n.clone();
    }
    let owned = id.to_string();
    let name = known_app_name(id)
        .or_else(|| shared.shell.run(move || shell::app_display_name(&owned)).flatten())
        .unwrap_or_else(|| app_name_from_id(id));
    lock(cache).insert(id.to_string(), name.clone());
    name
}

fn known_app_name(id: &str) -> Option<String> {
    let l = id.to_lowercase();
    const KNOWN: &[(&str, &str)] = &[
        ("spotify", "Spotify"),
        ("yandex.music", "Яндекс Музыка"),
        ("yandexmusic", "Яндекс Музыка"),
        ("zunemusic", "Медиаплеер"),
        ("applemusic", "Apple Music"),
        ("itunes", "iTunes"),
        ("vkmusic", "VK Музыка"),
        ("deezer", "Deezer"),
        ("tidal", "TIDAL"),
        ("soundcloud", "SoundCloud"),
        ("liner", "Liner"),
        ("dotify", "Dotify"),
        ("aimp", "AIMP"),
        ("foobar", "foobar2000"),
        ("vlc", "VLC"),
        ("telegram", "Telegram"),
        ("msedge", "Edge"),
        ("chrome", "Chrome"),
        ("firefox", "Firefox"),
        ("308046b0af4a39cb", "Firefox"),
        ("opera", "Opera"),
        ("brave", "Brave"),
        ("vivaldi", "Vivaldi"),
        ("yandex", "Яндекс Браузер"),
    ];
    KNOWN.iter().find(|(k, _)| l.contains(k)).map(|(_, v)| (*v).to_string())
}

/// "Publisher.Dotify_8wekyb3d8bbwe!App", "com.squirrel.Dotify.Dotify",
/// "C:\\…\\Dotify.exe" → "Dotify".
fn app_name_from_id(id: &str) -> String {
    const GENERIC: &[&str] = &[
        "app", "application", "desktop", "client", "player", "music", "win", "windows", "win32", "x64", "exe",
        "electron", "squirrel", "com", "org", "net", "io", "dev", "ru", "app_id", "main",
    ];
    let file = id.rsplit(['\\', '/']).next().unwrap_or(id);
    let file = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    let useful = |p: &&str| {
        !p.is_empty() && !GENERIC.contains(&p.to_lowercase().as_str()) && !p.chars().all(|c| c.is_ascii_hexdigit())
    };
    // after '!' Store apps put the entry point ("App"); the package name is before it
    let mut parts: Vec<&str> = Vec::new();
    for half in file.split('!').rev() {
        // drop the publisher hash of a package family name
        let half = half.split('_').next().unwrap_or(half);
        parts.extend(half.rsplit(['.', ' ']));
    }
    let Some(best) = parts.iter().find(|p| useful(p)) else { return "Плеер".into() };
    // "12345Publisher" style prefixes
    let best = best.trim_start_matches(|c: char| c.is_ascii_digit());
    let mut c = best.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => "Плеер".into(),
    }
}

// ---------------------------------------------------------------------------
// Background threads
// ---------------------------------------------------------------------------

/// Cursor tracking, click-through switching, foreground / fullscreen watch.
fn spawn_cursor_thread(app: AppHandle, shared: Arc<Shared>) {
    thread::Builder::new()
        .name("cursor".into())
        .spawn(move || {
            let Some(win) = app.get_webview_window("main") else { return };
            let mut input_on = false;
            let mut last: Option<(i32, i32, bool, bool)> = None;
            let mut was_near = false;
            let mut last_fg_check = Instant::now();
            let mut last_place = Instant::now();
            // 60 fps only while the cursor is around the island; far away a slower
            // poll is enough (the "near" zone is wide enough to switch back in time)
            let mut pause = 16u64;
            loop {
                thread::sleep(Duration::from_millis(pause));
                let geom = *lock(&shared.geom);
                if geom.scale <= 0.0 {
                    continue;
                }
                let (cx, cy) = input::cursor_pos();
                let lx = (cx - geom.x) as f64 / geom.scale;
                let ly = (cy - geom.y) as f64 / geom.scale;
                let rect = *lock(&shared.hit);
                let suppressed = shared.suppressed.load(Ordering::Relaxed);
                let lmb = input::lmb_down();
                let inside = !suppressed && rect.contains(lx, ly);
                // While a button is held (file drag) the catch zone is larger,
                // so the island can grow into drop targets before the cursor arrives.
                let magnet = !suppressed && lmb && rect.grow(90.0, 150.0).contains(lx, ly);
                let want = inside || magnet;
                if want != input_on && win.set_ignore_cursor_events(!want).is_ok() {
                    input_on = want;
                }
                let near = !suppressed && rect.grow(260.0, 320.0).contains(lx, ly);
                if near || was_near {
                    let key = (lx.round() as i32, ly.round() as i32, inside, lmb);
                    if last != Some(key) {
                        last = Some(key);
                        let _ = app.emit("cursor", CursorPayload { x: lx, y: ly, inside, lmb });
                    }
                }
                was_near = near;
                pause = if near || input_on { 16 } else { 64 };

                if last_fg_check.elapsed() >= Duration::from_millis(240) {
                    last_fg_check = Instant::now();
                    let own = shared.own_hwnd.load(Ordering::Relaxed);
                    let fg = input::foreground();
                    // remember the user's window, never one of ours (the capture
                    // overlay or the tray menu would get the focus back instead)
                    if fg != 0 && fg != own && !input::is_own_process(fg) {
                        shared.last_fg.store(fg, Ordering::Relaxed);
                    }
                    let hide = lock(&shared.settings).hide_fullscreen && input::foreground_is_fullscreen(own);
                    if hide != suppressed {
                        shared.suppressed.store(hide, Ordering::Relaxed);
                        let _ = app.emit("suppressed", hide);
                    }
                }
                if last_place.elapsed() >= Duration::from_millis(2400) {
                    last_place = Instant::now();
                    place_window(&win, &shared, false);
                }
            }
        })
        .expect("failed to start cursor thread");
}

/// Now playing (GSMTC), polled; commands are executed on the same thread.
fn spawn_media_thread(app: AppHandle, shared: Arc<Shared>, rx: Receiver<MediaCmd>) {
    thread::Builder::new()
        .name("media".into())
        .spawn(move || {
            let _com = util::Com::mta();
            let mut logged = false;
            let media = loop {
                match Media::new() {
                    Ok(m) => break m,
                    Err(e) => {
                        if !logged {
                            logged = true;
                            log::error(&format!("медиасессии недоступны: {e}"));
                        }
                        thread::sleep(Duration::from_secs(5))
                    }
                }
            };
            let mut last: Option<MediaPayload> = None;
            let mut cover_key = String::new();
            let mut cover_tries = 0u32;
            let mut cover_id = 0u32;
            let mut has_cover = false;
            let mut auto_key = String::new();
            loop {
                // while music plays the position is re-read more often: lyrics follow it
                let poll = if shared.media_playing.load(Ordering::Relaxed) { 300 } else { 700 };
                match rx.recv_timeout(Duration::from_millis(poll)) {
                    Ok(cmd) => {
                        route_media(&app, &shared, &media, &cmd);
                        // quick clicks (next, next, next) go out together, then
                        // one pause lets the player update its state
                        while let Ok(more) = rx.try_recv() {
                            route_media(&app, &shared, &media, &more);
                        }
                        thread::sleep(Duration::from_millis(250));
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                auto_uncensored(&app, &shared, &media, &mut auto_key);
                let state = media.state();
                if let Some(s) = &state {
                    let key = format!("{}\u{1}{}\u{1}{}\u{1}{}", s.app_id, s.title, s.artist, s.album);
                    if key != cover_key {
                        cover_key = key;
                        cover_tries = 0;
                        has_cover = false;
                        cover_id = cover_id.wrapping_add(1);
                        let cp = CoverPayload { id: cover_id, url: None };
                        *lock(&shared.cover) = cp.clone();
                        let _ = app.emit("cover", cp);
                    }
                    // Players often publish artwork a moment after the metadata.
                    if !has_cover && cover_tries < 6 {
                        cover_tries += 1;
                        if let Some(t) = media.thumbnail() {
                            has_cover = true;
                            let cp = CoverPayload {
                                id: cover_id,
                                url: Some(util::data_url(&t.mime, &t.bytes)),
                            };
                            *lock(&shared.cover) = cp.clone();
                            let _ = app.emit("cover", cp);
                        }
                    }
                }
                let payload = state.map(|s| MediaPayload {
                    app: media_app_name(&shared, &s.app_id),
                    app_id: s.app_id,
                    title: s.title,
                    artist: s.artist,
                    album: s.album,
                    playing: s.playing,
                    position_ms: s.position_ms,
                    duration_ms: s.duration_ms,
                    updated_ms: s.updated_ms,
                    can_prev: s.can_prev,
                    can_next: s.can_next,
                    cover_id,
                });
                shared
                    .media_playing
                    .store(payload.as_ref().map(|p| p.playing).unwrap_or(false), Ordering::Relaxed);
                if payload != last {
                    *lock(&shared.media) = payload.clone();
                    let _ = app.emit("media", payload.clone());
                    last = payload;
                }
            }
        })
        .expect("failed to start media thread");
}

fn publish_volume(app: &AppHandle, shared: &Shared, audio: &Audio) {
    let (level, muted) = audio.volume();
    let v = VolumePayload {
        level,
        muted,
        device: audio.device_short_name(),
    };
    let changed = *lock(&shared.volume) != v;
    if changed {
        *lock(&shared.volume) = v.clone();
        let _ = app.emit("volume", v);
    }
}

/// Output peak level (drives the wave) and master volume.
fn spawn_audio_thread(app: AppHandle, shared: Arc<Shared>, rx: Receiver<AudioCmd>) {
    thread::Builder::new()
        .name("audio".into())
        .spawn(move || {
            let _com = util::Com::mta();
            let mut logged = false;
            let mut audio = loop {
                match Audio::new() {
                    Ok(a) => break a,
                    Err(e) => {
                        if !logged {
                            logged = true;
                            log::error(&format!("звук недоступен: {e}"));
                        }
                        thread::sleep(Duration::from_secs(5))
                    }
                }
            };
            publish_volume(&app, &shared, &audio);
            let spectrum = spectrum::Spectrum::spawn();
            let mut last_bands = [0.0f32; spectrum::BANDS];
            let mut last_peak = -1.0f32;
            let mut last_volume_poll = Instant::now();
            let mut last_device_check = Instant::now();
            loop {
                let mut dirty = false;
                while let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        AudioCmd::Volume(v) => audio.set_volume(v),
                        AudioCmd::Mute(m) => audio.set_mute(m),
                        AudioCmd::List(reply) => {
                            let _ = reply.send(audio.outputs());
                        }
                        AudioCmd::SetDevice(id, reply) => {
                            let _ = reply.send(audio.set_default(&id));
                        }
                    }
                    dirty = true;
                }
                if last_device_check.elapsed() >= Duration::from_secs(3) {
                    last_device_check = Instant::now();
                    dirty |= audio.refresh();
                }
                if dirty || last_volume_poll.elapsed() >= Duration::from_millis(500) {
                    last_volume_poll = Instant::now();
                    publish_volume(&app, &shared, &audio);
                }
                let active = shared.media_playing.load(Ordering::Relaxed)
                    && !shared.suppressed.load(Ordering::Relaxed)
                    && lock(&shared.settings).music_reactive;
                spectrum.set_active(active);
                if active {
                    let p = audio.peak();
                    if (p - last_peak).abs() > 0.004 {
                        last_peak = p;
                        let _ = app.emit("audio", p);
                    }
                    // real bands for the equalizer bars (none: the bars follow the peak)
                    if let Some(b) = spectrum.bands() {
                        if b.iter().zip(&last_bands).any(|(x, y)| (x - y).abs() > 0.01) {
                            last_bands = b;
                            let _ = app.emit("spectrum", b);
                        }
                    }
                    thread::sleep(Duration::from_millis(33));
                } else {
                    if last_peak != 0.0 {
                        last_peak = 0.0;
                        let _ = app.emit("audio", 0.0f32);
                    }
                    thread::sleep(Duration::from_millis(130));
                }
            }
        })
        .expect("failed to start audio thread");
}

/// Clipboard history watcher.
fn spawn_clip_thread(app: AppHandle, shared: Arc<Shared>) {
    thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            let mut last = clip::sequence();
            loop {
                thread::sleep(Duration::from_millis(350));
                let seq = clip::sequence();
                if seq == last {
                    continue;
                }
                last = seq;
                if seq <= shared.clip_skip_seq.load(Ordering::Relaxed) || clip::should_ignore() {
                    continue;
                }
                // let the source app finish writing all of its formats
                thread::sleep(Duration::from_millis(60));
                let source = clip::owner_app()
                    .or_else(|| util::window_exe_path(input::foreground()).map(|p| util::exe_stem(&p)))
                    .unwrap_or_default();
                let Some(data) = clip::read() else { continue };
                let limit = lock(&shared.settings).clip_limit;
                let added = match data {
                    // hashing and PNG encoding happen outside the history lock
                    clip::ClipData::Image { width, height, rgba } => {
                        let hash = clips::image_hash(&rgba);
                        let known = lock(&shared.clips).bump_image(hash, width, height, &source);
                        known
                            || match clips::encode_image(&shared.paths.clip_dir(), width, height, &rgba, hash) {
                                Some(img) => lock(&shared.clips).add_image(img, source, limit),
                                None => false,
                            }
                    }
                    other => lock(&shared.clips).add(other, source, limit),
                };
                if !added {
                    continue;
                }
                let views = lock(&shared.clips).views();
                let _ = app.emit("clips", views);
            }
        })
        .expect("failed to start clipboard thread");
}

/// Forecast every 15 minutes (sooner when the city changes or on request).
fn spawn_weather_thread(app: AppHandle, shared: Arc<Shared>, rx: Receiver<()>) {
    thread::Builder::new()
        .name("weather".into())
        .spawn(move || {
            let mut ip_place: Option<weather::Place> = None;
            let mut wait = Duration::from_secs(3);
            loop {
                match rx.recv_timeout(wait) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                while rx.try_recv().is_ok() {}
                let s = lock(&shared.settings).clone();
                if !s.weather_enabled {
                    wait = Duration::from_secs(3600);
                    continue;
                }
                let result = tauri::async_runtime::block_on(async {
                    let (city, lat, lon) = if !s.weather_city.is_empty() {
                        (s.weather_city.clone(), s.weather_lat, s.weather_lon)
                    } else {
                        if ip_place.is_none() {
                            ip_place = Some(weather::locate().await?);
                        }
                        let p = ip_place.clone().unwrap();
                        (p.name, p.lat, p.lon)
                    };
                    let data = weather::forecast(lat, lon).await?;
                    Ok::<_, String>(weather::WeatherPayload {
                        city,
                        fetched_ms: store::now_ms(),
                        data,
                    })
                });
                match result {
                    Ok(w) => {
                        *lock(&shared.weather) = Some(w.clone());
                        let _ = app.emit("weather", w);
                        wait = Duration::from_secs(15 * 60);
                    }
                    Err(_) => wait = Duration::from_secs(120),
                }
            }
        })
        .expect("failed to start weather thread");
}

fn voice_state(app: &AppHandle, state: &str, started_ms: i64) {
    let _ = app.emit_to(
        EventTarget::webview_window("main"),
        "voice",
        VoiceState {
            state: state.into(),
            started_ms,
        },
    );
}

/// Pastes recognised text into the window the user was working in.
fn paste_into_last(shared: &Shared, text: &str) -> bool {
    if !clip::write_text(text) {
        return false;
    }
    shared.clip_skip_seq.store(clip::sequence(), Ordering::Relaxed);
    thread::sleep(Duration::from_millis(40));
    input::activate(shared.last_fg.load(Ordering::Relaxed));
    thread::sleep(Duration::from_millis(120));
    input::send_paste();
    true
}

/// Voice → text. The microphone stream lives on this thread (cpal streams are
/// not Send); Windows dictation (WinRT) needs MTA, which this thread is.
fn spawn_voice_thread(app: AppHandle, shared: Arc<Shared>, rx: Receiver<VoiceCmd>) {
    thread::Builder::new()
        .name("voice".into())
        .spawn(move || {
            let _com = util::Com::mta();
            const LIMIT: Duration = Duration::from_secs(120);
            for cmd in rx.iter() {
                if !matches!(cmd, VoiceCmd::Start | VoiceCmd::Toggle) {
                    continue;
                }
                let s = lock(&shared.settings).clone();
                if !s.voice_enabled {
                    continue;
                }
                let rec = match voice::Recorder::start() {
                    Ok(r) => r,
                    Err(e) => {
                        toast_error(&app, "Нет записи", &e);
                        continue;
                    }
                };
                let use_whisper = !s.voice_whisper_url.trim().is_empty();
                let mut dict = None;
                let mut dict_err = None;
                if !use_whisper {
                    match native::speech::Dictation::start() {
                        Ok(d) => dict = Some(d),
                        Err(e) => dict_err = Some(e.message().to_string()),
                    }
                }
                shared.voice_active.store(true, Ordering::Relaxed);
                let started = store::now_ms();
                let t0 = Instant::now();
                voice_state(&app, "recording", started);

                // record until ■ / × / hotkey again / time limit
                let mut outcome = VoiceCmd::Stop;
                loop {
                    match rx.recv_timeout(Duration::from_millis(40)) {
                        Ok(VoiceCmd::Stop) | Ok(VoiceCmd::Toggle) => break,
                        Ok(VoiceCmd::Cancel) => {
                            outcome = VoiceCmd::Cancel;
                            break;
                        }
                        Ok(VoiceCmd::Start) => {}
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    let _ = app.emit_to(EventTarget::webview_window("main"), "voice-level", rec.level());
                    if t0.elapsed() >= LIMIT {
                        break;
                    }
                }
                let samples = rec.finish();
                let _ = app.emit_to(EventTarget::webview_window("main"), "voice-level", 0.0f32);
                if outcome == VoiceCmd::Cancel {
                    if let Some(d) = dict {
                        d.cancel();
                    }
                    shared.voice_active.store(false, Ordering::Relaxed);
                    voice_state(&app, "idle", 0);
                    continue;
                }

                voice_state(&app, "processing", started);
                let text = if use_whisper {
                    let wav = voice::wav(&samples, voice::RATE);
                    tauri::async_runtime::block_on(voice::whisper(
                        &s.voice_whisper_url,
                        &s.voice_whisper_key,
                        &s.voice_whisper_model,
                        wav,
                    ))
                } else if let Some(d) = dict {
                    Ok(d.finish())
                } else {
                    Err(format!(
                        "Распознавание Windows недоступно ({}). Укажите адрес Whisper в настройках",
                        dict_err.unwrap_or_default()
                    ))
                };
                shared.voice_active.store(false, Ordering::Relaxed);
                voice_state(&app, "idle", 0);
                match text {
                    Ok(t) if !t.trim().is_empty() => {
                        let t = t.trim().to_string();
                        let n = t.chars().count();
                        if paste_into_last(&shared, &t) {
                            toast(
                                &app,
                                ToastPayload {
                                    icon: "mic".into(),
                                    title: format!("Вставлено · {n} симв."),
                                    subtitle: Some(t.chars().take(60).collect()),
                                    ms: Some(3000),
                                    ..Default::default()
                                },
                            );
                        } else {
                            toast_error(&app, "Не удалось вставить текст", "");
                        }
                    }
                    Ok(_) => toast_error(&app, "Ничего не расслышал", "Попробуйте ещё раз, ближе к микрофону"),
                    Err(e) => toast_error(&app, "Не удалось распознать", &e),
                }
            }
        })
        .expect("failed to start voice thread");
}

/// Browser downloads in the Downloads folder (notify wakes us, sizes are polled
/// while something is downloading).
fn spawn_downloads_thread(app: AppHandle, shared: Arc<Shared>) {
    thread::Builder::new()
        .name("downloads".into())
        .spawn(move || {
            use notify::{RecursiveMode, Watcher as _};
            let Some(dir) = capture::downloads_dir() else { return };
            let (tx, rx) = channel::<()>();
            let mut fs_watch = match notify::recommended_watcher(move |_res: notify::Result<notify::Event>| {
                let _ = tx.send(());
            }) {
                Ok(w) => w,
                Err(_) => return,
            };
            if fs_watch.watch(&dir, RecursiveMode::NonRecursive).is_err() {
                return;
            }
            let mut w = downloads::Watcher::new(dir);
            let mut last: Option<downloads::Progress> = None;
            let mut last_scan = Instant::now();
            loop {
                let wait = if w.active() { Duration::from_millis(700) } else { Duration::from_secs(30) };
                match rx.recv_timeout(wait) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                // a burst of fs events → one scan; a browser writing a file fires
                // events non-stop, so scans are also spaced out
                let pause = Duration::from_millis(600).saturating_sub(last_scan.elapsed()).max(Duration::from_millis(120));
                thread::sleep(pause);
                while rx.try_recv().is_ok() {}
                last_scan = Instant::now();
                if !lock(&shared.settings).downloads_enabled {
                    if last.take().is_some() {
                        let _ = app.emit("download", None::<downloads::Progress>);
                    }
                    continue;
                }
                let (progress, finished) = w.scan();
                if progress != last {
                    let _ = app.emit("download", progress.clone());
                    last = progress;
                }
                for f in finished {
                    let name = store::file_name(&f.path.to_string_lossy());
                    let path = f.path.to_string_lossy().to_string();
                    toast(
                        &app,
                        ToastPayload {
                            icon: "download".into(),
                            title: "Загружено".into(),
                            subtitle: Some(format!("{name} · {}", downloads_size(f.size))),
                            actions: vec![
                                ToastAction {
                                    label: "Открыть".into(),
                                    cmd: "open_target".into(),
                                    args: serde_json::json!({ "target": path, "count": false }),
                                },
                                ToastAction {
                                    label: "На полку".into(),
                                    cmd: "shelf_add".into(),
                                    args: serde_json::json!({ "paths": [path] }),
                                },
                            ],
                            ms: Some(7000),
                            ..Default::default()
                        },
                    );
                }
            }
        })
        .expect("failed to start downloads thread");
}

fn downloads_size(b: u64) -> String {
    let b = b as f64;
    let (v, u) = if b >= 1024f64.powi(3) {
        (b / 1024f64.powi(3), "ГБ")
    } else if b >= 1024f64.powi(2) {
        (b / 1024f64.powi(2), "МБ")
    } else if b >= 1024.0 {
        (b / 1024.0, "КБ")
    } else {
        (b, "Б")
    };
    let s = if v < 10.0 && u != "Б" { format!("{v:.1}") } else { format!("{v:.0}") };
    format!("{} {u}", s.replace('.', ","))
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InitPayload {
    settings: Settings,
    data: Data,
    clips: Vec<ClipView>,
    media: Option<MediaPayload>,
    cover: CoverPayload,
    volume: VolumePayload,
    version: String,
    hotkey_error: Option<String>,
    weather: Option<weather::WeatherPayload>,
}

#[tauri::command]
fn init(app: AppHandle, state: St<'_>) -> InitPayload {
    InitPayload {
        settings: lock(&state.settings).clone(),
        data: lock(&state.data).clone(),
        clips: lock(&state.clips).views(),
        media: lock(&state.media).clone(),
        cover: lock(&state.cover).clone(),
        volume: lock(&state.volume).clone(),
        version: app.package_info().version.to_string(),
        hotkey_error: lock(&state.hotkey_error).clone(),
        weather: lock(&state.weather).clone(),
    }
}

#[tauri::command]
fn set_hit(state: St<'_>, rect: Rect) {
    *lock(&state.hit) = rect;
}

#[tauri::command]
fn save_settings(app: AppHandle, state: St<'_>, settings: Settings) -> Result<Settings, String> {
    let old = lock(&state.settings).clone();
    let mut next = settings;
    next.clip_limit = next.clip_limit.clamp(10, 2000);
    if next.hotkey.trim().is_empty() {
        next.hotkey = old.hotkey.clone();
    }
    if next.hotkey_region.trim().is_empty() {
        next.hotkey_region = old.hotkey_region.clone();
    }
    if next.hotkey_ocr.trim().is_empty() {
        next.hotkey_ocr = old.hotkey_ocr.clone();
    }
    if next.hotkey_voice.trim().is_empty() {
        next.hotkey_voice = old.hotkey_voice.clone();
    }
    let failed_before = lock(&state.hotkey_error).is_some();
    let keys_changed = hotkey_bindings(&next) != hotkey_bindings(&old);
    if keys_changed || failed_before {
        let errors = apply_hotkeys(&app, &state, &next);
        if errors.is_empty() {
            *lock(&state.hotkey_error) = None;
        } else if keys_changed {
            let _ = apply_hotkeys(&app, &state, &old);
            return Err(format!("Не удалось назначить {}", errors.join(", ")));
        } else {
            // the same combos still clash with another app (e.g. PowerToys owns
            // Alt+Space): that must not block saving the city or a toggle
            *lock(&state.hotkey_error) = Some(errors.join(", "));
        }
    }
    if next.autostart != old.autostart {
        apply_autostart(&app, next.autostart);
    }
    if next.clip_limit != old.clip_limit {
        let views = {
            let mut c = lock(&state.clips);
            c.apply_limit(next.clip_limit);
            c.views()
        };
        let _ = app.emit("clips", views);
    }
    let weather_changed = next.weather_enabled != old.weather_enabled
        || next.weather_city != old.weather_city
        || next.weather_lat != old.weather_lat
        || next.weather_lon != old.weather_lon;
    store::save_json_synced(&state.paths.settings(), &next);
    *lock(&state.settings) = next.clone();
    if weather_changed {
        let _ = lock(&state.weather_wake).send(());
    }
    Ok(next)
}

#[tauri::command]
fn launcher_closed(state: St<'_>, restore: bool) {
    state.launcher_open.store(false, Ordering::Relaxed);
    if restore {
        input::activate(state.last_fg.load(Ordering::Relaxed));
    }
}

#[tauri::command]
fn open_launcher(app: AppHandle) {
    set_launcher(&app, true);
}

#[tauri::command]
fn media_control(state: St<'_>, action: String, value: Option<f64>) {
    let _ = lock(&state.media_tx).send(MediaCmd {
        action,
        value: value.unwrap_or(0.0),
    });
}

#[tauri::command]
fn volume_set(state: St<'_>, level: f32) {
    let _ = lock(&state.audio_tx).send(AudioCmd::Volume(level));
}

#[tauri::command]
fn volume_mute(state: St<'_>, muted: bool) {
    let _ = lock(&state.audio_tx).send(AudioCmd::Mute(muted));
}

/// Active output devices (for the "Динамики ⌄" menu).
#[tauri::command]
async fn audio_devices(state: St<'_>) -> Result<Vec<native::audio::OutputDevice>, String> {
    let (tx, rx) = channel();
    lock(&state.audio_tx).send(AudioCmd::List(tx)).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(3)).unwrap_or_default())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn audio_set_device(state: St<'_>, id: String) -> Result<bool, String> {
    let (tx, rx) = channel();
    lock(&state.audio_tx).send(AudioCmd::SetDevice(id, tx)).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(5)).unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_apps(state: St<'_>, refresh: bool) -> Result<Vec<AppItem>, String> {
    let shared = state.inner().clone();
    let cached = lock(&shared.apps).clone();
    if let (false, Some(apps)) = (refresh, cached) {
        return Ok(apps);
    }
    let worker = shared.clone();
    let apps = tauri::async_runtime::spawn_blocking(move || {
        worker
            .shell
            .run(|| shell::list_apps().unwrap_or_default())
            .unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())?;
    let items: Vec<AppItem> = apps
        .into_iter()
        .map(|a| AppItem {
            target: format!("app:{}", a.id),
            name: a.name,
        })
        .collect();
    *lock(&shared.apps) = Some(items.clone());
    Ok(items)
}

#[tauri::command]
async fn get_icons(
    state: St<'_>,
    keys: Vec<String>,
    size: Option<i32>,
    thumb: Option<bool>,
) -> Result<HashMap<String, Option<String>>, String> {
    let shared = state.inner().clone();
    let size = size.unwrap_or(64).clamp(16, 256);
    let thumb = thumb.unwrap_or(false);
    let ck = |k: &str| format!("{size}|{thumb}|{k}");
    let mut out = HashMap::new();
    let mut missing = Vec::new();
    {
        let cache = lock(&shared.icons);
        for k in keys {
            match cache.get(&ck(&k)) {
                Some(v) => {
                    out.insert(k, v.clone());
                }
                None => missing.push(k),
            }
        }
    }
    if missing.is_empty() {
        return Ok(out);
    }
    let worker = shared.clone();
    let fetched: Vec<(String, Option<String>)> = tauri::async_runtime::spawn_blocking(move || {
        worker
            .shell
            .run(move || {
                missing
                    .into_iter()
                    .map(|k| {
                        let url = shell::icon_rgba(&k, size, thumb)
                            .and_then(|(w, h, px)| img::encode_png(w, h, &px))
                            .map(|b| util::data_url("image/png", &b));
                        (k, url)
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut cache = lock(&shared.icons);
    for (k, v) in fetched {
        cache.insert(ck(&k), v.clone());
        out.insert(k, v);
    }
    Ok(out)
}

/// Opens an app (`app:<id>`), file, folder or URL.
#[tauri::command]
async fn open_target(state: St<'_>, target: String, count: Option<bool>) -> Result<bool, String> {
    let shared = state.inner().clone();
    if count.unwrap_or(true) {
        let mut d = lock(&shared.data);
        *d.usage.entry(target.clone()).or_insert(0) += 1;
        save_json(&shared.paths.data(), &*d);
    }
    let worker = shared.clone();
    tauri::async_runtime::spawn_blocking(move || worker.shell.run(move || shell::open(&target)).unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn reveal(state: St<'_>, path: String) -> Result<bool, String> {
    let worker = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || worker.shell.run(move || shell::reveal(&path)).unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pick_files(state: St<'_>, title: String) -> Result<Vec<String>, String> {
    let owner = state.own_hwnd.load(Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        thread::spawn(move || {
            let _com = util::Com::sta();
            shell::pick_files(owner, &title)
        })
        .join()
        .unwrap_or_default()
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn shelf_add(app: AppHandle, state: St<'_>, paths: Vec<String>) -> Vec<ShelfItem> {
    let mut d = lock(&state.data);
    for p in paths {
        if d.shelf.iter().any(|s| s.path == p) {
            continue;
        }
        if let Some(item) = store::shelf_item(&p) {
            d.shelf.insert(0, item);
        }
    }
    save_json(&state.paths.data(), &*d);
    let _ = app.emit("shelf", d.shelf.clone());
    d.shelf.clone()
}

#[tauri::command]
fn shelf_remove(state: St<'_>, id: String) -> Vec<ShelfItem> {
    let mut d = lock(&state.data);
    d.shelf.retain(|s| s.id != id);
    save_json(&state.paths.data(), &*d);
    d.shelf.clone()
}

#[tauri::command]
fn shelf_clear(state: St<'_>) -> Vec<ShelfItem> {
    let mut d = lock(&state.data);
    d.shelf.clear();
    save_json(&state.paths.data(), &*d);
    d.shelf.clone()
}

/// Puts shelf files on the clipboard as a file list.
#[tauri::command]
async fn shelf_copy(state: St<'_>, ids: Vec<String>) -> Result<bool, String> {
    let shared = state.inner().clone();
    let paths: Vec<String> = lock(&shared.data)
        .shelf
        .iter()
        .filter(|s| ids.contains(&s.id))
        .map(|s| s.path.clone())
        .collect();
    tauri::async_runtime::spawn_blocking(move || clip::write_files(&paths))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn pins_add(state: St<'_>, targets: Vec<String>) -> Vec<PinItem> {
    let mut d = lock(&state.data);
    for t in targets {
        if d.pins.iter().any(|p| p.target == t) {
            continue;
        }
        d.pins.push(store::pin_item(&t));
    }
    save_json(&state.paths.data(), &*d);
    d.pins.clone()
}

#[tauri::command]
fn pins_add_app(state: St<'_>, target: String, name: String) -> Vec<PinItem> {
    let mut d = lock(&state.data);
    if !d.pins.iter().any(|p| p.target == target) {
        d.pins.push(PinItem {
            id: store::new_id(),
            target,
            name,
        });
    }
    save_json(&state.paths.data(), &*d);
    d.pins.clone()
}

#[tauri::command]
fn pins_remove(state: St<'_>, id: String) -> Vec<PinItem> {
    let mut d = lock(&state.data);
    d.pins.retain(|p| p.id != id);
    save_json(&state.paths.data(), &*d);
    d.pins.clone()
}

fn write_entry(e: &ClipEntry, image: Option<PathBuf>) -> bool {
    match e.kind.as_str() {
        "image" => image
            .and_then(|p| fs::read(p).ok())
            .and_then(|png| img::decode_png(&png).map(|(w, h, px)| clip::write_image(w, h, &px, Some(&png))))
            .unwrap_or(false),
        "files" => clip::write_files(&e.files),
        _ => clip::write_text(&e.text),
    }
}

/// Copies a history entry back to the clipboard and (optionally) pastes it
/// into the window that was active before the island.
#[tauri::command]
async fn clip_paste(app: AppHandle, state: St<'_>, id: String, paste: bool) -> Result<bool, String> {
    let shared = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (entry, image) = {
            let store = lock(&shared.clips);
            match store.get(&id) {
                Some(e) => {
                    let img = store.image_path(&e);
                    (e, img)
                }
                None => return false,
            }
        };
        if !write_entry(&entry, image) {
            return false;
        }
        shared.clip_skip_seq.store(clip::sequence(), Ordering::Relaxed);
        let views = {
            let mut store = lock(&shared.clips);
            store.touch(&entry.id);
            store.views()
        };
        let _ = app.emit("clips", views);
        if paste {
            thread::sleep(Duration::from_millis(40));
            input::activate(shared.last_fg.load(Ordering::Relaxed));
            thread::sleep(Duration::from_millis(110));
            input::send_paste();
        }
        true
    })
    .await
    .map_err(|e| e.to_string())
}

/// "Чистый текст": strips formatting from the clipboard text and pastes it.
#[tauri::command]
async fn paste_plain(state: St<'_>) -> Result<bool, String> {
    let shared = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(clip::ClipData::Text(text)) = clip::read() else { return false };
        if !clip::write_text(&text) {
            return false;
        }
        shared.clip_skip_seq.store(clip::sequence(), Ordering::Relaxed);
        thread::sleep(Duration::from_millis(40));
        input::activate(shared.last_fg.load(Ordering::Relaxed));
        thread::sleep(Duration::from_millis(110));
        input::send_paste();
        true
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn copy_text(text: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || clip::write_text(&text))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn clip_delete(app: AppHandle, state: St<'_>, id: String) {
    let views = {
        let mut c = lock(&state.clips);
        c.remove(&id);
        c.views()
    };
    let _ = app.emit("clips", views);
}

#[tauri::command]
fn clip_pin(app: AppHandle, state: St<'_>, id: String, pinned: bool) {
    let views = {
        let mut c = lock(&state.clips);
        c.set_pinned(&id, pinned);
        c.views()
    };
    let _ = app.emit("clips", views);
}

#[tauri::command]
fn clip_clear(app: AppHandle, state: St<'_>) {
    let views = {
        let mut c = lock(&state.clips);
        c.clear();
        c.views()
    };
    let _ = app.emit("clips", views);
}

/// "Запись экрана": Xbox Game Bar records the active window / screen.
#[tauri::command]
async fn record_screen(state: St<'_>) -> Result<bool, String> {
    let shared = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Game Bar records the foreground app, so give focus back first.
        input::activate(shared.last_fg.load(Ordering::Relaxed));
        thread::sleep(Duration::from_millis(150));
        input::toggle_game_bar_recording();
        true
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn lock_screen() -> bool {
    input::lock_workstation()
}

/// An MP4 that was never finished has no index and won't play: close a running
/// recording properly before the process goes away.
fn finish_recording_before_exit(app: &AppHandle) {
    if let Some(shared) = app.try_state::<Arc<Shared>>() {
        let rec = lock(&shared.recording).take();
        if let Some(rec) = rec {
            let _ = rec.finish();
        }
    }
}

/// Front-end errors (window.onerror, unhandled rejections) go to the journal too.
#[tauri::command]
fn log_js(level: String, message: String) {
    let level = if level == "warn" { "WARN" } else { "ERROR" };
    let mut m: String = message.chars().take(2000).collect();
    m.insert_str(0, "js: ");
    log::write(level, &m);
}

#[tauri::command]
fn quit(app: AppHandle) {
    finish_recording_before_exit(&app);
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Screen capture overlay
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OverlayStart {
    id: u32,
    mode: String,
    width: u32,
    height: u32,
}

#[derive(Deserialize, Clone, Copy)]
struct PxRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

fn build_overlay(app: &AppHandle) -> tauri::Result<()> {
    let win = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("index.html#overlay".into()))
        .title("Island — захват")
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()?;
    if let Ok(h) = win.hwnd() {
        input::make_tool_window(h.0 as isize);
    }
    Ok(())
}

/// The island stays out of screenshots / recordings while a recording runs or the
/// glass look is on (glass reads the screen behind the island — it must not see itself).
fn keep_out_of_capture(shared: &Shared) -> bool {
    lock(&shared.recording).is_some() || lock(&shared.settings).theme == "glass"
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Backdrop {
    /// area in window (logical) pixels the image covers
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// served as http://backdrop.localhost/<id> (an uncompressed BMP)
    id: u32,
}

/// Glass look: a small, downscaled copy of the screen behind the island, a few
/// times a second; the canvas draws it blurred inside the liquid shape.
fn spawn_backdrop_thread(app: AppHandle, shared: Arc<Shared>) {
    thread::Builder::new()
        .name("backdrop".into())
        .spawn(move || {
            let mut excluded = false;
            let mut last_hash = 0u64;
            loop {
                let glass = lock(&shared.settings).theme == "glass";
                let own = shared.own_hwnd.load(Ordering::Relaxed);
                let want_excluded = glass || lock(&shared.recording).is_some();
                if want_excluded != excluded && own != 0 {
                    capture::exclude_from_capture(own, want_excluded);
                    excluded = want_excluded;
                }
                if !glass || shared.suppressed.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(400));
                    continue;
                }
                let hit = *lock(&shared.hit);
                // the collapsed island is small and barely moves: fewer snapshots
                let pause = if hit.h > 60.0 { 140 } else { 320 };
                thread::sleep(Duration::from_millis(pause));
                let geom = *lock(&shared.geom);
                let hit = *lock(&shared.hit);
                if geom.scale <= 0.0 || hit.w <= 0.0 {
                    continue;
                }
                // a margin around the island: the spring animation overshoots it
                let x = (hit.x - 40.0).max(0.0);
                let y = 0.0;
                let w = (hit.w + 80.0).min(WIN_W - x);
                let h = (hit.h + 70.0).min(WIN_H);
                let (px, py) = (geom.x + (x * geom.scale) as i32, geom.y + (y * geom.scale) as i32);
                let (pw, ph) = ((w * geom.scale) as u32, (h * geom.scale) as u32);
                if pw < 4 || ph < 4 {
                    continue;
                }
                // it gets blurred anyway: a small image is plenty; GDI shrinks it
                // while copying, and it goes out as a BMP (no PNG encoding, no base64)
                let Some((sw, sh, small)) = capture::grab_scaled(px, py, pw, ph, 240) else { continue };
                let hash = {
                    use std::hash::{Hash, Hasher};
                    let mut hs = std::collections::hash_map::DefaultHasher::new();
                    small.hash(&mut hs);
                    (x as i64, w as i64, h as i64).hash(&mut hs);
                    hs.finish()
                };
                if hash == last_hash {
                    continue;
                }
                last_hash = hash;
                let id = shared.backdrop_seq.fetch_add(1, Ordering::Relaxed) + 1;
                *lock(&shared.backdrop) = Some((id, Arc::new(capture::bmp(&small, sw, sh))));
                let _ = app.emit_to(
                    EventTarget::webview_window("main"),
                    "backdrop",
                    Backdrop {
                        x,
                        y,
                        w,
                        h,
                        id,
                    },
                );
            }
        })
        .expect("failed to start backdrop thread");
}

/// Takes a snapshot of the monitor under the cursor (without our own windows).
fn snapshot(shared: &Shared, mode: &str) -> Option<Shot> {
    let own = shared.own_hwnd.load(Ordering::Relaxed);
    let (cx, cy) = input::cursor_pos();
    let (x, y, w, h) = capture::monitor_at(cx, cy);
    capture::exclude_from_capture(own, true);
    thread::sleep(Duration::from_millis(60));
    let px = capture::grab(x, y, w, h);
    // a recording / the glass look keep the island out of captures
    capture::exclude_from_capture(own, keep_out_of_capture(shared));
    let bgra = Arc::new(px?);
    let bmp = Arc::new(capture::bmp(&bgra, w, h));
    Some(Shot {
        id: shared.shot_seq.fetch_add(1, Ordering::Relaxed) + 1,
        mode: mode.to_string(),
        x,
        y,
        w,
        h,
        bgra,
        bmp,
    })
}

fn end_overlay(app: &AppHandle, shared: &Shared) {
    if let Some(ov) = app.get_webview_window("overlay") {
        let _ = ov.emit_to(EventTarget::webview_window("overlay"), "overlay-end", ());
        let _ = ov.hide();
    }
    *lock(&shared.shot) = None;
    shared.capturing.store(false, Ordering::Relaxed);
}

/// Region / full screen / text / QR / colour picker. Runs off the main thread.
fn begin_capture(app: AppHandle, mode: String) {
    let shared = app.state::<Arc<Shared>>().inner().clone();
    if !lock(&shared.settings).capture_enabled || shared.capturing.swap(true, Ordering::Relaxed) {
        return;
    }
    thread::spawn(move || {
        let Some(shot) = snapshot(&shared, &mode) else {
            shared.capturing.store(false, Ordering::Relaxed);
            toast_error(&app, "Не удалось снять экран", "");
            return;
        };
        if mode == "full" {
            let (w, h) = (shot.w, shot.h);
            let bgra = shot.bgra.clone();
            shared.capturing.store(false, Ordering::Relaxed);
            finish_image(&app, &shared, &bgra, w, h);
            return;
        }
        let start = OverlayStart {
            id: shot.id,
            mode: mode.clone(),
            width: shot.w,
            height: shot.h,
        };
        let (x, y, w, h) = (shot.x, shot.y, shot.w, shot.h);
        *lock(&shared.shot) = Some(shot);
        let Some(ov) = app.get_webview_window("overlay") else {
            shared.capturing.store(false, Ordering::Relaxed);
            return;
        };
        let _ = ov.set_position(PhysicalPosition::new(x, y));
        let _ = ov.set_size(PhysicalSize::new(w, h));
        let id = start.id;
        let _ = app.emit_to(EventTarget::webview_window("overlay"), "overlay-start", start);
        // the overlay page never answered: don't stay stuck in "capturing"
        thread::sleep(Duration::from_secs(4));
        let stuck = lock(&shared.shot).as_ref().map(|s| s.id) == Some(id) && !ov.is_visible().unwrap_or(false);
        if stuck {
            end_overlay(&app, &shared);
            toast_error(&app, "Оверлей не открылся", "Попробуйте ещё раз");
        }
    });
}

/// Screenshot → clipboard + Pictures\Screenshots\Island-*.png + toast with a thumbnail.
fn finish_image(app: &AppHandle, shared: &Shared, bgra: &[u8], w: u32, h: u32) {
    let rgba = capture::bgra_to_rgba(bgra);
    let Some(png) = img::encode_png(w, h, &rgba) else {
        toast_error(app, "Не удалось сохранить скриншот", "");
        return;
    };
    let copied = clip::write_image(w, h, &rgba, Some(&png));
    if copied {
        shared.clip_skip_seq.store(clip::sequence(), Ordering::Relaxed);
    }
    let path = capture::screenshots_dir().map(|d| capture::unique_file(&d, "png"));
    let saved = path.as_ref().map(|p| fs::write(p, &png).is_ok()).unwrap_or(false);
    let (tw, th, thumb) = img::shrink(w, h, &rgba, 120);
    let image = img::encode_png(tw, th, &thumb).map(|b| util::data_url("image/png", &b));
    let mut actions = Vec::new();
    if let (true, Some(p)) = (saved, &path) {
        actions.push(ToastAction {
            label: "Показать".into(),
            cmd: "reveal".into(),
            args: serde_json::json!({ "path": p.to_string_lossy() }),
        });
    }
    toast(
        app,
        ToastPayload {
            icon: "image".into(),
            title: "Скриншот готов".into(),
            subtitle: Some(match (copied, saved) {
                (true, true) => format!("В буфере · {w} × {h}"),
                (true, false) => "В буфере · файл не сохранился".into(),
                (false, true) => "Сохранён в «Снимки экрана»".into(),
                (false, false) => "Не удалось сохранить".into(),
            }),
            image,
            actions,
            ms: Some(4200),
            ..Default::default()
        },
    );
}

fn finish_ocr(app: &AppHandle, shared: &Shared, bgra: Vec<u8>, w: u32, h: u32) {
    // WinRT OCR wants an MTA thread
    let text = thread::spawn(move || {
        let _com = util::Com::mta();
        capture::ocr(&bgra, w, h)
    })
    .join()
    .ok()
    .and_then(|r| r.ok())
    .unwrap_or_default();
    let text = text.trim().to_string();
    if text.is_empty() {
        toast_error(app, "Текст не найден", "Попробуйте выделить крупнее");
        return;
    }
    let n = text.chars().count();
    if clip::write_text(&text) {
        let _ = shared;
        toast(
            app,
            ToastPayload {
                icon: "text".into(),
                title: format!("Текст распознан · {n} симв."),
                subtitle: Some(text.lines().next().unwrap_or("").chars().take(60).collect()),
                ms: Some(3200),
                ..Default::default()
            },
        );
    } else {
        toast_error(app, "Не удалось скопировать", "");
    }
}

fn finish_qr(app: &AppHandle, bgra: &[u8], w: u32, h: u32) {
    let Some(content) = capture::qr(bgra, w, h) else {
        toast_error(app, "QR-код не найден", "Выделите код целиком");
        return;
    };
    let content = content.trim().to_string();
    let is_url = content.starts_with("http://") || content.starts_with("https://");
    let _ = clip::write_text(&content);
    let short: String = content.chars().take(60).collect();
    toast(
        app,
        ToastPayload {
            icon: "qr".into(),
            title: if is_url { "Ссылка из QR-кода".into() } else { "QR-код скопирован".into() },
            subtitle: Some(short),
            actions: if is_url {
                vec![ToastAction {
                    label: "Открыть".into(),
                    cmd: "open_target".into(),
                    args: serde_json::json!({ "target": content, "count": false }),
                }]
            } else {
                Vec::new()
            },
            ms: Some(if is_url { 6000 } else { 3200 }),
            ..Default::default()
        },
    );
}

fn recording_state(app: &AppHandle, active: bool, started_ms: i64) {
    let _ = app.emit_to(EventTarget::webview_window("main"), "recording", RecordingState { active, started_ms });
}

/// Starts an MP4 recording of the snapshot's monitor (or a part of it).
fn start_recording(app: &AppHandle, shared: &Arc<Shared>, shot: &Shot, rect: Option<(u32, u32, u32, u32)>) {
    if lock(&shared.recording).is_some() {
        toast_error(app, "Запись уже идёт", "Остановите её кликом по острову");
        return;
    }
    let Some(dir) = capture::videos_dir() else {
        toast_error(app, "Нет папки «Видео»", "");
        return;
    };
    let path = capture::unique_file(&dir, "mp4");
    let hmon = capture::monitor_handle_at(shot.x + shot.w as i32 / 2, shot.y + shot.h as i32 / 2);
    // the island (red dot and timer) stays out of the video
    capture::exclude_from_capture(shared.own_hwnd.load(Ordering::Relaxed), true);
    match record::start(hmon, shot.w, shot.h, rect, path) {
        Ok(rec) => {
            *lock(&shared.recording) = Some(rec);
            recording_state(app, true, store::now_ms());
        }
        Err(e) => {
            capture::exclude_from_capture(shared.own_hwnd.load(Ordering::Relaxed), keep_out_of_capture(shared));
            toast_error(app, "Не удалось начать запись", &e);
        }
    }
}

/// Stop button / click on the island while recording.
#[tauri::command]
async fn record_stop(app: AppHandle, state: St<'_>) -> Result<(), String> {
    let shared = state.inner().clone();
    let Some(rec) = lock(&shared.recording).take() else { return Ok(()) };
    recording_state(&app, false, 0);
    let result = tauri::async_runtime::spawn_blocking(move || rec.finish())
        .await
        .map_err(|e| e.to_string())?;
    capture::exclude_from_capture(shared.own_hwnd.load(Ordering::Relaxed), keep_out_of_capture(&shared));
    match result {
        Ok(path) => {
            let name = store::file_name(&path.to_string_lossy());
            toast(
                &app,
                ToastPayload {
                    icon: "video".into(),
                    title: "Запись сохранена".into(),
                    subtitle: Some(format!("{name} · Видео\\Island")),
                    actions: vec![ToastAction {
                        label: "Показать в папке".into(),
                        cmd: "reveal".into(),
                        args: serde_json::json!({ "path": path.to_string_lossy() }),
                    }],
                    ms: Some(6000),
                    ..Default::default()
                },
            );
        }
        Err(e) => toast_error(&app, "Запись не сохранилась", &e),
    }
    Ok(())
}

#[tauri::command]
fn capture(app: AppHandle, mode: String) {
    begin_capture(app, mode);
}

/// The overlay has the snapshot on screen: show it and take focus (for Esc).
#[tauri::command]
fn overlay_show(app: AppHandle, state: St<'_>, id: u32) {
    if lock(&state.shot).as_ref().map(|s| s.id) != Some(id) {
        return;
    }
    if let Some(ov) = app.get_webview_window("overlay") {
        let _ = ov.show();
        let _ = ov.set_focus();
    }
}

#[tauri::command]
fn capture_cancel(app: AppHandle, state: St<'_>) {
    end_overlay(&app, &state);
    input::activate(state.last_fg.load(Ordering::Relaxed));
}

/// `rect` is in snapshot pixels; `color` comes from the picker.
#[tauri::command]
fn capture_finish(app: AppHandle, state: St<'_>, id: u32, rect: Option<PxRect>, color: Option<String>) {
    let shot = lock(&state.shot).take();
    end_overlay(&app, &state);
    input::activate(state.last_fg.load(Ordering::Relaxed));
    let Some(shot) = shot.filter(|s| s.id == id) else { return };
    let shared = state.inner().clone();
    thread::spawn(move || {
        if shot.mode == "picker" {
            let Some(hex) = color else { return };
            let _ = clip::write_text(&hex);
            toast(
                &app,
                ToastPayload {
                    icon: "color".into(),
                    title: hex.clone(),
                    subtitle: Some("Цвет скопирован".into()),
                    swatch: Some(hex),
                    ms: Some(3000),
                    ..Default::default()
                },
            );
            return;
        }
        if shot.mode == "record" {
            let rect = rect.map(|r| {
                let x = r.x.max(0.0) as u32;
                let y = r.y.max(0.0) as u32;
                (x.min(shot.w - 2), y.min(shot.h - 2), r.w.max(2.0) as u32, r.h.max(2.0) as u32)
            });
            start_recording(&app, &shared, &shot, rect);
            return;
        }
        // whole screen when nothing was selected (QR: a click scans everything)
        let r = rect.unwrap_or(PxRect { x: 0.0, y: 0.0, w: shot.w as f64, h: shot.h as f64 });
        let x = r.x.max(0.0).min(shot.w as f64 - 1.0) as u32;
        let y = r.y.max(0.0).min(shot.h as f64 - 1.0) as u32;
        let w = (r.w.round() as u32).clamp(1, shot.w - x);
        let h = (r.h.round() as u32).clamp(1, shot.h - y);
        let px = capture::crop(&shot.bgra, shot.w, x, y, w, h);
        match shot.mode.as_str() {
            "ocr" => finish_ocr(&app, &shared, px, w, h),
            "qr" => finish_qr(&app, &px, w, h),
            _ => finish_image(&app, &shared, &px, w, h),
        }
    });
}

// ---------------------------------------------------------------------------
// AI chat
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AiChunk {
    id: String,
    delta: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AiDone {
    id: String,
    error: Option<String>,
}

#[tauri::command]
async fn ai_models(state: St<'_>, url: Option<String>, key: Option<String>) -> Result<Vec<String>, String> {
    let (u, k) = {
        let s = lock(&state.settings);
        (url.unwrap_or_else(|| s.ai_url.clone()), key.unwrap_or_else(|| s.ai_key.clone()))
    };
    ai::models(&u, &k).await
}

#[tauri::command]
async fn ai_attach(path: String) -> Result<ai::Attachment, String> {
    tauri::async_runtime::spawn_blocking(move || ai::attach(&path))
        .await
        .map_err(|e| e.to_string())?
}

/// Streams a chat completion: "ai-chunk" for every piece, then "ai-done".
#[tauri::command]
fn ai_send(app: AppHandle, state: St<'_>, id: String, messages: serde_json::Value) {
    let shared = state.inner().clone();
    let gen = shared.ai_gen.fetch_add(1, Ordering::Relaxed) + 1;
    let (base, key, model) = {
        let s = lock(&shared.settings);
        (s.ai_url.clone(), s.ai_key.clone(), s.ai_model.clone())
    };
    let task_id = id.clone();
    let task_app = app.clone();
    let task_shared = shared.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let app = task_app;
        let shared = task_shared;
        let done = |error: Option<String>| {
            let _ = app.emit("ai-done", AiDone { id: id.clone(), error });
        };
        let mut body = serde_json::json!({ "messages": messages, "stream": true, "temperature": 0.7 });
        if !model.is_empty() {
            body["model"] = serde_json::Value::String(model);
        }
        let mut req = ai::client().post(ai::endpoint(&base, "chat/completions")).json(&body);
        if !key.is_empty() {
            req = req.bearer_auth(&key);
        }
        let mut resp = match req.send().await {
            Ok(r) => r,
            Err(e) => return done(Some(ai::explain(&e, &base))),
        };
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().or(v["error"].as_str()).map(String::from))
                .unwrap_or_else(|| text.chars().take(200).collect());
            return done(Some(format!("Сервер ответил {status}: {msg}")));
        }
        // bytes, not text: a network chunk can end in the middle of a Cyrillic
        // letter, and decoding each chunk on its own would turn it into "��"
        let mut buf: Vec<u8> = Vec::new();
        loop {
            if shared.ai_gen.load(Ordering::Relaxed) != gen {
                return done(None);
            }
            let chunk = match resp.chunk().await {
                Ok(Some(c)) => c,
                Ok(None) => break,
                Err(e) => return done(Some(ai::explain(&e, &base))),
            };
            buf.extend_from_slice(&chunk);
            while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                let raw: Vec<u8> = buf.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&raw);
                match ai::parse_sse_line(&line) {
                    ai::Sse::Delta(d) => {
                        let _ = app.emit("ai-chunk", AiChunk { id: id.clone(), delta: d });
                    }
                    ai::Sse::Done => return done(None),
                    ai::Sse::Skip => {}
                }
            }
        }
        if let ai::Sse::Delta(d) = ai::parse_sse_line(&String::from_utf8_lossy(&buf)) {
            let _ = app.emit("ai-chunk", AiChunk { id: id.clone(), delta: d });
        }
        done(None)
    });
    // a new question replaces a reply still in flight
    let prev = lock(&shared.ai_task).replace((task_id, handle));
    if let Some((prev_id, h)) = prev {
        h.abort();
        let _ = app.emit("ai-done", AiDone { id: prev_id, error: None });
    }
}

#[tauri::command]
/// `id` = the reply to stop; a Stop that arrives after the next question was
/// already sent must not cancel that new reply.
fn ai_stop(app: AppHandle, state: St<'_>, id: Option<String>) {
    let task = {
        let mut slot = lock(&state.ai_task);
        let matches = match (&id, slot.as_ref()) {
            (Some(want), Some((cur, _))) => want == cur,
            (None, Some(_)) => true,
            _ => false,
        };
        if matches {
            slot.take()
        } else {
            None
        }
    };
    if let Some((id, h)) = task {
        state.ai_gen.fetch_add(1, Ordering::Relaxed);
        h.abort();
        let _ = app.emit("ai-done", AiDone { id, error: None });
    }
}

#[tauri::command]
async fn weather_search(q: String) -> Result<Vec<weather::Place>, String> {
    weather::search(q.trim()).await
}

#[tauri::command]
async fn weather_locate() -> Result<weather::Place, String> {
    weather::locate().await
}

#[tauri::command]
fn weather_refresh(state: St<'_>) {
    let _ = lock(&state.weather_wake).send(());
}

// ---------------------------------------------------------------------------
// Yandex Music: like / dislike
// ---------------------------------------------------------------------------

#[tauri::command]
async fn ym_state(state: St<'_>, artist: String, title: String, duration_ms: Option<i64>) -> Result<ym::TrackState, String> {
    let shared = state.inner().clone();
    let token = lock(&shared.settings).ym_token.clone();
    Ok(ym::state(&shared.ym, &token, &artist, &title, duration_ms.unwrap_or(0)).await)
}

/// kind: "like" | "dislike". A dislike also skips to the next track, like the app does.
#[tauri::command]
async fn ym_set(state: St<'_>, track_id: String, kind: String, on: bool) -> Result<(), String> {
    let shared = state.inner().clone();
    let token = lock(&shared.settings).ym_token.clone();
    if token.is_empty() {
        return Err("Войдите в Яндекс Музыку в настройках".into());
    }
    let list = if kind == "dislike" { "dislikes" } else { "likes" };
    ym::set(&shared.ym, &token, &track_id, list, on).await?;
    if kind == "dislike" && on {
        let _ = lock(&shared.media_tx).send(MediaCmd {
            action: "next".into(),
            value: 0.0,
        });
    }
    Ok(())
}

/// Lyrics from Yandex Music (first source when the account is connected).
#[tauri::command]
async fn ym_lyrics(state: St<'_>, artist: String, title: String, duration_ms: Option<i64>) -> Result<Option<ym::Lyrics>, String> {
    let shared = state.inner().clone();
    let token = lock(&shared.settings).ym_token.clone();
    ym::lyrics(&shared.ym, &token, &artist, &title, duration_ms.unwrap_or(0)).await
}


// ---------------------------------------------------------------------------
// «Без цензуры»: Yandex Music picks, YouTube Music plays (hidden window)
// ---------------------------------------------------------------------------

const YM: &[&str] = &["yandex"];
/// how the island's own WebView2 media session shows up in Windows
const OWN_PLAYER: &str = "island|webview";

/// Injected into the YouTube window: reports the end of a track by "navigating"
/// to a sentinel address (cancelled in on_navigation), since external pages
/// have no IPC.
const YT_SCRIPT: &str = r#"(function(){
  if (!/youtube\.com$/.test(location.hostname)) return;
  var sent = '';
  function hook(){
    var v = document.querySelector('video');
    if (!v || v.__island) return;
    v.__island = 1;
    var done = function(){
      var id = location.href;
      if (sent === id) return;
      sent = id;
      location.href = 'https://island.invalid/ended';
    };
    v.addEventListener('ended', done);
    v.addEventListener('timeupdate', function(){
      if (v.duration > 20 && v.currentTime > v.duration - 0.7) done();
    });
  }
  new MutationObserver(hook).observe(document.documentElement, {childList:true, subtree:true});
  hook();
})();"#;

fn yt_eval(app: &AppHandle, js: &str) {
    if let Some(w) = app.get_webview_window("yt") {
        let _ = w.eval(js);
    }
}

/// Island buttons while YouTube plays: play/pause/seek go to the YouTube page,
/// next/previous move Yandex Music's queue (which then hands the track over again).
fn route_media(app: &AppHandle, shared: &Shared, media: &Media, cmd: &MediaCmd) {
    if cmd.action == "dump" {
        let ok = clip::write_text(&media.dump(|id| media_app_name(shared, id)));
        toast(
            app,
            ToastPayload {
                icon: "copy".into(),
                title: if ok { "Медиасессии скопированы".into() } else { "Не удалось скопировать".into() },
                ..Default::default()
            },
        );
        return;
    }
    if let Some(a) = cmd.action.strip_prefix("ym:") {
        let _ = media.control_of(YM, a, cmd.value);
        return;
    }
    if shared.yt_active.load(Ordering::Relaxed) && app.get_webview_window("yt").is_some() {
        match cmd.action.as_str() {
            "toggle" => yt_eval(app, "(function(){var v=document.querySelector('video');if(v){v.paused?v.play():v.pause()}})()"),
            "pause" => yt_eval(app, "(function(){var v=document.querySelector('video');if(v)v.pause()})()"),
            "play" => yt_eval(app, "(function(){var v=document.querySelector('video');if(v)v.play()})()"),
            "seek" => yt_eval(
                app,
                &format!("(function(){{var v=document.querySelector('video');if(v)v.currentTime={}}})()", cmd.value / 1000.0),
            ),
            "next" | "prev" => {
                let _ = media.control_of(YM, &cmd.action, 0.0);
            }
            _ => {}
        }
        return;
    }
    let _ = media.control(&cmd.action, cmd.value);
}

/// Opens (or reuses) the YouTube player window. Own WebView2 profile: the user
/// signs in to YouTube there once; autoplay is allowed.
async fn yt_open(app: &AppHandle, shared: &Shared, url: &str, visible: bool) -> Result<(), String> {
    let target: tauri::Url = url.parse().map_err(|_| "bad url".to_string())?;
    if let Some(w) = app.get_webview_window("yt") {
        w.navigate(target).map_err(|e| e.to_string())?;
        if visible {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        return Ok(());
    }
    let dir = shared.paths.dir.join("youtube-webview");
    let handle = app.clone();
    WebviewWindowBuilder::new(app, "yt", WebviewUrl::External(target))
        .title("Island — без цензуры")
        .inner_size(980.0, 640.0)
        .center()
        .visible(visible)
        .skip_taskbar(!visible)
        .data_directory(dir)
        .additional_browser_args("--autoplay-policy=no-user-gesture-required")
        .initialization_script(YT_SCRIPT)
        .on_navigation(move |u| {
            if u.host_str() == Some("island.invalid") {
                // the YouTube track ended: next one comes from Yandex Music's queue
                let s = handle.state::<Arc<Shared>>();
                let _ = lock(&s.media_tx).send(MediaCmd {
                    action: "ym:next".into(),
                    value: 0.0,
                });
                return false;
            }
            true
        })
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Finds the Yandex track on YouTube and plays it in the hidden window.
async fn yt_switch(app: AppHandle, shared: Arc<Shared>, artist: String, title: String, duration_ms: i64, track: String) {
    let key = lock(&shared.settings).yt_key.trim().to_string();
    let found = yt::find(&key, &artist, &title, duration_ms).await;
    match found {
        Ok(Some(id)) => {
            if let Err(e) = yt_open(&app, &shared, &yt::watch_url(&id), false).await {
                toast_error(&app, "YouTube не открылся", &e);
            }
        }
        other => {
            // not found / no network: this track plays in Yandex Music as is
            *lock(&shared.yt_failed) = track;
            yt_eval(&app, "(function(){var v=document.querySelector('video');if(v)v.pause()})()");
            let _ = lock(&shared.media_tx).send(MediaCmd {
                action: "ym:play".into(),
                value: 0.0,
            });
            let why = match other {
                Err(e) => e,
                _ => "Трек не найден на YouTube".into(),
            };
            toast_error(&app, "Играет обычная версия", &why);
        }
    }
}

/// Called from the media thread on every poll.
fn auto_uncensored(app: &AppHandle, shared: &Arc<Shared>, media: &Media, auto_key: &mut String) {
    let (on, has_key) = {
        let s = lock(&shared.settings);
        (s.yt_auto, !s.yt_key.trim().is_empty())
    };
    if !(on && has_key) {
        if shared.yt_active.swap(false, Ordering::Relaxed) {
            // switched off: close the hidden player, Yandex Music continues
            media.set_prefer(None);
            auto_key.clear();
            if let Some(w) = app.get_webview_window("yt") {
                let _ = w.close();
            }
            let _ = media.control_of(YM, "play", 0.0);
        }
        return;
    }
    let Some(y) = media.state_of(YM) else { return };
    if !y.playing {
        return;
    }
    let track = format!("{}\u{1}{}", y.artist, y.title);
    if *lock(&shared.yt_failed) == track {
        return; // not on YouTube: let Yandex play this one
    }
    let _ = media.control_of(YM, "pause", 0.0);
    shared.yt_active.store(true, Ordering::Relaxed);
    media.set_prefer(Some(OWN_PLAYER));
    if *auto_key == track {
        // the user pressed play in Yandex Music: resume YouTube instead
        yt_eval(app, "(function(){var v=document.querySelector('video');if(v)v.play()})()");
        return;
    }
    *auto_key = track.clone();
    tauri::async_runtime::spawn(yt_switch(app.clone(), shared.clone(), y.artist, y.title, y.duration_ms, track));
}

/// Shows the YouTube player window (sign in once; Premium = no ads).
#[tauri::command]
async fn yt_show(app: AppHandle, state: St<'_>) -> Result<(), String> {
    let shared = state.inner().clone();
    if let Some(w) = app.get_webview_window("yt") {
        let _ = w.set_skip_taskbar(false);
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    yt_open(&app, &shared, "https://music.youtube.com/", true).await
}

/// «Без цензуры»: pauses the current player and plays the same track on
/// YouTube Music in a player window (its own WebView2 profile: the user signs in
/// to YouTube once, autoplay is allowed there).
#[tauri::command]
async fn uncensored(app: AppHandle, state: St<'_>, artist: String, title: String, duration_ms: Option<i64>) -> Result<bool, String> {
    let shared = state.inner().clone();
    let key = lock(&shared.settings).yt_key.clone();
    let found = if key.trim().is_empty() {
        None
    } else {
        yt::find(key.trim(), &artist, &title, duration_ms.unwrap_or(0)).await?
    };
    let url = match &found {
        Some(id) => yt::watch_url(id),
        None => yt::search_url(&artist, &title),
    };
    let _ = lock(&shared.media_tx).send(MediaCmd {
        action: "pause".into(),
        value: 0.0,
    });
    yt_open(&app, &shared, &url, true).await?;
    Ok(found.is_some())
}

fn save_ym_token(app: &AppHandle, token: String) {
    let shared = app.state::<Arc<Shared>>().inner().clone();
    let next = {
        let mut s = lock(&shared.settings);
        s.ym_token = token;
        s.clone()
    };
    save_json(&shared.paths.settings(), &next);
    let _ = app.emit("settings", next);
}

fn ym_connected(h: &AppHandle) {
    if let Some(w) = h.get_webview_window("ymlogin") {
        let _ = w.close();
    }
    toast(
        h,
        ToastPayload {
            icon: "check".into(),
            title: "Яндекс Музыка подключена".into(),
            subtitle: Some("Лайки и дизлайки — на главной".into()),
            ms: Some(3000),
            ..Default::default()
        },
    );
}

/// Opens the Yandex login page; the token is taken from the redirect URL.
/// Async on purpose: a window built inside a synchronous command (main thread)
/// stays blank on Windows — WebView2 can't finish while that thread is busy.
#[tauri::command]
async fn ym_login(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("ymlogin") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let url: tauri::Url = ym::LOGIN_URL.parse().map_err(|_| "bad login url".to_string())?;
    let done = Arc::new(AtomicBool::new(false));
    let (handle, flag) = (app.clone(), done.clone());
    let win = WebviewWindowBuilder::new(&app, "ymlogin", WebviewUrl::External(url))
        .title("Вход в Яндекс Музыку")
        .inner_size(480.0, 700.0)
        .center()
        .focused(true)
        .on_navigation(move |u| {
            if let Some(token) = ym::token_from_url(u.as_str()) {
                if !flag.swap(true, Ordering::Relaxed) {
                    save_ym_token(&handle, token);
                    let h = handle.clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(150));
                        ym_connected(&h);
                    });
                }
                return false;
            }
            true
        })
        .build()
        .map_err(|e| e.to_string())?;
    // fallback: the token can also arrive as a fragment change, which is not a
    // navigation — watch the address of the page while the window is open
    let h = app.clone();
    thread::spawn(move || {
        for _ in 0..1200 {
            thread::sleep(Duration::from_millis(500));
            if done.load(Ordering::Relaxed) {
                return;
            }
            let Ok(u) = win.url() else { return };
            if let Some(token) = ym::token_from_url(u.as_str()) {
                if !done.swap(true, Ordering::Relaxed) {
                    save_ym_token(&h, token);
                    ym_connected(&h);
                }
                return;
            }
        }
    });
    Ok(())
}

/// Manual fallback: the user pastes the token (or the whole address with it).
#[tauri::command]
fn ym_set_token(app: AppHandle, value: String) -> Result<(), String> {
    let v = value.trim();
    let token = ym::token_from_url(v).unwrap_or_else(|| v.to_string());
    if token.len() < 20 || token.contains(char::is_whitespace) {
        return Err("Это не похоже на токен".into());
    }
    save_ym_token(&app, token);
    Ok(())
}

#[tauri::command]
fn ym_logout(app: AppHandle) {
    save_ym_token(&app, String::new());
}

#[tauri::command]
fn voice(state: St<'_>, action: String) {
    let cmd = match action.as_str() {
        "start" => VoiceCmd::Start,
        "stop" => VoiceCmd::Stop,
        "cancel" => VoiceCmd::Cancel,
        _ => VoiceCmd::Toggle,
    };
    let _ = lock(&state.voice_tx).send(cmd);
}

/// Current clipboard text (for the «Текст» tab).
#[tauri::command]
async fn clip_read_text() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| match clip::read() {
        Some(clip::ClipData::Text(t)) => Some(t),
        _ => None,
    })
    .await
    .map_err(|e| e.to_string())
}

/// Puts text on the clipboard and pastes it into the window the user worked in.
#[tauri::command]
async fn paste_text(state: St<'_>, text: String) -> Result<bool, String> {
    let shared = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || paste_into_last(&shared, &text))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn notes_load(state: St<'_>) -> serde_json::Value {
    let v: serde_json::Value = load_json(&state.paths.notes());
    if v.is_array() {
        v
    } else {
        serde_json::Value::Array(Vec::new())
    }
}

#[tauri::command]
fn notes_save(state: St<'_>, notes: serde_json::Value) {
    save_json(&state.paths.notes(), &notes);
}

#[tauri::command]
fn chat_load(state: St<'_>) -> serde_json::Value {
    let v: serde_json::Value = load_json(&state.paths.chat());
    if v.is_array() {
        v
    } else {
        serde_json::Value::Array(Vec::new())
    }
}

/// Saved off the main thread (a dialogue with pictures is megabytes of JSON and
/// a sync command would freeze every window meanwhile). Only the newest
/// version is written when saves pile up.
#[tauri::command]
fn chat_save(state: St<'_>, messages: serde_json::Value) {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    static WRITING: Mutex<()> = Mutex::new(());
    let path = state.paths.chat();
    let me = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    thread::spawn(move || {
        let _guard = lock(&WRITING);
        if SEQ.load(Ordering::SeqCst) == me {
            save_json(&path, &messages);
        }
    });
}

// ---------------------------------------------------------------------------
// Updates from GitHub releases
// ---------------------------------------------------------------------------

fn update_toast(app: &AppHandle, r: &update::Release) {
    toast(
        app,
        ToastPayload {
            icon: "download".into(),
            title: format!("Вышел Island {}", r.version),
            subtitle: Some(format!("У вас {}", app.package_info().version)),
            actions: vec![
                ToastAction {
                    label: "Обновить".into(),
                    cmd: "update_install".into(),
                    args: serde_json::json!({}),
                },
                ToastAction {
                    label: "Пропустить".into(),
                    cmd: "update_skip".into(),
                    args: serde_json::json!({ "version": r.version }),
                },
            ],
            ms: Some(20000),
            ..Default::default()
        },
    );
}

/// 30 s after start, then every 6 hours; each version is offered once per run.
fn spawn_update_thread(app: AppHandle, shared: Arc<Shared>) {
    thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            thread::sleep(Duration::from_secs(30));
            let current = app.package_info().version.to_string();
            let mut offered = String::new();
            loop {
                let s = lock(&shared.settings).clone();
                if s.update_check {
                    if let Ok(Some(r)) = tauri::async_runtime::block_on(update::check(&current)) {
                        if r.version != offered && r.version != s.update_skip {
                            offered = r.version.clone();
                            update_toast(&app, &r);
                        }
                    }
                }
                thread::sleep(Duration::from_secs(6 * 3600));
            }
        })
        .expect("failed to start update thread");
}

/// Manual check (settings, tray): always answers with a toast.
#[tauri::command]
async fn update_check(app: AppHandle, manual: bool) {
    let current = app.package_info().version.to_string();
    match update::check(&current).await {
        Ok(Some(r)) => update_toast(&app, &r),
        Ok(None) if manual => toast(
            &app,
            ToastPayload {
                icon: "check".into(),
                title: "Обновлений нет".into(),
                subtitle: Some(format!("Island {current} — последняя версия")),
                ..Default::default()
            },
        ),
        Err(e) if manual => toast_error(&app, "Не удалось проверить обновления", &e),
        Err(e) => log::warn(&format!("проверка обновлений: {e}")),
        _ => {}
    }
}

#[tauri::command]
async fn update_install(app: AppHandle) -> Result<(), String> {
    let current = app.package_info().version.to_string();
    let r = match update::found() {
        Some(r) => r,
        None => update::check(&current).await?.ok_or("Обновлений нет")?,
    };
    toast(
        &app,
        ToastPayload {
            icon: "download".into(),
            title: format!("Скачиваю Island {}", r.version),
            subtitle: Some("Остров перезапустится сам".into()),
            ms: Some(60000),
            ..Default::default()
        },
    );
    match update::install(&r).await {
        Ok(()) => {
            finish_recording_before_exit(&app);
            app.exit(0);
            Ok(())
        }
        Err(e) => {
            toast_error(&app, "Обновление не скачалось", &e);
            Err(e)
        }
    }
}

#[tauri::command]
fn update_skip(app: AppHandle, state: St<'_>, version: String) {
    let next = {
        let mut s = lock(&state.settings);
        s.update_skip = version;
        s.clone()
    };
    store::save_json_synced(&state.paths.settings(), &next);
    let _ = app.emit("settings", next);
}

// ---------------------------------------------------------------------------
// AI agents (Claude Code / Codex)
// ---------------------------------------------------------------------------

#[tauri::command]
fn agents_list() -> agents::Payload {
    agents::snapshot()
}

#[tauri::command]
fn agents_answer(session: String, request: String, decision: String, text: Option<String>) -> Result<(), String> {
    agents::answer(&session, &request, &decision, text)
}

#[tauri::command]
async fn agents_focus(state: St<'_>, session: String) -> Result<bool, String> {
    let shared = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || shared.shell.run(move || agents::focus(&session)).unwrap_or(false))
        .await
        .map_err(|e| e.to_string())
}

/// "status" | "install" | "remove" for the hooks in ~/.claude/settings.json.
#[tauri::command]
async fn agents_hooks(action: String) -> Result<agents::HookStatus, String> {
    tauri::async_runtime::spawn_blocking(move || match action.as_str() {
        "install" => agents::hooks_install(),
        "remove" => agents::hooks_remove(),
        _ => Ok(agents::hooks_status()),
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Tray & entry point
// ---------------------------------------------------------------------------

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Открыть остров", true, None::<&str>)?;
    let launcher = MenuItem::with_id(app, "launcher", "Лаунчер", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Настройки", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let dump = MenuItem::with_id(app, "media-dump", "Скопировать медиасессии", true, None::<&str>)?;
    let upd = MenuItem::with_id(app, "update", "Проверить обновления", true, None::<&str>)?;
    let journal = MenuItem::with_id(app, "log-open", "Открыть журнал ошибок", true, None::<&str>)?;
    let journal_copy = MenuItem::with_id(app, "log-copy", "Скопировать журнал", true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", "Перезапустить", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &launcher, &settings, &sep, &dump, &upd, &journal, &journal_copy, &restart, &quit])?;
    let mut builder = TrayIconBuilder::with_id("island")
        .tooltip("Island")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                let _ = app.emit("open-panel", "home");
            }
            "launcher" => set_launcher(app, true),
            "settings" => {
                let _ = app.emit("open-panel", "settings");
            }
            "media-dump" => {
                let st = app.state::<Arc<Shared>>();
                let _ = lock(&st.media_tx).send(MediaCmd { action: "dump".into(), value: 0.0 });
            }
            "update" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { update_check(app, true).await });
            }
            "log-open" => {
                if let Some(p) = log::path() {
                    let p = p.to_string_lossy().into_owned();
                    let st = app.state::<Arc<Shared>>().inner().clone();
                    thread::spawn(move || st.shell.run(move || shell::open(&p)));
                }
            }
            "log-copy" => {
                let app = app.clone();
                thread::spawn(move || {
                    let text = log::tail(64 * 1024);
                    let ok = !text.is_empty() && clip::write_text(&text);
                    toast(
                        &app,
                        ToastPayload {
                            icon: "copy".into(),
                            title: if ok { "Журнал скопирован".into() } else { "Журнал пуст".into() },
                            ..Default::default()
                        },
                    );
                });
            }
            "restart" => {
                finish_recording_before_exit(app);
                app.restart()
            }
            "quit" => {
                finish_recording_before_exit(app);
                app.exit(0)
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = tray.app_handle().emit("open-panel", "home");
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn main() {
    // Claude Code runs `island.exe --cc-hook` for its hook events
    if agents::is_hook_call() {
        agents::hook_main();
        return;
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = app.emit("open-panel", "home");
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_drag::init())
        // http://shot.localhost/<id> — the frozen screen for the capture overlay
        .register_uri_scheme_protocol("shot", |ctx, req| {
            let shared = ctx.app_handle().state::<Arc<Shared>>();
            let want: u32 = req.uri().path().trim_matches('/').parse().unwrap_or(0);
            let body = lock(&shared.shot)
                .as_ref()
                .filter(|s| s.id == want)
                .map(|s| s.bmp.as_ref().clone());
            let builder = tauri::http::Response::builder()
                .header("Access-Control-Allow-Origin", "*")
                .header("Cache-Control", "no-store");
            match body {
                Some(b) => builder.header("Content-Type", "image/bmp").body(b),
                None => builder.status(404).body(Vec::new()),
            }
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
        })
        // http://backdrop.localhost/<id> — the screen under the island for the glass
        .register_uri_scheme_protocol("backdrop", |ctx, req| {
            let shared = ctx.app_handle().state::<Arc<Shared>>();
            let want: u32 = req.uri().path().trim_matches('/').parse().unwrap_or(0);
            // a slightly older id is fine too: the newest picture is always what we want
            let body = lock(&shared.backdrop)
                .as_ref()
                .filter(|(id, _)| *id >= want)
                .map(|(_, b)| b.as_ref().clone());
            let builder = tauri::http::Response::builder()
                .header("Access-Control-Allow-Origin", "*")
                .header("Cache-Control", "no-store");
            match body {
                Some(b) => builder.header("Content-Type", "image/bmp").body(b),
                None => builder.status(404).body(Vec::new()),
            }
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
        })
        .setup(|app| {
            let paths = Paths::new(app.path().app_data_dir()?);
            log::init(&paths.dir);
            let first_run = !paths.settings().exists();
            let settings: Settings = load_json(&paths.settings());
            let data: Data = load_json(&paths.data());
            let clips = ClipStore::load(paths.clips(), paths.clip_dir());
            let (media_tx, media_rx) = channel();
            let (audio_tx, audio_rx) = channel();
            let (weather_tx, weather_rx) = channel();
            let (voice_tx, voice_rx) = channel();

            let shared = Arc::new(Shared {
                paths,
                settings: Mutex::new(settings.clone()),
                data: Mutex::new(data),
                clips: Mutex::new(clips),
                hit: Mutex::new(Rect::default()),
                geom: Mutex::new(Geom::default()),
                own_hwnd: AtomicIsize::new(0),
                last_fg: AtomicIsize::new(0),
                suppressed: AtomicBool::new(false),
                launcher_open: AtomicBool::new(false),
                media_playing: AtomicBool::new(false),
                clip_skip_seq: AtomicU32::new(0),
                media: Mutex::new(None),
                cover: Mutex::new(CoverPayload::default()),
                volume: Mutex::new(VolumePayload::default()),
                media_tx: Mutex::new(media_tx),
                audio_tx: Mutex::new(audio_tx),
                shell: ShellWorker::spawn(),
                icons: Mutex::new(HashMap::new()),
                apps: Mutex::new(None),
                hotkeys: Mutex::new(HashMap::new()),
                hotkey_error: Mutex::new(None),
                shot: Mutex::new(None),
                shot_seq: AtomicU32::new(0),
                capturing: AtomicBool::new(false),
                ai_gen: AtomicU32::new(0),
                ai_task: Mutex::new(None),
                weather: Mutex::new(None),
                weather_wake: Mutex::new(weather_tx),
                voice_tx: Mutex::new(voice_tx),
                voice_active: AtomicBool::new(false),
                recording: Mutex::new(None),
                ym: Mutex::new(ym::Cache::default()),
                yt_active: AtomicBool::new(false),
                yt_failed: Mutex::new(String::new()),
                backdrop: Mutex::new(None),
                backdrop_seq: AtomicU32::new(0),
            });
            app.manage(shared.clone());

            if first_run {
                save_json(&shared.paths.settings(), &settings);
                apply_autostart(app.handle(), settings.autostart);
            }
            let errors = apply_hotkeys(app.handle(), &shared, &settings);
            if !errors.is_empty() {
                log::warn(&format!("хоткеи: {}", errors.join(", ")));
                *lock(&shared.hotkey_error) = Some(errors.join(", "));
            }

            let win = app.get_webview_window("main").ok_or("main window is missing")?;
            place_window(&win, &shared, true);
            if let Ok(h) = win.hwnd() {
                let h = h.0 as isize;
                shared.own_hwnd.store(h, Ordering::Relaxed);
                input::make_tool_window(h);
            }
            // The window is created visible but unfocused (see tauri.conf.json);
            // it never takes clicks until the cursor is over the island.
            let _ = win.set_ignore_cursor_events(true);

            build_tray(app.handle())?;
            build_overlay(app.handle())?;

            spawn_cursor_thread(app.handle().clone(), shared.clone());
            spawn_media_thread(app.handle().clone(), shared.clone(), media_rx);
            spawn_audio_thread(app.handle().clone(), shared.clone(), audio_rx);
            spawn_clip_thread(app.handle().clone(), shared.clone());
            spawn_weather_thread(app.handle().clone(), shared.clone(), weather_rx);
            spawn_voice_thread(app.handle().clone(), shared.clone(), voice_rx);
            spawn_downloads_thread(app.handle().clone(), shared.clone());
            spawn_backdrop_thread(app.handle().clone(), shared.clone());
            spawn_update_thread(app.handle().clone(), shared.clone());
            agents::start(app.handle().clone(), shared.paths.dir.clone());
            if settings.agents_enabled {
                thread::spawn(agents::hooks_refresh);
            }
            {
                let app = app.handle().clone();
                input::watch_double_ctrl(move || {
                    let st = app.state::<Arc<Shared>>();
                    let s = lock(&st.settings);
                    if s.agents_enabled && s.office_hotkey && !st.suppressed.load(Ordering::Relaxed) {
                        let _ = app.emit("office", ());
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            init,
            set_hit,
            save_settings,
            launcher_closed,
            open_launcher,
            media_control,
            volume_set,
            volume_mute,
            audio_devices,
            audio_set_device,
            list_apps,
            get_icons,
            open_target,
            reveal,
            pick_files,
            shelf_add,
            shelf_remove,
            shelf_clear,
            shelf_copy,
            pins_add,
            pins_add_app,
            pins_remove,
            clip_paste,
            paste_plain,
            copy_text,
            clip_delete,
            clip_pin,
            clip_clear,
            lock_screen,
            record_screen,
            capture,
            overlay_show,
            capture_cancel,
            capture_finish,
            record_stop,
            ai_models,
            ai_attach,
            ai_send,
            ai_stop,
            chat_load,
            chat_save,
            weather_search,
            weather_locate,
            weather_refresh,
            voice,
            ym_state,
            ym_set,
            ym_login,
            ym_logout,
            ym_set_token,
            ym_lyrics,
            uncensored,
            yt_show,
            clip_read_text,
            paste_text,
            notes_load,
            notes_save,
            update_check,
            update_install,
            update_skip,
            agents_list,
            agents_answer,
            agents_focus,
            agents_hooks,
            log_js,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("error while running Island");
}
