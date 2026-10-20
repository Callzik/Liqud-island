// «Офис»: every agent session is a little pixel person. Working ones sit at
// their desk and type, the ones who need you stand up and ask, finished ones
// rest in the lounge. Drawn on a small canvas and scaled up (pixelated).

import { useEffect, useRef } from "react";
import type { AgentSession, AgentsPayload } from "../api";
import { agentName, doing, hash, isBusy, needsYou, SOURCES } from "../lib/agents";
import { headline } from "./AgentsTab";
import { ClaudeMark, CodexMark, IHand } from "./Icons";

export const ART_W = 394;
export const ART_H = 245;
const DESKS = 9;
const SPEED = 38; // art px per second

// ---------------------------------------------------------------- sprites

const HEAD_FRONT = ["..HHHHHH..", ".HHHHHHHH.", ".HKKKKKKH.", ".KKKKKKKK.", ".KKEKKEKK.", ".KKKKKKKK.", "..KKKKKK..", "...KKKK..."];
const HEAD_BACK = ["..HHHHHH..", ".HHHHHHHH.", ".HHHHHHHH.", ".HHHHHHHH.", ".HHHHHHHH.", ".HHHHHHHH.", "..HHHHHH..", "...KKKK..."];
const BODY = [".SSSSSSSS.", "SSSSSSSSSS", "SSSSSSSSSS", "STSSSSSSTS", "KSSSSSSSSK", ".PPPPPPPP.", ".PPPPPPPP."];
const LEGS = [
  [".PPP..PPP.", ".PPP..PPP.", ".FFF..FFF."],
  [".PPP..PPP.", ".PPP...PP.", ".FFF......"],
  [".PPP..PPP.", ".PP...PPP.", "......FFF."],
];
const SIT_LEGS = [".PPPPPPPP.", ".FFF..FFF."];

interface Look {
  H: string;
  K: string;
  S: string;
  T: string;
  P: string;
  beard: boolean;
  glasses: boolean;
}

const HAIR = ["#2b2018", "#5a3b22", "#8a5a2b", "#c9a26b", "#1d1d1f", "#6e6a66", "#a0432a"];
const SKIN = ["#f1c7a5", "#e6b48e", "#c68a5e", "#8d5a3b"];
const SHIRT = ["#2f3a4f", "#3f5e4a", "#7a3e3e", "#e6e3dc", "#22252b", "#4b5f8a", "#8a6d3b", "#5b4a7a"];
const PANTS = ["#2f3b55", "#3a3a3e", "#5b4a3a", "#2a2c30"];

function shade(hex: string, k: number): string {
  const n = parseInt(hex.slice(1), 16);
  const c = (v: number) => Math.max(0, Math.min(255, Math.round(v * k)));
  return `rgb(${c(n >> 16)},${c((n >> 8) & 255)},${c(n & 255)})`;
}

function lookOf(id: string): Look {
  const h = hash(id);
  const S = SHIRT[(h >>> 8) % SHIRT.length];
  return {
    H: HAIR[h % HAIR.length],
    K: SKIN[(h >>> 4) % SKIN.length],
    S,
    T: shade(S, 0.78),
    P: PANTS[(h >>> 12) % PANTS.length],
    beard: (h >>> 16) % 10 < 3,
    glasses: (h >>> 20) % 10 < 2,
  };
}

function sprite(ctx: CanvasRenderingContext2D, rows: string[], x: number, y: number, l: Look) {
  const pal: Record<string, string> = { H: l.H, K: l.K, E: "#1f1f22", S: l.S, T: l.T, P: l.P, F: "#1c1c20", G: "#26282d" };
  for (let r = 0; r < rows.length; r++) {
    const row = rows[r];
    for (let c = 0; c < row.length; c++) {
      const ch = row[c];
      if (ch === ".") continue;
      ctx.fillStyle = pal[ch];
      ctx.fillRect(x + c, y + r, 1, 1);
    }
  }
}

