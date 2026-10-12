//! «Без цензуры»: finds the playing track on YouTube (official Data API v3,
//! with the user's own key) and plays it on YouTube Music in a player window.

use serde::Deserialize;

#[derive(Deserialize)]
struct SearchItem {
    id: SearchId,
}
#[derive(Deserialize)]
struct SearchId {
    #[serde(rename = "videoId")]
    video_id: Option<String>,
}
#[derive(Deserialize)]
struct List<T> {
    #[serde(default = "Vec::new")]
    items: Vec<T>,
}
#[derive(Deserialize)]
struct Video {
    id: String,
    #[serde(rename = "contentDetails")]
    details: Details,
}
#[derive(Deserialize)]
struct Details {
    duration: String,
}

/// "PT3M25S" → 205 000 ms
fn iso_ms(s: &str) -> i64 {
    let mut total = 0i64;
    let mut num = 0i64;
    for c in s.chars() {
        match c {
            '0'..='9' => num = num * 10 + (c as i64 - '0' as i64),
            'H' => {
                total += num * 3_600_000;
                num = 0
            }
            'M' => {
                total += num * 60_000;
                num = 0
            }
            'S' => {
                total += num * 1000;
                num = 0
            }
            _ => num = 0,
        }
    }
    total
}

/// Best matching video id: closest duration among the top results.
pub async fn find(key: &str, artist: &str, title: &str, duration_ms: i64) -> Result<Option<String>, String> {
    let q = format!("{artist} {title}");
    let client = crate::ai::client();
    let list: List<SearchItem> = client
        .get("https://www.googleapis.com/youtube/v3/search")
        .query(&[("part", "id"), ("type", "video"), ("maxResults", "6"), ("q", q.as_str()), ("key", key)])
        .send()
        .await
        .map_err(|e| format!("YouTube недоступен: {e}"))?
        .error_for_status()
        .map_err(|e| format!("YouTube отказал (ключ?): {e}"))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = list.items.into_iter().filter_map(|i| i.id.video_id).collect();
    if ids.is_empty() {
        return Ok(None);
    }
    if duration_ms <= 0 {
        return Ok(ids.into_iter().next());
    }
    let joined = ids.join(",");
    let videos: List<Video> = client
        .get("https://www.googleapis.com/youtube/v3/videos")
        .query(&[("part", "contentDetails"), ("id", joined.as_str()), ("key", key)])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    // search order = relevance; among those, the one closest in length wins (clips with intros lose)
    let best = videos
        .items
        .iter()
        .enumerate()
        .min_by_key(|(rank, v)| (iso_ms(&v.details.duration) - duration_ms).abs() + *rank as i64 * 1500)
        .map(|(_, v)| v.id.clone());
    Ok(best.or_else(|| ids.into_iter().next()))
}

pub fn watch_url(id: &str) -> String {
    format!("https://music.youtube.com/watch?v={id}")
}

pub fn search_url(artist: &str, title: &str) -> String {
    let q: String = format!("{artist} {title}")
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect();
    format!("https://music.youtube.com/search?q={q}")
}
