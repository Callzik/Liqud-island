//! Yandex Music: like / dislike the current track through the music API
//! (the same one the official apps use) with the user's own OAuth token.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

const API: &str = "https://api.music.yandex.net";

/// OAuth page of the Yandex Music client; after login Yandex redirects to
/// `https://music.yandex.ru/#access_token=…`, the token is taken from there.
pub const LOGIN_URL: &str =
    "https://oauth.yandex.ru/authorize?response_type=token&client_id=23cabbbdc6cd418abb4b39c32c41195d";

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrackState {
    /// a token is set and the track was found in Yandex Music
    pub available: bool,
    pub track_id: String,
    pub liked: bool,
    pub disliked: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Cache {
    token: String,
    uid: Option<u64>,
    tracks: HashMap<String, Option<String>>,
    likes: Option<(Instant, HashSet<String>)>,
    dislikes: Option<(Instant, HashSet<String>)>,
}

pub type Shared = Mutex<Cache>;

fn lock(m: &Shared) -> std::sync::MutexGuard<'_, Cache> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Pulls the token out of the redirect URL after login.
pub fn token_from_url(url: &str) -> Option<String> {
    let i = url.find("access_token=")?;
    let rest = &url[i + "access_token=".len()..];
    let t: String = rest.chars().take_while(|c| *c != '&' && *c != '#').collect();
    (!t.is_empty()).then_some(t)
}

fn get(token: &str, path: &str) -> reqwest::RequestBuilder {
    crate::ai::client()
        .get(format!("{API}{path}"))
        .header("Authorization", format!("OAuth {token}"))
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .timeout(Duration::from_secs(10))
}

fn post(token: &str, path: &str) -> reqwest::RequestBuilder {
    crate::ai::client()
        .post(format!("{API}{path}"))
        .header("Authorization", format!("OAuth {token}"))
        .header("X-Yandex-Music-Client", "YandexMusicAndroid/24023621")
        .timeout(Duration::from_secs(10))
}

async fn json(req: reqwest::RequestBuilder) -> Result<serde_json::Value, String> {
    let resp = req.send().await.map_err(|e| format!("Нет связи с Яндекс Музыкой: {e}"))?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED || resp.status() == reqwest::StatusCode::FORBIDDEN {
        return Err("Токен Яндекс Музыки не подошёл — войдите заново в настройках".into());
    }
    if !resp.status().is_success() {
        return Err(format!("Яндекс Музыка ответила {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

fn id_str(v: &serde_json::Value) -> Option<String> {
    v.as_str().map(String::from).or_else(|| v.as_u64().map(|n| n.to_string()))
}

async fn uid(cache: &Shared, token: &str) -> Result<u64, String> {
    {
        let c = lock(cache);
        if c.token == token {
            if let Some(u) = c.uid {
                return Ok(u);
            }
        }
    }
    let v = json(get(token, "/account/status")).await?;
    let u = v["result"]["account"]["uid"].as_u64().ok_or("Не удалось узнать аккаунт")?;
    let mut c = lock(cache);
    if c.token != token {
        *c = Cache { token: token.to_string(), ..Default::default() };
    }
    c.uid = Some(u);
    Ok(u)
}

/// Finds the track in the catalogue by artist + title (closest duration wins).
async fn find(cache: &Shared, token: &str, artist: &str, title: &str, duration_ms: i64) -> Result<Option<String>, String> {
    let key = format!("{artist}\u{1}{title}");
    if let Some(hit) = lock(cache).tracks.get(&key) {
        return Ok(hit.clone());
    }
    let text = format!("{artist} {title}");
    let v = json(get(token, "/search").query(&[("text", text.as_str()), ("type", "track"), ("page", "0")])).await?;
    let title_l = title.to_lowercase();
    let mut best: Option<(i64, String)> = None;
    for t in v["result"]["tracks"]["results"].as_array().into_iter().flatten().take(10) {
        let Some(id) = id_str(&t["id"]) else { continue };
        let name = t["title"].as_str().unwrap_or("").to_lowercase();
        let mut score = if name == title_l { 0 } else if name.contains(&title_l) || title_l.contains(&name) { 5_000 } else { 60_000 };
        if duration_ms > 0 {
            score += (t["durationMs"].as_i64().unwrap_or(0) - duration_ms).abs().min(60_000);
        }
        if best.as_ref().map(|(s, _)| score < *s).unwrap_or(true) {
            best = Some((score, id));
        }
    }
    let found = best.filter(|(s, _)| *s < 65_000).map(|(_, id)| id);
    lock(cache).tracks.insert(key, found.clone());
    Ok(found)
}

async fn library(cache: &Shared, token: &str, uid: u64, kind: &str, fresh: bool) -> Result<HashSet<String>, String> {
    {
        let c = lock(cache);
        let slot = if kind == "likes" { &c.likes } else { &c.dislikes };
        if let (false, Some((at, set))) = (fresh, slot) {
            if at.elapsed() < Duration::from_secs(90) {
                return Ok(set.clone());
            }
        }
    }
    let v = json(get(token, &format!("/users/{uid}/{kind}/tracks"))).await?;
    let set: HashSet<String> = v["result"]["library"]["tracks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| id_str(&t["id"]))
        .collect();
    let mut c = lock(cache);
    let slot = if kind == "likes" { &mut c.likes } else { &mut c.dislikes };
    *slot = Some((Instant::now(), set.clone()));
    Ok(set)
}

pub async fn state(cache: &Shared, token: &str, artist: &str, title: &str, duration_ms: i64) -> TrackState {
    if token.is_empty() || title.is_empty() {
        return TrackState::default();
    }
    let run = async {
        let u = uid(cache, token).await?;
        let Some(id) = find(cache, token, artist, title, duration_ms).await? else {
            return Ok(TrackState::default());
        };
        let likes = library(cache, token, u, "likes", false).await?;
        let dislikes = library(cache, token, u, "dislikes", false).await?;
        Ok::<_, String>(TrackState {
            available: true,
            liked: likes.contains(&id),
            disliked: dislikes.contains(&id),
            track_id: id,
            error: None,
        })
    };
    run.await.unwrap_or_else(|e| TrackState { error: Some(e), ..Default::default() })
}

/// kind: "likes" | "dislikes"; on = add, off = remove.
pub async fn set(cache: &Shared, token: &str, track_id: &str, kind: &str, on: bool) -> Result<(), String> {
    let u = uid(cache, token).await?;
    let action = if on { "add-multiple" } else { "remove" };
    json(post(token, &format!("/users/{u}/{kind}/tracks/{action}")).form(&[("track-ids", track_id)])).await?;
    // a like removes a dislike and vice versa on the server; refresh both lists
    let _ = library(cache, token, u, "likes", true).await;
    let _ = library(cache, token, u, "dislikes", true).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Lyrics (synced LRC when Yandex has it, otherwise plain text)
// ---------------------------------------------------------------------------

/// Signing key of the Android client for `/tracks/{id}/lyrics`.
const SIGN_KEY: &[u8] = b"p93jhgh689SBReK6ghtw62";

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Lyrics {
    pub synced: Option<String>,
    pub plain: Option<String>,
}

fn sign(track_id: &str) -> (i64, String) {
    use hmac::{Hmac, Mac};
    let id: String = track_id.split(':').next().unwrap_or(track_id).to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut mac = <Hmac<sha2::Sha256> as Mac>::new_from_slice(SIGN_KEY).expect("hmac key");
    mac.update(format!("{id}{ts}").as_bytes());
    (ts, crate::native::util::base64(&mac.finalize().into_bytes()))
}

async fn lyrics_of(token: &str, track_id: &str, format: &str) -> Result<Option<String>, String> {
    let (ts, sig) = sign(track_id);
    let id = track_id.split(':').next().unwrap_or(track_id);
    let req = get(token, &format!("/tracks/{id}/lyrics")).query(&[
        ("format", format.to_string()),
        ("timeStamp", ts.to_string()),
        ("sign", sig),
    ]);
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Ok(None); // 404 / 400: no lyrics of this kind
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let Some(url) = v["result"]["downloadUrl"].as_str() else { return Ok(None) };
    let text = crate::ai::client()
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    Ok((!text.trim().is_empty()).then_some(text))
}

/// Lyrics of the track as Yandex Music has them (needs the user's token).
pub async fn lyrics(cache: &Shared, token: &str, artist: &str, title: &str, duration_ms: i64) -> Result<Option<Lyrics>, String> {
    if token.is_empty() || title.is_empty() {
        return Ok(None);
    }
    let Some(id) = find(cache, token, artist, title, duration_ms).await? else { return Ok(None) };
    let synced = lyrics_of(token, &id, "LRC").await.unwrap_or(None);
    let plain = if synced.is_none() { lyrics_of(token, &id, "TEXT").await.unwrap_or(None) } else { None };
    Ok((synced.is_some() || plain.is_some()).then_some(Lyrics { synced, plain }))
}
