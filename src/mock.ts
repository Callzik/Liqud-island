// Demo backend used when the UI runs in a normal browser (no Tauri).
// Lets you develop and preview the island without Windows.

type Handler = (payload: any) => void;
const handlers = new Map<string, Set<Handler>>();

export function mockEmit(event: string, payload: unknown) {
  handlers.get(event)?.forEach((h) => h(payload));
}

export async function mockListen<T>(event: string, cb: (p: T) => void): Promise<() => void> {
  if (!handlers.has(event)) handlers.set(event, new Set());
  // a fresh wrapper per subscription: StrictMode subscribes the same setter twice,
  // and the first (async) unsubscribe must not remove the second one
  const h: Handler = (p) => cb(p as T);
  handlers.get(event)!.add(h);
  return () => handlers.get(event)?.delete(h);
}

const now = Date.now();
const coverSvg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 100 100'><defs><linearGradient id='g' x1='0' y1='0' x2='0' y2='1'><stop offset='0' stop-color='#f0598a'/><stop offset='.62' stop-color='#7b2b8f'/><stop offset='1' stop-color='#2a1450'/></linearGradient></defs><rect width='100' height='100' fill='url(#g)'/><circle cx='50' cy='52' r='22' fill='#ffd6a0'/><g fill='#2a1450'><rect x='0' y='56' width='100' height='3'/><rect x='0' y='62' width='100' height='3'/><rect x='0' y='68' width='100' height='4'/></g><path d='M0 78 L22 62 L38 72 L58 58 L80 72 L100 64 L100 100 L0 100Z' fill='#1b0d33'/></svg>`;
const cover = "data:image/svg+xml;utf8," + encodeURIComponent(coverSvg);

const state = {
  settings: {
    hotkey: "Alt+Space",
    autostart: true,
    musicReactive: true,
    hideFullscreen: true,
    hoverExpand: true,
    clipLimit: 200,
    cursorPull: true,
    captureEnabled: true,
    hotkeyRegion: "Ctrl+Shift+S",
    hotkeyOcr: "Ctrl+Shift+T",
    aiEnabled: true,
    aiUrl: "http://localhost:1234/v1",
    aiKey: "",
    aiModel: "qwen2.5-7b-instruct",
    weatherEnabled: true,
    weatherCity: "Москва",
    weatherLat: 55.75,
    weatherLon: 37.62,
    voiceEnabled: true,
    hotkeyVoice: "Ctrl+Alt+Space",
    voiceWhisperUrl: "",
    voiceWhisperKey: "",
    voiceWhisperModel: "whisper-1",
    downloadsEnabled: true,
    textEnabled: true,
    notesEnabled: true,
    residentsEnabled: true,
    residents: ["jelly", "cat", "ghost"],
    ymToken: "demo",
  },
  data: {
    shelf: [
      { id: "s1", path: "C:\\Users\\me\\Documents\\Отчёт Q3.pdf", name: "Отчёт Q3.pdf", size: 2_481_233, isDir: false, added: now - 3600e3 },
      { id: "s2", path: "C:\\Users\\me\\Pictures\\Скриншот.png", name: "Скриншот.png", size: 481_233, isDir: false, added: now - 7200e3 },
      { id: "s3", path: "C:\\Users\\me\\Downloads\\design", name: "design", size: 0, isDir: true, added: now - 9600e3 },
    ],
    pins: [
      { id: "p1", target: "app:explorer", name: "Проводник" },
      { id: "p2", target: "app:notepad", name: "Блокнот" },
      { id: "p3", target: "app:powershell", name: "PowerShell" },
      { id: "p4", target: "app:taskmgr", name: "Диспетчер задач" },
    ],
    usage: { "app:powershell": 4 } as Record<string, number>,
  },
  clips: [
    { id: "c1", kind: "text", preview: "https://github.com/tauri-apps/tauri", chars: 35, files: [], thumb: null, width: 0, height: 0, source: "chrome", ts: now - 5 * 3600e3, pinned: true },
    { id: "c2", kind: "text", preview: "npm run tauri dev", chars: 17, files: [], thumb: null, width: 0, height: 0, source: "WindowsTerminal", ts: now - 5 * 3600e3, pinned: false },
    { id: "c3", kind: "text", preview: "#D97757", chars: 7, files: [], thumb: null, width: 0, height: 0, source: "Figma", ts: now - 5.2 * 3600e3, pinned: false },
    { id: "c4", kind: "text", preview: "Встречу перенесли на четверг, 15:00. Повестка та же.", chars: 52, files: [], thumb: null, width: 0, height: 0, source: "Telegram", ts: now - 6 * 3600e3, pinned: false },
    { id: "c5", kind: "files", preview: "Отчёт Q3.pdf", chars: 12, files: ["C:\\Отчёт Q3.pdf"], thumb: null, width: 0, height: 0, source: "explorer", ts: now - 26 * 3600e3, pinned: false },
  ],
  media: {
    app: "Spotify",
    appId: "Spotify.exe",
    title: "Midnight Drive",
    artist: "Neon Harbor",
    album: "Night Lines",
    playing: true,
    positionMs: 85_000,
    durationMs: 214_000,
    updatedMs: now,
    canPrev: true,
    canNext: true,
    coverId: 1,
  },
  volume: { level: 0.62, muted: false, device: "Динамики" },
};

