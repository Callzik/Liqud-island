import type { Media } from "../api";
import type { LyricLine } from "./lyrics";

/** how far ahead of the reported position a line lights up (ms) */
const LEAD = 90;
/** jumps bigger than this are seeks — follow them instantly (ms) */
const SNAP = 750;
/** how fast small drift between our clock and the player is absorbed (ms) */
const SETTLE = 450;

export interface PlaybackState {
  pos: number;
  playing: boolean;
}

/** Current playback position in ms (players report it together with the time it was sampled). */
export function playbackPos(m: Media | null, mediaAt: number): number | null {
  return playbackState(m, mediaAt)?.pos ?? null;
}

export function playbackState(m: Media | null, mediaAt: number): PlaybackState | null {
  if (!m) return null;
  const now = Date.now();
  // the player's own sample time when it is sane; a time in the future or a
  // stale stamp for a paused-then-resumed track falls back to when we got it
  const sane = m.updatedMs > 0 && m.updatedMs <= now + 1000 && now - m.updatedMs < 6 * 3600e3;
  const base = sane ? Math.min(m.updatedMs, now) : mediaAt;
  return { pos: m.positionMs + (m.playing ? Math.max(0, now - base) : 0), playing: m.playing };
}

/**
 * Drives the word highlight: finds the active line, reports it, and every frame
 * writes --p (0..1) on the words of the active line element.
 *
 * Players report their position about once a second and slightly off from our
 * clock, so the position used here runs on its own smooth clock and only eases
 * towards the reported one — otherwise the sweep visibly stutters.
 *
 * `onSweep(el, x)` gets the horizontal position (px, inside the line element)
 * where the highlight currently is — used to scroll long lines.
 */
export function runLyricSync(
  lines: LyricLine[],
  getState: () => PlaybackState | null,
  onIdx: (i: number) => void,
  lineEl: (i: number) => HTMLElement | null,
  onSweep?: (el: HTMLElement, x: number) => void,
): () => void {
  let raf = 0;
  let cur = -2;
  let words: HTMLElement[] = [];
  let wordsOf: HTMLElement | null = null;
  let lastP: number[] = [];
  let shown: number | null = null;
  let last = performance.now();
  let prevTick = last;
  let refreshMs = 1000 / 60;

  // a line we leave stays fully lit (as when it was sung through)
  const release = () => {
    for (const w of words) {
      w.style.setProperty("--p", "1");
      w.classList.remove("cur");
    }
    words = [];
    wordsOf = null;
    lastP = [];
  };

  const tick = (now: number) => {
    raf = requestAnimationFrame(tick);
    // ~60–80 fps is plenty for the sweep: on 120 Hz+ monitors skip every other
    // refresh (60–100 Hz displays update every frame)
    const d = now - prevTick;
    prevTick = now;
    if (d > 0 && d < 50) refreshMs += (d - refreshMs) * 0.1;
    if (refreshMs < 9 && now - last < 12) return;
    const dt = Math.min(100, Math.max(0, now - last));
    last = now;

    const st = getState();
    if (!st) {
      shown = null;
      return;
    }
    if (shown === null || !st.playing || Math.abs(st.pos - shown) > SNAP) shown = st.pos;
    else {
      shown += dt;
      shown += (st.pos - shown) * Math.min(1, dt / SETTLE);
    }
    const pos = shown + LEAD;

    let lo = 0;
    let hi = lines.length - 1;
    let i = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (lines[mid].t <= pos) {
        i = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    if (i !== cur) {
      release();
      cur = i;
      onIdx(i);
    }
    if (i < 0) return;

    const el = lineEl(i);
    if (!el) return;
    if (wordsOf !== el) {
      words = Array.from(el.querySelectorAll<HTMLElement>(".w"));
      wordsOf = el;
      lastP = [];
    }
    const line = lines[i];
    const rel = pos - line.t;

    // progress of every word first (no DOM), then one geometry read, then writes
    const ps: number[] = [];
    let active = -1;
    for (let k = 0; k < words.length; k++) {
      const wd = line.words[k];
      if (!wd) break;
      const p = Math.max(0, Math.min(1, (rel - wd.from) / Math.max(1, wd.to - wd.from)));
      ps.push(Math.round(p * 400) / 400);
      if (active < 0 && p < 1) active = k;
    }
    if (onSweep && words.length) {
      const k = active < 0 ? words.length - 1 : active;
      const w = words[k];
      onSweep(el, w.offsetLeft + w.offsetWidth * (active < 0 ? 1 : ps[k]));
    }
    for (let k = 0; k < ps.length; k++) {
      const q = ps[k];
      if (lastP[k] === q) continue;
      lastP[k] = q;
      words[k].style.setProperty("--p", String(q));
      words[k].classList.toggle("cur", q > 0 && q < 1);
    }
  };
  raf = requestAnimationFrame(tick);
  return () => {
    cancelAnimationFrame(raf);
    release();
  };
}
