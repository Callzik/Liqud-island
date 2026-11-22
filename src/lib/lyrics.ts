import { call, isTauri } from "../api";
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
  source: "LRCLIB" | "Яндекс Музыка";
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
  artistName?: string;
  duration?: number | null;
  instrumental?: boolean;
  plainLyrics?: string | null;
  syncedLyrics?: string | null;
}

const BASE = "https://lrclib.net/api";
const cache = new Map<string, LyricsData | null>();

/** Yandex Music as the first source (when the account is connected in settings). */
let yandexOn = false;
export function setYandexLyrics(on: boolean) {
  yandexOn = on;
}

export const lyricsKey = (q: LyricsQuery) =>
  `${q.artist}\u0001${q.title}\u0001${q.album}\u0001${Math.round(q.durationMs / 1000)}\u0001${yandexOn ? "ym" : ""}`;

/** "Song (feat. X) - 2011 Remaster" → "Song" */
function cleanTitle(s: string): string {
  return s
    .replace(/\s*[(\[][^)\]]*(feat|ft\.|remaster|live|version|edit|mono|stereo|deluxe|bonus)[^)\]]*[)\]]/gi, "")
    .replace(/\s+-\s+(\d{4}\s+)?(remaster|live|single|radio|mono|stereo).*$/i, "")
    .trim();
}

/** "Song (Remix) [Live]" → "Song"; ё → е — for a last-chance search. */
function bareTitle(s: string): string {
  return s.replace(/\s*[(\[][^)\]]*[)\]]/g, "").replace(/\s+-\s+.*$/, "").trim();
}

const norm = (s: string) => s.toLowerCase().replace(/ё/g, "е").replace(/[^\p{L}\p{N}]+/gu, " ").trim();

/** Does an LRCLIB entry belong to (one of) the artists we're playing? */
function sameArtist(a: string | undefined, ours: string): boolean {
  if (!a) return false;
  const x = norm(a);
  const parts = ours.split(/\s*(?:,|;|&|\/|\bfeat\.?|\bft\.?|\sх\s|\sx\s)\s*/i).map(norm).filter(Boolean);
  return parts.some((p) => x.includes(p) || p.includes(x));
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

function pick(items: Item[], durationMs: number): LyricsData | null {
  const d = durationMs / 1000;
  let best: LyricsData | null = null;
  let bestScore = Infinity;
  for (const it of items) {
    const diff = d > 0 && it.duration ? Math.abs(it.duration - d) : 0;
    if (d > 0 && it.duration && diff > 6) continue;
    const data = toData(it, "LRCLIB");
    if (!data) continue;
    const score = diff + (data.lines ? 0 : 100);
    if (score < bestScore) {
      best = data;
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

function isServiceMessage(text: string): boolean {
  const message = norm(text.replace(/\[[^\]]*\]|<[^>]*>/g, ""));
  return /^(?:подключите|оформите|активируйте|купите) .*подписк[а-я]+.*текст[а-я]* (?:этой )?(?:пес[а-я]+|трек[а-я]+)$/u.test(message)
    || /^текст[а-я]* (?:пес[а-я]+|трек[а-я]+) (?:доступ[а-я]*|недоступ[а-я]*).*подписк[а-я]*/u.test(message)
    || /^(?:subscribe|upgrade) .* (?:see|view|read) (?:the )?lyrics$/i.test(message);
}

function toData(it: Item | null | undefined, source: LyricsData["source"]): LyricsData | null {
  if (!it) return null;
  const synced = typeof it.syncedLyrics === "string" ? it.syncedLyrics.trim() : "";
  const plain = typeof it.plainLyrics === "string" ? it.plainLyrics.trim() : "";
  const lines = synced && !isServiceMessage(synced) ? parseLrc(synced) : null;
  const cleanPlain = plain && !isServiceMessage(plain) ? plain : null;
  if (!lines?.length && !cleanPlain && !it.instrumental) return null;
  return {
    source,
    lines: lines && lines.length ? lines : null,
    plain: cleanPlain,
    instrumental: !!it.instrumental,
  };
}

/** Resolves as soon as a task gives synced lyrics; otherwise waits for all of
 *  them and takes the first plain-text / instrumental result. */
function bestOf(tasks: Promise<LyricsData | null>[]): Promise<LyricsData | null> {
  return new Promise((resolve) => {
    let left = tasks.length;
    let fallback: LyricsData | null = null;
    if (!left) return resolve(null);
    for (const t of tasks) {
      t.then((v) => {
        if (v?.lines) return resolve(v);
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
  const bare = bareTitle(q.title) || title;
  const titles = [...new Set([q.title, title, bare, bare.replace(/ё/gi, (m) => (m === "Ё" ? "Е" : "е"))])];

  // Prefer Yandex's synced text when configured. Start the fallback lookups
  // alongside it so a slow Yandex request cannot consume their time budget.
  const yandex = yandexOn && isTauri
    ? Promise.race([
      call<{ synced?: string | null; plain?: string | null } | null>("ym_lyrics", {
        artist: q.artist,
        title: q.title,
        durationMs: q.durationMs,
      }).catch(() => null),
      new Promise<null>((r) => setTimeout(() => r(null), 6000)),
    ]).then((ym) => ym && toData({ syncedLyrics: ym.synced, plainLyrics: ym.plain }, "Яндекс Музыка"))
    : Promise.resolve(null);

  // 2) LRCLIB: every lookup runs at the same time — the first synced answer wins
  const tasks: Promise<LyricsData | null>[] = [];
  if (q.album && q.durationMs > 0) {
    const p = new URLSearchParams({
      artist_name: q.artist,
      track_name: q.title,
      album_name: q.album,
      duration: String(Math.round(q.durationMs / 1000)),
    });
    tasks.push(getJson(`${BASE}/get?${p}`, signal).then((it) => toData(it as Item | null, "LRCLIB")));
  }
  const search = (p: URLSearchParams, strictArtist = false) =>
    getJson(`${BASE}/search?${p}`, signal).then((l) =>
      Array.isArray(l) ? pick((l as Item[]).filter((it) => !strictArtist || sameArtist(it.artistName, q.artist)), q.durationMs) : null,
    );
  for (const t of titles.slice(0, 3)) tasks.push(search(new URLSearchParams({ track_name: t, artist_name: artist })));
  tasks.push(search(new URLSearchParams({ q: `${artist} ${title}` })));
  // spelling of the artist differs (латиница / кириллица): search by title, check the artist ourselves
  tasks.push(search(new URLSearchParams({ track_name: bare }), true));

  // a failed request (offline at startup, 429/5xx) must not be remembered as
  // "this song has no lyrics" — only a clean "not found" is
  let failed = false;
  const lrclib = bestOf(
    tasks.map((t) =>
      t.catch(() => {
        failed = true;
        return null;
      }),
    ),
  );
  const ym = await yandex;
  const found = ym?.lines ? ym : await lrclib;
  // timed out waiting for a synced version: plain text found so far still counts
  if (signal.aborted && !found && !ym) throw new DOMException("aborted", "AbortError");

  // LRCLIB plain text vs Yandex plain text: either is better than nothing
  const data = found ?? ym;
  if (data || !failed) {
    cache.set(key, data);
    if (cache.size > CACHE_MAX) cache.delete(cache.keys().next().value!);
  }
  return data;
}
