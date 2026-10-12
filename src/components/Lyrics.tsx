import { useLayoutEffect, useRef, useSyncExternalStore, type CSSProperties } from "react";
import { getOffset, offsetKey, setOffset, subscribeOffsets } from "../lib/lyricOffset";
import { call } from "../api";
import { css } from "../lib/color";
import { useLyricSync } from "../lib/useLyrics";
import type { PanelProps } from "./Panel";
import { LyricWords } from "./Compact";

/** «Раньше / позже»: nudges the whole text of this track, remembered per track. */
function OffsetControl({ track }: { track: string }) {
  const off = useSyncExternalStore(subscribeOffsets, () => getOffset(track));
  const fmt = (ms: number) => `${ms > 0 ? "+" : ms < 0 ? "−" : ""}${(Math.abs(ms) / 1000).toFixed(1).replace(".", ",")} с`;
  return (
    <div className="ly-offset" onClick={(e) => e.stopPropagation()}>
      <button onClick={() => setOffset(track, off - 200)} title="Подсветка раньше времени — сдвинуть позже">
        Позже
      </button>
      <button className={`ly-offset-v ${off ? "on" : ""}`} onClick={() => setOffset(track, 0)} title={off ? "Сбросить сдвиг" : "Сдвиг текста для этого трека"}>
        {off ? fmt(off) : "0 с"}
      </button>
      <button onClick={() => setOffset(track, off + 200)} title="Подсветка опаздывает — сдвинуть раньше">
        Раньше
      </button>
    </div>
  );
}

export function Lyrics({ media, mediaAt, accent, lyrics }: Pick<PanelProps, "media" | "mediaAt" | "accent" | "lyrics">) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lineEls = useRef<(HTMLDivElement | null)[]>([]);
  const placed = useRef(false);

  const lines = lyrics.lines;
  const idx = useLyricSync(lines, media, mediaAt, (i) => lineEls.current[i] ?? null);
  useLayoutEffect(() => {
    placed.current = false;
  }, [lines]);

  // ---- keep the active line centred ----------------------------------------
  useLayoutEffect(() => {
    const wrap = wrapRef.current;
    const sc = scrollRef.current;
    if (!wrap || !sc || !lines) return;
    const el = lineEls.current[Math.max(0, idx)];
    if (!el) return;
    const y = Math.round(wrap.clientHeight * 0.4 - (el.offsetTop + el.offsetHeight / 2));
    if (!placed.current) {
      sc.style.transition = "none";
      sc.style.transform = `translateY(${y}px)`;
      sc.getBoundingClientRect();
      sc.style.transition = "";
      placed.current = true;
    } else {
      sc.style.transform = `translateY(${y}px)`;
    }
  }, [idx, lines]);

  // ---- render ------------------------------------------------------------------
  const data = lyrics.data;
  if (lyrics.status === "idle" || !media) {
    return (
      <div className="ly-state" key="idle">
        <div className="ly-state-title">Здесь будет текст песни</div>
        <div className="hint">Включите музыку — слова появятся и будут подсвечиваться в такт</div>
      </div>
    );
  }
  if (lyrics.status === "loading") {
    return (
      <div className="ly-state" key="loading">
        <div className="ly-dots">
          <i />
          <i />
          <i />
        </div>
        <div className="hint">Ищу текст…</div>
      </div>
    );
  }
  if (lyrics.status === "none" || !data) {
    return (
      <div className="ly-state" key="none">
        <div className="ly-state-title">Текст не найден</div>
        <div className="hint">Для этого трека в базе LRCLIB ничего нет</div>
      </div>
    );
  }
  if (data.instrumental && !lines && !data.plain) {
    return (
      <div className="ly-state" key="inst">
        <div className="ly-state-title">Инструментал</div>
        <div className="hint">В этом треке нет слов</div>
      </div>
    );
  }

  const glow = css(accent);

  if (!lines) {
    return (
      <div className="ly-wrap" style={{ "--ly-glow": glow } as CSSProperties}>
        <div className="ly-plain">{data.plain}</div>
        <div className="ly-credit">Без синхронизации · LRCLIB</div>
      </div>
    );
  }

  return (
    <div className="ly-wrap" ref={wrapRef} style={{ "--ly-glow": glow } as CSSProperties}>
      <div className="ly-scroll" ref={scrollRef}>
        {lines.map((l, i) => {
          const d = Math.abs(i - idx);
          return (
            <div
              key={i}
              ref={(el) => {
                lineEls.current[i] = el;
              }}
              className={`ly-line ${i === idx ? "act" : ""}`}
              data-far={d > 5 ? "" : undefined}
              style={{ "--d": d } as CSSProperties}
              onClick={() => call("media_control", { action: "seek", value: l.t }).catch(() => {})}
            >
              <LyricWords words={l.words} />
            </div>
          );
        })}
      </div>
      {media && <OffsetControl track={offsetKey(media.artist, media.title)} />}
      <div className="ly-credit">LRCLIB</div>
    </div>
  );
}
