import { useEffect, useMemo, useRef, useState } from "react";
import { call, type Settings } from "../api";
import { daySummary, deg, describe, mmHg, next24, parse, pressureTrend, sky, uvLabel, windName, type WeatherPayload } from "../lib/weather";
import { WeatherIcon } from "./WeatherIcon";

export function Weather({ weather, settings, openSettings }: { weather: WeatherPayload | null; settings: Settings; openSettings: () => void }) {
  const p = useMemo(() => (weather ? parse(weather) : null), [weather]);

  if (!settings.weatherEnabled) {
    return (
      <div className="chat-empty wx-empty">
        <WeatherIcon code={2} size={34} />
        <div className="chat-empty-title">Погода выключена</div>
        <button className="text-btn" onClick={openSettings}>
          Включить в настройках
        </button>
      </div>
    );
  }
  if (!weather || !p) {
    return (
      <div className="chat-empty wx-empty">
        <div className="ly-dots">
          <i />
          <i />
          <i />
        </div>
        <div className="hint">Загружаю прогноз…</div>
        <button className="text-btn" onClick={() => call("weather_refresh")}>
          Обновить
        </button>
      </div>
    );
  }

  return <WeatherTiles weather={weather} p={p} openSettings={openSettings} />;
}

type Parsed = ReturnType<typeof parse>;

