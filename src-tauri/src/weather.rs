//! Weather from Open-Meteo (no key): city search, IP-based location, forecast.

use serde::Serialize;

use std::time::Duration;

use crate::ai::client;

/// A stalled request (Wi-Fi roaming, sleep, captive portal) must not hang the
/// weather thread forever.
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub name: String,
    /// region / country, for telling namesakes apart
    pub area: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WeatherPayload {
    pub city: String,
    pub fetched_ms: i64,
    /// Raw Open-Meteo response: current / hourly / daily / utc_offset_seconds.
    pub data: serde_json::Value,
}

pub async fn search(q: &str) -> Result<Vec<Place>, String> {
    let resp = client()
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .timeout(TIMEOUT)
        .query(&[("name", q), ("count", "7"), ("language", "ru"), ("format", "json")])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let area = [r["admin1"].as_str(), r["country"].as_str()]
                        .into_iter()
                        .flatten()
                        .filter(|s| Some(*s) != r["name"].as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Some(Place {
                        name: r["name"].as_str()?.to_string(),
                        area,
                        lat: r["latitude"].as_f64()?,
                        lon: r["longitude"].as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Approximate location by IP (city-level).
pub async fn locate() -> Result<Place, String> {
    let v: serde_json::Value = client()
        .get("https://ipwho.is/?lang=ru")
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if v["success"].as_bool() == Some(false) {
        return Err("Не удалось определить город".into());
    }
    Ok(Place {
        name: v["city"].as_str().unwrap_or("").to_string(),
        area: v["country"].as_str().unwrap_or("").to_string(),
        lat: v["latitude"].as_f64().ok_or("нет координат")?,
        lon: v["longitude"].as_f64().ok_or("нет координат")?,
    })
}

pub async fn forecast(lat: f64, lon: f64) -> Result<serde_json::Value, String> {
    let resp = client()
        .get("https://api.open-meteo.com/v1/forecast")
        .timeout(TIMEOUT)
        .query(&[
            ("latitude", format!("{lat:.4}")),
            ("longitude", format!("{lon:.4}")),
            (
                "current",
                "temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,wind_direction_10m,wind_gusts_10m,weather_code,is_day,precipitation,surface_pressure,dew_point_2m,uv_index"
                    .into(),
            ),
            ("hourly", "temperature_2m,precipitation_probability,weather_code,is_day,surface_pressure,precipitation".into()),
            ("daily", "weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,uv_index_max,precipitation_sum".into()),
            ("wind_speed_unit", "ms".into()),
            ("timezone", "auto".into()),
            ("forecast_days", "7".into()),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Open-Meteo: {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}