function frontHead(l: Look): string[] {
  const h = [...HEAD_FRONT];
  if (l.glasses) h[4] = ".KGGKKGGK.";
  if (l.beard) {
    h[5] = ".HKKKKKKH.";
    h[6] = "..HHHHHH..";
  }
  return h;
}

/** Standing / walking person, feet at (fx, fy). */
function person(ctx: CanvasRenderingContext2D, fx: number, fy: number, l: Look, back: boolean, walk: number, bob: number) {
  const rows = [...(back ? HEAD_BACK : frontHead(l)), ...BODY, ...LEGS[walk]];
  ctx.fillStyle = "rgba(0,0,0,0.12)";
  ctx.fillRect(fx - 4, fy - 1, 9, 2);
  sprite(ctx, rows, Math.round(fx - 5), Math.round(fy - rows.length - bob), l);
}

/** Seated at a desk, seen from behind; hands tap the keyboard. */
function seatedBack(ctx: CanvasRenderingContext2D, x: number, y: number, l: Look, typing: number) {
  // arms reaching forward to the desk
  ctx.fillStyle = l.T;
  ctx.fillRect(x, y + 4, 1, 5);
  ctx.fillRect(x + 9, y + 4, 1, 5);
  ctx.fillStyle = l.K;
  ctx.fillRect(x, y + 3 - (typing === 1 ? 1 : 0), 1, 1);
  ctx.fillRect(x + 9, y + 3 - (typing === 2 ? 1 : 0), 1, 1);
  sprite(ctx, HEAD_BACK, x, y, l);
  sprite(ctx, [".SSSSSSSS.", "SSSSSSSSSS", "SSSSSSSSSS"], x, y + 8, l);
  // chair back
  ctx.fillStyle = "#1c1d21";
  ctx.fillRect(x - 1, y + 10, 12, 7);
  ctx.fillStyle = "#2a2c31";
  ctx.fillRect(x, y + 11, 10, 1);
  ctx.fillStyle = "#16171a";
  ctx.fillRect(x + 4, y + 17, 2, 3);
  ctx.fillRect(x + 1, y + 20, 8, 1);
}

/** Seated facing us (sofa, armchair). */
function seatedFront(ctx: CanvasRenderingContext2D, x: number, y: number, l: Look) {
  sprite(ctx, [...frontHead(l), ...BODY.slice(0, 5), ...SIT_LEGS], x, y, l);
}

// ---------------------------------------------------------------- the room

const R = (ctx: CanvasRenderingContext2D, c: string, x: number, y: number, w: number, h: number) => {
  ctx.fillStyle = c;
  ctx.fillRect(x, y, w, h);
};

const deskPos = (d: number) => ({ dx: 46 + (d % 3) * 80, dy: 70 + Math.floor(d / 3) * 60 });
const seatOf = (d: number) => {
  const { dx, dy } = deskPos(d);
  return { x: dx + 28, y: dy + 30 };
};
const standOf = (d: number) => {
  const { dx, dy } = deskPos(d);
  return { x: dx + 68, y: dy + 20 };
};

/** Rest spots: 4 on the sofa, 2 armchairs, then at the coffee machine. */
const REST = [
  { x: 305, y: 124, sit: true },
  { x: 323, y: 124, sit: true },
  { x: 341, y: 124, sit: true },
  { x: 359, y: 124, sit: true },
  { x: 305, y: 196, sit: true },
  { x: 365, y: 196, sit: true },
  { x: 322, y: 62, sit: false },
  { x: 338, y: 62, sit: false },
  { x: 354, y: 62, sit: false },
  { x: 370, y: 62, sit: false },
];

const CODE = ["#7ee787", "#79c0ff", "#ffa657", "#d2a8ff", "#c9d1d9", "#ff7b72"];

