//! What the speakers play, split into bands for the equalizer bars: WASAPI
//! loopback of the default output device (cpal), a 1024-point FFT on request.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};

pub const BANDS: usize = 4;
const N: usize = 1024;
/// band edges, Hz: bass, low mids, high mids, treble
const EDGES: [f32; BANDS + 1] = [40.0, 250.0, 1200.0, 4500.0, 14000.0];

struct Ring {
    buf: Vec<f32>,
    pos: usize,
    fresh: Option<Instant>,
}

pub struct Spectrum {
    ring: Arc<Mutex<Ring>>,
    rate: Arc<AtomicU32>,
    want: Arc<AtomicBool>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn push(ring: &Mutex<Ring>, data: &[f32], channels: usize) {
    let mut r = lock(ring);
    for frame in data.chunks(channels.max(1)) {
        let v = frame.iter().sum::<f32>() / frame.len() as f32;
        let p = r.pos;
        r.buf[p] = v;
        r.pos = (p + 1) % N;
    }
    r.fresh = Some(Instant::now());
}

fn open(ring: &Arc<Mutex<Ring>>, rate: &AtomicU32, broken: &Arc<AtomicBool>) -> Result<(Stream, String), String> {
    let host = cpal::default_host();
    let dev = host.default_output_device().ok_or("нет устройства вывода")?;
    let name = dev.name().unwrap_or_default();
    // on WASAPI an input stream on an output device is a loopback capture
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    rate.store(cfg.sample_rate().0, Ordering::Relaxed);
    let channels = cfg.channels() as usize;
    let stream_cfg = cfg.config();
    let (r, b) = (ring.clone(), broken.clone());
    let err = move |_e| b.store(true, Ordering::Relaxed);
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => dev.build_input_stream(&stream_cfg, move |d: &[f32], _| push(&r, d, channels), err, None),
        SampleFormat::I16 => dev.build_input_stream(
            &stream_cfg,
            move |d: &[i16], _| {
                let f: Vec<f32> = d.iter().map(|&x| x as f32 / 32768.0).collect();
                push(&r, &f, channels)
            },
            err,
            None,
        ),
        other => return Err(format!("формат {other:?} не поддерживается")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, name))
}

impl Spectrum {
    /// The capture runs on its own thread (cpal streams can't move between
    /// threads) and only while `set_active(true)`.
    pub fn spawn() -> Self {
        let ring = Arc::new(Mutex::new(Ring { buf: vec![0.0; N], pos: 0, fresh: None }));
        let rate = Arc::new(AtomicU32::new(48_000));
        let want = Arc::new(AtomicBool::new(false));
        let (r, rt, w) = (ring.clone(), rate.clone(), want.clone());
        let _ = thread::Builder::new().name("spectrum".into()).spawn(move || {
            let broken = Arc::new(AtomicBool::new(false));
            let mut stream: Option<(Stream, String)> = None;
            let mut last_check = Instant::now();
            let mut fails = 0u32;
            let mut logged = false;
            loop {
                thread::sleep(Duration::from_millis(250));
                if !w.load(Ordering::Relaxed) {
                    stream = None;
                    continue;
                }
                // follow the default output device (headphones plugged in, etc.)
                if stream.is_some() && last_check.elapsed() >= Duration::from_secs(2) {
                    last_check = Instant::now();
                    let now = cpal::default_host().default_output_device().and_then(|d| d.name().ok());
                    if now.as_deref() != stream.as_ref().map(|s| s.1.as_str()) {
                        stream = None;
                    }
                }
                if broken.swap(false, Ordering::Relaxed) {
                    stream = None;
                }
                if stream.is_none() {
                    // after failures retry less often
                    if fails > 0 && last_check.elapsed() < Duration::from_secs((fails * 2).min(30) as u64) {
                        continue;
                    }
                    last_check = Instant::now();
                    match open(&r, &rt, &broken) {
                        Ok(s) => {
                            fails = 0;
                            stream = Some(s);
                        }
                        Err(e) => {
                            fails += 1;
                            if !logged {
                                logged = true;
                                crate::log::warn(&format!("спектр звука недоступен: {e}"));
                            }
                        }
                    }
                }
            }
        });
        Self { ring, rate, want }
    }

    pub fn set_active(&self, on: bool) {
        self.want.store(on, Ordering::Relaxed);
    }

    /// Band levels 0..1, or None while nothing new came in (silence, no device).
    pub fn bands(&self) -> Option<[f32; BANDS]> {
        let samples = {
            let r = lock(&self.ring);
            if r.fresh.map_or(true, |t| t.elapsed() > Duration::from_millis(300)) {
                return None;
            }
            let mut s = Vec::with_capacity(N);
            s.extend_from_slice(&r.buf[r.pos..]);
            s.extend_from_slice(&r.buf[..r.pos]);
            s
        };
        Some(analyse(&samples, self.rate.load(Ordering::Relaxed) as f32))
    }
}

fn analyse(samples: &[f32], rate: f32) -> [f32; BANDS] {
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (N - 1) as f32).cos();
            v * w
        })
        .collect();
    let mut im = vec![0.0f32; N];
    fft(&mut re, &mut im);
    let hz = rate / N as f32;
    let mut out = [0.0f32; BANDS];
    for (b, o) in out.iter_mut().enumerate() {
        let lo = ((EDGES[b] / hz) as usize).max(1);
        let hi = ((EDGES[b + 1] / hz) as usize).clamp(lo + 1, N / 2);
        let mut best = 0.0f32;
        for k in lo..hi {
            best = best.max((re[k] * re[k] + im[k] * im[k]).sqrt());
        }
        // amplitude of a sine (Hann window halves it), then dBFS → 0..1
        let amp = best * 4.0 / N as f32;
        let db = 20.0 * amp.max(1e-6).log10();
        // treble is naturally quieter: give it a little lift
        let tilt = b as f32 * 4.0;
        *o = ((db + tilt + 54.0) / 48.0).clamp(0.0, 1.0);
    }
    out
}

/// In-place iterative radix-2 FFT (N is a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_lands_in_its_band() {
        let rate = 48_000.0;
        let s: Vec<f32> = (0..N).map(|i| (std::f32::consts::TAU * 2000.0 * i as f32 / rate).sin() * 0.5).collect();
        let b = analyse(&s, rate);
        assert!(b[2] > 0.8, "{b:?}");
        assert!(b[0] < 0.3 && b[3] < 0.5, "{b:?}");
    }

    #[test]
    fn silence_is_zero() {
        assert_eq!(analyse(&vec![0.0; N], 48_000.0), [0.0; BANDS]);
    }
}
