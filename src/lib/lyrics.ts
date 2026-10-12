// Synced lyrics from LRCLIB (https://lrclib.net) — fetched at runtime, never bundled.
// LRCLIB gives a timestamp per line; word timing inside a line is estimated
// from word lengths so the highlight can sweep through the line.

export interface LyricWord {
  text: string;
  /** ms from the start of the line */
  from: number;
  to: number;
}

export interface LyricLine {
  /** ms from the start of the track */
  t: number;
  text: string;
  words: LyricWord[];
}

export interface LyricsData {
  lines: LyricLine[] | null;
  plain: string | null;
  instrumental: boolean;
}

export interface LyricsQuery {
  artist: string;
  title: string;
  album: string;
  durationMs: number;
}

interface Item {
  duration?: number | null;
  instrumental?: boolean;
  plainLyrics?: string | null;
  syncedLyrics?: string | null;
}

const BASE = "https://lrclib.net/api";
const cache = new Map<string, LyricsData | null>();

export const lyricsKey = (q: LyricsQuery) => `${q.artist}\u0001${q.title}\u0001${q.album}\u0001${Math.round(q.durationMs / 1000)}`;

/** "Song (feat. X) - 2011 Remaster" → "Song" */
function cleanTitle(s: string): string {
  return s
    .replace(/\s*[(\[][^)\]]*(feat|ft\.|remaster|live|version|edit|mono|stereo|deluxe|bonus)[^)\]]*[)\]]/gi, "")
    .replace(/\s+-\s+(\d{4}\s+)?(remaster|live|single|radio|mono|stereo).*$/i, "")
    .trim();
}

function firstArtist(s: string): string {
  return s.split(/\s*(?:,|;|&|\/|\bfeat\.?|\bft\.?)\s*/i)[0].trim();
}

