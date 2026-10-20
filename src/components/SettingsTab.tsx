import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { call, type HookStatus, type Settings } from "../api";
import { IKeyboard, IPower } from "./Icons";
import { RESIDENTS, ResidentSvg } from "../lib/residents";

function Toggle({ checked, onChange }: { checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <button className={`toggle ${checked ? "on" : ""}`} role="switch" aria-checked={checked} onClick={() => onChange(!checked)}>
      <i />
    </button>
  );
}

const CODE_NAMES: Record<string, string> = {
  Space: "Space",
  Enter: "Enter",
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Tab: "Tab",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
};

/** KeyboardEvent -> "Ctrl+Shift+K" (format understood by the global-shortcut plugin). */
function comboFrom(e: KeyboardEvent): string | null {
  const code = e.code;
  let key: string | null = null;
  if (/^Key[A-Z]$/.test(code)) key = code.slice(3);
  else if (/^Digit\d$/.test(code)) key = code.slice(5);
  else if (/^F\d{1,2}$/.test(code)) key = code;
  else if (CODE_NAMES[code]) key = CODE_NAMES[code];
  if (!key) return null;
  const mods = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Super"].filter(Boolean) as string[];
  if (mods.length === 0 && !/^F\d/.test(key)) return null;
  return [...mods, key].join("+");
}

function HotkeyField({ value, onChange, setBusy }: { value: string; onChange: (v: string) => void; setBusy: (b: boolean) => void }) {
  const [recording, setRec] = useState(false);
  const setRecording = (r: boolean) => {
    setRec(r);
    setBusy(r);
  };
  return (
    <button
      className={`hotkey ${recording ? "rec" : ""}`}
      onClick={() => setRecording(true)}
      onBlur={() => recording && setRecording(false)}
      onKeyDown={(e) => {
        if (!recording) return;
        e.preventDefault();
        e.stopPropagation();
        if (e.key === "Escape") return setRecording(false);
        const combo = comboFrom(e);
        if (combo) {
          setRecording(false);
          onChange(combo);
        }
      }}
    >
      <IKeyboard size={16} />
      {recording ? "Нажмите сочетание…" : value}
    </button>
  );
}