function room(ctx: CanvasRenderingContext2D, t: number, activity: number[]) {
  // floor
  R(ctx, "#d9d6cf", 0, 40, ART_W, ART_H - 40);
  ctx.fillStyle = "#cdc9c1";
  for (let x = 4; x < ART_W; x += 32) ctx.fillRect(x, 41, 1, ART_H);
  for (let y = 66; y < ART_H; y += 32) ctx.fillRect(0, y, ART_W, 1);
  // wall
  R(ctx, "#2f3136", 0, 0, ART_W, 40);
  R(ctx, "#26282c", 0, 0, ART_W, 3);
  R(ctx, "#1f2024", 0, 38, ART_W, 3);
  // window
  R(ctx, "#1d1e22", 30, 6, 68, 27);
  R(ctx, "#c8d2db", 32, 8, 31, 23);
  R(ctx, "#c8d2db", 65, 8, 31, 23);
  R(ctx, "#dde5ec", 32, 8, 31, 9);
  R(ctx, "#dde5ec", 65, 8, 31, 9);
  R(ctx, "#e9ecef", 28, 33, 72, 2);
  // bookshelf
  R(ctx, "#3d3128", 120, 3, 46, 42);
  for (let s = 0; s < 3; s++) {
    const y = 6 + s * 13;
    R(ctx, "#2a221c", 122, y + 10, 42, 2);
    let x = 123;
    let i = s * 7;
    while (x < 161) {
      const w = 2 + ((i * 7) % 3);
      const hgt = 7 + ((i * 5) % 3);
      R(ctx, ["#6f7f74", "#8c8a7c", "#5d6b78", "#a39a86", "#4f5b52", "#7d6b5a"][i % 6], x, y + 10 - hgt, w, hgt);
      x += w + (i % 4 === 0 ? 2 : 0);
      i++;
    }
  }
  R(ctx, "#2a221c", 120, 44, 46, 2);
  // board (text is drawn in HTML on top)
  R(ctx, "#2a2d33", 183, 5, 81, 29);
  R(ctx, "#17191d", 185, 7, 77, 25);
  const max = Math.max(1, ...activity);
  activity.forEach((n, i) => {
    const h = Math.max(1, Math.round((n / max) * 14));
    R(ctx, i === activity.length - 1 ? "#a6f0b9" : "#5fcf80", 222 + i * 3, 28 - h, 2, h);
  });
  R(ctx, "#2f343b", 220, 29, 39, 1);
  // clock
  const now = new Date();
  ctx.fillStyle = "#e9e7e2";
  ctx.beginPath();
  ctx.arc(283, 19, 8, 0, Math.PI * 2);
  ctx.fill();
  ctx.strokeStyle = "#2a2a2e";
  ctx.lineWidth = 1;
  const hand = (a: number, len: number) => {
    ctx.beginPath();
    ctx.moveTo(283.5, 19.5);
    ctx.lineTo(283.5 + Math.sin(a) * len, 19.5 - Math.cos(a) * len);
    ctx.stroke();
  };
  hand(((now.getHours() % 12) + now.getMinutes() / 60) * (Math.PI / 6), 4);
  hand(now.getMinutes() * (Math.PI / 30), 6);
  // counter + coffee machine
  R(ctx, "#4a3d33", 308, 28, 82, 4);
  R(ctx, "#3a2f27", 308, 32, 82, 14);
  R(ctx, "#2b2e33", 352, 12, 18, 16);
  R(ctx, "#1d1f23", 355, 16, 12, 7);
  R(ctx, Math.floor(t * 1.5) % 2 ? "#ff6b5e" : "#a8433b", 366, 14, 2, 2);
  R(ctx, "#f2f0eb", 330, 23, 4, 5);
  R(ctx, "#f2f0eb", 338, 24, 4, 4);
  R(ctx, "#d8a35b", 316, 20, 8, 8);
  // plant (top left)
  R(ctx, "#6d6f73", 9, 44, 12, 11);
  R(ctx, "#4f5155", 9, 44, 12, 2);
  for (const [x, y, w, h] of [[6, 30, 10, 8], [12, 25, 10, 9], [16, 33, 8, 8], [4, 36, 8, 7], [10, 36, 9, 8]])
    R(ctx, (x + y) % 3 ? "#5f8a5a" : "#4c7348", x, y, w, h);
  // filing cabinets
  for (const y of [72, 114]) {
    R(ctx, "#e7e6e2", 4, y, 17, 32);
    R(ctx, "#cfcdc7", 4, y + 30, 17, 2);
    for (let k = 0; k < 3; k++) {
      R(ctx, "#cfcdc7", 5, y + 10 + k * 10, 15, 1);
      R(ctx, "#9c9a95", 10, y + 4 + k * 10, 5, 1);
    }
  }
  // server rack
  R(ctx, "#1b1c1f", 4, 196, 22, 44);
  for (let k = 0; k < 6; k++) {
    R(ctx, "#26282c", 6, 199 + k * 7, 18, 5);
    R(ctx, (Math.floor(t * 3) + k) % 4 ? "#3ddc84" : "#1f5b3a", 21, 201 + k * 7, 1, 1);
  }
  // lounge: rug, sofa, table, armchairs, lamp, plant
  R(ctx, "#4c6a50", 284, 108, 106, 102);
  R(ctx, "#43604a", 286, 110, 102, 98);
  R(ctx, "#4c6a50", 289, 113, 96, 92);
  R(ctx, "#33363c", 296, 110, 78, 12);
  R(ctx, "#2b2e33", 296, 110, 78, 2);
  // round table
  ctx.fillStyle = "#ece9e3";
  ctx.beginPath();
  ctx.ellipse(335, 160, 13, 8, 0, 0, Math.PI * 2);
  ctx.fill();
  R(ctx, "#cfcbc3", 333, 167, 4, 6);
  // lamp
  R(ctx, "#2b2b2f", 381, 92, 2, 34);
  R(ctx, "#f3ecd8", 377, 86, 10, 7);
  R(ctx, "#2b2b2f", 378, 126, 8, 2);
  // plant (bottom right)
  R(ctx, "#6d6f73", 372, 228, 12, 12);
  for (const [x, y, w, h] of [[368, 212, 10, 8], [375, 206, 10, 10], [380, 214, 9, 8], [366, 219, 9, 8], [373, 218, 10, 9]])
    R(ctx, (x + y) % 3 ? "#5f8a5a" : "#4c7348", x, y, w, h);
}

