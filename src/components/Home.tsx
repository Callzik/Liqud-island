import { useCallback, useEffect, useRef, useState, type MouseEvent } from "react";
import { call, type Clip, type PinItem } from "../api";
import { css } from "../lib/color";
import { ago, clock } from "../lib/format";
import { useIcon } from "../lib/icons";
import { Cover } from "./Compact";
import { ICheck, IChevron, IHeart, IHeartOff, ICommand, IEraser, IFullscreen, IMic, IMute, IPipette, IQr, IRecord, IScanText, INext, IPause, IPlay, IPlus, IPrev, IRegion, ISpeaker, IVolume, IClose, IMusic, ISwap } from "./Icons";
import type { PanelProps } from "./Panel";

function useTicker(active: boolean, ms = 250) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!active) return;
    const id = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(id);
  }, [active, ms]);
  return now;
}

interface YmState {
  available: boolean;
  trackId: string;
  liked: boolean;
  disliked: boolean;
  error?: string;
}

const isYandex = (m: { app: string; appId: string }) => /yandex/i.test(m.appId) || /Яндекс Музыка/.test(m.app);

/** ♥ / «не рекомендовать» for the current Yandex Music track. */
function useYm(media: PanelProps["media"], token: string) {
  const [st, setSt] = useState<YmState | null>(null);
  const key = media && isYandex(media) ? `${media.artist}\u0001${media.title}` : "";
  const load = useCallback(() => {
    if (!media || !key || !token) return setSt(null);
    call<YmState>("ym_state", { artist: media.artist, title: media.title, durationMs: media.durationMs })
      .then(setSt)
      .catch(() => setSt(null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, token]);
  useEffect(load, [load]);
  return [st, setSt, !!key] as const;
}

function MediaCard({
  media,
  mediaAt,
  cover,
  accent,
  settings,
  saveSettings,
  toast,
  setTab,
}: Pick<PanelProps, "media" | "mediaAt" | "cover" | "accent" | "settings" | "saveSettings" | "toast" | "setTab">) {
  const [ym, setYm, ymPlayer] = useYm(media, settings.ymToken);
  const rate = async (kind: "like" | "dislike") => {
    if (!settings.ymToken) {
      toast({ icon: "error", tone: "error", title: "Войдите в Яндекс Музыку", subtitle: "Настройки → Яндекс Музыка" }, 3500);
      setTab("settings");
      return;
    }
    if (!ym?.available) {
      toast({ icon: "error", tone: "error", title: ym?.error ? "Не получилось" : "Трек не найден в Яндекс Музыке", subtitle: ym?.error }, 3500);
      return;
    }
    const on = kind === "like" ? !ym.liked : !ym.disliked;
    // optimistic: a like clears the dislike and vice versa
    setYm({ ...ym, liked: kind === "like" ? on : false, disliked: kind === "dislike" ? on : false });
    try {
      await call("ym_set", { trackId: ym.trackId, kind, on });
      if (on) toast({ icon: "check", title: kind === "like" ? "Добавлено в «Мне нравится»" : "Не рекомендовать", subtitle: media?.title }, 2000);
    } catch (e) {
      setYm(ym);
      toast({ icon: "error", tone: "error", title: "Не получилось", subtitle: String(e) }, 3500);
    }
  };
  const now = useTicker(!!media?.playing || !media, media ? 250 : 1000);
  const [sources, setSources] = useState(false);
  if (media && sources) return <SourceList current={media.appId} onClose={() => setSources(false)} />;
  if (!media) {
    const d = new Date(now);
    return (
      <div className="card media-card empty">
        <div className="big-clock">{d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" })}</div>
        <div className="big-date">{d.toLocaleDateString("ru-RU", { weekday: "long", day: "numeric", month: "long" })}</div>
        <div className="hint">Включите музыку — она появится здесь</div>
      </div>
    );
  }
  // players report the position with the time it was sampled
  const base = media.updatedMs > 0 && Math.abs(now - media.updatedMs) < 6 * 3600e3 ? media.updatedMs : mediaAt;
  const dur = media.durationMs;
  const pos = Math.min(dur || Infinity, media.positionMs + (media.playing ? Math.max(0, now - base) : 0));
  const pct = dur > 0 ? Math.max(0, Math.min(1, pos / dur)) : 0;

  const seek = (e: MouseEvent<HTMLDivElement>) => {
    if (!dur) return;
    const r = e.currentTarget.getBoundingClientRect();
    const f = Math.max(0, Math.min(1, (e.clientX - r.left) / r.width));
    call("media_control", { action: "seek", value: f * dur });
  };

  return (
    <div className="card media-card">
      <button className="src-btn" onClick={() => setSources(true)} title="Выбрать плеер">
        <ISwap size={13} />
      </button>
      <Cover url={cover} size={128} radius={18} />
      <div className="media-info">
        <div className="media-app">
          {media.app}
          {(ymPlayer || settings.ytAuto) && (
            <button
              className={`uncensored ${settings.ytAuto ? "on" : ""}`}
              title={
                settings.ytKey
                  ? settings.ytAuto
                    ? "Без цензуры включено: треки из Яндекс Музыки играют с YouTube Music. Нажмите, чтобы выключить"
                    : "Играть треки Яндекс Музыки с YouTube Music — оригинальные версии, без вырезанных слов"
                  : "Открыть этот трек на YouTube Music (для автоматического режима добавьте ключ YouTube в настройках)"
              }
              onClick={async () => {
                if (settings.ytKey) {
                  saveSettings({ ...settings, ytAuto: !settings.ytAuto });
                  toast(
                    settings.ytAuto
                      ? { icon: "check", title: "Без цензуры выключено", subtitle: "Дальше играет Яндекс Музыка" }
                      : { icon: "check", title: "Без цензуры включено", subtitle: "Яндекс выбирает треки, звук — с YouTube Music" },
                    3000,
                  );
                  return;
                }
                if (!media) return;
                try {
                  await call<boolean>("uncensored", { artist: media.artist, title: media.title, durationMs: media.durationMs });
                  toast({ icon: "check", title: "Открыл поиск на YouTube Music", subtitle: "Для автоматического режима добавьте ключ YouTube в настройках" }, 3500);
                } catch (e) {
                  toast({ icon: "error", tone: "error", title: "Не получилось", subtitle: String(e) }, 3500);
                }
              }}
            >
              E · без цензуры{settings.ytAuto ? " · вкл" : ""}
            </button>
          )}
        </div>
        <div className="media-title" title={media.title}>
          {media.title}
        </div>
        <div className="media-artist" title={media.artist}>
          {media.artist}
        </div>
        <div className="progress" onClick={seek} style={{ cursor: dur ? "pointer" : "default" }}>
          <div className="progress-fill" style={{ width: `${pct * 100}%`, background: css(accent) }} />
        </div>
        <div className="media-times">
          <span>{dur ? clock(pos) : ""}</span>
          <span>{dur ? `-${clock(dur - pos)}` : ""}</span>
        </div>
        <div className="media-controls">
          {ymPlayer && (
            <button className={`ctl ctl-rate ${ym?.liked ? "on" : ""}`} onClick={() => rate("like")} title={ym?.liked ? "Убрать из «Мне нравится»" : "Мне нравится"}>
              <IHeart size={19} filled={ym?.liked} />
            </button>
          )}
          <button className="ctl" disabled={!media.canPrev} onClick={() => call("media_control", { action: "prev" })} title="Назад">
            <IPrev size={20} />
          </button>
          <button className="ctl ctl-main" onClick={() => call("media_control", { action: "toggle" })} title={media.playing ? "Пауза" : "Играть"}>
            {media.playing ? <IPause size={20} /> : <IPlay size={20} />}
          </button>
          <button className="ctl" disabled={!media.canNext} onClick={() => call("media_control", { action: "next" })} title="Вперёд">
            <INext size={20} />
          </button>
          {ymPlayer && (
            <button className={`ctl ctl-rate ${ym?.disliked ? "on" : ""}`} onClick={() => rate("dislike")} title={ym?.disliked ? "Снова рекомендовать" : "Не рекомендовать (и следующий трек)"}>
              <IHeartOff size={19} filled={ym?.disliked} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

interface OutputDevice {
  id: string;
  name: string;
  default: boolean;
}

/** "Динамики ⌄": switch the default output device. */
// The list opens upwards inside the panel, so it doesn't need to hold the panel
// open ("busy"): leaving the panel simply folds it, list included.
function DeviceMenu({ volume }: Pick<PanelProps, "volume">) {
  const [open, setOpen] = useState(false);
  const [list, setList] = useState<OutputDevice[] | null>(null);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", away, true);
    return () => window.removeEventListener("pointerdown", away, true);
  }, [open]);
  const toggle = async () => {
    if (open) {
      setOpen(false);
      return;
    }
    setOpen(true);
    setList(await call<OutputDevice[]>("audio_devices").catch(() => []));
  };
  const pick = async (d: OutputDevice) => {
    setOpen(false);
    if (!d.default) await call("audio_set_device", { id: d.id });
  };
  return (
    <div className="device-menu" ref={root}>
      <button className={`device-chip ${open ? "open" : ""}`} title={volume.device} onClick={toggle}>
        <ISpeaker size={15} />
        <span className="device-name">{volume.device}</span>
        <IChevron size={13} />
      </button>
      {open && (
        <div className="device-pop">
          {list === null && <div className="device-empty">Загружаю…</div>}
          {list?.length === 0 && <div className="device-empty">Устройств не найдено</div>}
          {list?.map((d) => (
            <button key={d.id} className={`device-item ${d.default ? "on" : ""}`} onClick={() => pick(d)} title={d.name}>
              <span>{d.name}</span>
              {d.default && <ICheck size={14} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function VolumeRow({ volume, media }: Pick<PanelProps, "volume" | "media">) {
  const [local, setLocal] = useState<number | null>(null);
  // "app": the slider moves only the player's own volume
  const [scope, setScope] = useState<"all" | "app">("all");
  const [appLevel, setAppLevel] = useState<number | null>(null);
  const last = useRef(0);
  const appId = media?.appId ?? "";
  useEffect(() => {
    if (scope !== "app" || !appId) return;
    let alive = true;
    const load = () =>
      call<number | null>("app_volume_get", { appId }).then((v) => {
        if (!alive) return;
        if (v === null) setScope("all");
        else setAppLevel(Math.round(v * 100));
      });
    load();
    const t = window.setInterval(load, 2500);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, [scope, appId]);
  const appMode = scope === "app" && !!appId && appLevel !== null;
  const level = local ?? (appMode ? appLevel! : Math.round(volume.level * 100));
  const send = (v: number, force = false) => {
    const t = Date.now();
    if (force || t - last.current > 40) {
      last.current = t;
      if (appMode) {
        setAppLevel(v);
        call("app_volume_set", { appId, level: v / 100 });
      } else call("volume_set", { level: v / 100 });
    }
  };
  const muted = !appMode && (volume.muted || level === 0);
  return (
    <div className="card volume-row">
      {appId && (
        <div className="vol-scope">
          <button className={!appMode ? "on" : ""} onClick={() => setScope("all")} title="Громкость всего компьютера">
            <ISpeaker size={13} />
          </button>
          <button className={appMode ? "on" : ""} onClick={() => setScope("app")} title={`Только ${media?.app ?? "плеер"} — остальной звук не меняется`}>
            <IMusic size={13} />
          </button>
        </div>
      )}
      <button className="icon-btn" onClick={() => call("volume_mute", { muted: !volume.muted })} title={volume.muted ? "Включить звук" : "Выключить звук"}>
        {muted ? <IMute size={18} /> : <IVolume size={18} />}
      </button>
      <input
        className="slider"
        type="range"
        min={0}
        max={100}
        value={level}
        style={{ ["--p" as string]: `${level}%` }}
        // no "busy" here: while the button is held the panel stays open anyway
        // (Rust reports the mouse button), and a release outside the window
        // might never reach us and leave the panel stuck open
        onPointerUp={() => {
          if (local !== null) send(local, true);
          window.setTimeout(() => setLocal(null), 600);
        }}
        onChange={(e) => {
          const v = Number(e.target.value);
          setLocal(v);
          send(v);
        }}
      />
      <span className="volume-value">{level}</span>
      {volume.device && <DeviceMenu volume={volume} />}
    </div>
  );
}

interface Source {
  appId: string;
  app: string;
  title: string;
  artist: string;
  playing: boolean;
}

/** «⇄»: every player that has a media session (AIMP, Winamp, browsers…); pick which one the island shows. */
function SourceList({ current, onClose }: { current: string; onClose: () => void }) {
  const [list, setList] = useState<Source[] | null>(null);
  useEffect(() => {
    call<Source[]>("media_sessions").then(setList).catch(() => setList([]));
  }, []);
  return (
    <div className="card media-card src-list">
      {list === null && <div className="hint">Ищу плееры…</div>}
      {list?.length === 0 && <div className="hint">Других плееров не видно</div>}
      {list?.map((s) => (
        <button
          key={s.appId}
          className={`src-item ${s.appId === current ? "on" : ""}`}
          onClick={() => {
            call("media_control", { action: `pick:${s.appId}` });
            onClose();
          }}
        >
          {s.appId === current ? <ICheck size={13} /> : <i className={s.playing ? "src-dot on" : "src-dot"} />}
          <span className="src-app">{s.app}</span>
          <span className="src-title">{s.title || "—"}</span>
          {s.playing && <IPlay size={11} />}
        </button>
      ))}
    </div>
  );
}

/** «Недавно скопировано»: the last three clipboard entries, click pastes. */
function RecentClips({ clips, onClose, setTab }: { clips: Clip[]; onClose: () => void; setTab: PanelProps["setTab"] }) {
  const list = clips.slice(0, 3);
  return (
    <div className="card recent-card">
      <button className="recent-head" onClick={() => setTab("clipboard")} title="Вся история">
        Недавно скопировано <IChevron size={12} />
      </button>
      {list.length === 0 && <div className="hint">Пока ничего не скопировано</div>}
      {list.map((c) => (
        <button
          key={c.id}
          className="recent-item"
          title="Вставить"
          onClick={() => {
            onClose();
            call("clip_paste", { id: c.id, paste: true });
          }}
        >
          <span className="recent-icon">
            {c.kind === "image" && c.thumb ? <img src={c.thumb} alt="" /> : c.kind === "image" ? <IRegion size={14} /> : <span>T</span>}
          </span>
          <span className="recent-text">{c.kind === "image" ? `Изображение ${c.width}×${c.height}` : c.kind === "files" ? c.files.map((f) => f.split(/[\\/]/).pop()).join(", ") : c.preview}</span>
          <span className="recent-ago">{ago(c.ts)}</span>
        </button>
      ))}
    </div>
  );
}

function Action({ icon: Icon, label, onClick }: { icon: typeof IRegion; label: string; onClick: () => void }) {
  return (
    <button className="action" onClick={onClick}>
      <Icon size={20} />
      <span>{label}</span>
    </button>
  );
}

function PinButton({ pin, onRemove }: { pin: PinItem; onRemove: () => void }) {
  const icon = useIcon(pin.target);
  return (
    <div className="dock-item" title={pin.name}>
      <button
        className="dock-btn"
        onClick={() => call("open_target", { target: pin.target })}
        onContextMenu={(e) => {
          e.preventDefault();
          onRemove();
        }}
      >
        {icon ? <img src={icon} alt="" draggable={false} /> : <span className="dock-letter">{pin.name.charAt(0)}</span>}
      </button>
      <button className="dock-remove" onClick={onRemove} title="Убрать">
        <IClose size={11} />
      </button>
    </div>
  );
}

export function Home(p: PanelProps) {
  const capture = (mode: "region" | "full" | "ocr" | "qr" | "picker" | "record") => {
    p.onClose();
    // let the panel fold before the screen is captured
    window.setTimeout(() => call("capture", { mode }), 280);
  };
  const addPins = async () => {
    p.setBusy(true);
    try {
      const files = await call<string[]>("pick_files", { title: "Добавить в лаунчер" });
      if (files.length) p.setPins(await call<PinItem[]>("pins_add", { targets: files }));
    } finally {
      p.setBusy(false);
    }
  };
  const removePin = async (pin: PinItem) => {
    p.setPins(await call<PinItem[]>("pins_remove", { id: pin.id }));
  };

  const hide = new Set(p.settings.homeHide ?? []);
  const showPlayer = !hide.has("player") && !!p.media;
  const showRecent = !hide.has("recent") && !showPlayer;
  const showClock = !showPlayer && !showRecent && !hide.has("player");
  const showVolume = !hide.has("volume");
  const left = showPlayer || showRecent || showClock || showVolume;
  const tiles: { id: string; icon: typeof IRegion; label: string; run: () => void }[] = [
    { id: "region", icon: IRegion, label: "Область", run: () => capture("region") },
    { id: "full", icon: IFullscreen, label: "Весь экран", run: () => capture("full") },
    { id: "ocr", icon: IScanText, label: "Текст с экрана", run: () => capture("ocr") },
    { id: "qr", icon: IQr, label: "QR-код", run: () => capture("qr") },
    { id: "picker", icon: IPipette, label: "Пипетка", run: () => capture("picker") },
    {
      id: "plain",
      icon: IEraser,
      label: "Чистый текст",
      run: async () => {
        p.onClose();
        const ok = await call<boolean>("paste_plain");
        if (!ok) p.toast({ icon: "error", tone: "error", title: "В буфере нет текста" });
      },
    },
    { id: "record", icon: IRecord, label: "Запись экрана", run: () => capture("record") },
    { id: "command", icon: ICommand, label: "Команда", run: p.onLauncher },
  ].filter((t) => !hide.has(t.id));
  const dock = !hide.has("dock");
  const cols = left ? 2 : 4;
  const rows = Math.max(1, Math.ceil(tiles.length / cols));

  return (
    <div className={`home ${left ? "" : "no-left"} ${tiles.length ? "" : "no-tiles"} ${dock ? "" : "no-dock"}`}>
      {left && (
        <div className="home-left">
          {(showPlayer || showClock) && (
            <MediaCard media={p.media} mediaAt={p.mediaAt} cover={p.cover} accent={p.accent} settings={p.settings} saveSettings={p.saveSettings} toast={p.toast} setTab={p.setTab} />
          )}
          {showRecent && <RecentClips clips={p.clips} onClose={p.onClose} setTab={p.setTab} />}
          {showVolume && <VolumeRow volume={p.volume} media={p.media} />}
        </div>
      )}
      {tiles.length > 0 && (
        <div className="actions" style={{ gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`, gridTemplateRows: `repeat(${rows}, minmax(0, 1fr))` }}>
          {tiles.map((t) => (
            <Action key={t.id} icon={t.icon} label={t.label} onClick={t.run} />
          ))}
        </div>
      )}
      {dock && (
        <div className="card dock">
          {p.pins.map((pin) => (
            <PinButton key={pin.id} pin={pin} onRemove={() => removePin(pin)} />
          ))}
          <button className="dock-add" onClick={addPins} title="Добавить программу или файл">
            <IPlus size={18} />
          </button>
          {p.settings.voiceEnabled && (
            <button
              className="dock-mic"
              title={`Голос → текст · ${p.settings.hotkeyVoice}`}
              onClick={() => {
                p.onClose();
                window.setTimeout(() => call("voice", { action: "start" }), 200);
              }}
            >
              <IMic size={18} />
              <span>Диктовка</span>
            </button>
          )}
          {p.pins.length === 0 && <span className="dock-hint">Перетащите ярлык на остров → «В лаунчер»</span>}
        </div>
      )}
    </div>
  );
}
