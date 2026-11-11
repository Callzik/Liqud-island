//! Default output device: peak meter (for the liquid wave), master volume, name.

use windows::core::{Result, GUID};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
use windows::Win32::Media::Audio::{
    eCommunications, eConsole, eMultimedia, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL, STGM_READ};

use super::util::{pcwstr, take_pwstr, wide};

#[allow(non_snake_case, dead_code)]
mod policy {
    use std::ffi::c_void;
    use windows::core::{interface, IUnknown, IUnknown_Vtbl, HRESULT, PCWSTR};
    use windows::Win32::Media::Audio::ERole;

    /// Undocumented (but stable since Windows 7) interface used by the Sound control
    /// panel to change the default endpoint. Only `SetDefaultEndpoint` is called;
    /// the other slots just keep the vtable layout.
    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
        pub fn GetMixFormat(&self, id: PCWSTR, fmt: *mut *mut c_void) -> HRESULT;
        pub fn GetDeviceFormat(&self, id: PCWSTR, default: i32, fmt: *mut *mut c_void) -> HRESULT;
        pub fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
        pub fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut c_void, mix: *mut c_void) -> HRESULT;
        pub fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, def: *mut i64, min: *mut i64) -> HRESULT;
        pub fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
        pub fn GetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
        pub fn SetShareMode(&self, id: PCWSTR, mode: *mut c_void) -> HRESULT;
        pub fn GetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
        pub fn SetPropertyValue(&self, id: PCWSTR, key: *const c_void, value: *mut c_void) -> HRESULT;
        pub fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
        pub fn SetEndpointVisibility(&self, id: PCWSTR, visible: i32) -> HRESULT;
    }

}
use policy::IPolicyConfig;

const CLSID_POLICY_CONFIG: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

#[derive(serde::Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OutputDevice {
    pub id: String,
    pub name: String,
    pub default: bool,
}

fn friendly_name(device: &IMMDevice) -> String {
    unsafe {
        device
            .OpenPropertyStore(STGM_READ)
            .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName))
            .and_then(|pv| PropVariantToStringAlloc(&pv))
            .map(take_pwstr)
            .unwrap_or_default()
    }
}

pub struct Audio {
    enumerator: IMMDeviceEnumerator,
    device_id: String,
    device_name: String,
    meter: Option<IAudioMeterInformation>,
    volume: Option<IAudioEndpointVolume>,
}

impl Audio {
    /// Must be called on a COM-initialised thread.
    pub fn new() -> Result<Self> {
        let enumerator: IMMDeviceEnumerator = unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
        let mut a = Self {
            enumerator,
            device_id: String::new(),
            device_name: String::new(),
            meter: None,
            volume: None,
        };
        a.refresh();
        Ok(a)
    }

