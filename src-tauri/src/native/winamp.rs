//! Classic Winamp (and WACUP) publish no Windows media session: read and drive the
//! running player through its window ("Winamp v1.x") and the old WM_USER IPC.

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowTextW, SendMessageTimeoutW, SMTO_ABORTIFHUNG, WM_COMMAND, WM_USER};

use super::media::MediaState;

pub const APP_ID: &str = "winamp.exe";

fn window() -> Option<HWND> {
    unsafe { FindWindowW(w!("Winamp v1.x"), None) }.ok().filter(|h| !h.0.is_null())
}

fn send(h: HWND, msg: u32, wp: usize, lp: isize) -> Option<isize> {
    let mut out = 0usize;
    let r = unsafe { SendMessageTimeoutW(h, msg, WPARAM(wp), LPARAM(lp), SMTO_ABORTIFHUNG, 300, Some(&mut out)) };
    (r.0 != 0).then_some(out as isize)
}

/// "12. Artist - Title - Winamp [Paused]" → ("Artist", "Title")
fn parse_title(raw: &str) -> Option<(String, String)> {
    let mut t = raw.trim();
    if let Some(i) = t.rfind(" - Winamp") {
        t = &t[..i];
    } else if t.eq_ignore_ascii_case("winamp") || t.starts_with("Winamp ") {
        return None;
    }
    // playlist number in front ("12. ")
    if let Some((num, rest)) = t.split_once(". ") {
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            t = rest;
        }
    }
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    Some(match t.split_once(" - ") {
        Some((a, b)) if !a.trim().is_empty() && !b.trim().is_empty() => (a.trim().to_string(), b.trim().to_string()),
        _ => (String::new(), t.to_string()),
    })
}

pub fn state() -> Option<MediaState> {
    let h = window()?;
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(h, &mut buf) } as usize;
    let (artist, title) = parse_title(&String::from_utf16_lossy(&buf[..n]))?;
    // IPC_ISPLAYING: 1 playing, 3 paused, 0 stopped
    let status = send(h, WM_USER, 0, 104).unwrap_or(0);
    // IPC_GETOUTPUTTIME: position in ms (0), length in seconds (1)
    let pos = send(h, WM_USER, 0, 105).unwrap_or(0).max(0) as i64;
    let len = send(h, WM_USER, 1, 105).unwrap_or(0).max(0) as i64 * 1000;
    Some(MediaState {
        app_id: APP_ID.into(),
        title,
        artist,
        album: String::new(),
        playing: status == 1,
        position_ms: pos,
        duration_ms: len,
        updated_ms: crate::store::now_ms(),
        can_prev: true,
        can_next: true,
    })
}

pub fn control(action: &str, value: f64) {
    let Some(h) = window() else { return };
    let playing = send(h, WM_USER, 0, 104).unwrap_or(0) == 1;
    let cmd = match action {
        "prev" => 40044,
        "next" => 40048,
        "play" => 40045,
        "pause" => 40046,
        // 40046 pauses and resumes; from "stopped" only "play" starts
        "toggle" if playing || send(h, WM_USER, 0, 104).unwrap_or(0) == 3 => 40046,
        "toggle" => 40045,
        "seek" => {
            // IPC_JUMPTOTIME
            send(h, WM_USER, value.max(0.0) as usize, 106);
            return;
        }
        _ => return,
    };
    send(h, WM_COMMAND, cmd, 0);
}

#[cfg(test)]
mod tests {
    use super::parse_title;

    #[test]
    fn titles() {
        assert_eq!(parse_title("12. Daft Punk - One More Time - Winamp"), Some(("Daft Punk".into(), "One More Time".into())));
        assert_eq!(parse_title("3. Song - Winamp [Paused]"), Some(("".into(), "Song".into())));
        assert_eq!(parse_title("Winamp 5.9"), None);
    }
}
