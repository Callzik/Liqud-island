//! Updates from GitHub releases: the latest release's `*-setup.exe` is
//! downloaded and run in passive mode (/P /UPDATE /R restarts Island).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::ai::client;

const LATEST: &str = "https://api.github.com/repos/Callzik/Liqud-island/releases/latest";

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub url: String,
}

/// Last release found newer than the running version.
static FOUND: Mutex<Option<Release>> = Mutex::new(None);
static INSTALLING: AtomicBool = AtomicBool::new(false);

/// "v0.4.10" → [0, 4, 10]
fn parse(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

pub fn newer(remote: &str, current: &str) -> bool {
    parse(remote) > parse(current)
}

/// The latest release when it is newer than `current`.
pub async fn check(current: &str) -> Result<Option<Release>, String> {
    let resp = client()
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("GitHub ответил {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("");
    if v["draft"].as_bool() == Some(true) || v["prerelease"].as_bool() == Some(true) || !newer(tag, current) {
        return Ok(None);
    }
    let url = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["name"].as_str().is_some_and(|n| n.to_ascii_lowercase().ends_with("-setup.exe")))
        .find_map(|a| a["browser_download_url"].as_str().map(String::from));
    // the tag exists before the build attaches the installer
    let Some(url) = url else { return Ok(None) };
    let r = Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        url,
    };
    *FOUND.lock().unwrap_or_else(|e| e.into_inner()) = Some(r.clone());
    Ok(Some(r))
}

pub fn found() -> Option<Release> {
    FOUND.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Downloads the installer to %TEMP% and starts it. Ok → the app should exit.
pub async fn install(r: &Release) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("Обновление уже скачивается".into());
    }
    let res = async {
        let resp = client()
            .get(&r.url)
            .timeout(Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("GitHub ответил {}", resp.status()));
        }
        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        // a GitHub error page instead of the installer
        if bytes.len() < 1_000_000 || !bytes.starts_with(b"MZ") {
            return Err("Скачался не установщик".into());
        }
        let path = std::env::temp_dir().join(format!("Island_{}_x64-setup.exe", r.version));
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        std::process::Command::new(&path)
            .args(["/P", "/UPDATE", "/R"])
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    if res.is_err() {
        INSTALLING.store(false, Ordering::SeqCst);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn versions() {
        assert!(newer("v0.4.10", "0.4.9"));
        assert!(newer("v0.5.0", "0.4.6"));
        assert!(!newer("v0.4.6", "0.4.6"));
        assert!(!newer("v0.4.4", "0.4.6"));
    }
}