/** Text input that saves on Enter / when focus leaves. */
function TextField({
  value,
  onSave,
  setBusy,
  placeholder,
  secret,
}: {
  value: string;
  onSave: (v: string) => void;
  setBusy: (b: boolean) => void;
  placeholder?: string;
  secret?: boolean;
}) {
  const [v, setV] = useState(value);
  useEffect(() => setV(value), [value]);
  const commit = () => {
    if (v !== value) onSave(v.trim());
  };
  // Esc or leaving the panel unmounts the field without a blur: keep the edit
  const latest = useRef({ v, value, onSave });
  latest.current = { v, value, onSave };
  useEffect(
    () => () => {
      const l = latest.current;
      if (l.v !== l.value) l.onSave(l.v.trim());
    },
    [],
  );
  return (
    <input
      className="field"
      type={secret ? "password" : "text"}
      value={v}
      placeholder={placeholder}
      spellCheck={false}
      autoComplete="off"
      onFocus={() => setBusy(true)}
      onBlur={() => {
        setBusy(false);
        commit();
      }}
      onChange={(e) => setV(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
    />
  );
}

function ModelPicker({ settings, save, setBusy }: { settings: Settings; save: (m: string) => void; setBusy: (b: boolean) => void }) {
  const [models, setModels] = useState<string[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = () => {
    setErr(null);
    call<string[]>("ai_models", {})
      .then((m) => setModels(m))
      .catch((e) => {
        setModels([]);
        setErr(String(e));
      });
  };
  useEffect(load, [settings.aiUrl, settings.aiKey]);
  const list = models ?? [];
  const options = settings.aiModel && !list.includes(settings.aiModel) ? [settings.aiModel, ...list] : list;
  return (
    <div className="model-picker">
      <select
        className="select"
        value={settings.aiModel}
        onFocus={() => setBusy(true)}
        onBlur={() => setBusy(false)}
        onChange={(e) => {
          setBusy(false);
          save(e.target.value);
        }}
      >
        <option value="">{models === null ? "Загружаю…" : "По умолчанию сервера"}</option>
        {options.map((m) => (
          <option key={m} value={m}>
            {m}
          </option>
        ))}
      </select>
      <button className="text-btn" onClick={load} title={err ?? "Обновить список"}>
        {err ? "Нет связи" : "Обновить"}
      </button>
    </div>
  );
}

interface Place {
  name: string;
  area: string;
  lat: number;
  lon: number;
}

function CityPicker({ settings, save, setBusy }: { settings: Settings; save: (patch: Partial<Settings>) => void; setBusy: (b: boolean) => void }) {
  const [q, setQ] = useState("");
  const [found, setFound] = useState<Place[] | null>(null);
  const [busy, setLocal] = useState(false);
  useEffect(() => {
    const query = q.trim();
    if (query.length < 2) return setFound(null);
    const t = window.setTimeout(() => {
      call<Place[]>("weather_search", { q: query })
        .then(setFound)
        .catch(() => setFound([]));
    }, 300);
    return () => window.clearTimeout(t);
  }, [q]);
  const choose = (p: Place) => {
    save({ weatherCity: p.name, weatherLat: p.lat, weatherLon: p.lon });
    setQ("");
    setFound(null);
  };
  return (
    <div className="city-picker">
      <div className="city-row">
        <input
          className="field"
          value={q}
          placeholder={settings.weatherCity ? `${settings.weatherCity} — найти другой город` : "Определяется по IP · найти город"}
          spellCheck={false}
          autoComplete="off"
          onFocus={() => setBusy(true)}
          onBlur={() => setBusy(false)}
          onChange={(e) => setQ(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && found?.[0]) choose(found[0]);
          }}
        />
        <button
          className="text-btn"
          disabled={busy}
          title="Определить по IP-адресу"
          onClick={async () => {
            setLocal(true);
            try {
              const p = await call<Place>("weather_locate");
              choose(p);
            } catch {
              save({ weatherCity: "", weatherLat: 0, weatherLon: 0 });
            } finally {
              setLocal(false);
            }
          }}
        >
          {busy ? "Ищу…" : "Авто"}
        </button>
      </div>
      {found && (
        <div className="city-list">
          {found.length === 0 && <div className="hint">Ничего не нашлось</div>}
          {found.map((p) => (
            <button key={`${p.lat},${p.lon}`} onMouseDown={(e) => e.preventDefault()} onClick={() => choose(p)}>
              <b>{p.name}</b> <span>{p.area}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function AgentsSettings({ settings, set }: { settings: Settings; set: <K extends keyof Settings>(k: K, v: Settings[K]) => void }) {
  const [hooks, setHooks] = useState<HookStatus | null>(null);
  const [err, setErr] = useState("");
  useEffect(() => {
    call<HookStatus>("agents_hooks", { action: "status" }).then(setHooks).catch(() => {});
  }, []);
  const run = (action: "install" | "remove") =>
    call<HookStatus>("agents_hooks", { action })
      .then((h) => {
        setHooks(h);
        setErr("");
      })
      .catch((e) => setErr(String(e)));
  return (
    <>
      <div className="settings-section">Агенты</div>
      <div className="setting">
        <div>
          <div className="setting-title">Claude Code и Codex в острове</div>
          <div className="setting-sub">Вкладки «Агенты» и «Офис», плашка «Нужен ответ»</div>
        </div>
        <Toggle checked={settings.agentsEnabled} onChange={(v) => set("agentsEnabled", v)} />
      </div>
      <div className="setting">
        <div>
          <div className="setting-title">Двойной Ctrl — офис</div>
          <div className="setting-sub">Пиксельный офис: кто пишет код, кто ждёт, кто отдыхает</div>
        </div>
        <Toggle checked={settings.officeHotkey} onChange={(v) => set("officeHotkey", v)} />
      </div>
      <div className="setting setting-wide">
        <div>
          <div className="setting-title">
            {hooks?.installed ? (hooks.stale ? "Хуки Claude Code устарели" : "Claude Code подключён") : "Подключить Claude Code"}
          </div>
          <div className="setting-sub">
            {err ||
              (hooks?.installed
                ? `Хуки в ${hooks.settingsPath}. Разрешения и вопросы ждут ответа в острове 2 минуты, потом — в терминале`
                : "Добавит хуки в ~/.claude/settings.json (копия файла сохранится рядом). Codex виден без настройки")}
          </div>
        </div>
        {hooks?.installed && !hooks.stale ? (
          <button className="text-btn" onClick={() => run("remove")}>
            Отключить
          </button>
        ) : (
          <button className="text-btn" onClick={() => run("install")}>
            {hooks?.stale ? "Обновить" : "Подключить"}
          </button>
        )}
      </div>
    </>
  );
}

export function SettingsTab({
  settings,
  save,
  version,
  setBusy,
}: {
  settings: Settings;
  save: (patch: Partial<Settings>) => void;
  version: string;
  setBusy: (b: boolean) => void;
}) {
  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => save({ [k]: v } as Partial<Settings>);
  const [confirm, setConfirm] = useState(false);

  const rows: { key: keyof Settings; title: string; sub: string }[] = [
    { key: "autostart", title: "Запускать вместе с Windows", sub: "Остров появится сразу после входа" },
    { key: "hoverExpand", title: "Раскрывать при наведении", sub: "Иначе — по клику" },
    { key: "musicReactive", title: "Волна под музыку", sub: "Край острова дрожит в такт звуку" },
    { key: "coverTint", title: "Цвет под обложку", sub: "Остров подсвечивается цветом обложки трека" },
    { key: "cursorPull", title: "Тянуться к курсору", sub: "Жидкий край вытягивается навстречу" },
    { key: "hideFullscreen", title: "Прятать в полноэкранных приложениях", sub: "Игры, видео, презентации" },
    { key: "downloadsEnabled", title: "Загрузки браузера", sub: "Прогресс в острове, «Загружено» → открыть или на полку" },
    { key: "textEnabled", title: "Вкладка «Текст»", sub: "Регистр, транслит, раскладка, пробелы" },
    { key: "notesEnabled", title: "Вкладка «Заметки»", sub: "Сохраняются автоматически" },
    { key: "lyricsInIsland", title: "Текст песни в свёрнутом острове", sub: "Текущая строка между обложкой и эквалайзером" },
    { key: "updateCheck", title: "Проверять обновления", sub: "Новая версия с GitHub — кнопка «Обновить» в острове" },
  ];

  return (
    <div className="settings">
      <div className="settings-grid">
        <div className="settings-section">Основное</div>
        <div className="setting setting-wide">
          <div>
            <div className="setting-title">Оформление</div>
            <div className="setting-sub">
              {settings.theme === "glass" ? "Матовое стекло: сквозь остров видно, что под ним" : "Классический чёрный остров"}
            </div>
          </div>
          <div className="seg">
            <button className={settings.theme === "glass" ? "on" : ""} onClick={() => set("theme", "glass")}>
              Liquid Glass
            </button>
            <button className={settings.theme !== "glass" ? "on" : ""} onClick={() => set("theme", "black")}>
              Чёрный
            </button>
          </div>
        </div>
        {rows.map((r) => (
          <div className="setting" key={r.key}>
            <div>
              <div className="setting-title">{r.title}</div>
              <div className="setting-sub">{r.sub}</div>
            </div>
            <Toggle checked={settings[r.key] as boolean} onChange={(v) => set(r.key, v as never)} />
          </div>
        ))}
        <div className="setting">
          <div>
            <div className="setting-title">Лаунчер</div>
            <div className="setting-sub">Глобальное сочетание клавиш</div>
          </div>
          <HotkeyField value={settings.hotkey} onChange={(v) => set("hotkey", v)} setBusy={setBusy} />
        </div>
        <div className="setting">
          <div>
            <div className="setting-title">История буфера</div>
            <div className="setting-sub">Сколько записей хранить (закреплённые — всегда)</div>
          </div>
          <select
            className="select"
            value={settings.clipLimit}
            onFocus={() => setBusy(true)}
            onBlur={() => setBusy(false)}
            onChange={(e) => {
              setBusy(false);
              set("clipLimit", Number(e.target.value));
            }}
          >
            {[50, 100, 200, 500, 1000].map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </div>

        <AgentsSettings settings={settings} set={set} />

        <div className="settings-section">Захват экрана</div>
        <div className="setting">
          <div>
            <div className="setting-title">Оверлей захвата</div>
            <div className="setting-sub">Область, текст, QR-код, пипетка</div>
          </div>
          <Toggle checked={settings.captureEnabled} onChange={(v) => set("captureEnabled", v)} />
        </div>
        <div className="setting">
          <div>
            <div className="setting-title">Снимок области</div>
            <div className="setting-sub">PNG в буфер и в «Снимки экрана»</div>
          </div>
          <HotkeyField value={settings.hotkeyRegion} onChange={(v) => set("hotkeyRegion", v)} setBusy={setBusy} />
        </div>
        <div className="setting">
          <div>
            <div className="setting-title">Текст с экрана</div>
            <div className="setting-sub">Распознать и скопировать</div>
          </div>
          <HotkeyField value={settings.hotkeyOcr} onChange={(v) => set("hotkeyOcr", v)} setBusy={setBusy} />
        </div>

        <div className="settings-section">ИИ-чат</div>
        <div className="setting">
          <div>
            <div className="setting-title">Вкладка «Чат»</div>
            <div className="setting-sub">И строка «Спросить ИИ» в лаунчере</div>
          </div>
          <Toggle checked={settings.aiEnabled} onChange={(v) => set("aiEnabled", v)} />
        </div>
        <div className="setting">
          <div>
            <div className="setting-title">Модель</div>
            <div className="setting-sub">Список с сервера</div>
          </div>
          <ModelPicker settings={settings} save={(m) => set("aiModel", m)} setBusy={setBusy} />
        </div>
        <div className="setting setting-wide">
          <div>
            <div className="setting-title">Адрес API</div>
            <div className="setting-sub">OpenAI-совместимый: LM Studio, Ollama, OpenRouter…</div>
          </div>
          <TextField value={settings.aiUrl} onSave={(v) => set("aiUrl", v || "http://localhost:1234/v1")} setBusy={setBusy} placeholder="http://localhost:1234/v1" />
        </div>
        <div className="setting setting-wide">
          <div>
            <div className="setting-title">Ключ API</div>
            <div className="setting-sub">Необязательно · хранится только на этом компьютере</div>
          </div>
          <TextField value={settings.aiKey} onSave={(v) => set("aiKey", v)} setBusy={setBusy} placeholder="sk-…" secret />
        </div>

        <div className="settings-section">Погода</div>
        <div className="setting">
          <div>
            <div className="setting-title">Погода в острове</div>
            <div className="setting-sub">Температура без музыки, вкладка, «через час дождь»</div>
          </div>
          <Toggle checked={settings.weatherEnabled} onChange={(v) => set("weatherEnabled", v)} />
        </div>
        <div className="setting">
          <div>
            <div className="setting-title">Город</div>
            <div className="setting-sub">{settings.weatherCity || "По IP-адресу"}</div>
          </div>
        </div>
        <div className="setting setting-wide city-setting">
          <CityPicker settings={settings} save={save} setBusy={setBusy} />
        </div>

        <div className="settings-section">Яндекс Музыка</div>
        <div className="setting setting-wide">
          <div>
            <div className="setting-title">{settings.ymToken ? "Аккаунт подключён" : "Лайки и дизлайки"}</div>
            <div className="setting-sub">
              {settings.ymToken
                ? "♥ и «не рекомендовать» на главной ставятся в вашу Яндекс Музыку"
                : "Войдите, чтобы ставить ♥ и «не рекомендовать» прямо с острова. Токен хранится только на этом компьютере"}
            </div>
          </div>
          {settings.ymToken ? (
            <button className="text-btn" onClick={() => call("ym_logout")}>
              Выйти
            </button>
          ) : (
            <button className="tt-paste" onClick={() => call("ym_login")}>
              Войти
            </button>
          )}
        </div>
        {!settings.ymToken && (
          <div className="setting setting-wide">
            <div>
              <div className="setting-title">Или вставьте токен вручную</div>
              <div className="setting-sub">
                <a
                  href="#"
                  onClick={(e) => {
                    e.preventDefault();
                    call("open_target", { target: "https://oauth.yandex.ru/authorize?response_type=token&client_id=23cabbbdc6cd418abb4b39c32c41195d", count: false });
                  }}
                >
                  Откройте вход в браузере
                </a>
                , войдите и скопируйте адрес страницы, куда вас перенесёт (в нём есть access_token)
              </div>
            </div>
            <TextField
              value=""
              secret
              setBusy={setBusy}
              placeholder="Адрес или токен"
              onSave={(v) => {
                if (v) call("ym_set_token", { value: v }).catch((e) => alert(String(e)));
              }}
            />
          </div>
        )}

        <div className="setting setting-wide">
          <div>
            <div className="setting-title">«Без цензуры» — ключ YouTube</div>
            <div className="setting-sub">
              Необязательно. С ключом трек находится и включается сам, без ключа открывается поиск.{" "}
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  call("open_target", { target: "https://console.cloud.google.com/apis/library/youtube.googleapis.com", count: false });
                }}
              >
                Получить ключ YouTube Data API
              </a>
            </div>
          </div>
          <TextField value={settings.ytKey} onSave={(v) => set("ytKey", v)} setBusy={setBusy} placeholder="AIza…" secret />
        </div>
        {settings.ytKey && (
          <div className="setting setting-wide">
            <div>
              <div className="setting-title">Без цензуры автоматически</div>
              <div className="setting-sub">
                Яндекс Музыка выбирает треки, звук идёт с YouTube Music в скрытом окне. Пауза и «вперёд» на острове управляют обоими.
              </div>
            </div>
            <div className="model-picker">
              <button className="text-btn" onClick={() => call("yt_show")} title="Показать окно YouTube — например, чтобы войти в аккаунт">
                Окно YouTube
              </button>
              <Toggle checked={settings.ytAuto} onChange={(v) => set("ytAuto", v)} />
            </div>
          </div>
        )}

        <div className="settings-section">Жильцы</div>
        <div className="setting">
          <div>
            <div className="setting-title">Жильцы у острова</div>
            <div className="setting-sub">Прыгают под музыку, следят за курсором, ночью спят</div>
          </div>
          <Toggle checked={settings.residentsEnabled} onChange={(v) => set("residentsEnabled", v)} />
        </div>
        <div className="setting">
          <div>
            <div className="setting-title">{settings.residents.length ? `Живут: ${settings.residents.length} из ${RESIDENTS.length}` : "Без жильца"}</div>
            <div className="setting-sub">Нажмите на жильца, чтобы поселить или выселить</div>
          </div>
          <button className={`res-none ${settings.residents.length ? "" : "on"}`} onClick={() => set("residents", [])}>
            Без жильца
          </button>
        </div>
        <div className="setting setting-wide res-grid">
          {RESIDENTS.map((r) => {
            const on = settings.residents.includes(r.id);
            return (
              <button
                key={r.id}
                className={`res-pick ${on ? "on" : ""}`}
                title={r.name}
                onClick={() => set("residents", on ? settings.residents.filter((x) => x !== r.id) : [...settings.residents, r.id])}
              >
                <ResidentSvg r={r} face={{ look: { x: 0, y: 0.2 }, asleep: false }} size={30} />
                <span>{r.name}</span>
              </button>
            );
          })}
        </div>

        <div className="settings-section">Голос → текст</div>
        <div className="setting">
          <div>
            <div className="setting-title">Диктовка</div>
            <div className="setting-sub">Текст вставляется туда, где вы печатали</div>
          </div>
          <Toggle checked={settings.voiceEnabled} onChange={(v) => set("voiceEnabled", v)} />
        </div>
        <div className="setting">
          <div>
            <div className="setting-title">Сочетание клавиш</div>
            <div className="setting-sub">Начать / закончить запись</div>
          </div>
          <HotkeyField value={settings.hotkeyVoice} onChange={(v) => set("hotkeyVoice", v)} setBusy={setBusy} />
        </div>
        <div className="setting setting-wide">
          <div>
            <div className="setting-title">Whisper (необязательно)</div>
            <div className="setting-sub">Адрес …/audio/transcriptions. Пусто — распознавание Windows, офлайн</div>
          </div>
          <TextField
            value={settings.voiceWhisperUrl}
            onSave={(v) => set("voiceWhisperUrl", v)}
            setBusy={setBusy}
            placeholder="http://localhost:8000/v1/audio/transcriptions"
          />
        </div>
        {settings.voiceWhisperUrl && (
          <>
            <div className="setting">
              <div>
                <div className="setting-title">Модель Whisper</div>
              </div>
              <TextField value={settings.voiceWhisperModel} onSave={(v) => set("voiceWhisperModel", v || "whisper-1")} setBusy={setBusy} placeholder="whisper-1" />
            </div>
            <div className="setting">
              <div>
                <div className="setting-title">Ключ Whisper</div>
              </div>
              <TextField value={settings.voiceWhisperKey} onSave={(v) => set("voiceWhisperKey", v)} setBusy={setBusy} placeholder="необязательно" secret />
            </div>
          </>
        )}
      </div>
      <div className="settings-foot">
        <span className="muted">Island {version} · Alt+Space — лаунчер · перетащите файл на остров — полка</span>
        <button className="text-btn" style={{ marginLeft: "auto" }} onClick={() => call("update_check", { manual: true }).catch(() => {})}>
          Проверить обновления
        </button>
        <button
          className={`text-btn ${confirm ? "danger" : ""}`}
          onClick={() => {
            if (!confirm) {
              setConfirm(true);
              window.setTimeout(() => setConfirm(false), 2500);
              return;
            }
            call("quit");
          }}
        >
          <IPower size={15} /> {confirm ? "Точно выйти?" : "Выйти"}
        </button>
      </div>
    </div>
  );
}
