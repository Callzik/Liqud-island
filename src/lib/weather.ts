// Open-Meteo response → what the island shows.

export interface WeatherPayload {
  city: string;
  fetchedMs: number;
  data: any;
}

export type Sky = "clear" | "partly" | "cloudy" | "fog" | "drizzle" | "rain" | "snow" | "thunder";

export interface Hour {
  t: number; // unix ms
  label: string; // "19:00"
  temp: number;
  prob: number;
  code: number;
  day: boolean;
  pressure: number; // hPa
  rain: number; // mm
}

export interface Day {
  t: number;
  label: string; // "Сегодня", "Пн"
  min: number;
  max: number;
  code: number;
  sunrise: number; // unix ms
  sunset: number;
  uv: number;
  rain: number; // mm
}

export interface Now {
  temp: number;
  feels: number;
  humidity: number;
  wind: number;
  code: number;
  day: boolean;
  precipitation: number;
  windDir: number; // degrees, where the wind comes from
  gusts: number;
  pressure: number; // hPa
  dew: number;
  uv: number;
}

export function sky(code: number): Sky {
  if (code === 0 || code === 1) return code === 0 ? "clear" : "partly";
  if (code === 2) return "partly";
  if (code === 3) return "cloudy";
  if (code === 45 || code === 48) return "fog";
  if (code >= 51 && code <= 57) return "drizzle";
  if ((code >= 61 && code <= 67) || (code >= 80 && code <= 82)) return "rain";
  if ((code >= 71 && code <= 77) || code === 85 || code === 86) return "snow";
  if (code >= 95) return "thunder";
  return "cloudy";
}

const DESC: Record<number, string> = {
  0: "Ясно",
  1: "Преимущественно ясно",
  2: "Переменная облачность",
  3: "Пасмурно",
  45: "Туман",
  48: "Изморозь и туман",
  51: "Слабая морось",
  53: "Морось",
  55: "Сильная морось",
  56: "Ледяная морось",
  57: "Ледяная морось",
  61: "Небольшой дождь",
  63: "Дождь",
  65: "Сильный дождь",
  66: "Ледяной дождь",
  67: "Ледяной дождь",
  71: "Небольшой снег",
  73: "Снег",
  75: "Сильный снег",
  77: "Снежная крупа",
  80: "Ливень",
  81: "Ливень",
  82: "Сильный ливень",
  85: "Снегопад",
  86: "Сильный снегопад",
  95: "Гроза",
  96: "Гроза с градом",
  99: "Гроза с градом",
};

export const describe = (code: number) => DESC[code] ?? "—";

const WEEK = ["Вс", "Пн", "Вт", "Ср", "Чт", "Пт", "Сб"];

/** Local wall-clock time of the forecast place → unix ms. */
function toMs(local: string, offsetSec: number) {
  return Date.parse(local + (local.length === 10 ? "T00:00" : "") + "Z") - offsetSec * 1000;
}

export const deg = (t: number) => `${Math.round(t) === 0 ? 0 : Math.round(t)}°`;

