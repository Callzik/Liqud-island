import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { Media } from "../api";
import { useLyricSync, type LyricsState } from "../lib/useLyrics";
import { IMusic } from "./Icons";

function Cover({ url, size, radius }: { url: string | null; size: number; radius: number }) {
  // a cover the player sent broken shows the placeholder, not a broken image
  const [bad, setBad] = useState<string | null>(null);
  return url && url !== bad ? (
    <img className="cover" src={url} alt="" style={{ width: size, height: size, borderRadius: radius }} draggable={false} onError={() => setBad(url)} />
  ) : (
    <div className="cover cover-empty" style={{ width: size, height: size, borderRadius: radius }}>
      <IMusic size={size * 0.55} />
    </div>
  );
}

/** Words of a lyric line. Each word carries its text in data-t: the bright copy is
 *  drawn by ::after on top of the dim word and revealed with a mask, so the glyphs
 *  stay ordinary crisp text (no background-clip, no scaling). */
export function LyricWords({ words }: { words: { text: string }[] }) {
  return (
    <>
      {words.map((w, k) => (
        <span key={k}>
          <span className="w" data-t={w.text}>
            {w.text}
          </span>{" "}
        </span>
      ))}
    </>
  );
}

const FONT = 14;
const FONT_MIN = 12.5;
/** width of the soft edge where a long line runs out of the island (px) */
const FADE = 18;

/** Music playing, island collapsed: artwork on the left, the current lyric line in the middle, bars are drawn on the canvas. */
export function Compact({ cover, media, mediaAt, lyrics }: { cover: string | null; media: Media | null; mediaAt: number; lyrics: LyricsState }) {
  const lines = lyrics.lines;
  const wrapRef = useRef<HTMLDivElement>(null);
  const els = useRef(new Map<number, HTMLDivElement>());
  const scroll = useRef<{ el: HTMLElement | null; x: number; fl: number; fr: number }>({ el: null, x: 0, fl: 0, fr: 0 });

  // long lines: follow the highlight smoothly instead of shrinking the text
  const onSweep = (el: HTMLElement, sweep: number) => {
    const wrap = wrapRef.current;
    if (!wrap) return;
    const s = scroll.current;
    if (s.el !== el) {
      s.el = el;
      s.x = 0;
    }
    const over = el.scrollWidth - wrap.clientWidth;
    const target = over > 0 ? Math.max(0, Math.min(over + 10, sweep - wrap.clientWidth * 0.62)) : 0;
    s.x += (target - s.x) * 0.08;
    if (Math.abs(target - s.x) < 0.1) s.x = target;
    el.style.transform = s.x > 0.05 ? `translateX(${-s.x}px)` : "";
    // fade an edge only where text actually runs past it, so the first and
    // last letters of a line that fits are never dimmed
    const fl = Math.round(Math.min(FADE, s.x) * 2) / 2;
    const fr = Math.round(Math.min(FADE, Math.max(0, over - s.x)) * 2) / 2;
    if (s.fl !== fl) wrap.style.setProperty("--fl", `${(s.fl = fl)}px`);
    if (s.fr !== fr) wrap.style.setProperty("--fr", `${(s.fr = fr)}px`);
  };

  const idx = useLyricSync(lines, media, mediaAt, (i) => els.current.get(i) ?? null, onSweep);

  // the current line plus the one fading out
  const [shown, setShown] = useState<{ i: number; out: boolean }[]>([]);
  useEffect(() => setShown([]), [lines]);
  useEffect(() => {
    setShown((prev) => {
      const leaving = prev.filter((s) => s.i !== idx).map((s) => ({ ...s, out: true }));
      const next = leaving.slice(-1);
      if (idx >= 0) next.push({ i: idx, out: false });
      return next;
    });
  }, [idx]);

  // fit: drop the font size a little for long lines (re-rendered, never scaled)
  useLayoutEffect(() => {
    const wrap = wrapRef.current;
    const el = els.current.get(idx);
    if (!wrap || !el || el.dataset.fit) return;
    el.dataset.fit = "1";
    el.style.fontSize = `${FONT}px`;
    if (el.scrollWidth > wrap.clientWidth) {
      const f = Math.max(FONT_MIN, Math.floor((FONT * wrap.clientWidth) / el.scrollWidth * 2) / 2);
      el.style.fontSize = `${f}px`;
    }
  }, [idx, shown]);

  return (
    <div className="compact">
      <Cover url={cover} size={24} radius={7} />
      {lines && (
        <div className="c-lyric-wrap" ref={wrapRef}>
          {shown.map(({ i, out }) => {
            const line = lines[i];
            if (!line) return null;
            return (
              <div className={`c-lyric-slot ${out ? "out" : "in"}`} key={i} onAnimationEnd={() => out && setShown((p) => p.filter((s) => !(s.out && s.i === i)))}>
                <div
                  className="c-lyric"
                  ref={(el) => {
                    if (el) els.current.set(i, el);
                    else els.current.delete(i);
                  }}
                >
                  <LyricWords words={line.words} />
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** Track changed: show title and artist for a moment. */
export function Peek({ media, cover }: { media: Media; cover: string | null }) {
  return (
    <div className="peek">
      <Cover url={cover} size={44} radius={11} />
      <div className="peek-text">
        <div className="peek-title">{media.title}</div>
        <div className="peek-artist">{media.artist || media.app}</div>
      </div>
    </div>
  );
}

export { Cover };
