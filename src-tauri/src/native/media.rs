//! Now-playing information through Global System Media Transport Controls
//! (Spotify, Яндекс Музыка, browsers, Windows Media Player, ...).

use windows::core::Result;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

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
}

impl Media {
    /// Must be called on a COM-initialised (MTA) thread.
    pub fn new() -> Result<Self> {
        let manager = Manager::RequestAsync()?.join()?;
        Ok(Self { manager })
    }

    /// Prefers a session that is actually playing, then the system "current" one.
    fn session(&self) -> Option<Session> {
        if let Ok(list) = self.manager.GetSessions() {
            let n = list.Size().unwrap_or(0);
            for i in 0..n {
                if let Ok(s) = list.GetAt(i) {
                    let playing = s
                        .GetPlaybackInfo()
                        .and_then(|p| p.PlaybackStatus())
                        .map(|st| st == Status::Playing)
                        .unwrap_or(false);
                    if playing {
                        return Some(s);
                    }
                }
            }
        }
        self.manager.GetCurrentSession().ok()
    }

    pub fn state(&self) -> Option<MediaState> {
        let s = self.session()?;
        let props = s.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
        let title = props.Title().map(|h| h.to_string_lossy()).unwrap_or_default();
        if title.is_empty() {
            return None;
        }
        let artist = props.Artist().map(|h| h.to_string_lossy()).unwrap_or_default();
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
        let mime = stream
            .ContentType()
            .map(|h| h.to_string_lossy())
            .ok()
            .filter(|m| m.starts_with("image/"))
            .unwrap_or_else(|| "image/png".into());
        let input = stream.GetInputStreamAt(0).ok()?;
        let reader = DataReader::CreateDataReader(&input).ok()?;
        let loaded = reader.LoadAsync(size).ok()?.join().ok()?;
        let mut bytes = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        Some(Thumb { mime, bytes })
    }

    pub fn control(&self, action: &str, value: f64) -> Result<()> {
        let Some(s) = self.session() else { return Ok(()) };
        match action {
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