    /// Re-binds to the default output device if it changed. Returns true on change.
    pub fn refresh(&mut self) -> bool {
        let device: IMMDevice = match unsafe { self.enumerator.GetDefaultAudioEndpoint(eRender, eConsole) } {
            Ok(d) => d,
            Err(_) => {
                let changed = self.meter.is_some();
                self.meter = None;
                self.volume = None;
                self.device_id.clear();
                return changed;
            }
        };
        let id = unsafe { device.GetId() }.map(take_pwstr).unwrap_or_default();
        if id == self.device_id && self.meter.is_some() {
            return false;
        }
        self.device_id = id;
        self.meter = unsafe { device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) }.ok();
        self.volume = unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.ok();
        self.device_name = friendly_name(&device);
        true
    }

    /// Active output devices (speakers, headphones, HDMI…).
    pub fn outputs(&self) -> Vec<OutputDevice> {
        let mut out = Vec::new();
        unsafe {
            let Ok(list) = self.enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return out };
            let n = list.GetCount().unwrap_or(0);
            for i in 0..n {
                if let Ok(d) = list.Item(i) {
                    let id = d.GetId().map(take_pwstr).unwrap_or_default();
                    out.push(OutputDevice {
                        default: id == self.device_id,
                        name: friendly_name(&d),
                        id,
                    });
                }
            }
        }
        out
    }

    /// Makes `id` the default device for every role.
    pub fn set_default(&mut self, id: &str) -> bool {
        let w = wide(id);
        let ok = unsafe {
            match CoCreateInstance::<_, IPolicyConfig>(&CLSID_POLICY_CONFIG, None, CLSCTX_ALL) {
                Ok(pc) => [eConsole, eMultimedia, eCommunications]
                    .into_iter()
                    .all(|role| pc.SetDefaultEndpoint(pcwstr(&w), role).is_ok()),
                Err(_) => false,
            }
        };
        self.refresh();
        ok
    }

    pub fn peak(&self) -> f32 {
        self.meter
            .as_ref()
            .and_then(|m| unsafe { m.GetPeakValue() }.ok())
            .unwrap_or(0.0)
    }

    pub fn volume(&self) -> (f32, bool) {
        match &self.volume {
            Some(v) => unsafe {
                (
                    v.GetMasterVolumeLevelScalar().unwrap_or(0.0),
                    v.GetMute().map(|b| b.as_bool()).unwrap_or(false),
                )
            },
            None => (0.0, false),
        }
    }

    pub fn set_volume(&self, level: f32) {
        if let Some(v) = &self.volume {
            unsafe {
                let _ = v.SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null());
                if level > 0.0 {
                    let _ = v.SetMute(false, std::ptr::null());
                }
            }
        }
    }

    pub fn set_mute(&self, mute: bool) {
        if let Some(v) = &self.volume {
            unsafe {
                let _ = v.SetMute(mute, std::ptr::null());
            }
        }
    }

    /// "Динамики (Realtek(R) Audio)" -> "Динамики"
    pub fn device_short_name(&self) -> String {
        let n = self.device_name.trim();
        match n.find(" (") {
            Some(i) if i > 0 => n[..i].to_string(),
            _ => n.to_string(),
        }
    }
}

// ---------- one app's volume (the player), the rest of the sound untouched ----------

/// Does the media-session app id belong to this executable? Classic apps report
/// "AIMP.exe"-like ids, packaged ones "Vendor.App_hash!App"; both usually contain the exe name.
fn app_matches(app_id: &str, exe_path: &str) -> bool {
    let id = app_id.to_lowercase();
    let stem = super::util::exe_stem(exe_path).to_lowercase();
    if stem.is_empty() {
        return false;
    }
    if id == format!("{stem}.exe") || id.ends_with(&format!("\\{stem}.exe")) {
        return true;
    }
    let id_head = id.split(['!', '_']).next().unwrap_or("");
    id_head.split('.').any(|part| part.len() > 2 && (part == stem || stem.contains(part))) || id.contains(&stem)
}

/// All volume controls of the sessions playing from `app_id` on the default device.
fn app_volumes(app_id: &str) -> Vec<windows::Win32::Media::Audio::ISimpleAudioVolume> {
    use windows::core::Interface;
    use windows::Win32::Media::Audio::{IAudioSessionControl2, IAudioSessionManager2, ISimpleAudioVolume};
    let _com = super::util::Com::mta();
    let mut out = Vec::new();
    unsafe {
        let Ok(en) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return out };
        let Ok(dev) = en.GetDefaultAudioEndpoint(eRender, eConsole) else { return out };
        let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { return out };
        let Ok(list) = mgr.GetSessionEnumerator() else { return out };
        let n = list.GetCount().unwrap_or(0);
        for i in 0..n {
            let Ok(ctl) = list.GetSession(i) else { continue };
            let Ok(ctl2) = ctl.cast::<IAudioSessionControl2>() else { continue };
            let pid = ctl2.GetProcessId().unwrap_or(0);
            if pid == 0 {
                continue;
            }
            let Some(path) = super::util::process_exe_path(pid) else { continue };
            if app_matches(app_id, &path) {
                if let Ok(v) = ctl.cast::<ISimpleAudioVolume>() {
                    out.push(v);
                }
            }
        }
    }
    out
}

/// The player's own volume (0..1), None when its audio session isn't found.
pub fn app_volume(app_id: &str) -> Option<f32> {
    app_volumes(app_id).first().and_then(|v| unsafe { v.GetMasterVolume() }.ok())
}

pub fn set_app_volume(app_id: &str, level: f32) -> bool {
    let vols = app_volumes(app_id);
    for v in &vols {
        unsafe {
            let _ = v.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null());
            if level > 0.0 {
                let _ = v.SetMute(false, std::ptr::null());
            }
        }
    }
    !vols.is_empty()
}
