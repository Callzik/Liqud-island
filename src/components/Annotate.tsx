// Lightshot-style editor over a selected screen region: arrows, frames, pen,
// marker, text and pixelation. The final PNG is rendered here at full resolution.

import { useEffect, useRef, useState, type PointerEvent as RPointerEvent } from "react";

export type Tool = "arrow" | "rect" | "pen" | "marker" | "text" | "blur";

export interface Op {
  tool: Tool;
  color: string;
  size: number;
  pts: { x: number; y: number }[];
  text?: string;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const COLORS = ["#ff3b30", "#ffcc00", "#34c759", "#0a84ff", "#ffffff", "#000000"];
export const SIZES = [2, 4, 7];

const TOOLS: { id: Tool; title: string; d: string }[] = [
  { id: "arrow", title: "Стрелка", d: "M5 19 19 5M10 5h9v9" },
  { id: "rect", title: "Рамка", d: "M4 6h16v12H4z" },
  { id: "pen", title: "Карандаш", d: "M4 20l4-1 11-11-3-3L5 16zM14 6l3 3" },
  { id: "marker", title: "Маркер", d: "M5 19h6M14 4l6 6-8 8H8v-4z" },
  { id: "text", title: "Текст", d: "M5 6V4h14v2M12 4v16M9 20h6" },
  { id: "blur", title: "Скрыть (размыть)", d: "M4 4h4v4H4zM10 4h4v4h-4zM16 4h4v4h-4zM4 10h4v4H4zM16 10h4v4h-4zM4 16h4v4H4zM10 16h4v4h-4zM16 16h4v4h-4z" },
];

const fontPx = (size: number) => 12 + size * 3;

/** Pixelates `r` (canvas pixels) in place. */
function pixelate(ctx: CanvasRenderingContext2D, r: Rect, cell: number) {
  const x = Math.max(0, Math.round(r.x));
  const y = Math.max(0, Math.round(r.y));
  const w = Math.min(ctx.canvas.width - x, Math.round(r.w));
  const h = Math.min(ctx.canvas.height - y, Math.round(r.h));
  if (w < 2 || h < 2) return;
  const img = ctx.getImageData(x, y, w, h);
  const d = img.data;
  for (let by = 0; by < h; by += cell) {
    for (let bx = 0; bx < w; bx += cell) {
      let rr = 0, g = 0, b = 0, n = 0;
      for (let yy = by; yy < Math.min(h, by + cell); yy++)
        for (let xx = bx; xx < Math.min(w, bx + cell); xx++) {
          const i = (yy * w + xx) * 4;
          rr += d[i];
          g += d[i + 1];
          b += d[i + 2];
          n++;
        }
      ctx.fillStyle = `rgb(${rr / n | 0},${g / n | 0},${b / n | 0})`;
      ctx.fillRect(x + bx, y + by, Math.min(cell, w - bx), Math.min(cell, h - by));
    }
  }
}

const norm = (a: { x: number; y: number }, b: { x: number; y: number }): Rect => ({
  x: Math.min(a.x, b.x),
  y: Math.min(a.y, b.y),
  w: Math.abs(b.x - a.x),
  h: Math.abs(b.y - a.y),
});

/** Draws one op; `k` scales css px → canvas px. Blur needs the pixels underneath already drawn. */
export function drawOp(ctx: CanvasRenderingContext2D, op: Op, k: number) {
  const p = op.pts.map((q) => ({ x: q.x * k, y: q.y * k }));
  if (!p.length) return;
  const lw = op.size * k;
  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.strokeStyle = op.color;
  ctx.fillStyle = op.color;
  ctx.lineWidth = lw;
  const a = p[0];
  const b = p[p.length - 1];
  switch (op.tool) {
    case "rect": {
      const r = norm(a, b);
      ctx.strokeRect(r.x, r.y, r.w, r.h);
      break;
    }
    case "arrow": {
      const ang = Math.atan2(b.y - a.y, b.x - a.x);
      const head = Math.max(10 * k, lw * 4);
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x - Math.cos(ang) * head * 0.6, b.y - Math.sin(ang) * head * 0.6);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(b.x, b.y);
      ctx.lineTo(b.x - Math.cos(ang - 0.45) * head, b.y - Math.sin(ang - 0.45) * head);
      ctx.lineTo(b.x - Math.cos(ang + 0.45) * head, b.y - Math.sin(ang + 0.45) * head);
      ctx.closePath();
      ctx.fill();
      break;
    }
    case "pen":
    case "marker": {
      if (op.tool === "marker") {
        ctx.globalAlpha = 0.38;
        ctx.lineWidth = lw * 4;
        ctx.lineCap = "square";
      }
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      for (const q of p.slice(1)) ctx.lineTo(q.x, q.y);
      if (p.length === 1) ctx.lineTo(a.x + 0.1, a.y);
      ctx.stroke();
      break;
    }
    case "text": {
      const fs = fontPx(op.size) * k;
      ctx.font = `700 ${fs}px "Segoe UI", system-ui, sans-serif`;
      ctx.textBaseline = "top";
      // thin contrasting outline keeps the text readable on any background
      ctx.lineWidth = Math.max(2, fs / 8);
      ctx.strokeStyle = op.color === "#000000" ? "rgba(255,255,255,.8)" : "rgba(0,0,0,.55)";
      (op.text ?? "").split("\n").forEach((line, i) => {
        ctx.strokeText(line, a.x, a.y + i * fs * 1.2);
        ctx.fillText(line, a.x, a.y + i * fs * 1.2);
      });
      break;
    }
    case "blur":
      pixelate(ctx, norm(a, b), Math.max(8, Math.round(14 * k)));
      break;
  }
  ctx.restore();
}

