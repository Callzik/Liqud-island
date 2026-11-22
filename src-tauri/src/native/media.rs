//! Now-playing information through Global System Media Transport Controls
//! (Spotify, Яндекс Музыка, browsers, Windows Media Player, ...).

use windows::core::{Interface, Result};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode,
};
use windows::Storage::Streams::{DataReader, IInputStream, IRandomAccessStreamWithContentType, InputStreamOptions};

/// 100ns ticks between 1601-01-01 and 1970-01-01.
const EPOCH_DIFF: i64 = 116_444_736_000_000_000;

#[derive(Clone, Debug, PartialEq)]
pub struct MediaState {
    pub app_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Unix time (ms) at which `position_ms` was sampled by the player.
    pub updated_ms: i64,
    pub can_prev: bool,
    pub can_next: bool,
    pub can_seek: bool,
}

pub struct Thumb {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub struct Media {
    manager: Manager,
    /// the shown player is classic Winamp, driven through its window (no session)
    winamp: std::cell::Cell<bool>,
    /// sessions whose app id contains this are shown first (the island's own
    /// YouTube player while «без цензуры» runs)
    prefer: std::cell::RefCell<Option<String>>,
    /// the session shown last time: cover and buttons go to the same one
    active: std::cell::RefCell<Option<Session>>,
    active_props: std::cell::RefCell<Option<Properties>>,
    /// last good state; bridges moments when a player (Liner, Electron apps)
    /// briefly returns no metadata while switching tracks
    last: std::cell::RefCell<Option<(MediaState, std::time::Instant)>>,
    /// app ids (lowercase) the user hid in the settings: never shown
    hidden: std::cell::RefCell<Vec<String>>,
    /// player the user picked in the island's source list (app id, lowercase)
    pick: std::cell::RefCell<Option<String>>,
}

/// How long a vanished state is still shown while its player stays in the list.
const GRACE: std::time::Duration = std::time::Duration::from_millis(2500);

fn status_of(s: &Session) -> Option<Status> {
    s.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).ok()
}

fn app_of(s: &Session) -> String {
    s.SourceAppUserModelId().map(|h| h.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Image type by its first bytes; players' ContentType is often empty or wrong.
fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if b.starts_with(b"GIF8") {
        Some("image/gif")
    } else if b.len() > 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"BM") {
        Some("image/bmp")
    } else {
        None
    }
}

fn transcode(stream: &IRandomAccessStreamWithContentType) -> Option<Vec<u8>> {
    stream.Seek(0).ok()?;
    let dec = BitmapDecoder::CreateAsync(stream).ok()?.join().ok()?;
    let (w, h) = (dec.OrientedPixelWidth().ok()?, dec.OrientedPixelHeight().ok()?);
    let px = dec
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &BitmapTransform::new().ok()?,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )
        .ok()?
        .join()
        .ok()?
        .DetachPixelData()
        .ok()?;
    if w == 0 || h == 0 || px.len() != (w * h * 4) as usize {
        return None;
    }
    let (w, h, px) = super::img::shrink(w, h, &px, 512);
    super::img::encode_png(w, h, &px)
}

impl Media {
    /// Must be called on a COM-initialised (MTA) thread.
    pub fn new() -> Result<Self> {
        let manager = Manager::RequestAsync()?.join()?;
        Ok(Self {
            manager,
            prefer: std::cell::RefCell::new(None),
            active: std::cell::RefCell::new(None),
            active_props: std::cell::RefCell::new(None),
            last: std::cell::RefCell::new(None),
            hidden: std::cell::RefCell::new(Vec::new()),
            pick: std::cell::RefCell::new(None),
            winamp: std::cell::Cell::new(false),
        })
    }

    pub fn set_prefer(&self, needle: Option<&str>) {
        *self.prefer.borrow_mut() = needle.map(|n| n.to_lowercase());
    }

