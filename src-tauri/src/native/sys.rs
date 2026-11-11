//! Control center: Wi-Fi networks (Native Wifi API), Bluetooth / Wi-Fi radios (WinRT Radios),
//! dark theme (registry), Windows VPN connections (RAS), lock / sleep / restart / shut down.

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::process::Command;

use serde::Serialize;
use windows::core::{GUID, PCWSTR};
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::Foundation::{HANDLE, LPARAM, WPARAM};
use windows::Win32::NetworkManagement::Rras::{
    RasEnumConnectionsW, RasEnumEntriesW, RasHangUpW, RASCONNW, RASENTRYNAMEW,
};
use windows::Win32::NetworkManagement::WiFi::*;
use windows::Win32::System::Registry::{
    RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
};

use super::util::{pcwstr, wide, Com};

const NO_WINDOW: u32 = 0x0800_0000;

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Net {
    pub ssid: String,
    /// 0..100
    pub signal: u32,
    pub secure: bool,
    pub connected: bool,
    /// a saved profile exists, so no password is needed
    pub known: bool,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SysState {
    /// None = no such radio on this PC
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
    pub dark: bool,
    pub vpn: Vec<Vpn>,
    pub ssid: String,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Vpn {
    pub name: String,
    pub connected: bool,
}

// ---------- Wi-Fi ----------

struct Wlan(HANDLE);

impl Wlan {
    fn open() -> Result<Self, String> {
        let mut ver = 0u32;
        let mut h = HANDLE::default();
        let r = unsafe { WlanOpenHandle(2, None, &mut ver, &mut h) };
        if r != 0 {
            return Err(format!("Wi-Fi недоступен ({r})"));
        }
        Ok(Wlan(h))
    }

    fn interfaces(&self) -> Vec<(GUID, bool)> {
        let mut list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if unsafe { WlanEnumInterfaces(self.0, None, &mut list) } != 0 || list.is_null() {
            return vec![];
        }
        let out = unsafe {
            let n = (*list).dwNumberOfItems as usize;
            let first = (*list).InterfaceInfo.as_ptr();
            (0..n)
                .map(|i| {
                    let it = &*first.add(i);
                    (it.InterfaceGuid, it.isState == wlan_interface_state_connected)
                })
                .collect()
        };
        unsafe { WlanFreeMemory(list as *const c_void) };
        out
    }

    fn iface(&self) -> Result<GUID, String> {
        self.interfaces().first().map(|i| i.0).ok_or_else(|| "Нет адаптера Wi-Fi".to_string())
    }

    fn current_ssid(&self, g: &GUID) -> String {
        let mut size = 0u32;
        let mut data: *mut c_void = std::ptr::null_mut();
        let r = unsafe { WlanQueryInterface(self.0, g, wlan_intf_opcode_current_connection, None, &mut size, &mut data, None) };
        if r != 0 || data.is_null() {
            return String::new();
        }
        let s = unsafe {
            let a = &*(data as *const WLAN_CONNECTION_ATTRIBUTES);
            if a.isState == wlan_interface_state_connected {
                ssid_str(&a.wlanAssociationAttributes.dot11Ssid)
            } else {
                String::new()
            }
        };
        unsafe { WlanFreeMemory(data) };
        s
    }
}

impl Drop for Wlan {
    fn drop(&mut self) {
        unsafe { WlanCloseHandle(self.0, None) };
    }
}

fn ssid_str(s: &DOT11_SSID) -> String {
    let n = (s.uSSIDLength as usize).min(32);
    String::from_utf8_lossy(&s.ucSSID[..n]).into_owned()
}

fn from_wide(w: &[u16]) -> String {
    let n = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..n])
}

/// Starts a background scan; results show up in `wifi_list` a few seconds later.
pub fn wifi_scan() {
    if let Ok(w) = Wlan::open() {
        for (g, _) in w.interfaces() {
            unsafe { WlanScan(w.0, &g, None, None, None) };
        }
    }
}

