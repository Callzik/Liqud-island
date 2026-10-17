//! Start-menu apps, shell icons/thumbnails, launching and the file picker.
//! Everything here must run on an STA thread (see `ShellWorker` in lib.rs).

use std::ffi::c_void;
use std::mem::size_of;

use windows::core::{Interface, Result, PCWSTR};
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::System::Com::{CoCreateInstance, IBindCtx, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FileOpenDialog, FOLDERID_AppsFolder, IEnumShellItems, IFileOpenDialog, IShellItem,
    IShellItemImageFactory, SHCreateItemFromParsingName, SHGetKnownFolderItem, ShellExecuteW, FOS_ALLOWMULTISELECT,
    FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, KF_FLAG_DEFAULT, SIGDN_FILESYSPATH, SIGDN_NORMALDISPLAY,
    SIGDN_PARENTRELATIVEPARSING, SIIGBF, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::util::{hwnd, pcwstr, take_pwstr, wide};

#[derive(Clone, Debug)]
pub struct AppEntry {
    /// Parsing name inside shell:AppsFolder (AUMID or path).
    pub id: String,
    pub name: String,
}

fn is_noise(name: &str) -> bool {
    let n = name.to_lowercase();
    ["uninstall", "удален", "удалить", "деинсталл", "readme", "release notes", "documentation", "website"]
        .iter()
        .any(|w| n.contains(w))
}

/// Everything that shows up in Start → All apps (Win32 + UWP/Store apps).
pub fn list_apps() -> Result<Vec<AppEntry>> {
    unsafe {
        let folder: IShellItem = SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None)?;
        let items: IEnumShellItems = folder.BindToHandler(None::<&IBindCtx>, &BHID_EnumItems)?;
        let mut out = Vec::new();
        loop {
            let mut batch: [Option<IShellItem>; 1] = [None];
            let mut fetched = 0u32;
            if items.Next(&mut batch, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = batch[0].take() else { break };
            let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(take_pwstr).unwrap_or_default();
            let id = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING).map(take_pwstr).unwrap_or_default();
            if name.is_empty() || id.is_empty() || is_noise(&name) {
                continue;
            }
            // Skip web links and documents pinned into the Start menu.
            let lid = id.to_lowercase();
            if lid.starts_with("http") || lid.ends_with(".url") || lid.ends_with(".txt") || lid.ends_with(".chm") {
                continue;
            }
            out.push(AppEntry { id, name });
        }
        out.sort_by_key(|a| a.name.to_lowercase());
        out.dedup_by(|a, b| a.name == b.name);
        Ok(out)
    }
}

/// Name of an app as Start shows it, by its AUMID (media sessions report
/// only that). Desktop apps without an AUMID report their exe name or path;
/// those are matched against the Start-menu shortcuts' targets.
pub fn app_display_name(aumid: &str) -> Option<String> {
    let name_of = |parsing: &str| unsafe {
        let w = wide(parsing);
        let item: IShellItem = SHCreateItemFromParsingName(pcwstr(&w), None::<&IBindCtx>).ok()?;
        let n = item.GetDisplayName(SIGDN_NORMALDISPLAY).map(take_pwstr).ok()?;
        (!n.trim().is_empty()).then(|| n.trim().to_string())
    };
    if !aumid.contains('\\') {
        if let Some(n) = name_of(&format!("shell:AppsFolder\\{aumid}")) {
            // an unknown id may come back as itself
            if !n.eq_ignore_ascii_case(aumid) {
                return Some(n);
            }
        }
    }
    let exe = aumid.rsplit(['\\', '/']).next().unwrap_or(aumid).to_lowercase();
    if !exe.ends_with(".exe") {
        return None;
    }
    list_apps()
        .ok()?
        .into_iter()
        .find(|a| a.id.to_lowercase().rsplit(['\\', '/']).next() == Some(exe.as_str()))
        .map(|a| a.name)
}

/// `target` is either `app:<parsing name>` or a filesystem path.
fn parsing_name(target: &str) -> String {
    match target.strip_prefix("app:") {
        Some(id) => format!("shell:AppsFolder\\{id}"),
        None => target.to_string(),
    }
}