export function parse(w: WeatherPayload) {
  const d = w.data ?? {};
  const off: number = d.utc_offset_seconds ?? 0;
  const c = d.current ?? {};
  const now: Now = {
    temp: c.temperature_2m ?? 0,
    feels: c.apparent_temperature ?? c.temperature_2m ?? 0,
    humidity: c.relative_humidity_2m ?? 0,
    wind: c.wind_speed_10m ?? 0,
    code: c.weather_code ?? 0,
    day: c.is_day !== 0,
    precipitation: c.precipitation ?? 0,
    windDir: c.wind_direction_10m ?? 0,
    gusts: c.wind_gusts_10m ?? 0,
    pressure: c.surface_pressure ?? 1013,
    dew: c.dew_point_2m ?? 0,
    uv: c.uv_index ?? 0,
  };
  const h = d.hourly ?? {};
  const hours: Hour[] = (h.time ?? []).map((t: string, i: number) => ({
    t: toMs(t, off),
    label: t.slice(11, 16),
    temp: h.temperature_2m?.[i] ?? 0,
    prob: h.precipitation_probability?.[i] ?? 0,
    code: h.weather_code?.[i] ?? 0,
    day: h.is_day?.[i] !== 0,
    pressure: h.surface_pressure?.[i] ?? 0,
    rain: h.precipitation?.[i] ?? 0,
  }));
  const dl = d.daily ?? {};
  const days: Day[] = (dl.time ?? []).map((t: string, i: number) => {
    const ms = toMs(t, off);
    const wd = new Date(Date.parse(t + "T12:00Z")).getUTCDay();
    return {
      t: ms,
      label: i === 0 ? "Сегодня" : WEEK[wd],
      min: dl.temperature_2m_min?.[i] ?? 0,
      max: dl.temperature_2m_max?.[i] ?? 0,
      code: dl.weather_code?.[i] ?? 0,
      sunrise: dl.sunrise?.[i] ? toMs(dl.sunrise[i], off) : 0,
      sunset: dl.sunset?.[i] ? toMs(dl.sunset[i], off) : 0,
      uv: dl.uv_index_max?.[i] ?? 0,
      rain: dl.precipitation_sum?.[i] ?? 0,
    };
  });
  return { now, hours, days };
}

/** The next 24 hours starting with the current one. */
export function next24(hours: Hour[], at = Date.now()): Hour[] {
  const i = hours.findIndex((h) => h.t + 3600e3 > at);
  return i < 0 ? [] : hours.slice(i, i + 24);
}

/**
 * "Через час дождь": it is dry now, but within 60–90 minutes the chance of
 * precipitation reaches 60%. Returns the hour that triggers the warning.
 */
export function rainSoon(w: WeatherPayload, at = Date.now()): Hour | null {
  const { now, hours } = parse(w);
  const wet = ["drizzle", "rain", "snow", "thunder"].includes(sky(now.code)) || now.precipitation > 0.05;
  if (wet) return null;
  // an hourly slot that starts between +30 and +90 min covers the 60–90 min window
  const h = hours.find((x) => x.t > at + 30 * 60e3 && x.t <= at + 90 * 60e3);
  return h && h.prob >= 60 ? h : null;
}

export const mmHg = (hpa: number) => Math.round(hpa * 0.750062);

const DIRS = ["северный", "северо-восточный", "восточный", "юго-восточный", "южный", "юго-западный", "западный", "северо-западный"];
export const windName = (deg: number) => DIRS[Math.round((((deg % 360) + 360) % 360) / 45) % 8];

export function uvLabel(uv: number) {
  if (uv < 3) return "низкий";
  if (uv < 6) return "умеренный";
  if (uv < 8) return "высокий";
  if (uv < 11) return "очень высокий";
  return "экстремальный";
}

/** Pressure change over the next 3 hours: "растёт" / "падает" / "стабильно". */
export function pressureTrend(hours: Hour[], at = Date.now()) {
  const i = hours.findIndex((h) => h.t + 3600e3 > at);
  if (i < 0 || !hours[i + 3]?.pressure) return "стабильно";
  const d = hours[i + 3].pressure - hours[i].pressure;
  return d > 0.6 ? "растёт" : d < -0.6 ? "падает" : "стабильно";
}

/** "Сутки без осадков, ночью до 7°" — a one-line summary of the next 24 hours. */
export function daySummary(hours: Hour[], at = Date.now()) {
  const next = next24(hours, at);
  if (!next.length) return "";
  const wet = next.find((h) => h.prob >= 50 || h.rain > 0.2);
  const night = next.filter((h) => !h.day);
  const lo = night.length ? Math.min(...night.map((h) => h.temp)) : null;
  const tail = lo !== null ? `, ночью до ${deg(lo)}` : "";
  if (!wet) return `Сутки без осадков${tail}`;
  const k = sky(wet.code) === "snow" ? "снег" : "дождь";
  return wet === next[0] ? `Сейчас ${k}${tail}` : `В ${wet.label} ${k} (${wet.prob}%)${tail}`;
}