pub fn wifi_list() -> Result<Vec<Net>, String> {
    let w = Wlan::open()?;
    let g = w.iface()?;
    let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
    let r = unsafe { WlanGetAvailableNetworkList(w.0, &g, 0, None, &mut list) };
    if r != 0 || list.is_null() {
        return Err(format!("Не удалось получить сети ({r})"));
    }
    let mut out: Vec<Net> = vec![];
    unsafe {
        let n = (*list).dwNumberOfItems as usize;
        let first = (*list).Network.as_ptr();
        for i in 0..n {
            let it = &*first.add(i);
            let ssid = ssid_str(&it.dot11Ssid);
            if ssid.is_empty() {
                continue;
            }
            let net = Net {
                ssid,
                signal: it.wlanSignalQuality,
                secure: it.bSecurityEnabled.as_bool(),
                connected: it.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED != 0,
                known: it.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0,
            };
            // the same SSID is listed once per profile: keep the best entry
            match out.iter_mut().find(|o| o.ssid == net.ssid) {
                Some(o) => {
                    o.connected |= net.connected;
                    o.known |= net.known;
                    o.signal = o.signal.max(net.signal);
                }
                None => out.push(net),
            }
        }
        WlanFreeMemory(list as *const c_void);
    }
    out.sort_by(|a, b| b.connected.cmp(&a.connected).then(b.signal.cmp(&a.signal)));
    Ok(out)
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

fn find_net(w: &Wlan, g: &GUID, ssid: &str) -> Option<WLAN_AVAILABLE_NETWORK> {
    let mut list: *mut WLAN_AVAILABLE_NETWORK_LIST = std::ptr::null_mut();
    if unsafe { WlanGetAvailableNetworkList(w.0, g, 0, None, &mut list) } != 0 || list.is_null() {
        return None;
    }
    let mut found: Option<WLAN_AVAILABLE_NETWORK> = None;
    unsafe {
        let n = (*list).dwNumberOfItems as usize;
        let first = (*list).Network.as_ptr();
        for i in 0..n {
            let it = &*first.add(i);
            if ssid_str(&it.dot11Ssid) == ssid {
                // prefer the entry that already has a profile
                if found.is_none() || it.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE != 0 {
                    found = Some(*it);
                }
            }
        }
        WlanFreeMemory(list as *const c_void);
    }
    found
}

/// Connects to `ssid`. With a saved profile the password is ignored; otherwise a profile
/// is created (WPA2/WPA3-personal or open) the same way Windows does it.
pub fn wifi_connect(ssid: &str, password: Option<&str>) -> Result<(), String> {
    let w = Wlan::open()?;
    let g = w.iface()?;
    let net = find_net(&w, &g, ssid).ok_or("Сеть не найдена")?;
    let mut profile = from_wide(&net.strProfileName);
    if profile.is_empty() || net.dwFlags & WLAN_AVAILABLE_NETWORK_HAS_PROFILE == 0 {
        let auth = net.dot11DefaultAuthAlgorithm;
        let (auth_s, enc_s) = if !net.bSecurityEnabled.as_bool() || auth == DOT11_AUTH_ALGO_80211_OPEN {
            ("open", "none")
        } else if auth == DOT11_AUTH_ALGO_WPA3_SAE {
            ("WPA3SAE", "AES")
        } else if auth == DOT11_AUTH_ALGO_WPA_PSK {
            ("WPAPSK", if net.dot11DefaultCipherAlgorithm == DOT11_CIPHER_ALGO_CCMP { "AES" } else { "TKIP" })
        } else if auth == DOT11_AUTH_ALGO_RSNA_PSK {
            ("WPA2PSK", "AES")
        } else {
            return Err("Эта сеть требует входа через параметры Windows".into());
        };
        let key = if auth_s == "open" {
            String::new()
        } else {
            let pw = password.unwrap_or("");
            if pw.len() < 8 {
                return Err("Пароль слишком короткий".into());
            }
            format!(
                "<sharedKey><keyType>passPhrase</keyType><protected>false</protected><keyMaterial>{}</keyMaterial></sharedKey>",
                xml_escape(pw)
            )
        };
        let hex: String = ssid.as_bytes().iter().map(|b| format!("{b:02X}")).collect();
        let name = xml_escape(ssid);
        let xml = format!(
            r#"<?xml version="1.0"?><WLANProfile xmlns="http://www.microsoft.com/networking/WLAN/profile/v1"><name>{name}</name><SSIDConfig><SSID><hex>{hex}</hex><name>{name}</name></SSID></SSIDConfig><connectionType>ESS</connectionType><connectionMode>auto</connectionMode><MSM><security><authEncryption><authentication>{auth_s}</authentication><encryption>{enc_s}</encryption><useOneX>false</useOneX></authEncryption>{key}</security></MSM></WLANProfile>"#
        );
        let xw = wide(&xml);
        let mut reason = 0u32;
        let r = unsafe { WlanSetProfile(w.0, &g, 0, pcwstr(&xw), PCWSTR::null(), true, None, &mut reason) };
        if r != 0 {
            return Err(format!("Не удалось сохранить сеть ({r}/{reason})"));
        }
        profile = ssid.to_string();
    }
    let pw = wide(&profile);
    let params = WLAN_CONNECTION_PARAMETERS {
        wlanConnectionMode: wlan_connection_mode_profile,
        strProfile: pcwstr(&pw),
        pDot11Ssid: std::ptr::null_mut(),
        pDesiredBssidList: std::ptr::null_mut(),
        dot11BssType: dot11_BSS_type_any,
        dwFlags: 0,
    };
    let r = unsafe { WlanConnect(w.0, &g, &params, None) };
    if r != 0 {
        return Err(format!("Не удалось подключиться ({r})"));
    }
    Ok(())
}

pub fn wifi_disconnect() -> Result<(), String> {
    let w = Wlan::open()?;
    let g = w.iface()?;
    let r = unsafe { WlanDisconnect(w.0, &g, None) };
    if r != 0 {
        return Err(format!("Не удалось отключиться ({r})"));
    }
    Ok(())
}

// ---------- radios ----------

fn radio(kind: RadioKind) -> Option<Radio> {
    let all = Radio::GetRadiosAsync().ok()?.join().ok()?;
    (0..all.Size().ok()?).filter_map(|i| all.GetAt(i).ok()).find(|r| r.Kind().ok() == Some(kind))
}

fn radio_on(kind: RadioKind) -> Option<bool> {
    radio(kind).and_then(|r| r.State().ok()).map(|s| s == RadioState::On)
}

pub fn set_radio(kind: &str, on: bool) -> Result<(), String> {
    let _com = Com::mta();
    let k = if kind == "bluetooth" { RadioKind::Bluetooth } else { RadioKind::WiFi };
    let _ = Radio::RequestAccessAsync().and_then(|a| a.join());
    let r = radio(k).ok_or("Нет такого адаптера")?;
    let st = if on { RadioState::On } else { RadioState::Off };
    let res = r.SetStateAsync(st).and_then(|a| a.join()).map_err(|e| e.to_string())?;
    // RadioAccessStatus::Allowed == 1
    if res.0 != 1 {
        return Err("Windows не разрешила переключить".into());
    }
    Ok(())
}

// ---------- theme ----------

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

fn reg_dword(name: &str) -> Option<u32> {
    let key = wide(PERSONALIZE);
    let n = wide(name);
    let mut v = 0u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, pcwstr(&key), pcwstr(&n), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut c_void), Some(&mut size))
    };
    r.is_ok().then_some(v)
}