/** Numbers count up from zero when the tab opens. */
function useCountUp(target: number, ms = 900) {
  const [v, setV] = useState(0);
  const from = useRef(0);
  useEffect(() => {
    const a = from.current;
    const t0 = performance.now();
    let raf = 0;
    const step = (t: number) => {
      const k = Math.min(1, (t - t0) / ms);
      const e = 1 - Math.pow(1 - k, 3);
      const x = a + (target - a) * e;
      from.current = x;
      setV(x);
      if (k < 1) raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [target, ms]);
  return v;
}

/** Wall-clock time at the forecast place (the city may be in another time zone). */
const hm = (ms: number, offSec: number) =>
  ms ? new Date(ms + offSec * 1000).toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit", timeZone: "UTC" }) : "—";
const num = (x: number, d = 0) => x.toFixed(d).replace(".", ",");

function WeatherTiles({ weather, p, openSettings }: { weather: WeatherPayload; p: Parsed; openSettings: () => void }) {
  const { now, hours, days } = p;
  const [mode, setMode] = useState<"hours" | "days">("hours");
  const strip = next24(hours);
  const today = days[0];
  const tomorrow = days[1];
  const off: number = weather.data?.utc_offset_seconds ?? 0;

  const temp = useCountUp(now.temp);
  const wind = useCountUp(now.wind);
  const hum = useCountUp(now.humidity);
  const press = useCountUp(mmHg(now.pressure));
  const uv = useCountUp(now.uv);
  const rain = useCountUp(today?.rain ?? 0);
  const windDir = useCountUp(now.windDir, 1200);

  // sun position along the arc: 0 at sunrise, 1 at sunset
  const sunK = today?.sunrise && today?.sunset ? Math.max(0, Math.min(1, (Date.now() - today.sunrise) / (today.sunset - today.sunrise))) : 0;
  const sunAnim = useCountUp(sunK, 1200);
  const sx = 10 + 100 * sunAnim;
  const sy = 50 - Math.sin(Math.PI * sunAnim) * 38;

  // pressure gauge: 720..790 mm
  const pk = Math.max(0, Math.min(1, (press - 720) / 70));
  const pa = Math.PI * (1 - pk);

  const lo = Math.min(...days.map((d) => d.min));
  const hi = Math.max(...days.map((d) => d.max));
  const span = Math.max(1, hi - lo);

  return (
    <div className="wx2">
      <div className={`wx2-now sky-${now.day ? sky(now.code) : "night"}`}>
        <div className="wx2-city">
          <button onClick={openSettings} title="Сменить город">
            {weather.city || "Здесь"}
          </button>
          <button className="wx2-refresh" onClick={() => call("weather_refresh")} title="Обновить">
            ↻
          </button>
        </div>
        <WeatherIcon code={now.code} day={now.day} size={40} />
        <div className="wx2-temp">{deg(temp)}</div>
        <div className="wx2-desc">{describe(now.code)}</div>
        <div className="wx2-sub">
          Ощущается как {deg(now.feels)}
          {today && (
            <>
              <br />
              Сегодня {deg(today.min)}…{deg(today.max)}
            </>
          )}
        </div>
        <div className="wx2-note">{daySummary(hours)}</div>
        <div className="wx2-src">Данные Open-Meteo</div>
      </div>

      <div className="card wx2-strip">
        <div className="wx2-strip-list">
          {mode === "hours"
            ? strip.map((h, i) => (
                <div className="wx2-h" key={h.t}>
                  <span>{i === 0 ? "Сейчас" : h.label}</span>
                  <WeatherIcon code={h.code} day={h.day} size={18} />
                  <b>{deg(h.temp)}</b>
                  {h.prob >= 30 && <i>{h.prob}%</i>}
                </div>
              ))
            : days.map((d) => (
                <div className="wx2-d" key={d.t}>
                  <span>{d.label}</span>
                  <WeatherIcon code={d.code} size={18} />
                  <b>{deg(d.max)}</b>
                  <small>{deg(d.min)}</small>
                  <em>
                    <u style={{ left: `${((d.min - lo) / span) * 100}%`, width: `${Math.max(6, ((d.max - d.min) / span) * 100)}%` }} />
                  </em>
                </div>
              ))}
        </div>
        <div className="wx2-mode">
          <button className={mode === "hours" ? "on" : ""} onClick={() => setMode("hours")}>
            Часы
          </button>
          <button className={mode === "days" ? "on" : ""} onClick={() => setMode("days")}>
            Неделя
          </button>
        </div>
      </div>

      <div className="card wx2-tile">
        <div className="wx2-t">Ветер</div>
        <div className="wx2-wind">
          <svg viewBox="0 0 60 60" className="wx2-compass">
            <circle cx="30" cy="30" r="26" />
            {[0, 90, 180, 270].map((a) => (
              <line key={a} x1="30" y1="6" x2="30" y2="10" transform={`rotate(${a} 30 30)`} />
            ))}
            <text x="30" y="17" textAnchor="middle">С</text>
            {/* arrow points where the wind blows to */}
            <g transform={`rotate(${windDir + 180} 30 30)`}>
              <path d="M30 12 L34 30 L30 27 L26 30 Z" className="wx2-arrow" />
              <path d="M30 48 L30 30" className="wx2-tail" />
            </g>
          </svg>
          <div>
            <b className="wx2-big">{num(wind, wind < 10 ? 1 : 0)}</b> <small>м/с</small>
            <div className="wx2-mini">{windName(now.windDir)}{now.gusts > now.wind + 2 ? `, порывы ${Math.round(now.gusts)}` : ""}</div>
          </div>
        </div>
      </div>

      <div className="card wx2-tile">
        <div className="wx2-t">Влажность</div>
        <b className="wx2-big">{Math.round(hum)}%</b>
        <div className="wx2-meter">
          <i style={{ width: `${hum}%` }} />
        </div>
        <div className="wx2-mini">Точка росы {deg(now.dew)}</div>
      </div>

      <div className="card wx2-tile">
        <div className="wx2-t">Давление</div>
        <div className="wx2-press">
          <svg viewBox="0 0 60 36" className="wx2-gauge">
            <path d="M6 32 A24 24 0 0 1 54 32" className="track" />
            <path d="M6 32 A24 24 0 0 1 54 32" className="fill" strokeDasharray={`${75.4 * pk} 100`} />
            <line x1="30" y1="32" x2={30 + Math.cos(pa) * 18} y2={32 - Math.sin(pa) * 18} className="needle" />
            <circle cx="30" cy="32" r="2.4" />
          </svg>
          <div>
            <b className="wx2-big">{Math.round(press)}</b>
            <div className="wx2-mini">мм рт. ст.</div>
          </div>
        </div>
        <div className="wx2-mini">{{ растёт: "↑ растёт", падает: "↓ падает", стабильно: "→ стабильно" }[pressureTrend(hours)]}</div>
      </div>

      <div className="card wx2-tile">
        <div className="wx2-t">Солнце</div>
        <svg viewBox="0 0 120 56" className="wx2-sun">
          <path d="M10 50 Q60 -26 110 50" className="arc" />
          <line x1="4" y1="50" x2="116" y2="50" className="horizon" />
          <circle cx={sx} cy={sy} r="4.5" className="dot" />
        </svg>
        <div className="wx2-sunrow">
          <span>↑ {hm(today?.sunrise ?? 0, off)}</span>
          <span>↓ {hm(today?.sunset ?? 0, off)}</span>
        </div>
      </div>

      <div className="card wx2-tile">
        <div className="wx2-t">Осадки</div>
        <div>
          <b className="wx2-big">{num(rain, rain > 0 && rain < 10 ? 1 : 0)}</b> <small>мм за сутки</small>
        </div>
        <div className="wx2-mini">{tomorrow ? (tomorrow.rain > 0.1 ? `Завтра ${num(tomorrow.rain, 1)} мм` : "Завтра сухо") : ""}</div>
      </div>

      <div className="card wx2-tile">
        <div className="wx2-t">УФ-индекс</div>
        <div>
          <b className="wx2-big">{Math.round(uv)}</b> <small>{uvLabel(now.uv)}</small>
        </div>
        <div className="wx2-uv">
          <i style={{ left: `${Math.min(100, (uv / 11) * 100)}%` }} />
        </div>
        <div className="wx2-mini">{tomorrow ? `Завтра до ${Math.round(tomorrow.uv)}` : ""}</div>
      </div>
    </div>
  );
}

/** Collapsed island without music: icon on the left, temperature on the right. */
export function WeatherCompact({ weather }: { weather: WeatherPayload }) {
  const { now } = parse(weather);
  return (
    <div className="wx-compact">
      <WeatherIcon code={now.code} day={now.day} size={20} />
      <span>{deg(now.temp)}</span>
    </div>
  );
}