/** Sofa / armchair fronts go over the sitters' legs. */
function loungeFront(ctx: CanvasRenderingContext2D) {
  R(ctx, "#3b3f46", 294, 119, 82, 10);
  R(ctx, "#454a52", 296, 120, 78, 2);
  R(ctx, "#33363c", 292, 112, 4, 18);
  R(ctx, "#33363c", 374, 112, 4, 18);
  for (const x of [298, 358]) {
    R(ctx, "#33363c", x, 184, 16, 4);
    R(ctx, "#3b3f46", x - 1, 192, 18, 9);
    R(ctx, "#454a52", x, 193, 16, 2);
  }
}

function desk(ctx: CanvasRenderingContext2D, d: number, screen: "off" | "idle" | "code", scroll: number) {
  const { dx, dy } = deskPos(d);
  // legs + top
  R(ctx, "#5a4231", dx + 2, dy + 13, 3, 9);
  R(ctx, "#5a4231", dx + 51, dy + 13, 3, 9);
  R(ctx, "#7a5a40", dx, dy, 56, 13);
  R(ctx, "#8a6a4d", dx, dy, 56, 2);
  R(ctx, "#5f4532", dx, dy + 13, 56, 3);
  // monitor
  R(ctx, "#e8e8e6", dx + 15, dy - 13, 26, 15);
  R(ctx, "#cfcfcc", dx + 26, dy + 2, 4, 3);
  R(ctx, "#bdbdba", dx + 23, dy + 4, 10, 1);
  R(ctx, screen === "off" ? "#3a3d42" : "#1b1f25", dx + 17, dy - 11, 22, 11);
  if (screen !== "off") {
    for (let i = 0; i < 5; i++) {
      const k = i + scroll;
      const seed = hash(`${d}:${k}`);
      const w = 4 + (seed % 15);
      const ind = (seed >>> 5) % 4;
      ctx.globalAlpha = screen === "idle" ? 0.45 : 1;
      R(ctx, CODE[(seed >>> 9) % CODE.length], dx + 18 + ind, dy - 10 + i * 2, Math.min(w, 20 - ind), 1);
      ctx.globalAlpha = 1;
    }
  }
  // keyboard, mouse, mug
  R(ctx, "#cfcfcc", dx + 20, dy + 7, 16, 3);
  R(ctx, "#b7b7b4", dx + 20, dy + 9, 16, 1);
  R(ctx, "#cfcfcc", dx + 40, dy + 8, 2, 3);
  R(ctx, "#f2f0eb", dx + 47, dy + 4, 4, 4);
  R(ctx, "#f2f0eb", dx + 51, dy + 5, 1, 2);
}

