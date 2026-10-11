// Bridge to the Rust side. In a plain browser (vite dev without Tauri) a mock
// with demo data is used, so the UI can be worked on without Windows.

import { invoke as tauriInvoke, Channel } from "@tauri-apps/api/core";
import { listen as tauriListen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { mockInvoke, mockListen } from "./mock";

export interface Settings {
  hotkey: string;
  autostart: boolean;
  musicReactive: boolean;
  hideFullscreen: boolean;
  hoverExpand: boolean;
  clipLimit: number;
  cursorPull: boolean;
  captureEnabled: boolean;
  hotkeyRegion: string;
  hotkeyOcr: string;
  aiEnabled: boolean;
  aiUrl: string;
  aiKey: string;
  aiModel: string;
  weatherEnabled: boolean;
  weatherCity: string;
  weatherLat: number;
  weatherLon: number;
  voiceEnabled: boolean;
  hotkeyVoice: string;
  voiceWhisperUrl: string;
  voiceWhisperKey: string;
  voiceWhisperModel: string;
  downloadsEnabled: boolean;
  textEnabled: boolean;
  notesEnabled: boolean;
  residentsEnabled: boolean;
  residents: string[];
  ymToken: string;
}

export interface ShelfItem {
  id: string;
  path: string;
  name: string;
  size: number;
  isDir: boolean;
  added: number;
}

export interface PinItem {
  id: string;
  target: string;
  name: string;
}

export interface Data {
  shelf: ShelfItem[];
  pins: PinItem[];
  usage: Record<string, number>;
}

export interface Clip {
  id: string;
  kind: "text" | "image" | "files";
  preview: string;
  chars: number;
  files: string[];
  thumb: string | null;
  width: number;
  height: number;
  source: string;
  ts: number;
  pinned: boolean;
}

export interface Media {
  app: string;
  appId: string;
  title: string;
  artist: string;
  album: string;
  playing: boolean;
  positionMs: number;
  durationMs: number;
  updatedMs: number;
  canPrev: boolean;
  canNext: boolean;
  coverId: number;
}

export interface Cover {
  id: number;
  url: string | null;
}

export interface Volume {
  level: number;
  muted: boolean;
  device: string;
}

export interface Cursor {
  x: number;
  y: number;
  inside: boolean;
  lmb: boolean;
}

export interface AppItem {
  target: string;
  name: string;
}

export interface InitPayload {
  settings: Settings;
  data: Data;
  clips: Clip[];
  media: Media | null;
  cover: Cover;
  volume: Volume;
  version: string;
  hotkeyError: string | null;
  weather?: import("./lib/weather").WeatherPayload | null;
}

export type DropEvent =
  | { type: "enter"; paths: string[]; x: number; y: number }
  | { type: "over"; x: number; y: number }
  | { type: "drop"; paths: string[]; x: number; y: number }
  | { type: "leave" };

export const isTauri = typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__ !== undefined;

export function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return isTauri ? tauriInvoke<T>(cmd, args) : mockInvoke<T>(cmd, args);
}

export async function on<T>(event: string, cb: (payload: T) => void): Promise<() => void> {
  if (!isTauri) return mockListen<T>(event, cb);
  return tauriListen<T>(event, (e) => cb(e.payload));
}

export async function onDrop(cb: (e: DropEvent) => void): Promise<() => void> {
  if (!isTauri) return mockListen<DropEvent>("__drop", cb);
  const dpr = () => window.devicePixelRatio || 1;
  return getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "leave") cb({ type: "leave" });
    else if (p.type === "over") cb({ type: "over", x: p.position.x / dpr(), y: p.position.y / dpr() });
    else cb({ type: p.type, paths: p.paths, x: p.position.x / dpr(), y: p.position.y / dpr() });
  });
}

export async function onFocus(cb: (focused: boolean) => void): Promise<() => void> {
  if (!isTauri) {
    const f = () => cb(true);
    const b = () => cb(false);
    window.addEventListener("focus", f);
    window.addEventListener("blur", b);
    return () => {
      window.removeEventListener("focus", f);
      window.removeEventListener("blur", b);
    };
  }
  return getCurrentWindow().onFocusChanged((e) => cb(e.payload));
}

/** Starts a native drag of files out of the island (tauri-plugin-drag). */
export async function dragOut(paths: string[], icon: string): Promise<void> {
  if (!isTauri) return;
  const onEvent = new Channel<unknown>();
  try {
    await tauriInvoke("plugin:drag|start_drag", { item: paths, image: icon, onEvent });
  } catch (e) {
    console.warn("drag failed", e);
  }
}
