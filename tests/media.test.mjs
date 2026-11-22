import assert from "node:assert/strict";
import { before, test } from "node:test";
import { build } from "vite";

let playback;
let lyrics;

async function loadLibrary(entry) {
  const result = await build({
    configFile: false,
    logLevel: "silent",
    ssr: { noExternal: true },
    build: { ssr: entry, write: false, rollupOptions: { output: { inlineDynamicImports: true } } },
  });
  const [{ code }] = result.output;
  return import(`data:text/javascript;base64,${Buffer.from(code).toString("base64")}`);
}

before(async () => {
  playback = await loadLibrary("src/lib/playback.ts");
  lyrics = await loadLibrary("src/lib/lyrics.ts");
});

const sample = {
  appId: "player.exe", coverId: 1, playing: true,
  positionMs: 10_000, durationMs: 200_000, updatedMs: 1_000, canSeek: true,
};

test("an unchanged session sample freezes on pause and resumes without counting paused time", () => {
  const { updatePlaybackClock, playbackPosition } = playback;
  let clock = updatePlaybackClock(null, sample, 2_000);
  assert.equal(playbackPosition(clock, 2_000), 11_000);
  clock = updatePlaybackClock(clock, { ...sample, playing: false }, 3_000);
  assert.equal(playbackPosition(clock, 12_000), 12_000);
  clock = updatePlaybackClock(clock, sample, 15_000);
  assert.equal(playbackPosition(clock, 17_000), 14_000);

  const afterSeek = updatePlaybackClock(clock, { ...sample, updatedMs: 17_000, positionMs: 0 }, 17_000);
  assert.equal(playbackPosition(afterSeek, 17_000), 0);
});

test("Dotify's zero, old initial samples and stale samples from a different track do not start a timer", () => {
  const { updatePlaybackClock, playbackPosition } = playback;
  const clock = updatePlaybackClock(null, sample, 2_000);
  const dotify = { ...sample, appId: "com.dotify.app", positionMs: 0, canSeek: false };
  assert.equal(playbackPosition(updatePlaybackClock(clock, dotify, 2_000), 10_000), null);
  assert.equal(playbackPosition(updatePlaybackClock(null, sample, 30_000), 31_000), null);
  assert.equal(playbackPosition(updatePlaybackClock(clock, { ...sample, coverId: 2 }, 30_000), 31_000), null);
});

test("late seek capabilities validate an initial position of zero", () => {
  const { updatePlaybackClock, playbackPosition } = playback;
  const zero = { ...sample, appId: "com.dotify.app", positionMs: 0, canSeek: false };
  const initial = updatePlaybackClock(null, zero, 1_000);
  const withControls = updatePlaybackClock(initial, { ...zero, canSeek: true }, 1_000);
  assert.equal(playbackPosition(withControls, 2_000), 1_000);
  assert.equal(playbackPosition(updatePlaybackClock(null, { ...zero, appId: "player.exe" }, 1_000), 2_000), 1_000);
});

test("a paused track retains its known position when Island starts later", () => {
  const { updatePlaybackClock, playbackPosition } = playback;
  const clock = updatePlaybackClock(null, { ...sample, playing: false }, 30_000);
  assert.equal(playbackPosition(clock, 40_000), sample.positionMs);
});

const serviceMessage = "[00:00.00]Подключите подписку на Плюс, чтобы видеть тексты песен";
const query = { artist: "Тестовый исполнитель", title: "Песня для теста", album: "Альбом", durationMs: 189_000 };

test("a synchronized subscription notice falls back to a valid search result", async (t) => {
  t.mock.method(globalThis, "fetch", async (url) => {
    const placeholder = { duration: 189, syncedLyrics: serviceMessage, artistName: query.artist };
    const valid = { duration: 190, syncedLyrics: "[00:03.00]Обычная строка песни", artistName: query.artist };
    return Response.json(new URL(url).pathname.endsWith("/get") ? placeholder : [placeholder, valid]);
  });
  const result = await lyrics.fetchLyrics(query, new AbortController().signal);
  assert.equal(result.source, "LRCLIB");
  assert.equal(result.lines[0].text, "Обычная строка песни");
});

test("a plain subscription notice is treated as missing lyrics", async (t) => {
  t.mock.method(globalThis, "fetch", async (url) => {
    const placeholder = { duration: 189, plainLyrics: "Подключите подписку на Плюс, чтобы видеть тексты песен" };
    return Response.json(new URL(url).pathname.endsWith("/get") ? placeholder : [placeholder]);
  });
  const result = await lyrics.fetchLyrics({ ...query, title: "Второй тест" }, new AbortController().signal);
  assert.equal(result, null);
});
