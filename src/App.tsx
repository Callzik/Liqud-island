import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  call,
  on,
  onDrop,
  onFocus,
  type AppItem,
  type Clip,
  type Cover,
  type Cursor,
  type InitPayload,
  type Media,
  type PinItem,
  type Settings,
  type ShelfItem,
  type Volume,
} from "./api";
import { Liquid, type LiquidTarget } from "./liquid";
import { accentFrom, DEFAULT_ACCENT, type RGB } from "./lib/color";
import { Compact, Peek } from "./components/Compact";
import { chat } from "./lib/chatStore";
import { Toast, type ToastData } from "./components/Toast";
import { DropZones, type DropZone } from "./components/DropZones";
import { Panel, type Tab } from "./components/Panel";
import { Launcher } from "./components/Launcher";
import { useLyrics } from "./lib/useLyrics";
import { setYandexLyrics } from "./lib/lyrics";
import { rainSoon, sky, deg, type WeatherPayload } from "./lib/weather";
import { WeatherCompact } from "./components/Weather";
import { VoicePill, type VoiceState } from "./components/VoicePill";
import { DownloadPill, type DownloadProgress } from "./components/DownloadPill";
import { Residents, type IslandBox } from "./components/Residents";
import { RecPill, type RecordingState } from "./components/RecPill";

type Mode = "hidden" | "idle" | "music" | "peek" | "toast" | "drop" | "panel" | "launcher" | "voice" | "download" | "rec";

const BOX: Record<Exclude<Mode, "launcher">, { w: number; h: number; r: number }> = {
  hidden: { w: 0, h: 0, r: 0 },
  idle: { w: 176, h: 30, r: 15 },
  music: { w: 320, h: 38, r: 19 },
  peek: { w: 430, h: 70, r: 28 },
  toast: { w: 400, h: 66, r: 27 },
  drop: { w: 600, h: 160, r: 30 },
  panel: { w: 820, h: 440, r: 30 },
  voice: { w: 330, h: 46, r: 23 },
  download: { w: 360, h: 48, r: 24 },
  rec: { w: 196, h: 40, r: 20 },
};

const LAUNCHER_W = 680;
const MUSIC_LYRICS_W = 470;

/** Cursor left the panel: how long before it folds (ms). Just enough to ignore
 *  brushing the edge. */
const LEAVE_MS = 90;

/** Field with text being typed into right now (losing it would be annoying). */
function typing(): boolean {
  if (!document.hasFocus()) return false;
  const a = document.activeElement;
  if (a instanceof HTMLTextAreaElement) return a.value.trim() !== "";
  if (a instanceof HTMLInputElement && a.type !== "range" && a.type !== "checkbox") return a.value.trim() !== "";
  return false;
}

/** Reasons to keep the panel open after the cursor has left it. */
function holdOpen(n: { lmb: boolean; busy: boolean }): boolean {
  // button held: slider drag, text selection, file drag out
  if (n.lmb) return true;
  const a = document.activeElement;
  const editing = document.hasFocus() && (a instanceof HTMLTextAreaElement || (a instanceof HTMLInputElement && a.type !== "range"));
  // a focused field decides by itself: empty → fold, text in it → wait
  if (editing) return typing();
  // native file dialog / device list / drag-out in progress
  return n.busy;
}

