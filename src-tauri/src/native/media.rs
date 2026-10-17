//! Now-playing information through Global System Media Transport Controls
//! (Spotify, Яндекс Музыка, browsers, Windows Media Player, ...).

use windows::core::Result;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Graphics::Imaging::{
    BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamWithContentType};

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
}

pub struct Thumb {
    pub mime: String,
    pub bytes: Vec<u8>,
}

pub struct Media {
    manager: Manager,
    /// sessions whose app id contains this are shown first (the island's own
    /// YouTube player while «без цензуры» runs)
    prefer: std::cell::RefCell<Option<String>>,
    /// the session shown last time: cover and buttons go to the same one
    active: std::cell::RefCell<Option<Session>>,
    /// last good state; bridges moments when a player (Liner, Electron apps)
    /// briefly returns no metadata while switching tracks
    last: std::cell::RefCell<Option<(MediaState, std::time::Instant)>>,
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
            last: std::cell::RefCell::new(None),
        })
    }

    pub fn set_prefer(&self, needle: Option<&str>) {
        *self.prefer.borrow_mut() = needle.map(|n| n.to_lowercase());
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
            .map(|s| {
                let id = app_of(&s);
                let preferred = !parts.is_empty() && parts.iter().any(|k| id.contains(k.as_str()));
                let playing = matches!(status_of(&s), Some(Status::Playing) | Some(Status::Changing));
                // a paused preferred player must not hide another app that plays
                let rank = match (preferred, playing) {
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
            out.push_str(&format!("{id} [{}] | {status} | {artist} — {title}\n", name(&id)));
        }
        out
    }

    fn session(&self) -> Option<Session> {
        if let Some(s) = self.active.borrow().clone() {
            return Some(s);
        }
        self.candidates().into_iter().next()
    }

    pub fn state(&self) -> Option<MediaState> {
        let cands = self.candidates();
        let last = self.last.borrow().clone();
        for s in cands {
            if let Some(st) = self.state_from(s.clone()) {
                *self.active.borrow_mut() = Some(s);
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
        None
    }

    fn state_from(&self, s: Session) -> Option<MediaState> {
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

        let (playing, can_prev, can_next) = match s.GetPlaybackInfo() {
            Ok(info) => {
                let playing = info.PlaybackStatus().map(|st| st == Status::Playing).unwrap_or(false);
                let (prev, next) = info
                    .Controls()
                    .map(|c| {
                        (
                            c.IsPreviousEnabled().unwrap_or(true),
                            c.IsNextEnabled().unwrap_or(true),
                        )
                    })
                    .unwrap_or((true, true));
                (playing, prev, next)
            }
            Err(_) => (false, true, true),
        };

        let (position_ms, duration_ms, updated_ms) = match s.GetTimelineProperties() {
            Ok(t) => {
                let pos = t.Position().map(|d| d.Duration / 10_000).unwrap_or(0);
                let start = t.StartTime().map(|d| d.Duration / 10_000).unwrap_or(0);
                let end = t.EndTime().map(|d| d.Duration / 10_000).unwrap_or(0);
                let upd = t
                    .LastUpdatedTime()
                    .map(|d| (d.UniversalTime - EPOCH_DIFF) / 10_000)
                    .unwrap_or(0);
                (pos - start, (end - start).max(0), upd)
            }
            Err(_) => (0, 0, 0),
        };

        Some(MediaState {
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
        })
    }

    pub fn thumbnail(&self) -> Option<Thumb> {
        let s = self.session()?;
        let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
        let reference = props.Thumbnail().ok()?;
        let stream = reference.OpenReadAsync().ok()?.join().ok()?;
        let size = stream.Size().ok()? as u32;
        if size == 0 || size > 8 * 1024 * 1024 {
            return None;
        }
        let input = stream.GetInputStreamAt(0).ok()?;
        let reader = DataReader::CreateDataReader(&input).ok()?;
        // streams from some players (Dotify, Electron) arrive in pieces
        let mut bytes = Vec::with_capacity(size as usize);
        while (bytes.len() as u32) < size {
            let got = reader.LoadAsync(size - bytes.len() as u32).ok()?.join().ok()?;
            if got == 0 {
                break;
            }
            let mut chunk = vec![0u8; got as usize];
            reader.ReadBytes(&mut chunk).ok()?;
            bytes.extend_from_slice(&chunk);
        }
        if let Some(mime) = sniff(&bytes) {
            return Some(Thumb { mime: mime.into(), bytes });
        }
        // a format WebView2 can't show, or a declared type that lies:
        // let Windows decode it and hand over PNG
        let png = transcode(&stream)?;
        Some(Thumb { mime: "image/png".into(), bytes: png })
    }

    pub fn control(&self, action: &str, value: f64) -> Result<()> {
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
                // value: position in milliseconds, relative to StartTime
                let start = s.GetTimelineProperties().and_then(|t| t.StartTime()).map(|d| d.Duration).unwrap_or(0);
                let ticks = start + (value.max(0.0) * 10_000.0) as i64;
                s.TryChangePlaybackPositionAsync(ticks)?.join()?;
            }
            _ => {}
        }
        Ok(())
    }
}
