import { useEffect, useMemo, useRef, useState } from "react";
import type { Media } from "../api";
import { fetchLyrics, lyricsKey, type LyricLine, type LyricsData } from "./lyrics";
import { runLyricSync, playbackState } from "./lyricSync";
import { getOffset, offsetKey } from "./lyricOffset";

export interface LyricsState {
  status: "idle" | "loading" | "ok" | "none";
  data: LyricsData | null;
  /** synced lines, if the track has them */
  lines: LyricLine[] | null;
}

const IDLE: LyricsState = { status: "idle", data: null, lines: null };
const LOADING: LyricsState = { status: "loading", data: null, lines: null };

/** Loads lyrics for the current track as soon as it changes (no matter which tab is open). */
export function useLyrics(media: Media | null): LyricsState {
  const key = media && media.title ? lyricsKey({ artist: media.artist, title: media.title, album: media.album, durationMs: media.durationMs }) : "";
  const mref = useRef(media);
  mref.current = media;
  const [loaded, setLoaded] = useState<{ key: string; data: LyricsData | null } | null>(null);

  useEffect(() => {
    const m = mref.current;
    if (!key || !m) return;
    let dead = false;
    const ac = new AbortController();
    let timeout = 0;
    // players often publish metadata in two steps — let it settle
    const start = window.setTimeout(() => {
      timeout = window.setTimeout(() => ac.abort(), 7000);
      fetchLyrics({ artist: m.artist, title: m.title, album: m.album, durationMs: m.durationMs }, ac.signal)
        .then((data) => !dead && setLoaded({ key, data }))
        .catch(() => !dead && setLoaded({ key, data: null }))
        .finally(() => window.clearTimeout(timeout));
    }, 150);
    return () => {
      dead = true;
      ac.abort();
      window.clearTimeout(start);
      window.clearTimeout(timeout);
    };
  }, [key]);

  return useMemo(() => {
    if (!key) return IDLE;
    if (!loaded || loaded.key !== key) return LOADING;
    if (!loaded.data) return { status: "none", data: null, lines: null };
    return { status: "ok", data: loaded.data, lines: loaded.data.lines };
  }, [key, loaded]);
}

/** Index of the active line; also animates the words of that line (via the line element). */
export function useLyricSync(
  lines: LyricLine[] | null,
  media: Media | null,
  mediaAt: number,
  lineEl: (i: number) => HTMLElement | null,
  onSweep?: (el: HTMLElement, x: number) => void,
): number {
  const [idx, setIdx] = useState(-1);
  const mref = useRef(media);
  mref.current = media;
  const aref = useRef(mediaAt);
  aref.current = mediaAt;
  const eref = useRef(lineEl);
  eref.current = lineEl;
  const sref = useRef(onSweep);
  sref.current = onSweep;

  useEffect(() => {
    setIdx(-1);
    if (!lines) return;
    return runLyricSync(
      lines,
      () => {
        const m = mref.current;
        const st = playbackState(m, aref.current);
        // the user's per-track nudge: positive = highlight earlier
        return st && m ? { ...st, pos: st.pos + getOffset(offsetKey(m.artist, m.title)) } : st;
      },
      setIdx,
      (i) => eref.current(i),
      sref.current ? (el, x) => sref.current?.(el, x) : undefined,
    );
  }, [lines]);

  return idx;
}