// ---------------------------------------------------------------- agents

interface Walker {
  x: number;
  y: number;
  path: { x: number; y: number }[];
  spot: string;
  arrived: boolean;
  dir: number; // -1 going up (back view), 1 otherwise
}

// positions survive the tab being closed and reopened
const walkers = new Map<string, Walker>();
const deskOf = new Map<string, number>();
const restOf = new Map<string, number>();

const AISLES = [34, 114, 194, 274];
const CORRIDORS = [56, 112, 172, 236];
const nearest = (v: number, list: number[]) => list.reduce((a, b) => (Math.abs(b - v) < Math.abs(a - v) ? b : a));

function route(from: { x: number; y: number }, to: { x: number; y: number }) {
  const c0 = nearest(from.y, CORRIDORS);
  const c1 = nearest(to.y, CORRIDORS);
  const ax = nearest(to.x, AISLES);
  const pts = [
    { x: from.x, y: c0 },
    { x: ax, y: c0 },
    { x: ax, y: c1 },
    { x: to.x, y: c1 },
    { x: to.x, y: to.y },
  ];
  // the lounge and coffee corner are open: walk straight once there
  if (to.x > 280) return [{ x: from.x, y: c0 }, { x: 280, y: c0 }, { x: 280, y: to.y }, to];
  return pts;
}

interface Target {
  x: number;
  y: number;
  spot: string;
  sit: "desk" | "sofa" | null;
}

function targetOf(s: AgentSession): Target | null {
  if (needsYou(s) || isBusy(s)) {
    restOf.delete(s.id);
    const d = deskOf.get(s.id);
    if (d === undefined) return null;
    if (needsYou(s)) {
      const p = standOf(d);
      return { ...p, spot: `stand${d}`, sit: null };
    }
    const p = seatOf(d);
    return { ...p, spot: `desk${d}`, sit: "desk" };
  }
  let r = restOf.get(s.id);
  if (r === undefined) {
    const used = new Set(restOf.values());
    r = REST.findIndex((_, i) => !used.has(i));
    if (r < 0) return null;
    restOf.set(s.id, r);
  }
  const p = REST[r];
  return { x: p.x, y: p.y, spot: `rest${r}`, sit: p.sit ? "sofa" : null };
}

function assignDesks(list: AgentSession[]) {
  const ids = new Set(list.map((s) => s.id));
  for (const id of [...deskOf.keys()]) if (!ids.has(id)) deskOf.delete(id);
  for (const id of [...restOf.keys()]) if (!ids.has(id)) restOf.delete(id);
  for (const id of [...walkers.keys()]) if (!ids.has(id)) walkers.delete(id);
  const used = new Set(deskOf.values());
  for (const s of [...list].sort((a, b) => a.startedMs - b.startedMs)) {
    if (deskOf.has(s.id)) continue;
    for (let d = 0; d < DESKS; d++) {
      if (!used.has(d)) {
        deskOf.set(s.id, d);
        used.add(d);
        break;
      }
    }
  }
}

function shortText(t: string, n: number) {
  return t.length > n ? t.slice(0, n - 1) + "…" : t;
}

