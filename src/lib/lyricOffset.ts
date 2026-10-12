// Per-track lyric offset: LRCLIB timings are sometimes made for another version
// of the track, so the whole text runs early or late. The user nudges it once,
// it is remembered for that track (only in this browser profile of the app).

const KEY = "island.lyricOffsets";
const listeners = new Set<() => void>();
let map: Record<string, number> = {};
try {
  map = JSON.parse(localStorage.getItem(KEY) || "{}") ?? {};
} catch {
  map = {};
}

export const offsetKey = (artist: string, title: string) => `${artist}\u0001${title}`.toLowerCase();

export function getOffset(key: string): number {
  return map[key] ?? 0;
}

export function setOffset(key: string, ms: number) {
  const v = Math.max(-5000, Math.min(5000, Math.round(ms / 50) * 50));
  if (v) map[key] = v;
  else delete map[key];
  try {
    localStorage.setItem(KEY, JSON.stringify(map));
  } catch {
    /* storage unavailable — keep it for this session */
  }
  listeners.forEach((l) => l());
}

export function subscribeOffsets(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
