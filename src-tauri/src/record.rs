//! Screen recording to MP4: Windows.Graphics.Capture frames (windows-capture)
//! → Media Foundation H.264 encoder. A region is cut out of every frame.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};

type Shared = Arc<Mutex<Option<VideoEncoder>>>;

pub struct Flags {
    encoder: Shared,
    /// (x0, y0, x1, y1) in monitor pixels, or the whole monitor
    crop: Option<(u32, u32, u32, u32)>,
}

struct Handler {
    encoder: Shared,
    crop: Option<(u32, u32, u32, u32)>,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = String;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self {
            encoder: ctx.flags.encoder,
            crop: ctx.flags.crop,
        })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        let mut guard = self.encoder.lock().unwrap_or_else(|e| e.into_inner());
        let Some(enc) = guard.as_mut() else {
            // finished from outside: stop capturing
            control.stop();
            return Ok(());
        };
        let ts = frame.timestamp().Duration;
        match self.crop {
            Some((x0, y0, x1, y1)) => {
                let mut buf = frame.buffer_crop(x0, y0, x1, y1).map_err(|e| e.to_string())?;
                let px = buf.as_nopadding_buffer().map_err(|e| e.to_string())?;
                enc.send_frame_buffer(px, ts).map_err(|e| e.to_string())?;
            }
            None => enc.send_frame(frame).map_err(|e| e.to_string())?,
        }
        Ok(())
    }
}

pub struct Recording {
    control: Option<CaptureControl<Handler, String>>,
    encoder: Shared,
    pub path: PathBuf,
    taps: Vec<crate::recaudio::Tap>,
    /// why the sound could not start (shown when the video is saved)
    audio_error: Option<String>,
}

/// H.264 wants even frame sizes.
fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

/// Starts recording the monitor `hmonitor` (`mon_w`×`mon_h` pixels), optionally
/// only the `rect` (x, y, w, h) part of it.
pub fn start(
    hmonitor: *mut std::ffi::c_void,
    mon_w: u32,
    mon_h: u32,
    rect: Option<(u32, u32, u32, u32)>,
    path: PathBuf,
    audio: &str,
) -> Result<Recording, String> {
    let (crop, w, h) = match rect {
        Some((x, y, w, h)) => {
            let w = even(w.min(mon_w - x));
            let h = even(h.min(mon_h - y));
            (Some((x, y, x + w, y + h)), w, h)
        }
        None => (None, even(mon_w), even(mon_h)),
    };
    let encoder = VideoEncoder::new(
        VideoSettingsBuilder::new(w, h)
            .sub_type(VideoSettingsSubType::H264)
            .frame_rate(30)
            .bitrate((w * h * 4).clamp(4_000_000, 16_000_000)),
        AudioSettingsBuilder::default().disabled(true),
        ContainerSettingsBuilder::default(),
        &path,
    )
    .map_err(|e| format!("Кодировщик видео недоступен: {e}"))?;
    let shared: Shared = Arc::new(Mutex::new(Some(encoder)));
    let settings = Settings::new(
        Monitor::from_raw_hmonitor(hmonitor),
        CursorCaptureSettings::WithCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        Flags {
            encoder: shared.clone(),
            crop,
        },
    );
    let control = Handler::start_free_threaded(settings).map_err(|e| format!("Захват экрана недоступен: {e}"))?;
    // sound goes to temporary WAV files beside the video and is mixed in at the end
    use crate::recaudio::{Source, Tap};
    let mut taps = Vec::new();
    let mut audio_error = None;
    let sources: &[Source] = match audio {
        "system" => &[Source::Speakers],
        "mic" => &[Source::Speakers, Source::Mic],
        _ => &[],
    };
    for (i, src) in sources.iter().enumerate() {
        match Tap::start(*src, path.with_extension(format!("a{i}.wav"))) {
            Ok(t) => taps.push(t),
            Err(e) => audio_error = Some(e),
        }
    }
    Ok(Recording {
        control: Some(control),
        encoder: shared,
        path,
        taps,
        audio_error,
    })
}

impl Recording {
    pub fn has_audio(&self) -> bool {
        !self.taps.is_empty()
    }

    /// Stops capturing and finalises the MP4 file; the second value is a warning about the sound.
    pub fn finish(mut self) -> Result<(PathBuf, Option<String>), String> {
        let enc = self.encoder.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(c) = self.control.take() {
            let _ = c.stop();
        }
        let wavs: Vec<PathBuf> = self.taps.drain(..).filter_map(|t| t.finish()).collect();
        let video = match enc {
            Some(e) => e.finish().map(|_| self.path.clone()).map_err(|e| e.to_string()),
            None => Err("запись уже остановлена".into()),
        };
        let mut warn = self.audio_error.take().map(|e| format!("Без звука: {e}"));
        if let (Ok(v), false) = (&video, wavs.is_empty()) {
            if let Err(e) = crate::recaudio::mux(v, &wavs) {
                crate::log::warn(&format!("звук записи: {e}"));
                warn = Some(format!("Звук не добавился: {e}"));
            }
        }
        for w in &wavs {
            let _ = std::fs::remove_file(w);
        }
        video.map(|v| (v, warn))
    }
}