/** null = "not there" (404); throws on network errors and other HTTP failures. */
async function getJson(url: string, signal: AbortSignal): Promise<unknown> {
  const r = await fetch(url, { signal });
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

function pick(items: Item[], durationMs: number): Item | null {
  const d = durationMs / 1000;
  let best: Item | null = null;
  let bestScore = Infinity;
  for (const it of items) {
    if (!it.syncedLyrics && !it.plainLyrics && !it.instrumental) continue;
    const diff = d > 0 && it.duration ? Math.abs(it.duration - d) : 0;
    if (d > 0 && it.duration && diff > 6) continue;
    const score = diff + (it.syncedLyrics ? 0 : 100);
    if (score < bestScore) {
      best = it;
      bestScore = score;
    }
  }
  return best;
}

/** Rough singing weight of a word: syllables (vowel groups) plus a pause after punctuation. */
function weight(w: string): number {
  const syl = (w.toLowerCase().match(/[aeiouyаеёиоуыэюя]+/g) ?? []).length;
  const pause = /[,;:—–-]$/.test(w) ? 0.8 : /[.!?…]$/.test(w) ? 1.2 : 0;
  return Math.max(1, syl) + 0.35 + pause;
}

function makeWords(text: string, span: number): LyricWord[] {
  const parts = text.split(/\s+/).filter(Boolean);
  const weights = parts.map(weight);
  const sum = weights.reduce((a, b) => a + b, 0) || 1;
  let acc = 0;
  return parts.map((p, i) => {
    const from = (acc / sum) * span;
    acc += weights[i];
    // a word's own pause is silence, not singing: the highlight finishes before it
    const pause = weights[i] - Math.max(1, (p.toLowerCase().match(/[aeiouyаеёиоуыэюя]+/g) ?? []).length) - 0.35;
    return { text: p, from, to: ((acc - Math.max(0, pause)) / sum) * span };
  });
}

const TAG = /<(\d+):(\d+)(?:[.:](\d+))?>/g;
const tagMs = (m: RegExpMatchArray) => Math.round((Number(m[1]) * 60 + Number(m[2]) + (m[3] ? Number(`0.${m[3]}`) : 0)) * 1000);

/** Enhanced LRC: "<00:12.30>word <00:12.80>word" — exact word times when the file has them. */
function taggedWords(body: string, lineT: number, end: number): { text: string; words: LyricWord[] } | null {
  const marks = [...body.matchAll(TAG)];
  if (marks.length < 2) return null;
  const words: LyricWord[] = [];
  marks.forEach((m, k) => {
    const start = m.index! + m[0].length;
    const stop = k + 1 < marks.length ? marks[k + 1].index! : body.length;
    const chunk = body.slice(start, stop).trim();
    if (!chunk) return;
    const from = tagMs(m) - lineT;
    const to = (k + 1 < marks.length ? tagMs(marks[k + 1]) : Math.min(end, tagMs(m) + 600)) - lineT;
    // a tag may cover several words: split its time evenly
    const parts = chunk.split(/\s+/);
    parts.forEach((p, j) => {
      const a = from + ((to - from) * j) / parts.length;
      words.push({ text: p, from: a, to: a + (to - from) / parts.length });
    });
  });
  return words.length ? { text: words.map((w) => w.text).join(" "), words } : null;
}

export function parseLrc(src: string): LyricLine[] {
  const raw: { t: number; text: string; body: string }[] = [];
  for (const row of src.split(/\r?\n/)) {
    const m = row.match(/^((?:\[\d+:\d+(?:[.:]\d+)?\])+)(.*)$/);
    if (!m) continue;
    const text = m[2].replace(TAG, "").replace(/\s+/g, " ").trim();
    for (const s of m[1].matchAll(/\[(\d+):(\d+)(?:[.:](\d+))?\]/g)) {
      const frac = s[3] ? Number(`0.${s[3]}`) : 0;
      raw.push({ t: Math.round((Number(s[1]) * 60 + Number(s[2]) + frac) * 1000), text, body: m[2] });
    }
  }
  raw.sort((a, b) => a.t - b.t);
  const out: LyricLine[] = [];
  raw.forEach((r, i) => {
    if (!r.text) return;
    const end = i + 1 < raw.length ? raw[i + 1].t : r.t + 5000;
    const exact = taggedWords(r.body, r.t, end);
    if (exact) {
      out.push({ t: r.t, text: exact.text, words: exact.words });
      return;
    }
    // ~4.5 syllables per second when sung, never past the next line
    const syl = r.text.split(/\s+/).reduce((n, w) => n + weight(w), 0);
    const span = Math.max(400, Math.min(end - r.t - 150, syl * 230 + 300));
    out.push({ t: r.t, text: r.text, words: makeWords(r.text, span) });
  });
  return out;
}

function toData(it: Item): LyricsData {
  const lines = it.syncedLyrics ? parseLrc(it.syncedLyrics) : null;
  return {
    lines: lines && lines.length ? lines : null,
    plain: it.plainLyrics?.trim() || null,
    instrumental: !!it.instrumental,
  };
}

function useful(it: Item | null | undefined): it is Item {
  return !!it && !!(it.syncedLyrics || it.plainLyrics || it.instrumental);
}

/** Resolves as soon as a task gives synced lyrics; otherwise waits for all of
 *  them and takes the first plain-text / instrumental result. */
function bestOf(tasks: Promise<Item | null>[]): Promise<Item | null> {
  return new Promise((resolve) => {
    let left = tasks.length;
    let fallback: Item | null = null;
    if (!left) return resolve(null);
    for (const t of tasks) {
      t.then((v) => {
        if (v?.syncedLyrics) return resolve(v);
        if (v && !fallback) fallback = v;
        if (--left === 0) resolve(fallback);
      });
    }
  });
}

const CACHE_MAX = 120;

/** null = nothing found. Throws when the request was aborted. */
export async function fetchLyrics(q: LyricsQuery, signal: AbortSignal): Promise<LyricsData | null> {
  const key = lyricsKey(q);
  if (cache.has(key)) return cache.get(key)!;

  const title = cleanTitle(q.title) || q.title;
  const artist = firstArtist(q.artist) || q.artist;

  // all lookups run at the same time — the first useful answer wins
  const tasks: Promise<Item | null>[] = [];
  if (q.album && q.durationMs > 0) {
    const p = new URLSearchParams({
      artist_name: q.artist,
      track_name: q.title,
      album_name: q.album,
      duration: String(Math.round(q.durationMs / 1000)),
    });
    tasks.push(getJson(`${BASE}/get?${p}`, signal).then((it) => (useful(it as Item) ? (it as Item) : null)));
  }
  const search = (p: URLSearchParams) =>
    getJson(`${BASE}/search?${p}`, signal).then((l) => (Array.isArray(l) ? pick(l as Item[], q.durationMs) : null));
  tasks.push(search(new URLSearchParams({ track_name: title, artist_name: artist })));
  tasks.push(search(new URLSearchParams({ q: `${artist} ${title}` })));

  // a failed request (offline at startup, 429/5xx) must not be remembered as
  // "this song has no lyrics" — only a clean "not found" is
  let failed = false;
  const found = await bestOf(
    tasks.map((t) =>
      t.catch(() => {
        failed = true;
        return null;
      }),
    ),
  );
  // timed out waiting for a synced version: plain text found so far still counts
  if (signal.aborted && !found) throw new DOMException("aborted", "AbortError");

  const data = found ? toData(found) : null;
  if (data || !failed) {
    cache.set(key, data);
    if (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value!);
  }
  return data;
}