    pub fn set_hidden(&self, ids: &[String]) {
        let ids: Vec<String> = ids.iter().map(|i| i.to_lowercase()).collect();
        if *self.hidden.borrow() != ids {
            *self.hidden.borrow_mut() = ids;
        }
    }

    pub fn set_pick(&self, id: Option<&str>) {
        *self.pick.borrow_mut() = id.map(|i| i.to_lowercase()).filter(|i| !i.is_empty());
    }

    /// Every visible session with what it plays (for the source list).
    pub fn sessions(&self) -> Vec<MediaState> {
        let Ok(list) = self.manager.GetSessions() else { return Vec::new() };
        let n = list.Size().unwrap_or(0);
        (0..n)
            .filter_map(|i| list.GetAt(i).ok())
            .filter(|s| !self.is_hidden(&app_of(s)))
            .filter_map(|s| self.state_from(s))
            .chain(self.winamp_state())
            .collect()
    }

    /// Classic Winamp, unless it already has a media session (plugin) or is hidden.
    fn winamp_state(&self) -> Option<MediaState> {
        if self.is_hidden(super::winamp::APP_ID) || self.apps().iter().any(|a| a.to_lowercase().contains("winamp")) {
            return None;
        }
        super::winamp::state()
    }

    pub fn state(&self) -> Option<MediaState> {
        let wa = self.winamp_state();
        let pick = self.pick.borrow().clone();
        let free = self.prefer.borrow().is_none();
        if wa.is_some() && free && pick.as_deref() == Some(super::winamp::APP_ID) {
            self.winamp.set(true);
            return wa;
        }
        let st = self.session_state();
        match (wa, &st) {
            // Winamp plays and the session player does not (and nothing else was picked)
            (Some(w), cur) if cur.as_ref().map_or(true, |c| w.playing && !c.playing && pick.is_none() && free) => {
                self.winamp.set(true);
                Some(w)
            }
            _ => {
                self.winamp.set(false);
                st
            }
        }
    }

    fn is_hidden(&self, id: &str) -> bool {
        self.hidden.borrow().iter().any(|h| h == id)
    }

    /// App ids of all current sessions, as Windows reports them.
    pub fn apps(&self) -> Vec<String> {
        let Ok(list) = self.manager.GetSessions() else { return Vec::new() };
        let n = list.Size().unwrap_or(0);
        let mut out: Vec<String> = (0..n)
            .filter_map(|i| list.GetAt(i).ok())
            .filter_map(|s| s.SourceAppUserModelId().ok().map(|h| h.to_string_lossy()))
            .filter(|id| !id.is_empty())
            .collect();
        out.dedup();
        out
    }

    /// The first session whose app id contains one of `needles`.
    fn find(&self, needles: &[&str]) -> Option<Session> {
        let list = self.manager.GetSessions().ok()?;
        let n = list.Size().unwrap_or(0);
        (0..n).filter_map(|i| list.GetAt(i).ok()).find(|s| {
            let id = app_of(s);
            needles.iter().any(|k| id.contains(k))
        })
    }

    /// State of a specific app's session (e.g. "yandex"), playing or not.
    pub fn state_of(&self, needles: &[&str]) -> Option<MediaState> {
        self.find(needles).and_then(|s| self.state_from(s))
    }

    /// Sends a command to a specific app's session.
    pub fn control_of(&self, needles: &[&str], action: &str, value: f64) -> Result<()> {
        match self.find(needles) {
            Some(s) => Self::run(&s, action, value),
            None => Ok(()),
        }
    }

