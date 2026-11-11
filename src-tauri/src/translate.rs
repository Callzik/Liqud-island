//! Quick translation through Google's public web endpoint (no key needed).

use std::time::Duration;

/// Mostly Cyrillic text goes to English, everything else to Russian.
pub fn target_for(text: &str) -> &'static str {
    let letters = text.chars().filter(|c| c.is_alphabetic()).count().max(1);
    let cyr = text.chars().filter(|c| ('\u{0400}'..='\u{04FF}').contains(c)).count();
    if cyr * 2 > letters { "en" } else { "ru" }
}

pub async fn translate(text: &str, to: Option<&str>) -> Result<String, String> {
    let tl = to.unwrap_or_else(|| target_for(text));
    let resp = crate::ai::client()
        .post("https://translate.googleapis.com/translate_a/single")
        .query(&[("client", "gtx"), ("sl", "auto"), ("tl", tl), ("dt", "t")])
        .form(&[("q", text)])
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Нет связи с переводчиком: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Переводчик ответил {}", resp.status()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let out: String = v[0]
        .as_array()
        .map(|parts| parts.iter().filter_map(|p| p[0].as_str()).collect())
        .unwrap_or_default();
    if out.trim().is_empty() {
        return Err("Пустой перевод".into());
    }
    Ok(out)
}