/// RGBA pixels of the shell icon (or thumbnail when `thumbnail` is true).
pub fn icon_rgba(target: &str, size: i32, thumbnail: bool) -> Option<(u32, u32, Vec<u8>)> {
    unsafe {
        let w = wide(&parsing_name(target));
        let item: IShellItem = SHCreateItemFromParsingName(pcwstr(&w), None::<&IBindCtx>).ok()?;
        let factory: IShellItemImageFactory = item.cast().ok()?;
        let flags = if thumbnail { SIIGBF_BIGGERSIZEOK } else { SIIGBF(SIIGBF_ICONONLY.0 | SIIGBF_BIGGERSIZEOK.0) };
        let hbmp: HBITMAP = factory.GetImage(SIZE { cx: size, cy: size }, flags).ok()?;
        let result = bitmap_rgba(hbmp);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        result
    }
}

unsafe fn bitmap_rgba(hbmp: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut bm = BITMAP::default();
    let got = GetObjectW(HGDIOBJ(hbmp.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut c_void));
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut buf = vec![0u8; (w * h * 4) as usize];
    let hdc = GetDC(None);
    let lines = GetDIBits(
        hdc,
        hbmp,
        0,
        h as u32,
        Some(buf.as_mut_ptr() as *mut c_void),
        &mut info,
        DIB_RGB_COLORS,
    );
    ReleaseDC(None, hdc);
    if lines == 0 {
        return None;
    }
    // BGRA (premultiplied for icons) -> straight RGBA
    let any_alpha = buf.chunks_exact(4).any(|p| p[3] != 0);
    let premultiplied = any_alpha && buf.chunks_exact(4).all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]);
    for p in buf.chunks_exact_mut(4) {
        let (b, g, r) = (p[0], p[1], p[2]);
        let a = if any_alpha { p[3] } else { 255 };
        if premultiplied && a > 0 && a < 255 {
            let f = 255.0 / a as f32;
            p[0] = (r as f32 * f).min(255.0) as u8;
            p[1] = (g as f32 * f).min(255.0) as u8;
            p[2] = (b as f32 * f).min(255.0) as u8;
        } else {
            p[0] = r;
            p[1] = g;
            p[2] = b;
        }
        p[3] = a;
    }
    Some((w as u32, h as u32, buf))
}

/// Opens an app / file / folder / URL with the shell. Returns success.
pub fn open(target: &str) -> bool {
    let file = wide(&parsing_name(target));
    let verb = wide("open");
    let r = unsafe { ShellExecuteW(None, pcwstr(&verb), pcwstr(&file), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    r.0 as isize > 32
}

/// Opens Explorer with the item selected.
pub fn reveal(path: &str) -> bool {
    let verb = wide("open");
    let exe = wide("explorer.exe");
    let args = wide(&format!("/select,\"{path}\""));
    let r = unsafe { ShellExecuteW(None, pcwstr(&verb), pcwstr(&exe), pcwstr(&args), PCWSTR::null(), SW_SHOWNORMAL) };
    r.0 as isize > 32
}

/// Native multi-select "Open" dialog. Empty vec when cancelled.
pub fn pick_files(owner: isize, title: &str) -> Vec<String> {
    unsafe {
        let Ok(dlg) = CoCreateInstance::<_, IFileOpenDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER) else {
            return Vec::new();
        };
        if let Ok(opts) = dlg.GetOptions() {
            let _ = dlg.SetOptions(opts | FOS_ALLOWMULTISELECT | FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST);
        }
        let t = wide(title);
        let _ = dlg.SetTitle(pcwstr(&t));
        let owner = if owner != 0 { Some(hwnd(owner)) } else { None };
        if dlg.Show(owner).is_err() {
            return Vec::new();
        }
        let Ok(results) = dlg.GetResults() else { return Vec::new() };
        let n = results.GetCount().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..n {
            if let Ok(item) = results.GetItemAt(i) {
                if let Ok(p) = item.GetDisplayName(SIGDN_FILESYSPATH) {
                    out.push(take_pwstr(p));
                }
            }
        }
        out
    }
}