    /// All sessions, best first: the preferred app, playing (or switching
    /// tracks), the system "current" one, paused, the rest. Some players
    /// (Liner among them) are never the "current" session and sometimes
    /// publish an empty title, so every session is a candidate.
    fn candidates(&self) -> Vec<Session> {
        let Ok(list) = self.manager.GetSessions() else { return Vec::new() };
        let n = list.Size().unwrap_or(0);
        let prefer = self.prefer.borrow().clone();
        let parts: Vec<String> = prefer.map(|p| p.split('|').map(String::from).collect()).unwrap_or_default();
        let current = self.manager.GetCurrentSession().ok().map(|s| app_of(&s));
        let mut all: Vec<(u8, Session)> = (0..n)
            .filter_map(|i| list.GetAt(i).ok())
            .filter(|s| !self.is_hidden(&app_of(s)))
            .map(|s| {
                let id = app_of(&s);
                let preferred = !parts.is_empty() && parts.iter().any(|k| id.contains(k.as_str()));
                let playing = matches!(status_of(&s), Some(Status::Playing) | Some(Status::Changing));
                // a paused preferred player must not hide another app that plays
                let picked = self.pick.borrow().as_deref() == Some(id.as_str());
                let rank = match (preferred, playing) {
                    // «без цензуры» still wins: it replaces the picked Yandex player
                    (false, _) if picked && parts.is_empty() => 0,
                    (true, true) => 0,
                    (false, true) => 1,
                    (true, false) => 2,
                    _ if current.as_deref() == Some(id.as_str()) => 3,
                    _ if status_of(&s) == Some(Status::Paused) => 4,
                    _ => 5,
                };
                (rank, s)
            })
            .collect();
        all.sort_by_key(|(r, _)| *r);
        all.into_iter().map(|(_, s)| s).collect()
    }

    /// Every session as Windows reports it, for the tray's diagnostics item.
    pub fn dump(&self, name: impl Fn(&str) -> String) -> String {
        let current = self.manager.GetCurrentSession().ok().map(|s| app_of(&s));
        let mut out = String::new();
        let Ok(list) = self.manager.GetSessions() else { return "GetSessions: ошибка".into() };
        let n = list.Size().unwrap_or(0);
        out.push_str(&format!("Сессий: {n}, текущая: {}\n", current.clone().unwrap_or_else(|| "-".into())));
        for s in (0..n).filter_map(|i| list.GetAt(i).ok()) {
            let id = s.SourceAppUserModelId().map(|h| h.to_string_lossy()).unwrap_or_default();
            let status = match status_of(&s) {
                Some(Status::Playing) => "playing",
                Some(Status::Paused) => "paused",
                Some(Status::Changing) => "changing",
                Some(Status::Stopped) => "stopped",
                Some(Status::Opened) => "opened",
                Some(Status::Closed) => "closed",
                _ => "?",
            };
            let props = s.TryGetMediaPropertiesAsync().and_then(|p| p.join());
            let (title, artist) = match &props {
                Ok(p) => (
                    p.Title().map(|h| h.to_string_lossy()).unwrap_or_default(),
                    p.Artist().map(|h| h.to_string_lossy()).unwrap_or_default(),
                ),
                Err(e) => (format!("<ошибка {e}>"), String::new()),
            };
            let can_seek = s.GetPlaybackInfo().and_then(|p| p.Controls()).and_then(|c| c.IsPlaybackPositionEnabled());
            let seek = can_seek.map(|v| if v { "да" } else { "нет" }).unwrap_or("неизвестно");
            let timing = s.GetTimelineProperties().ok().map(|t| {
                let position = t.Position().ok().map(|p| p.Duration / 10_000);
                let updated = t.LastUpdatedTime().ok()
                    .map(|d| (d.UniversalTime - EPOCH_DIFF) / 10_000)
                    .filter(|d| *d > 0);
                let sample = updated.map(|d| format!("{d} (возраст {} мс)", crate::store::now_ms() - d))
                    .unwrap_or_else(|| "нет".into());
                format!("позиция: {} мс | замер Unix мс: {sample}", position.map(|p| p.to_string()).unwrap_or_else(|| "нет".into()))
            }).unwrap_or_else(|| "замер недоступен".into());
            out.push_str(&format!("{id} [{}] | {status} | {artist} — {title}\n  перемотка: {seek} | {timing}\n", name(&id)));
        }
        out
    }

    fn session(&self) -> Option<Session> {
        if let Some(s) = self.active.borrow().clone() {
            return Some(s);
        }
        self.candidates().into_iter().next()
    }

