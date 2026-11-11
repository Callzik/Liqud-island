//! Sound for screen recordings: the speakers (WASAPI loopback) and/or the microphone are
//! written to temporary WAV files next to the video, then mixed into the MP4 with
//! Windows.Media.Editing (MediaComposition) when the recording stops.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;

#[derive(Clone, Copy, PartialEq)]
pub enum Source {
    Speakers,
    Mic,
}

/// One running capture into a WAV file.
pub struct Tap {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<Option<PathBuf>>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// 32-bit float WAV; sizes are patched in `finish`.
struct Wav {
    w: BufWriter<File>,
    frames: u64,
    channels: u16,
}

impl Wav {
    fn create(path: &Path, rate: u32, channels: u16) -> std::io::Result<Self> {
        let mut w = BufWriter::new(File::create(path)?);
        let block = channels as u32 * 4;
        w.write_all(b"RIFF\0\0\0\0WAVEfmt ")?;
        w.write_all(&16u32.to_le_bytes())?;
        w.write_all(&3u16.to_le_bytes())?; // IEEE float
        w.write_all(&channels.to_le_bytes())?;
        w.write_all(&rate.to_le_bytes())?;
        w.write_all(&(rate * block).to_le_bytes())?;
        w.write_all(&(block as u16).to_le_bytes())?;
        w.write_all(&32u16.to_le_bytes())?;
        w.write_all(b"data\0\0\0\0")?;
        Ok(Wav { w, frames: 0, channels })
    }

    fn write(&mut self, samples: &[f32]) {
        for s in samples {
            let _ = self.w.write_all(&s.clamp(-1.0, 1.0).to_le_bytes());
        }
        self.frames += (samples.len() / self.channels as usize) as u64;
    }

    fn silence(&mut self, frames: u64) {
        let zeros = vec![0f32; (frames as usize) * self.channels as usize];
        self.write(&zeros);
    }

    fn finish(mut self) -> std::io::Result<()> {
        let data = self.frames * self.channels as u64 * 4;
        self.w.flush()?;
        let f = self.w.get_mut();
        f.seek(SeekFrom::Start(4))?;
        f.write_all(&((36 + data) as u32).to_le_bytes())?;
        f.seek(SeekFrom::Start(40))?;
        f.write_all(&(data as u32).to_le_bytes())?;
        f.flush()
    }
}

impl Tap {
    pub fn start(src: Source, path: PathBuf) -> Result<Tap, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        let st = stop.clone();
        let thread = thread::Builder::new()
            .name("rec-audio".into())
            .spawn(move || {
                let host = cpal::default_host();
                let dev = match src {
                    Source::Speakers => host.default_output_device(),
                    Source::Mic => host.default_input_device(),
                };
                let Some(dev) = dev else {
                    let _ = ready_tx.send(Err(if src == Source::Mic { "нет микрофона" } else { "нет устройства вывода" }.into()));
                    return None;
                };
                let cfg = match src {
                    Source::Speakers => dev.default_output_config(),
                    Source::Mic => dev.default_input_config(),
                };
                let cfg = match cfg {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return None;
                    }
                };
                let rate = cfg.sample_rate().0;
                let channels = cfg.channels();
                let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(rate as usize)));
                let b = buf.clone();
                let err = |e| crate::log::warn(&format!("звук записи: {e}"));
                // on WASAPI an input stream on an output device is a loopback capture
                let stream = match cfg.sample_format() {
                    SampleFormat::F32 => dev.build_input_stream(&cfg.config(), move |d: &[f32], _| lock(&b).extend_from_slice(d), err, None),
                    SampleFormat::I16 => dev.build_input_stream(
                        &cfg.config(),
                        move |d: &[i16], _| lock(&b).extend(d.iter().map(|&x| x as f32 / 32768.0)),
                        err,
                        None,
                    ),
                    other => {
                        let _ = ready_tx.send(Err(format!("формат {other:?} не поддерживается")));
                        return None;
                    }
                };
                let stream = match stream {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return None;
                    }
                };
                if let Err(e) = stream.play() {
                    let _ = ready_tx.send(Err(e.to_string()));
                    return None;
                }
                let mut wav = match Wav::create(&path, rate, channels) {
                    Ok(w) => w,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.to_string()));
                        return None;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                let t0 = Instant::now();
                let mut chunk = Vec::new();
                loop {
                    let stopping = st.load(Ordering::Relaxed);
                    thread::sleep(Duration::from_millis(if stopping { 0 } else { 100 }));
                    std::mem::swap(&mut *lock(&buf), &mut chunk);
                    let whole = chunk.len() - chunk.len() % channels as usize;
                    wav.write(&chunk[..whole]);
                    chunk.clear();
                    // loopback delivers nothing while the PC is silent: fill the gap so the
                    // sound stays in sync with the picture
                    let expected = (t0.elapsed().as_secs_f64() * rate as f64) as u64;
                    let slack = rate as u64 / 5;
                    if wav.frames + slack < expected {
                        wav.silence(expected - wav.frames - slack / 2);
                    }
                    if stopping {
                        break;
                    }
                }
                drop(stream);
                wav.finish().ok().map(|_| path)
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Tap { stop, thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err("звук не запустился".into())
            }
        }
    }

    pub fn finish(mut self) -> Option<PathBuf> {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().and_then(|t| t.join().ok().flatten())
    }
}

impl Drop for Tap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Mixes the WAV tracks into the video (re-encodes through MediaComposition) and replaces it.
pub fn mux(video: &Path, tracks: &[PathBuf]) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Media::Editing::{BackgroundAudioTrack, MediaClip, MediaComposition};
    use windows::Media::Transcoding::TranscodeFailureReason;
    use windows::Storage::{CreationCollisionOption, StorageFile, StorageFolder};

    let _com = crate::native::util::Com::mta();
    let e = |e: windows::core::Error| e.message().to_string();
    let file = |p: &Path| StorageFile::GetFileFromPathAsync(&HSTRING::from(p.as_os_str())).and_then(|a| a.join());

    let comp = MediaComposition::new().map_err(e)?;
    let clip = MediaClip::CreateFromFileAsync(&file(video).map_err(e)?).and_then(|a| a.join()).map_err(e)?;
    comp.Clips().map_err(e)?.Append(&clip).map_err(e)?;
    for t in tracks {
        let f = file(t).map_err(e)?;
        let track = BackgroundAudioTrack::CreateFromFileAsync(&f).and_then(|a| a.join()).map_err(e)?;
        comp.BackgroundAudioTracks().map_err(e)?.Append(&track).map_err(e)?;
    }
    let dir = video.parent().ok_or("нет папки")?;
    let tmp_name = format!("{}.sound.mp4", video.file_stem().and_then(|s| s.to_str()).unwrap_or("rec"));
    let folder = StorageFolder::GetFolderFromPathAsync(&HSTRING::from(dir.as_os_str())).and_then(|a| a.join()).map_err(e)?;
    let out = folder
        .CreateFileAsync(&HSTRING::from(tmp_name.as_str()), CreationCollisionOption::ReplaceExisting)
        .and_then(|a| a.join())
        .map_err(e)?;
    let res = comp.RenderToFileAsync(&out).and_then(|a| a.join()).map_err(e)?;
    let tmp = dir.join(&tmp_name);
    if res != TranscodeFailureReason::None {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("сведение не удалось ({})", res.0));
    }
    std::fs::rename(&tmp, video).map_err(|e| e.to_string())
}