/** Crop of `src` (physical pixels) + the ops → PNG data url. */
export function renderPng(src: CanvasImageSource, sel: Rect, k: number, ops: Op[]): string {
  const c = document.createElement("canvas");
  c.width = Math.max(1, Math.round(sel.w * k));
  c.height = Math.max(1, Math.round(sel.h * k));
  const ctx = c.getContext("2d", { willReadFrequently: true })!;
  ctx.drawImage(src, Math.round(sel.x * k), Math.round(sel.y * k), c.width, c.height, 0, 0, c.width, c.height);
  for (const op of ops) drawOp(ctx, op, k);
  return c.toDataURL("image/png");
}

export function Annotate({
  sel,
  src,
  onDone,
  onCancel,
}: {
  sel: Rect;
  /** full screenshot in physical pixels (null if the image could not be read) */
  src: HTMLCanvasElement | null;
  onDone: (ops: Op[]) => void;
  onCancel: () => void;
}) {
  const [tool, setTool] = useState<Tool>("arrow");
  const [color, setColor] = useState(COLORS[0]);
  const [size, setSize] = useState(1);
  const [ops, setOps] = useState<Op[]>([]);
  const [redo, setRedo] = useState<Op[]>([]);
  const [cur, setCur] = useState<Op | null>(null);
  const [typing, setTyping] = useState<{ x: number; y: number; text: string } | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const dpr = window.devicePixelRatio || 1;

  // live preview: the crop itself (so blur shows real pixels) + ops
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const ctx = c.getContext("2d", { willReadFrequently: true })!;
    ctx.clearRect(0, 0, c.width, c.height);
    if (src) {
      const sk = src.width / window.innerWidth;
      ctx.drawImage(src, sel.x * sk, sel.y * sk, sel.w * sk, sel.h * sk, 0, 0, c.width, c.height);
    }
    for (const op of cur ? [...ops, cur] : ops) drawOp(ctx, op, dpr);
  }, [ops, cur, src, sel, dpr]);

  const commitText = () => {
    if (typing && typing.text.trim()) {
      setOps((o) => [...o, { tool: "text", color, size: SIZES[size], pts: [{ x: typing.x, y: typing.y }], text: typing.text }]);
      setRedo([]);
    }
    setTyping(null);
  };

  const finish = () => {
    if (typing) commitText();
    // commitText's state update is async: include the pending text by hand
    const pending = typing && typing.text.trim() ? [{ tool: "text" as Tool, color, size: SIZES[size], pts: [{ x: typing.x, y: typing.y }], text: typing.text }] : [];
    onDone([...ops, ...pending]);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typing) return;
      if (e.key === "Enter") {
        e.preventDefault();
        e.stopPropagation();
        finish();
      } else if (e.key === "Escape") {
        e.stopPropagation();
        onCancel();
      }
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) {
          setRedo((r) => {
            if (!r.length) return r;
            setOps((o) => [...o, r[r.length - 1]]);
            return r.slice(0, -1);
          });
        } else {
          setOps((o) => {
            if (!o.length) return o;
            setRedo((r) => [...r, o[o.length - 1]]);
            return o.slice(0, -1);
          });
        }
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
        e.preventDefault();
        finish();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const local = (e: RPointerEvent) => {
    const r = canvas.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const down = (e: RPointerEvent) => {
    e.stopPropagation();
    if (e.button !== 0) return;
    const p = local(e);
    if (tool === "text") {
      // otherwise the mousedown that follows moves focus off the new text box
      e.preventDefault();
      if (typing) commitText();
      setTyping({ ...p, text: "" });
      return;
    }
    (e.target as Element).setPointerCapture(e.pointerId);
    setCur({ tool, color, size: SIZES[size], pts: [p] });
  };
  const move = (e: RPointerEvent) => {
    e.stopPropagation();
    if (!cur) return;
    const p = local(e);
    setCur((c) => (c ? { ...c, pts: c.tool === "pen" || c.tool === "marker" ? [...c.pts, p] : [c.pts[0], p] } : c));
  };
  const up = (e: RPointerEvent) => {
    e.stopPropagation();
    if (!cur) return;
    const a = cur.pts[0];
    const b = cur.pts[cur.pts.length - 1];
    const tiny = Math.hypot(b.x - a.x, b.y - a.y) < 3 && cur.tool !== "pen" && cur.tool !== "marker";
    if (!tiny) {
      setOps((o) => [...o, cur]);
      setRedo([]);
    }
    setCur(null);
  };

  const below = sel.y + sel.h + 56 < window.innerHeight;
  const barTop = below ? sel.y + sel.h + 10 : Math.max(8, sel.y - 50);
  const barLeft = Math.min(Math.max(8, sel.x + sel.w - 460), window.innerWidth - 470);

  return (
    <>
      <canvas
        ref={canvas}
        className={`an-canvas an-${tool}`}
        style={{ left: sel.x, top: sel.y, width: sel.w, height: sel.h }}
        width={Math.max(1, Math.round(sel.w * dpr))}
        height={Math.max(1, Math.round(sel.h * dpr))}
        onPointerDown={down}
        onPointerMove={move}
        onPointerUp={up}
        onMouseDown={(e) => {
          e.preventDefault();
          e.stopPropagation();
        }}
        onMouseUp={(e) => e.stopPropagation()}
      />
      {typing && (
        <textarea
          className="an-text"
          autoFocus
          rows={1}
          style={{ left: sel.x + typing.x, top: sel.y + typing.y, color, fontSize: fontPx(SIZES[size]) }}
          value={typing.text}
          onChange={(e) => setTyping({ ...typing, text: e.target.value })}
          onMouseDown={(e) => e.stopPropagation()}
          onBlur={commitText}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              commitText();
            } else if (e.key === "Escape") setTyping(null);
          }}
        />
      )}
      <div className="an-bar" style={{ left: barLeft, top: barTop }} onMouseDown={(e) => e.stopPropagation()} onMouseUp={(e) => e.stopPropagation()}>
        {TOOLS.map((t) => (
          <button key={t.id} className={`an-btn ${tool === t.id ? "on" : ""}`} title={t.title} onClick={() => setTool(t.id)}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d={t.d} />
            </svg>
          </button>
        ))}
        <span className="an-sep" />
        {COLORS.map((c) => (
          <button key={c} className={`an-color ${color === c ? "on" : ""}`} style={{ background: c }} title={c} onClick={() => setColor(c)} />
        ))}
        <span className="an-sep" />
        {SIZES.map((s, i) => (
          <button key={s} className={`an-size ${size === i ? "on" : ""}`} title="Толщина" onClick={() => setSize(i)}>
            <i style={{ width: 3 + i * 3, height: 3 + i * 3 }} />
          </button>
        ))}
        <span className="an-sep" />
        <button
          className="an-btn"
          title="Отменить (Ctrl+Z)"
          disabled={!ops.length}
          onClick={() => {
            setRedo((r) => [...r, ops[ops.length - 1]]);
            setOps((o) => o.slice(0, -1));
          }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <path d="M9 14 4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3" />
          </svg>
        </button>
        <button
          className="an-btn"
          title="Вернуть (Ctrl+Shift+Z)"
          disabled={!redo.length}
          onClick={() => {
            setOps((o) => [...o, redo[redo.length - 1]]);
            setRedo((r) => r.slice(0, -1));
          }}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <path d="m15 14 5-5-5-5M20 9H10a6 6 0 0 0 0 12h3" />
          </svg>
        </button>
        <button className="an-btn" title="Отмена (Esc)" onClick={onCancel}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
            <path d="M6 6l12 12M18 6 6 18" />
          </svg>
        </button>
        <button className="an-done" onClick={finish} title="Enter">
          ✓ Готово
        </button>
      </div>
    </>
  );
}