    fn session_state(&self) -> Option<MediaState> {
        let cands = self.candidates();
        let last = self.last.borrow().clone();
        for s in cands {
            if let Some((st, props)) = self.state_with_properties(&s) {
                *self.active.borrow_mut() = Some(s);
                *self.active_props.borrow_mut() = Some(props);
                *self.last.borrow_mut() = Some((st.clone(), std::time::Instant::now()));
                return Some(st);
            }
            // the shown player is still here but its metadata blinked:
            // keep the old state instead of jumping to another app
            if let Some((prev, at)) = &last {
                if at.elapsed() < GRACE && app_of(&s) == prev.app_id.to_lowercase() {
                    let mut st = prev.clone();
                    st.playing = matches!(status_of(&s), Some(Status::Playing) | Some(Status::Changing));
                    *self.active.borrow_mut() = Some(s);
                    return Some(st);
                }
            }
        }
        *self.active.borrow_mut() = None;
        *self.active_props.borrow_mut() = None;
        None
    }

    fn state_from(&self, s: Session) -> Option<MediaState> {
        self.state_with_properties(&s).map(|(state, _)| state)
    }

    fn state_with_properties(&self, s: &Session) -> Option<(MediaState, Properties)> {
        let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
        let mut title = props.Title().map(|h| h.to_string_lossy()).unwrap_or_default().trim().to_string();
        let mut artist = props.Artist().map(|h| h.to_string_lossy()).unwrap_or_default().trim().to_string();
        if artist.is_empty() {
            artist = props.AlbumArtist().map(|h| h.to_string_lossy()).unwrap_or_default().trim().to_string();
        }
        // some players put everything into the title: "Artist - Song"
        if artist.is_empty() {
            for sep in [" - ", " \u{2014} ", " \u{2013} "] {
                if let Some((a, t)) = title.split_once(sep) {
                    if !a.trim().is_empty() && !t.trim().is_empty() {
                        let (a, t) = (a.trim().to_string(), t.trim().to_string());
                        artist = a;
                        title = t;
                        break;
                    }
                }
            }
        }
        if title.is_empty() {
            return None;
        }
        let album = props.AlbumTitle().map(|h| h.to_string_lossy()).unwrap_or_default();
        let app_id = s.SourceAppUserModelId().map(|h| h.to_string_lossy()).unwrap_or_default();

        let (playing, can_prev, can_next, can_seek) = match s.GetPlaybackInfo() {
            Ok(info) => {
                let playing = info.PlaybackStatus().map(|st| st == Status::Playing).unwrap_or(false);
                let (prev, next, seek) = info
                    .Controls()
                    .map(|c| {
                        (
                            c.IsPreviousEnabled().unwrap_or(true),
                            c.IsNextEnabled().unwrap_or(true),
                            c.IsPlaybackPositionEnabled().unwrap_or(false),
                        )
                    })
                    .unwrap_or((true, true, false));
                (playing, prev, next, seek)
            }
            Err(_) => (false, true, true, false),
        };

        let (position_ms, duration_ms, updated_ms) = match s.GetTimelineProperties() {
            Ok(t) => {
                let pos = t.Position().ok().map(|d| d.Duration / 10_000);
                let start = t.StartTime().map(|d| d.Duration / 10_000).unwrap_or(0);
                let end = t.EndTime().map(|d| d.Duration / 10_000).unwrap_or(0);
                let upd = t
                    .LastUpdatedTime()
                    .map(|d| (d.UniversalTime - EPOCH_DIFF) / 10_000)
                    .unwrap_or(0);
                (pos.map(|p| p - start).unwrap_or(-1), (end - start).max(0), upd)
            }
            Err(_) => (-1, 0, 0),
        };

        Some((MediaState {
            app_id,
            title,
            artist,
            album,
            playing,
            position_ms,
            duration_ms,
            updated_ms,
            can_prev,
            can_next,
            can_seek,
        }, props))
    }