const apps = [
  "PowerShell", "Проводник", "Блокнот", "Диспетчер задач", "Параметры", "Калькулятор", "Google Chrome",
  "Telegram", "Visual Studio Code", "Spotify", "Figma", "Paint", "Терминал", "Microsoft Edge", "Word", "Excel",
].map((name) => ({ target: "app:" + name.toLowerCase(), name }));

function iconFor(key: string): string {
  let h = 0;
  for (const ch of key) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  const hue = h % 360;
  const letter = (key.split(/[:\\]/).pop() || "?").charAt(0).toUpperCase();
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 64 64'><rect x='4' y='4' width='56' height='56' rx='14' fill='hsl(${hue} 70% 55%)'/><text x='32' y='42' font-size='28' font-family='Segoe UI,Arial' font-weight='700' text-anchor='middle' fill='white'>${letter}</text></svg>`;
  return "data:image/svg+xml;utf8," + encodeURIComponent(svg);
}

export async function mockInvoke<T>(cmd: string, args: Record<string, any> = {}): Promise<T> {
  const r = (v: unknown) => Promise.resolve(v as T);
  switch (cmd) {
    case "init":
      (state as any).weather = demoWeather();
      return r({ ...state, cover: { id: 1, url: cover }, version: "0.1.0-dev", hotkeyError: null });
    case "list_apps":
      return r(apps);
    case "get_icons":
      return r(Object.fromEntries((args.keys as string[]).map((k) => [k, iconFor(k)])));
    case "save_settings":
      state.settings = args.settings;
      return r(state.settings);
    case "shelf_add":
      for (const p of args.paths as string[]) {
        const name = p.split(/[\\/]/).pop() || p;
        state.data.shelf.unshift({ id: String(Math.random()), path: p, name, size: 123456, isDir: false, added: Date.now() });
      }
      return r(state.data.shelf);
    case "shelf_remove":
      state.data.shelf = state.data.shelf.filter((s) => s.id !== args.id);
      return r(state.data.shelf);
    case "shelf_clear":
      state.data.shelf = [];
      return r([]);
    case "pins_add":
      for (const t of args.targets as string[]) state.data.pins.push({ id: String(Math.random()), target: t, name: t.split(/[\\/]/).pop()! });
      return r(state.data.pins);
    case "pins_add_app":
      state.data.pins.push({ id: String(Math.random()), target: args.target, name: args.name });
      return r(state.data.pins);
    case "pins_remove":
      state.data.pins = state.data.pins.filter((p) => p.id !== args.id);
      return r(state.data.pins);
    case "media_control":
      if (args.action === "toggle") {
        state.media = { ...state.media, playing: !state.media.playing, updatedMs: Date.now() };
        mockEmit("media", state.media);
      }
      return r(null);
    case "volume_set":
      state.volume = { ...state.volume, level: args.level };
      mockEmit("volume", state.volume);
      return r(null);
    case "clip_delete":
      state.clips = state.clips.filter((c) => c.id !== args.id);
      mockEmit("clips", state.clips);
      return r(null);
    case "clip_pin":
      state.clips = state.clips.map((c) => (c.id === args.id ? { ...c, pinned: args.pinned } : c));
      mockEmit("clips", state.clips);
      return r(null);
    case "pick_files":
      return r([]);
    case "capture": {
      // the real overlay is a separate window; here the result is simulated
      const demo: Record<string, unknown> = {
        region: { icon: "image", title: "Скриншот готов", subtitle: "В буфере · 463 × 463", image: cover, actions: [{ label: "Показать", cmd: "reveal", args: {} }], ms: 4200 },
        full: { icon: "image", title: "Скриншот готов", subtitle: "В буфере · 2560 × 1440", image: cover, actions: [{ label: "Показать", cmd: "reveal", args: {} }], ms: 4200 },
        ocr: { icon: "text", title: "Текст распознан · 128 симв.", subtitle: "Встречу перенесли на четверг, 15:00", ms: 3200 },
        qr: { icon: "qr", title: "Ссылка из QR-кода", subtitle: "https://github.com/Callzik", actions: [{ label: "Открыть", cmd: "open_target", args: {} }], ms: 6000 },
        picker: { icon: "color", title: "#D97757", subtitle: "Цвет скопирован", swatch: "#D97757", ms: 3000 },
      };
      if (args.mode === "record") {
        setTimeout(() => mockEmit("recording", { active: true, startedMs: Date.now() }), 400);
        return r(null);
      }
      setTimeout(() => mockEmit("toast", demo[args.mode as string] ?? demo.region), 500);
      return r(null);
    }
    case "ai_models":
      return r(["qwen2.5-7b-instruct", "llama-3.2-3b-instruct", "gemma-3-4b-it"]);
    case "ai_attach": {
      const name = String(args.path).split(/[\\/]/).pop()!;
      return r({ name, kind: "text", label: "12 стр", text: "Демо-текст документа." });
    }
    case "chat_load":
      return r([]);
    case "ai_send": {
      // stream a canned answer word by word
      const reply =
        "Вот что можно сделать:\n\n1. **Открыть** файл в редакторе\n2. Найти строку с `TODO`\n3. Заменить её\n\n```ts\nconst answer = 42;\n```\n\nГотово — *это демо-ответ*, настоящий придёт от вашей модели.";
      const parts = reply.split(/(?<=\s)/);
      let i = 0;
      const tick = setInterval(() => {
        if (i >= parts.length) {
          clearInterval(tick);
          mockEmit("ai-done", { id: args.id, error: null });
          return;
        }
        mockEmit("ai-chunk", { id: args.id, delta: parts[i++] });
      }, 45);
      return r(null);
    }
    case "weather_search":
      return r([
        { name: "Москва", area: "Россия", lat: 55.75, lon: 37.62 },
        { name: "Москва", area: "Айдахо, США", lat: 46.73, lon: -117 },
      ]);
    case "weather_locate":
      return r({ name: "Санкт-Петербург", area: "Россия", lat: 59.94, lon: 30.31 });
    case "voice": {
      const a = args.action as string;
      const w = window as any;
      if ((a === "start" || a === "toggle") && !w.__voiceT) {
        mockEmit("voice", { state: "recording", startedMs: Date.now() });
        let t = 0;
        w.__voiceT = setInterval(() => mockEmit("voice-level", Math.abs(Math.sin((t += 0.4)) * 0.7 + Math.random() * 0.3)), 50);
      } else if (w.__voiceT) {
        clearInterval(w.__voiceT);
        w.__voiceT = 0;
        if (a === "cancel") mockEmit("voice", { state: "idle", startedMs: 0 });
        else {
          mockEmit("voice", { state: "processing", startedMs: Date.now() });
          setTimeout(() => {
            mockEmit("voice", { state: "idle", startedMs: 0 });
            mockEmit("toast", { icon: "mic", title: "Вставлено · 42 симв.", subtitle: "Созвон переносим на завтра, на 11 утра", ms: 3000 });
          }, 900);
        }
      }
      return r(null);
    }
    case "record_stop":
      mockEmit("recording", { active: false, startedMs: 0 });
      setTimeout(
        () =>
          mockEmit("toast", {
            icon: "video",
            title: "Запись сохранена",
            subtitle: "Island-20261009-204500.mp4 · Видео\\Island",
            actions: [{ label: "Показать в папке", cmd: "reveal", args: {} }],
            ms: 6000,
          }),
        300,
      );
      return r(null);
    case "ym_state": {
      const w = window as any;
      w.__ym ??= { liked: false, disliked: false };
      return r({ available: true, trackId: "1", ...w.__ym });
    }
    case "ym_set": {
      const w = window as any;
      w.__ym ??= { liked: false, disliked: false };
      if (args.kind === "like") w.__ym = { liked: args.on, disliked: false };
      else w.__ym = { liked: false, disliked: args.on };
      return r(null);
    }
    case "audio_devices":
      return r([
        { id: "a", name: "Динамики (Realtek(R) Audio)", default: state.volume.device === "Динамики" },
        { id: "b", name: "Наушники (WH-1000XM4)", default: state.volume.device === "Наушники" },
        { id: "c", name: "LG ULTRAGEAR (NVIDIA High Definition Audio)", default: state.volume.device === "LG ULTRAGEAR" },
      ]);
    case "audio_set_device": {
      const names: Record<string, string> = { a: "Динамики", b: "Наушники", c: "LG ULTRAGEAR" };
      state.volume = { ...state.volume, device: names[args.id as string] ?? "Динамики" };
      mockEmit("volume", state.volume);
      return r(true);
    }
    case "clip_read_text":
      return r("ghbdtn! это ТЕКСТ из буфера,   с лишними    пробелами\nи переносом посреди\nпредложения.");
    case "notes_load":
      return r(
        (window as any).__notes ?? [
          { id: "n1", text: "Список покупок\nмолоко, хлеб, кофе", pinned: true, updated: Date.now() - 3600e3 },
          { id: "n2", text: "Идеи для Island\nжильцы, погода, голос", pinned: false, updated: Date.now() - 86400e3 },
        ],
      );
    case "notes_save":
      (window as any).__notes = args.notes;
      return r(null);
    case "open_launcher":
      mockEmit("launcher", true);
      return r(null);
    default:
      return r(true);
  }
}

