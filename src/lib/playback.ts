import type { Media } from "../api";

const MAX_SAMPLE_AGE = 15_000;
const MAX_FUTURE_SKEW = 1_000;

type MediaSample = Pick<Media, "appId" | "coverId" | "playing" | "positionMs" | "updatedMs" | "durationMs" | "canSeek">;

export interface PlaybackClock {
  sample: MediaSample;
  anchor: { position: number; at: number } | null;
}

export function playbackPosition(clock: PlaybackClock | null, now = Date.now()): number | null {
  if (!clock?.anchor) return null;
  const { anchor, sample } = clock;
  const elapsed = sample.playing ? Math.max(0, now - anchor.at) : 0;
  return Math.min(sample.durationMs > 0 ? sample.durationMs : Infinity, anchor.position + elapsed);
}

export function updatePlaybackClock(
  previous: PlaybackClock | null,
  sample: MediaSample | null,
  now = Date.now(),
): PlaybackClock | null {
  if (!sample) return null;
  const sameTrack = previous?.sample.appId === sample.appId && previous.sample.coverId === sample.coverId;
  const prior = sameTrack ? previous : null;
  const changed = !prior || sample.updatedMs !== prior.sample.updatedMs
    || sample.positionMs !== prior.sample.positionMs || sample.canSeek !== prior.sample.canSeek;
  const plausibleTime = sample.updatedMs > 0 && sample.updatedMs - now <= MAX_FUTURE_SKEW;
  const fresh = plausibleTime && now - sample.updatedMs <= MAX_SAMPLE_AGE;
  const pausedPosition = !sample.playing && sample.positionMs > 0 && plausibleTime;
  // Dotify's media session publishes zero even when its player is well into a track.
  // Other sources can report a real zero without supporting seek.
  const dotifyDefault = /dotify/i.test(sample.appId) && !sample.canSeek;
  const validPosition = sample.positionMs > 0 || (sample.positionMs === 0 && !dotifyDefault);
  if (changed && validPosition && (fresh || pausedPosition)) {
    return { sample, anchor: { position: sample.positionMs, at: Math.min(sample.updatedMs, now) } };
  }

  let anchor = prior?.anchor ?? null;
  if (anchor && prior && prior.sample.playing !== sample.playing) {
    // Some apps only change the playback status. Rebase the clock at the
    // transition so seconds spent paused never accumulate on resume.
    anchor = { position: playbackPosition(prior, now) ?? anchor.position, at: now };
  }
  return { sample, anchor };
}