export function Office({ agents, onOpen }: { agents: AgentsPayload; onOpen: (id: string) => void }) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const labelRefs = useRef(new Map<string, HTMLButtonElement>());
  const bubbleRefs = useRef(new Map<string, HTMLDivElement>());
  const live = useRef(agents);
  live.current = agents;
  const list = agents.sessions;
  assignDesks(list);
  const head = headline(list);
  const firstWaiting = list.find(needsYou);

  useEffect(() => {
    const canvas = canvasRef.current!;
    const ctx = canvas.getContext("2d")!;
    canvas.width = ART_W;
    canvas.height = ART_H;
    let last = performance.now();
    let scale = 2;

    const fit = () => {
      const wrap = wrapRef.current;
      if (!wrap) return;
      scale = Math.min(wrap.clientWidth / ART_W, wrap.clientHeight / ART_H);
      canvas.style.width = `${ART_W * scale}px`;
      canvas.style.height = `${ART_H * scale}px`;
    };
    fit();
    const ro = new ResizeObserver(fit);
    if (wrapRef.current) ro.observe(wrapRef.current);

    const frame = () => {
      const nowP = performance.now();
      const dt = Math.min(0.1, (nowP - last) / 1000);
      last = nowP;
      const t = nowP / 1000;
      const { sessions, activity } = live.current;
      assignDesks(sessions);

      room(ctx, t, activity.length ? activity : [0]);
      const byDesk = new Map<number, AgentSession>();
      for (const s of sessions) {
        const d = deskOf.get(s.id);
        if (d !== undefined) byDesk.set(d, s);
      }
      const scroll = Math.floor(t * 4);
      for (let d = 0; d < DESKS; d++) {
        const s = byDesk.get(d);
        const w = s ? walkers.get(s.id) : null;
        const atDesk = !!(w && w.arrived && w.spot === `desk${d}`);
        desk(ctx, d, !s ? "off" : atDesk && s.status === "working" ? "code" : "idle", atDesk && s?.status === "working" ? scroll + d * 3 : d * 3);
      }

      // move everyone, then draw back to front
      const draw: { y: number; fn: () => void }[] = [];
      for (const s of sessions) {
        const tg = targetOf(s);
        if (!tg) continue;
        let w = walkers.get(s.id);
        if (!w) {
          w = { x: tg.x, y: tg.y, path: [], spot: tg.spot, arrived: true, dir: 1 };
          walkers.set(s.id, w);
        } else if (w.spot !== tg.spot) {
          w.path = route({ x: w.x, y: w.y }, tg);
          w.spot = tg.spot;
          w.arrived = false;
        }
        let moving = false;
        let budget = SPEED * dt;
        while (budget > 0 && w.path.length) {
          const p = w.path[0];
          const dx = p.x - w.x;
          const dy = p.y - w.y;
          const dist = Math.hypot(dx, dy);
          if (dist <= budget) {
            w.x = p.x;
            w.y = p.y;
            budget -= dist;
            w.path.shift();
          } else {
            w.x += (dx / dist) * budget;
            w.y += (dy / dist) * budget;
            if (Math.abs(dy) > 0.01) w.dir = dy < 0 ? -1 : 1;
            budget = 0;
          }
          moving = true;
        }
        if (!w.path.length) w.arrived = true;
        const look = lookOf(s.id);
        const ww = w;
        const typing = s.status === "working" ? 1 + (Math.floor(t * 8 + hash(s.id)) % 2) : 0;
        if (ww.arrived && tg.sit === "desk") {
          const { dx, dy } = deskPos(deskOf.get(s.id)!);
          draw.push({ y: dy + 30, fn: () => seatedBack(ctx, dx + 23, dy + 12 + (s.status === "thinking" && Math.floor(t * 1.2) % 2 ? 1 : 0), look, typing) });
        } else if (ww.arrived && tg.sit === "sofa") {
          draw.push({ y: ww.y, fn: () => seatedFront(ctx, Math.round(ww.x - 5), Math.round(ww.y - 20), look) });
        } else {
          const walk = moving ? 1 + (Math.floor(t * 8) % 2) : 0;
          const bob = !moving && needsYou(s) ? Math.floor(t * 2.5) % 2 : 0;
          const back = moving ? ww.dir < 0 : !needsYou(s) && tg.spot.startsWith("rest") && !REST[restOf.get(s.id) ?? 0]?.sit;
          draw.push({ y: ww.y, fn: () => person(ctx, ww.x, ww.y, look, back, walk, bob) });
        }

        // labels follow their person
        const label = labelRefs.current.get(s.id);
        const bubble = bubbleRefs.current.get(s.id);
        let lx = ww.x;
        let ly = ww.y - 23;
        if (ww.arrived && tg.sit === "desk") {
          const { dx, dy } = deskPos(deskOf.get(s.id)!);
          lx = dx + 28;
          ly = dy - 14;
        } else if (ww.arrived && tg.sit === "sofa") {
          ly = ww.y - 22 - ((restOf.get(s.id) ?? 0) % 2) * 9;
        }
        if (label) label.style.transform = `translate(${Math.round(lx * scale)}px, ${Math.round(ly * scale)}px) translate(-50%, -100%)`;
        if (bubble) bubble.style.transform = `translate(${Math.round(ww.x * scale)}px, ${Math.round((ww.y - 34) * scale)}px) translate(-50%, -100%)`;
      }
      draw.sort((a, b) => a.y - b.y);
      // sofa fronts belong between the sitters and whoever stands in front
      let fronted = false;
      for (const d of draw) {
        if (!fronted && d.y > 126) {
          loungeFront(ctx);
          fronted = true;
        }
        d.fn();
      }
      if (!fronted) loungeFront(ctx);
    };

    frame();
    const timer = window.setInterval(() => {
      if (!document.hidden) frame();
    }, 50);
    return () => {
      window.clearInterval(timer);
      ro.disconnect();
    };
  }, []);

  return (
    <div className="office">
      <div className="office-head">
        <span className={firstWaiting ? "hot" : ""}>
          {firstWaiting ? (
            <>
              <i className="office-dot" /> Нужны вы: {agentName(firstWaiting)}
            </>
          ) : (
            <>
              {head.icon && <IHand size={14} />}
              {head.text || "Офис пуст"}
            </>
          )}
        </span>
        <span className="office-keys">Ctrl Ctrl — офис</span>
      </div>
      <div className="office-room" ref={wrapRef}>
        <div className="office-stage">
          <canvas ref={canvasRef} className="office-canvas" />
          <div className="office-board" style={{ left: `${(187 / ART_W) * 100}%`, top: `${(8 / ART_H) * 100}%` }}>
            <span>✓</span>
            {agents.done}
          </div>
          {list.map((s) => {
            const src = SOURCES[s.source];
            const waiting = needsYou(s);
            const resting = !waiting && !isBusy(s);
            return (
              <button
                key={s.id}
                className={`office-label ${waiting ? "hot" : ""} ${resting ? "rest" : ""}`}
                ref={(el) => {
                  if (el) labelRefs.current.set(s.id, el);
                  else labelRefs.current.delete(s.id);
                }}
                onClick={() => onOpen(s.id)}
                title={s.action}
              >
                {waiting ? (
                  <i className="office-dot" />
                ) : s.source === "claude" ? (
                  <ClaudeMark size={10} color={src.color} />
                ) : (
                  <CodexMark size={9} color={src.color} />
                )}
                <b>{shortText(s.project || agentName(s), resting ? 12 : 16)}</b>
                {!resting && <span>{doing(s)}</span>}
                {resting && s.status === "done" && <span>✓</span>}
              </button>
            );
          })}
          {list
            .filter((s) => needsYou(s) && s.request)
            .map((s) => (
              <div
                key={s.id}
                className="office-bubble"
                ref={(el) => {
                  if (el) bubbleRefs.current.set(s.id, el);
                  else bubbleRefs.current.delete(s.id);
                }}
                onClick={() => onOpen(s.id)}
              >
                {s.request!.kind === "question" ? shortText(s.request!.summary, 34) : `Можно? ${shortText(s.request!.detail.replace(/^\$ /, ""), 26)}`}
              </div>
            ))}
          {list.length === 0 && <div className="office-empty">Пока никого. Запустите claude или codex в терминале</div>}
        </div>
      </div>
    </div>
  );
}