// --- demo forecast ----------------------------------------------------------

function demoWeather() {
  const off = 3 * 3600;
  const base = new Date(Date.now() + off * 1000);
  base.setUTCMinutes(0, 0, 0);
  const iso = (d: Date) => d.toISOString().slice(0, 16);
  const time: string[] = [];
  const temp: number[] = [];
  const prob: number[] = [];
  const code: number[] = [];
  const isDay: number[] = [];
  for (let i = -2; i < 7 * 24; i++) {
    const d = new Date(base.getTime() + i * 3600e3);
    const hr = d.getUTCHours();
    time.push(iso(d));
    temp.push(Math.round((10 + 6 * Math.sin(((hr - 9) / 24) * 2 * Math.PI) + (i % 5) * 0.3) * 10) / 10);
    prob.push(i === 1 ? 75 : i > 1 && i < 4 ? 60 : (i * 13) % 40);
    code.push(i === 1 || i === 2 ? 61 : hr > 9 && hr < 15 ? 2 : hr < 6 ? 0 : 3);
    isDay.push(hr >= 7 && hr < 19 ? 1 : 0);
  }
  const days = Array.from({ length: 7 }, (_, i) => iso(new Date(base.getTime() + i * 86400e3)).slice(0, 10));
  return {
    city: "Москва",
    fetchedMs: Date.now(),
    data: {
      utc_offset_seconds: off,
      current: { temperature_2m: 15.4, apparent_temperature: 13.9, relative_humidity_2m: 71, wind_speed_10m: 3.4, weather_code: 2, is_day: 1, precipitation: 0 },
      hourly: { time, temperature_2m: temp, precipitation_probability: prob, weather_code: code, is_day: isDay },
      daily: {
        time: days,
        weather_code: [2, 61, 3, 0, 71, 95, 1],
        temperature_2m_min: [7, 5, 4, 2, -1, 6, 3],
        temperature_2m_max: [16, 12, 10, 11, 4, 14, 9],
      },
    },
  };
}