pub fn dark_mode() -> bool {
    reg_dword("AppsUseLightTheme").map(|v| v == 0).unwrap_or(false)
}

pub fn set_dark_mode(dark: bool) -> Result<(), String> {
    let key = wide(PERSONALIZE);
    let v: u32 = if dark { 0 } else { 1 };
    for name in ["AppsUseLightTheme", "SystemUsesLightTheme"] {
        let n = wide(name);
        let r = unsafe {
            RegSetKeyValueW(HKEY_CURRENT_USER, pcwstr(&key), pcwstr(&n), REG_DWORD.0, Some(&v as *const u32 as *const c_void), 4)
        };
        if r.is_err() {
            return Err("Не удалось сменить тему".into());
        }
    }
    let area = wide("ImmersiveColorSet");
    unsafe {
        SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, WPARAM(0), LPARAM(area.as_ptr() as isize), SMTO_ABORTIFHUNG, 200, None);
    }
    Ok(())
}

// ---------- VPN (Windows built-in connections) ----------

fn ras_entries() -> Vec<String> {
    let mut cb = 0u32;
    let mut n = 0u32;
    unsafe { RasEnumEntriesW(PCWSTR::null(), PCWSTR::null(), None, &mut cb, &mut n) };
    if n == 0 {
        return vec![];
    }
    let mut buf = vec![RASENTRYNAMEW { dwSize: std::mem::size_of::<RASENTRYNAMEW>() as u32, ..Default::default() }; n as usize];
    cb = (buf.len() * std::mem::size_of::<RASENTRYNAMEW>()) as u32;
    if unsafe { RasEnumEntriesW(PCWSTR::null(), PCWSTR::null(), Some(buf.as_mut_ptr()), &mut cb, &mut n) } != 0 {
        return vec![];
    }
    buf.iter().take(n as usize).map(|e| from_wide(&e.szEntryName)).filter(|s| !s.is_empty()).collect()
}

