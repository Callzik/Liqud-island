//! Screen capture (GDI), text recognition (Windows.Media.Ocr), timestamps and
//! the Pictures\Screenshots folder.

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::Result;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MonitorFromPoint,
    ReleaseDC, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CAPTUREBLT, DIB_RGB_COLORS, HALFTONE, MONITORINFO, MONITOR_DEFAULTTONEAREST, SRCCOPY,
};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{FOLDERID_Downloads, FOLDERID_Pictures, FOLDERID_Videos, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};

use super::util::{hwnd, take_pwstr};

/// Physical rectangle of the monitor under the cursor.
pub fn monitor_at(x: i32, y: i32) -> (i32, i32, u32, u32) {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let r = mi.rcMonitor;
            (r.left, r.top, (r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32)
        } else {
            (0, 0, 1920, 1080)
        }
    }
}

/// Raw HMONITOR of the monitor that contains the point (for screen recording).
pub fn monitor_handle_at(x: i32, y: i32) -> *mut c_void {
    unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) }.0
}

/// %USERPROFILE%\Videos\Island (created if missing).
pub fn videos_dir() -> Option<std::path::PathBuf> {
    let v = unsafe { SHGetKnownFolderPath(&FOLDERID_Videos, KF_FLAG_DEFAULT, None) }.ok()?;
    let dir = std::path::PathBuf::from(take_pwstr(v)).join("Island");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Copies a screen rectangle already shrunk so its longest side is at most
/// `max` (GDI scales it while copying: no full-size copy). Plain SRCCOPY, no
/// CAPTUREBLT: that one makes the cursor flicker and is much slower, and the
/// composited desktop already has layered windows in it. Top-down BGRA.
pub fn grab_scaled(x: i32, y: i32, w: u32, h: u32, max: u32) -> Option<(u32, u32, Vec<u8>)> {
    let longest = w.max(h).max(1);
    let (sw, sh) = if longest <= max {
        (w, h)
    } else {
        let k = max as f32 / longest as f32;
        (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1))
    };
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: sw as i32,
                biHeight: -(sh as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let out = match CreateDIBSection(Some(mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) if !bits.is_null() => {
                let old = SelectObject(mem, bmp.into());
                SetStretchBltMode(mem, HALFTONE);
                let _ = SetBrushOrgEx(mem, 0, 0, None);
                let ok = StretchBlt(mem, 0, 0, sw as i32, sh as i32, Some(screen), x, y, w as i32, h as i32, SRCCOPY).as_bool();
                let px = if ok {
                    Some((sw, sh, std::slice::from_raw_parts(bits as *const u8, (sw * sh * 4) as usize).to_vec()))
                } else {
                    None
                };
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                px
            }
            _ => None,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        out
    }
}

/// Copies a screen rectangle; returns top-down BGRA pixels.
pub fn grab(x: i32, y: i32, w: u32, h: u32) -> Option<Vec<u8>> {
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let out = match CreateDIBSection(Some(mem), &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bmp) if !bits.is_null() => {
                let old = SelectObject(mem, bmp.into());
                let ok = BitBlt(mem, 0, 0, w as i32, h as i32, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
                let px = if ok {
                    Some(std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec())
                } else {
                    None
                };
                SelectObject(mem, old);
                let _ = DeleteObject(bmp.into());
                px
            }
            _ => None,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        out
    }
}

/// Hides (or shows again) one of our windows in screenshots / recordings.
pub fn exclude_from_capture(h: isize, exclude: bool) {
    if h == 0 {
        return;
    }
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd(h), if exclude { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE });
    }
}

/// Crops a top-down BGRA image.
pub fn crop(bgra: &[u8], w: u32, x: u32, y: u32, cw: u32, ch: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for row in y..y + ch {
        let o = ((row * w + x) * 4) as usize;
        out.extend_from_slice(&bgra[o..o + (cw * 4) as usize]);
    }
    out
}

pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect()
}

/// Uncompressed 32-bit BMP (instant to build; the webview decodes it natively).
pub fn bmp(bgra: &[u8], w: u32, h: u32) -> Vec<u8> {
    let data = (w * h * 4) as usize;
    let mut out = Vec::with_capacity(54 + data);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&((54 + data) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(w as i32).to_le_bytes());
    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(data as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 16]);
    out.extend(bgra.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], 255]));
    out
}

/// "20261009-191400"
pub fn local_stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

/// `dir\Island-YYYYMMDD-HHMMSS.ext`, or `… (2).ext` when that name is already
/// taken (two screenshots within one second must not overwrite each other).
pub fn unique_file(dir: &std::path::Path, ext: &str) -> std::path::PathBuf {
    let stamp = local_stamp();
    let first = dir.join(format!("Island-{stamp}.{ext}"));
    if !first.exists() {
        return first;
    }
    (2..1000)
        .map(|n| dir.join(format!("Island-{stamp} ({n}).{ext}")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// %USERPROFILE%\Pictures\Screenshots (created if missing).
pub fn screenshots_dir() -> Option<std::path::PathBuf> {
    let pics = unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) }.ok()?;
    let dir = std::path::PathBuf::from(take_pwstr(pics)).join("Screenshots");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// The user's Downloads folder.
pub fn downloads_dir() -> Option<std::path::PathBuf> {
    let p = unsafe { SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None) }.ok()?;
    Some(std::path::PathBuf::from(take_pwstr(p)))
}

/// Recognises text with the Windows OCR engine of the user's languages.
/// Must run on an MTA thread.
pub fn ocr(bgra: &[u8], w: u32, h: u32) -> Result<String> {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);

    // small crops are upscaled (OCR likes ~20px text), huge ones shrunk to the limit
    let longest = w.max(h);
    let factor = if longest * 2 <= max && h < 400 { 2 } else { 1 };
    let (mut img, mut iw, mut ih) = (bgra.to_vec(), w, h);
    if factor == 2 {
        let (nw, nh) = (w * 2, h * 2);
        let mut up = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let s = (((y / 2) * w + x / 2) * 4) as usize;
                let d = ((y * nw + x) * 4) as usize;
                up[d..d + 4].copy_from_slice(&bgra[s..s + 4]);
            }
        }
        img = up;
        iw = nw;
        ih = nh;
    } else if longest > max {
        let rgba = bgra_to_rgba(bgra);
        let (nw, nh, small) = super::img::shrink(w, h, &rgba, max);
        img = small.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
        iw = nw;
        ih = nh;
    }

    let writer = DataWriter::new()?;
    writer.WriteBytes(&img)?;
    let buffer = writer.DetachBuffer()?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, iw as i32, ih as i32)?;
    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        lines.push(line.Text()?.to_string());
    }
    Ok(lines.join("\n"))
}

/// Reads the first QR code in a top-down BGRA image.
pub fn qr(bgra: &[u8], w: u32, h: u32) -> Option<String> {
    let (w, h) = (w as usize, h as usize);
    let gray: Vec<u8> = bgra
        .chunks_exact(4)
        .map(|p| ((p[2] as u32 * 77 + p[1] as u32 * 150 + p[0] as u32 * 29) >> 8) as u8)
        .collect();
    let mut img = rqrr::PreparedImage::prepare_from_greyscale(w, h, |x, y| gray[y * w + x]);
    for grid in img.detect_grids() {
        if let Ok((_, content)) = grid.decode() {
            return Some(content);
        }
    }
    None
}