// --- simulated native events -------------------------------------------------

if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__ === undefined) {
  let lmb = false;
  window.addEventListener("mousedown", () => (lmb = true));
  window.addEventListener("mouseup", () => (lmb = false));
  window.addEventListener("mousemove", (e) => {
    const hit = (window as any).__islandHit as { x: number; y: number; w: number; h: number } | undefined;
    const inside = !!hit && e.clientX >= hit.x && e.clientX <= hit.x + hit.w && e.clientY >= hit.y && e.clientY <= hit.y + hit.h;
    mockEmit("cursor", { x: e.clientX, y: e.clientY, inside, lmb });
  });
  window.addEventListener("keydown", (e) => {
    if (e.altKey && e.code === "Space") {
      e.preventDefault();
      mockEmit("launcher", true);
    }
  });
  // fake audio level
  let t = 0;
  setInterval(() => {
    t += 0.05;
    if (!state.media.playing) return;
    const beat = Math.abs(Math.sin(t * 3.1)) * 0.55 + Math.random() * 0.35;
    mockEmit("audio", Math.min(1, beat));
  }, 50);
  // demo download: window.__mock.download()
  const download = () => {
    let bytes = 0;
    const t = setInterval(() => {
      bytes += 14 * 1024 * 1024 * 0.25 * (0.8 + Math.random() * 0.4);
      if (bytes > 2.4 * 1024 ** 3 || bytes > 180 * 1024 ** 2) {
        clearInterval(t);
        mockEmit("download", null);
        mockEmit("toast", {
          icon: "download",
          title: "Загружено",
          subtitle: "ubuntu-24.04-desktop.iso · 180 МБ",
          actions: [
            { label: "Открыть", cmd: "open_target", args: {} },
            { label: "На полку", cmd: "shelf_add", args: { paths: ["C:\\Users\\me\\Downloads\\ubuntu-24.04-desktop.iso"] } },
          ],
          ms: 7000,
        });
        return;
      }
      mockEmit("download", { name: "ubuntu-24.04-desktop.iso", bytes, speed: 14 * 1024 * 1024, count: 1 });
    }, 250);
  };
  (window as any).__mock = { emit: mockEmit, state, download };
}