fn ras_active() -> Vec<RASCONNW> {
    let mut buf: Vec<RASCONNW> = vec![unsafe { std::mem::zeroed() }; 8];
    for c in buf.iter_mut() {
        c.dwSize = std::mem::size_of::<RASCONNW>() as u32;
    }
    let mut cb = (buf.len() * std::mem::size_of::<RASCONNW>()) as u32;
    let mut n = 0u32;
    if unsafe { RasEnumConnectionsW(Some(buf.as_mut_ptr()), &mut cb, &mut n) } != 0 {
        return vec![];
    }
    buf.truncate(n as usize);
    buf
}

pub fn vpn_list() -> Vec<Vpn> {
    let active: Vec<String> = ras_active().iter().map(|c| from_wide(&c.szEntryName)).collect();
    ras_entries().into_iter().map(|name| Vpn { connected: active.contains(&name), name }).collect()
}

pub fn vpn_set(name: &str, on: bool) -> Result<(), String> {
    if !on {
        for c in ras_active() {
            if from_wide(&c.szEntryName) == name {
                unsafe { RasHangUpW(c.hrasconn) };
            }
        }
        return Ok(());
    }
    // saved credentials are used by rasdial; without them Windows' own dialog asks
    let ok = Command::new("rasdial").arg(name).creation_flags(NO_WINDOW).status().map(|s| s.success()).unwrap_or(false);
    if !ok {
        Command::new("rasphone").args(["-d", name]).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ---------- power ----------

pub fn power(action: &str) -> Result<(), String> {
    match action {
        "lock" => unsafe { windows::Win32::System::Shutdown::LockWorkStation() }.map_err(|e| e.to_string()),
        "sleep" => {
            let ok = unsafe { windows::Win32::System::Power::SetSuspendState(false, false, false) };
            if ok { Ok(()) } else { Err("Сон недоступен".into()) }
        }
        "restart" | "shutdown" => {
            let flag = if action == "restart" { "/r" } else { "/s" };
            Command::new("shutdown").args([flag, "/t", "0"]).creation_flags(NO_WINDOW).spawn().map(|_| ()).map_err(|e| e.to_string())
        }
        _ => Err("Неизвестное действие".into()),
    }
}

pub fn state() -> SysState {
    let _com = Com::mta();
    let ssid = Wlan::open().ok().and_then(|w| w.iface().ok().map(|g| w.current_ssid(&g))).unwrap_or_default();
    SysState {
        wifi: radio_on(RadioKind::WiFi),
        bluetooth: radio_on(RadioKind::Bluetooth),
        dark: dark_mode(),
        vpn: vpn_list(),
        ssid,
    }
}

/// Opens a Settings page (fallback for things we can't do ourselves).
pub fn open_settings(page: &str) {
    let uri = match page {
        "wifi" => "ms-settings:network-wifi",
        "bluetooth" => "ms-settings:bluetooth",
        "vpn" => "ms-settings:network-vpn",
        "colors" => "ms-settings:colors",
        _ => "ms-settings:",
    };
    let _ = Command::new("explorer.exe").arg(uri).spawn();
}