    pub fn thumbnail(&self) -> std::result::Result<Thumb, String> {
        if self.winamp.get() {
            return Err("Winamp не передаёт обложку".into());
        }
        let props = self.active_props.borrow();
        let props = props.as_ref().ok_or("Нет метаданных медиасессии")?;
        let reference = props.Thumbnail().map_err(|e| format!("Ссылка на обложку: {e}"))?;
        let stream = reference.OpenReadAsync().and_then(|op| op.join())
            .map_err(|e| format!("Открытие потока обложки: {e}"))?;
        const LIMIT: u64 = 8 * 1024 * 1024;
        let size = stream.Size().ok().filter(|s| *s > 0);
        if size.is_some_and(|s| s > LIMIT) {
            return Err("Обложка больше 8 МБ".into());
        }
        let input = stream.GetInputStreamAt(0).or_else(|_| stream.cast::<IInputStream>())
            .map_err(|e| format!("Получение потока обложки: {e}"))?;
        let reader = DataReader::CreateDataReader(&input).map_err(|e| format!("Подготовка чтения обложки: {e}"))?;
        reader.SetInputStreamOptions(InputStreamOptions::Partial)
            .map_err(|e| format!("Настройка чтения обложки: {e}"))?;
        let mut bytes = Vec::new();
        let length = size.unwrap_or(LIMIT + 1) as usize;
        while bytes.len() < length {
            let count = (length - bytes.len()).min(64 * 1024);
            let got = reader.LoadAsync(count as u32).and_then(|op| op.join())
                .map_err(|e| format!("Чтение обложки: {e}"))?;
            if got == 0 {
                break;
            }
            let offset = bytes.len();
            bytes.resize(offset + got as usize, 0);
            reader.ReadBytes(&mut bytes[offset..]).map_err(|e| format!("Данные обложки: {e}"))?;
        }
        let _ = reader.DetachStream();
        if bytes.len() as u64 > LIMIT {
            return Err("Обложка больше 8 МБ".into());
        }
        if bytes.is_empty() || size.is_some_and(|s| s != bytes.len() as u64) {
            return Err(format!("Неполный поток обложки: {} байт", bytes.len()));
        }
        if let Some(mime) = sniff(&bytes) {
            return Ok(Thumb { mime: mime.into(), bytes });
        }
        // a format WebView2 can't show, or a declared type that lies:
        // let Windows decode it and hand over PNG
        let png = transcode(&stream).ok_or("Не удалось декодировать формат обложки")?;
        Ok(Thumb { mime: "image/png".into(), bytes: png })
    }

    pub fn control(&self, action: &str, value: f64) -> Result<()> {
        if self.winamp.get() {
            super::winamp::control(action, value);
            return Ok(());
        }
        let Some(s) = self.session() else { return Ok(()) };
        Self::run(&s, action, value)
    }

    fn run(s: &Session, action: &str, value: f64) -> Result<()> {
        match action {
            "play" => {
                s.TryPlayAsync()?.join()?;
            }
            "toggle" => {
                s.TryTogglePlayPauseAsync()?.join()?;
            }
            "pause" => {
                s.TryPauseAsync()?.join()?;
            }
            "next" => {
                s.TrySkipNextAsync()?.join()?;
            }
            "prev" => {
                s.TrySkipPreviousAsync()?.join()?;
            }
            "seek" => {
                if !value.is_finite() || !s.GetPlaybackInfo().and_then(|p| p.Controls())
                    .and_then(|c| c.IsPlaybackPositionEnabled()).unwrap_or(false) {
                    return Ok(());
                }
                // value: position in milliseconds, relative to StartTime
                let start = s.GetTimelineProperties().and_then(|t| t.StartTime()).map(|d| d.Duration).unwrap_or(0);
                let ticks = start.saturating_add((value.max(0.0) * 10_000.0) as i64);
                s.TryChangePlaybackPositionAsync(ticks)?.join()?;
            }
            _ => {}
        }
        Ok(())
    }
}