export default function App({ boot }: { boot: InitPayload }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const clipRef = useRef<HTMLDivElement>(null);
  const islandBox = useRef<IslandBox>({ cx: 500, w: 0, h: 0 });
  const innerRef = useRef<HTMLDivElement>(null);
  // DOM snapshot of the previous state's content, faded out while the shape morphs
  const ghostRef = useRef<{ mode: Mode; node: Node | null }>({ mode: "hidden", node: null });
  const liquidRef = useRef<Liquid | null>(null);

  const [settings, setSettings] = useState<Settings>(boot.settings);
  const [media, setMedia] = useState<Media | null>(boot.media);
  const [mediaAt, setMediaAt] = useState(Date.now());
  const [cover, setCover] = useState<Cover>(boot.cover);
  const [accent, setAccent] = useState<RGB>(DEFAULT_ACCENT);
  const [volume, setVolume] = useState<Volume>(boot.volume);
  const [clips, setClips] = useState<Clip[]>(boot.clips);
  const [shelf, setShelf] = useState<ShelfItem[]>(boot.data.shelf);
  const [pins, setPins] = useState<PinItem[]>(boot.data.pins);
  const [usage, setUsage] = useState<Record<string, number>>(boot.data.usage);
  const [apps, setApps] = useState<AppItem[]>([]);

  const [panelOpen, setPanelOpen] = useState(false);
  const [pinned, setPinned] = useState(false);
  const [tab, setTab] = useState<Tab>("home");
  const [ask, setAsk] = useState<{ text: string; n: number } | null>(null);
  const [weather, setWeather] = useState<WeatherPayload | null>(boot.weather ?? null);
  const [voice, setVoice] = useState<VoiceState>({ state: "idle", startedMs: 0 });
  const [download, setDownload] = useState<DownloadProgress | null>(null);
  const [rec, setRec] = useState<RecordingState>({ active: false, startedMs: 0 });
  const chatDrop = useRef<((paths: string[]) => void) | null>(null);
  const setChatDrop = useCallback((fn: ((paths: string[]) => void) | null) => {
    chatDrop.current = fn;
  }, []);
  const clearAsk = useCallback(() => setAsk(null), []);
  const [launcherOpen, setLauncherOpen] = useState(false);
  const [launcherH, setLauncherH] = useState(64);
  const [drop, setDrop] = useState<{ paths: string[]; zone: DropZone | null } | null>(null);
  const [toast, setToast] = useState<ToastData | null>(null);
  const [peek, setPeek] = useState(false);
  const [suppressed, setSuppressed] = useState(false);

  // latest values for long-lived event listeners
  const live = useRef({ panelOpen, pinned, launcherOpen, drop, settings, media, armed: true, lmb: false, busy: false, toastActions: false, tab: "home" as Tab });
  live.current = { ...live.current, panelOpen, pinned, launcherOpen, drop, settings, media, toastActions: !!toast?.actions?.length || voice.state !== "idle" || rec.active, tab };

  const mode: Mode = suppressed
    ? "hidden"
    : voice.state !== "idle"
      ? "voice"
      : rec.active && !launcherOpen && !panelOpen && !drop
        ? "rec"
      : launcherOpen
      ? "launcher"
      : drop
        ? "drop"
        : panelOpen
          ? "panel"
          : toast
            ? "toast"
            : download && settings.downloadsEnabled
              ? "download"
            : peek && media
              ? "peek"
              : media?.playing
                ? "music"
                : "idle";

  if (ghostRef.current.mode !== mode) {
    // render runs before the DOM swap: the old content is still in the document
    ghostRef.current = { mode, node: innerRef.current ? innerRef.current.cloneNode(true) : null };
  }

  // set during render, so the lyrics key below already knows the source
  setYandexLyrics(!!settings.ymToken);
  const lyrics = useLyrics(media);
  const lyricsInIsland = mode === "music" && !!lyrics.lines;

  const box =
    mode === "launcher"
      ? { w: LAUNCHER_W, h: launcherH, r: 26 }
      : lyricsInIsland
        ? { ...BOX.music, w: MUSIC_LYRICS_W }
        : BOX[mode];

  useLayoutEffect(() => {
    placeContent();
  });

  useLayoutEffect(() => {
    const node = ghostRef.current.node as HTMLElement | null;
    const host = clipRef.current;
    ghostRef.current.node = null;
    if (!node || !host || !node.childNodes.length) return;
    node.classList.add("island-ghost");
    node.removeAttribute("id");
    host.appendChild(node);
    const t = window.setTimeout(() => node.remove(), 260);
    return () => {
      window.clearTimeout(t);
      node.remove();
    };
  }, [mode]);

  // ---- "Через час дождь" (once per rainy hour) --------------------------------
  const rainAlerted = useRef(0);
  useEffect(() => {
    if (!weather || !settings.weatherEnabled) return;
    const check = () => {
      const h = rainSoon(weather);
      if (!h || rainAlerted.current === h.t) return;
      rainAlerted.current = h.t;
      const what = sky(h.code) === "snow" ? "снег" : "дождь";
      showToast({ icon: "rain", title: `Через час ${what}`, subtitle: `с ${h.label} · ${deg(h.temp)} · ${h.prob}%`, wx: h.code, ms: 6000 });
    };
    check();
    const t = window.setInterval(check, 5 * 60e3);
    return () => window.clearInterval(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [weather, settings.weatherEnabled]);

  // Content sits on whole pixels: the clip box animates with fractional sizes, so
  // centring by CSS (left: 50% + translateX(-50%)) put text on half pixels and
  // made it look smeared. Every frame each content layer gets an integer x.
  const placeContent = useCallback(() => {
    const el = clipRef.current;
    if (!el) return;
    const cx = islandBox.current.cx;
    const clipLeft = parseFloat(el.style.left) || 0;
    for (const child of Array.from(el.children)) {
      if (!(child instanceof HTMLElement) || !child.classList.contains("island-inner")) continue;
      const cw = parseFloat(child.style.width) || child.offsetWidth;
      child.style.left = `${Math.round(cx - cw / 2) - clipLeft}px`;
    }
  }, []);

  // ---- liquid canvas -------------------------------------------------------
  useLayoutEffect(() => {
    const liquid = new Liquid(canvasRef.current!);
    liquidRef.current = liquid;
    liquid.onBox = (w, h, r) => {
      islandBox.current = { cx: liquid.centerX, w, h: Math.max(0, h) };
      const el = clipRef.current;
      if (!el) return;
      el.style.left = `${Math.round(liquid.centerX - w / 2)}px`;
      el.style.width = `${Math.round(w)}px`;
      el.style.height = `${Math.max(0, h)}px`;
      el.style.borderRadius = `0 0 ${r}px ${r}px`;
      placeContent();
    };
    return () => {
      liquid.destroy();
      if (liquidRef.current === liquid) liquidRef.current = null;
    };
  }, []);

  useEffect(() => {
    const liquid = liquidRef.current;
    if (!liquid) return;
    const music = !!media?.playing;
    const rimFor = (): LiquidTarget["rim"] => {
      switch (mode) {
        case "voice":
        case "rec":
          return [255, 77, 77, 0.95];
        case "download":
          return [110, 180, 255, 0.85];
        case "music":
        case "peek":
          return [accent[0], accent[1], accent[2], 0.95];
        case "drop":
          return [151, 135, 255, 0.9];
        case "toast":
          return toast?.tone === "error" ? [255, 99, 99, 0.85] : [255, 255, 255, 0.16];
        case "panel":
        case "launcher":
          return music ? [accent[0], accent[1], accent[2], 0.35] : [255, 255, 255, 0.08];
        default:
          return [255, 255, 255, 0.1];
      }
    };
    liquid.setTarget({
      w: box.w,
      h: box.h,
      r: box.r,
      pull: settings.cursorPull && (mode === "idle" || mode === "music" || mode === "toast" || mode === "peek"),
      wave: settings.musicReactive && music && (mode === "music" || mode === "peek"),
      bars: music && (mode === "music" || mode === "peek"),
      rim: rimFor(),
      shadow: mode === "panel" || mode === "launcher" || mode === "drop",
    });
    const pad = mode === "drop" ? 24 : mode === "panel" || mode === "launcher" ? 10 : 6;
    const rect = box.w > 0 ? { x: liquid.centerX - box.w / 2 - pad, y: 0, w: box.w + pad * 2, h: box.h + pad } : { x: 0, y: 0, w: 0, h: 0 };
    (window as any).__islandHit = rect;
    call("set_hit", { rect }).catch(() => {});
  }, [mode, box.w, box.h, box.r, accent, media?.playing, settings.cursorPull, settings.musicReactive, toast?.tone]);

  // ---- accent from cover ---------------------------------------------------
  useEffect(() => {
    if (!cover.url) {
      setAccent(DEFAULT_ACCENT);
      return;
    }
    let alive = true;
    accentFrom(cover.url).then((c) => alive && setAccent(c));
    return () => {
      alive = false;
    };
  }, [cover.url]);

  // ---- helpers -------------------------------------------------------------
  const toastTimer = useRef(0);
  const showToast = useCallback((t: ToastData, ms = 2400) => {
    setToast(t);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), t.ms ?? ms);
  }, []);
  const hideToast = useCallback(() => {
    window.clearTimeout(toastTimer.current);
    setToast(null);
  }, []);

  // A command that failed and nobody handled it (Rust returned Err(String)):
  // say so instead of silently doing nothing.
  useEffect(() => {
    const onRejection = (e: PromiseRejectionEvent) => {
      if (typeof e.reason !== "string" || !e.reason) return;
      e.preventDefault();
      showToast({ icon: "error", tone: "error", title: "Не получилось", subtitle: e.reason.slice(0, 120) }, 3500);
    };
    window.addEventListener("unhandledrejection", onRejection);
    return () => window.removeEventListener("unhandledrejection", onRejection);
  }, [showToast]);

  const peekTimer = useRef(0);
  const triggerPeek = useCallback(() => {
    setPeek(true);
    window.clearTimeout(peekTimer.current);
    peekTimer.current = window.setTimeout(() => setPeek(false), 3600);
  }, []);

  const restoreFocus = useCallback(() => {
    if (document.hasFocus()) call("launcher_closed", { restore: true }).catch(() => {});
  }, []);

  const closePanel = useCallback(() => {
    setPanelOpen(false);
    restoreFocus();
  }, [restoreFocus]);

  // Opened from the tray: the hover logic is armed only once the cursor reaches
  // the island; if it never does, fold back after a while.
  const unarmedTimer = useRef(0);
  const openPanel = useCallback(
    (t?: Tab, armed = true) => {
      if (t) setTab(t);
      live.current.armed = armed;
      setPanelOpen(true);
      window.clearTimeout(unarmedTimer.current);
      if (!armed) {
        // an answer being written keeps the panel open, and once it's done
        // there is time to read it (longer answers get longer)
        let readUntil = 0;
        unarmedTimer.current = window.setTimeout(function fold() {
          const n = live.current;
          if (chat.get().streaming) {
            readUntil = -1;
            unarmedTimer.current = window.setTimeout(fold, 1000);
            return;
          }
          if (readUntil === -1) {
            const msgs = chat.get().messages;
            const len = msgs.length ? msgs[msgs.length - 1].text.length : 0;
            readUntil = Date.now() + Math.min(40000, 8000 + len * 40);
          }
          if (readUntil > Date.now()) {
            unarmedTimer.current = window.setTimeout(fold, readUntil - Date.now());
            return;
          }
          if (!n.armed && n.panelOpen && !n.pinned && !n.busy) closePanel();
        }, 9000);
      }
    },
    [closePanel],
  );

  const bumpUsage = useCallback((target: string) => {
    setUsage((u) => ({ ...u, [target]: (u[target] ?? 0) + 1 }));
  }, []);

  const appsLoadedAt = useRef(0);
  const loadApps = useCallback((refresh = false) => {
    if (!refresh && Date.now() - appsLoadedAt.current < 5 * 60e3 && appsLoadedAt.current) return;
    appsLoadedAt.current = Date.now();
    call<AppItem[]>("list_apps", { refresh }).then(setApps).catch(() => {});
  }, []);

  const openLauncher = useCallback(() => {
    setPanelOpen(false);
    setLauncherOpen(true);
    loadApps(false);
  }, [loadApps]);

  const closeLauncher = useCallback((restore: boolean) => {
    setLauncherOpen(false);
    call("launcher_closed", { restore }).catch(() => {});
  }, []);

  // "Спросить ИИ" from the launcher: open the chat tab and send right away
  const askAi = useCallback(
    (text: string) => {
      setLauncherOpen(false);
      call("launcher_closed", { restore: false }).catch(() => {});
      setAsk({ text, n: Date.now() });
      // not armed: the cursor is usually somewhere else (the launcher input had
      // focus), and an armed panel would fold on the first mouse move
      openPanel("chat", false);
    },
    [openPanel],
  );

  // ---- native events -------------------------------------------------------
  useEffect(() => {
    let enterT = 0;
    let leaveT = 0;
    const subs: Promise<() => void>[] = [];

    subs.push(
      on<Cursor>("cursor", (c) => {
        const s = live.current;
        s.lmb = c.lmb;
        liquidRef.current?.setCursor(c.x, c.y, c.inside);
        if (c.inside) {
          s.armed = true;
          window.clearTimeout(leaveT);
          leaveT = 0;
          if (!s.panelOpen && !s.launcherOpen && !s.drop && !s.toastActions && s.settings.hoverExpand && !c.lmb && !enterT) {
            enterT = window.setTimeout(() => {
              enterT = 0;
              const n = live.current;
              if (!n.panelOpen && !n.launcherOpen && !n.drop && !n.toastActions) openPanel(undefined, true);
            }, 200);
          }
        } else {
          window.clearTimeout(enterT);
          enterT = 0;
          if (s.panelOpen && !s.pinned && s.armed && !leaveT) {
            leaveT = window.setTimeout(function check() {
              leaveT = 0;
              const n = live.current;
              if (!n.panelOpen || n.pinned) return;
              if (holdOpen(n)) {
                leaveT = window.setTimeout(check, 150);
                return;
              }
              closePanel();
            }, LEAVE_MS);
          }
        }
      }),
    );
    subs.push(
      on<Media | null>("media", (m) => {
        const prev = live.current.media;
        setMedia(m);
        setMediaAt(Date.now());
        if (m && m.playing && (!prev || prev.title !== m.title || prev.artist !== m.artist) && !live.current.panelOpen) triggerPeek();
      }),
    );
    subs.push(on<Cover>("cover", setCover));
    subs.push(on<number>("audio", (l) => liquidRef.current?.setAudio(l)));
    subs.push(on<Volume>("volume", setVolume));
    subs.push(on<Clip[]>("clips", setClips));
    subs.push(on<boolean>("suppressed", setSuppressed));
    subs.push(on<ToastData>("toast", (t) => showToast(t, t.ms ?? 3000)));
    subs.push(on<WeatherPayload>("weather", setWeather));
    subs.push(on<Settings>("settings", setSettings));
    subs.push(on<VoiceState>("voice", setVoice));
    subs.push(on<RecordingState>("recording", setRec));
    subs.push(on<DownloadProgress | null>("download", setDownload));
    subs.push(on<ShelfItem[]>("shelf", setShelf));
    subs.push(
      on<boolean>("launcher", (open) => {
        if (open) openLauncher();
        else closeLauncher(true);
      }),
    );
    subs.push(
      on<string>("open-panel", (t) => {
        setLauncherOpen(false);
        openPanel((t as Tab) || "home", false);
      }),
    );
    subs.push(
      onFocus((focused) => {
        if (!focused && live.current.launcherOpen) closeLauncher(false);
      }),
    );
    subs.push(
      onDrop((e) => {
        const cx = liquidRef.current?.centerX ?? window.innerWidth / 2;
        // the chat tab takes files as attachments
        const n = live.current;
        if (n.panelOpen && n.tab === "chat" && chatDrop.current) {
          if (e.type === "drop") chatDrop.current(e.paths);
          return;
        }
        if (e.type === "enter") setDrop({ paths: e.paths, zone: null });
        else if (e.type === "over") {
          const zone: DropZone | null = e.y > BOX.drop.h + 30 ? null : e.x < cx ? "shelf" : "launcher";
          setDrop((d) => (d && d.zone !== zone ? { ...d, zone } : d));
        } else if (e.type === "leave") setDrop(null);
        else if (e.type === "drop") {
          const zone: DropZone = e.x >= cx && e.y <= BOX.drop.h + 30 ? "launcher" : "shelf";
          setDrop(null);
          handleDrop(e.paths, zone);
        }
      }),
    );

    // app list in the background so the first Alt+Space is instant
    const t = window.setTimeout(() => loadApps(true), 2500);
    return () => {
      window.clearTimeout(t);
      subs.forEach((p) => p.then((un) => un()));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleDrop = async (paths: string[], zone: DropZone) => {
    if (!paths.length) return;
    const name = paths.length === 1 ? paths[0].split(/[\\/]/).pop()! : `${paths.length} файла(ов)`;
    if (zone === "launcher") {
      const next = await call<PinItem[]>("pins_add", { targets: paths });
      setPins(next);
      showToast({ icon: "launcher", title: "Добавлено в лаунчер", subtitle: name });
    } else {
      const next = await call<ShelfItem[]>("shelf_add", { paths });
      setShelf(next);
      showToast({ icon: "shelf", title: "Положено на полку", subtitle: name });
    }
  };

  useEffect(() => {
    if (boot.hotkeyError)
      showToast({ icon: "error", tone: "error", title: "Сочетание клавиш занято", subtitle: boot.hotkeyError }, 6000);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Esc closes the panel
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && live.current.panelOpen && !live.current.launcherOpen) {
        setPinned(false);
        closePanel();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closePanel]);

  // Changes are applied at once and merged into the latest settings, so two quick
  // edits (a field saved on blur, then a toggle) never overwrite each other.
  const settingsRef = useRef(settings);
  const saveSeq = useRef(0);
  const saveSettings = useCallback(
    async (patch: Partial<Settings>) => {
      const prev = settingsRef.current;
      const next = { ...prev, ...patch };
      settingsRef.current = next;
      setSettings(next);
      const seq = ++saveSeq.current;
      try {
        const saved = await call<Settings>("save_settings", { settings: next });
        if (seq === saveSeq.current) {
          settingsRef.current = saved;
          setSettings(saved);
        }
      } catch (e) {
        if (seq === saveSeq.current) {
          settingsRef.current = prev;
          setSettings(prev);
        }
        showToast({ icon: "error", tone: "error", title: "Не сохранилось", subtitle: String(e) }, 3500);
      }
    },
    [showToast],
  );

  const setBusy = useCallback((b: boolean) => {
    live.current.busy = b;
  }, []);

  // a tab that set "busy" and then went away (unmounted while a field was
  // focused, menu left open) must not keep the panel from folding
  useEffect(() => {
    live.current.busy = false;
  }, [tab, panelOpen]);

  const content = useMemo(() => {
    switch (mode) {
      case "voice":
        return <VoicePill voice={voice} />;
      case "rec":
        return <RecPill rec={rec} />;
      case "download":
        return download ? <DownloadPill d={download} /> : null;
      case "idle":
        return weather && settings.weatherEnabled ? <WeatherCompact weather={weather} /> : null;
      case "music":
        return <Compact cover={cover.url} media={media} mediaAt={mediaAt} lyrics={lyrics} />;
      case "peek":
        return media ? <Peek media={media} cover={cover.url} /> : null;
      case "toast":
        return toast ? <Toast data={toast} onDone={hideToast} /> : null;
      case "drop":
        return <DropZones zone={drop?.zone ?? null} count={drop?.paths.length ?? 0} />;
      case "panel":
        return (
          <Panel
            tab={tab}
            setTab={setTab}
            pinned={pinned}
            setPinned={setPinned}
            settings={settings}
            saveSettings={saveSettings}
            media={media}
            mediaAt={mediaAt}
            cover={cover.url}
            lyrics={lyrics}
            accent={accent}
            volume={volume}
            clips={clips}
            shelf={shelf}
            setShelf={setShelf}
            pins={pins}
            setPins={setPins}
            version={boot.version}
            onClose={closePanel}
            onLauncher={() => call("open_launcher")}
            toast={showToast}
            setBusy={setBusy}
            ask={ask}
            onAsked={clearAsk}
            setDropHandler={setChatDrop}
            weather={weather}
          />
        );
      case "launcher":
        return (
          <Launcher
            apps={apps}
            pins={pins}
            shelf={shelf}
            usage={usage}
            hotkey={settings.hotkey}
            onHeight={setLauncherH}
            onClose={closeLauncher}
            toast={showToast}
            onLaunch={bumpUsage}
            ai={settings.aiEnabled ? settings.aiModel || "ИИ" : null}
            onAsk={askAi}
          />
        );
      default:
        return null;
    }
  }, [mode, cover.url, media, mediaAt, lyrics, toast, drop, tab, pinned, settings, accent, volume, clips, shelf, pins, apps, usage, boot.version, saveSettings, closePanel, closeLauncher, showToast, hideToast, setBusy, bumpUsage, ask, clearAsk, setChatDrop, askAi, weather, voice, download, rec]);

  const onIslandClick = () => {
    if ((mode === "toast" && toast?.actions?.length) || mode === "voice") return;
    if (mode === "rec") return void call("record_stop");
    if (mode === "download") return openPanel("shelf", true);
    if (mode === "idle" || mode === "music" || mode === "peek" || mode === "toast") openPanel("home", true);
  };

  return (
    <div className={`root ${suppressed ? "is-hidden" : ""}`}>
      <canvas ref={canvasRef} className="liquid" />
      {settings.residentsEnabled && (
        <Residents
          ids={settings.residents}
          boxRef={islandBox}
          visible={!["panel", "launcher", "drop", "hidden"].includes(mode)}
          playing={!!media?.playing}
        />
      )}
      <div ref={clipRef} className={`island mode-${mode}`} onClick={onIslandClick}>
        <div ref={innerRef} className="island-inner" style={{ width: box.w, height: box.h }} key={mode}>
          {content}
        </div>
      </div>
    </div>
  );
}
