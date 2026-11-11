// Full-monitor capture overlay (separate window "overlay"): shows the frozen
// screen dimmed, lets the user select a region or pick a colour.

import { useCallback, useEffect, useRef, useState, type MouseEvent as RMouseEvent } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { call, isTauri, on } from "../api";
import { Annotate, renderPng, type Op, type Rect } from "./Annotate";

type Mode = "region" | "ocr" | "qr" | "picker" | "record";

interface Start {
  id: number;
  mode: Mode;
  width: number;
  height: number;
}

const HINTS: Record<Mode, string> = {
  region: "Выделите область — потом можно порисовать",
  ocr: "Выделите текст для распознавания",
  qr: "Выделите QR-код или кликните — поиск по всему экрану",
  picker: "Кликните, чтобы взять цвет",
  record: "Выделите область для записи или кликните — весь экран",
};

const ZOOM_PX = 11; // source pixels across the loupe
const LOUPE = 132; // loupe size, css px

const hex2 = (n: number) => n.toString(16).padStart(2, "0");

function demoShot(): string {
  const svg = `<svg xmlns='http://www.w3.org/2000/svg' width='1600' height='900'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='#d97757'/><stop offset='.5' stop-color='#7b2b8f'/><stop offset='1' stop-color='#1b2a4a'/></linearGradient></defs><rect width='1600' height='900' fill='url(#g)'/><rect x='220' y='180' width='760' height='480' rx='18' fill='#16161a'/><rect x='260' y='240' width='420' height='26' rx='6' fill='#e8e6e1'/><rect x='260' y='290' width='560' height='18' rx='5' fill='#8b8a85'/><rect x='260' y='326' width='500' height='18' rx='5' fill='#8b8a85'/><circle cx='1220' cy='420' r='130' fill='#f2c14e'/></svg>`;
  return "data:image/svg+xml;utf8," + encodeURIComponent(svg);
}

export function Overlay() {
  const [start, setStart] = useState<Start | null>(null);
  const [url, setUrl] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [pos, setPos] = useState<{ x: number; y: number } | null>(null);
  const [drag, setDrag] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [color, setColor] = useState("#000000");
  // region screenshot: the selection waits for drawing before it is saved
  const [edit, setEdit] = useState<Rect | null>(null);
  const srcRef = useRef<HTMLCanvasElement | null>(null);
  const loupeRef = useRef<HTMLCanvasElement>(null);
  const startRef = useRef<Start | null>(null);
  startRef.current = start;

  const reset = useCallback(() => {
    setStart(null);
    setUrl(null);
    setReady(false);
    setDrag(null);
    setPos(null);
    setEdit(null);
    srcRef.current = null;
  }, []);

  useEffect(() => {
    document.documentElement.classList.add("is-overlay");
    const subs = [
      on<Start>("overlay-start", (s) => {
        reset();
        setStart(s);
        setUrl(isTauri ? `${convertFileSrc(String(s.id), "shot")}?v=${s.id}` : demoShot());
      }),
      on<null>("overlay-end", reset),
    ];
    if (!isTauri) {
      const mode = (new URLSearchParams(location.hash.split("?")[1] ?? "").get("mode") as Mode) || "region";
      setStart({ id: 1, mode, width: 1600, height: 900 });
      setUrl(demoShot());
    }
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, [reset]);

  const cancel = useCallback(() => {
    reset();
    if (isTauri) call("capture_cancel");
  }, [reset]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [cancel]);

  // physical pixels per css pixel
  const k = start ? start.width / window.innerWidth : 1;

  const onLoad = (img: HTMLImageElement) => {
    const s = startRef.current;
    if (!s) return;
    const c = document.createElement("canvas");
    c.width = img.naturalWidth;
    c.height = img.naturalHeight;
    try {
      c.getContext("2d", { willReadFrequently: true })!.drawImage(img, 0, 0);
      srcRef.current = c;
    } catch {
      srcRef.current = null;
    }
    setReady(true);
    if (isTauri) call("overlay_show", { id: s.id });
  };

  // loupe + colour under the cursor
  useEffect(() => {
    if (!start || start.mode !== "picker" || !pos || !srcRef.current) return;
    const src = srcRef.current;
    const sx = Math.floor((pos.x * src.width) / window.innerWidth);
    const sy = Math.floor((pos.y * src.height) / window.innerHeight);
    const px = src.getContext("2d")!.getImageData(Math.min(src.width - 1, sx), Math.min(src.height - 1, sy), 1, 1).data;
    setColor(`#${hex2(px[0])}${hex2(px[1])}${hex2(px[2])}`.toUpperCase());
    const ctx = loupeRef.current?.getContext("2d");
    if (!ctx) return;
    const half = Math.floor(ZOOM_PX / 2);
    ctx.imageSmoothingEnabled = false;
    ctx.fillStyle = "#000";
    ctx.fillRect(0, 0, LOUPE, LOUPE);
    ctx.drawImage(src, sx - half, sy - half, ZOOM_PX, ZOOM_PX, 0, 0, LOUPE, LOUPE);
    const cell = LOUPE / ZOOM_PX;
    ctx.strokeStyle = "rgba(255,255,255,0.08)";
    ctx.lineWidth = 1;
    for (let i = 1; i < ZOOM_PX; i++) {
      ctx.beginPath();
      ctx.moveTo(i * cell, 0);
      ctx.lineTo(i * cell, LOUPE);
      ctx.moveTo(0, i * cell);
      ctx.lineTo(LOUPE, i * cell);
      ctx.stroke();
    }
    ctx.strokeStyle = "#fff";
    ctx.lineWidth = 2;
    ctx.strokeRect(half * cell, half * cell, cell, cell);
  }, [pos, start]);

  if (!start || !url) return <div className="ov-root" />;

  const finish = (rect: { x: number; y: number; w: number; h: number } | null, col?: string) => {
    const s = start;
    reset();
    if (isTauri) call("capture_finish", { id: s.id, rect, color: col ?? null });
    else console.info("capture_finish", s.mode, rect, col);
  };

  const colorAt = (x: number, y: number): string | null => {
    const src = srcRef.current;
    if (!src) return null;
    const sx = Math.min(src.width - 1, Math.max(0, Math.floor((x * src.width) / window.innerWidth)));
    const sy = Math.min(src.height - 1, Math.max(0, Math.floor((y * src.height) / window.innerHeight)));
    const px = src.getContext("2d")!.getImageData(sx, sy, 1, 1).data;
    return `#${hex2(px[0])}${hex2(px[1])}${hex2(px[2])}`.toUpperCase();
  };

  const done = (ops: Op[]) => {
    const s = start;
    const sel = edit!;
    const rect = { x: Math.round(sel.x * k), y: Math.round(sel.y * k), w: Math.round(sel.w * k), h: Math.round(sel.h * k) };
    const src = srcRef.current;
    if (!ops.length || !src) return finish(rect);
    let png: string;
    try {
      png = renderPng(src, sel, src.width / window.innerWidth, ops);
    } catch {
      return finish(rect);
    }
    reset();
    if (isTauri) call("capture_finish_png", { id: s.id, png: png.slice(png.indexOf(",") + 1) });
    else console.info("capture_finish_png", png.length);
  };

  const down = (e: RMouseEvent) => {
    if (e.button === 2) return cancel();
    if (edit) return;
    if (e.button !== 0) return;
    // sample right under the click: no mousemove may have happened yet
    if (start.mode === "picker") return finish(null, colorAt(e.clientX, e.clientY) ?? color);
    setDrag({ x0: e.clientX, y0: e.clientY, x1: e.clientX, y1: e.clientY });
  };
  const move = (e: RMouseEvent) => {
    setPos({ x: e.clientX, y: e.clientY });
    if (drag) setDrag({ ...drag, x1: e.clientX, y1: e.clientY });
  };
  const up = () => {
    if (!drag) return;
    const x = Math.min(drag.x0, drag.x1);
    const y = Math.min(drag.y0, drag.y1);
    const w = Math.abs(drag.x1 - drag.x0);
    const h = Math.abs(drag.y1 - drag.y0);
    if (w < 4 || h < 4) {
      setDrag(null);
      if (start.mode === "qr" || start.mode === "record") finish(null);
      return;
    }
    if (start.mode === "region") {
      setDrag(null);
      setEdit({ x, y, w, h });
      return;
    }
    finish({ x: Math.round(x * k), y: Math.round(y * k), w: Math.round(w * k), h: Math.round(h * k) });
  };

  const sel = edit
    ? edit
    : drag
      ? { x: Math.min(drag.x0, drag.x1), y: Math.min(drag.y0, drag.y1), w: Math.abs(drag.x1 - drag.x0), h: Math.abs(drag.y1 - drag.y0) }
      : null;

  const loupeLeft = pos ? (pos.x + LOUPE + 40 > window.innerWidth ? pos.x - LOUPE - 24 : pos.x + 24) : 0;
  const loupeTop = pos ? (pos.y + LOUPE + 64 > window.innerHeight ? pos.y - LOUPE - 48 : pos.y + 24) : 0;

  return (
    <div
      className={`ov-root ${ready ? "ready" : ""} ov-${start.mode}`}
      onMouseDown={down}
      onMouseMove={move}
      onMouseUp={up}
      onContextMenu={(e) => e.preventDefault()}
    >
      <img className="ov-shot" src={url} crossOrigin="anonymous" alt="" draggable={false} onLoad={(e) => onLoad(e.currentTarget)} onError={() => isTauri && call("overlay_show", { id: start.id })} />
      {sel ? <div className="ov-sel" style={{ left: sel.x, top: sel.y, width: sel.w, height: sel.h }} /> : start.mode !== "picker" && <div className="ov-dim" />}
      {sel && (
        <div className="ov-size" style={edit ? { left: sel.x, top: Math.max(4, sel.y - 26) } : { left: sel.x, top: sel.y + sel.h + 8 > window.innerHeight - 30 ? sel.y - 30 : sel.y + sel.h + 8 }}>
          {Math.round(sel.w * k)} × {Math.round(sel.h * k)}
        </div>
      )}
      {edit && <Annotate sel={edit} src={srcRef.current} onDone={done} onCancel={cancel} />}
      {pos && !sel && start.mode !== "picker" && (
        <>
          <div className="ov-cross-h" style={{ top: pos.y }} />
          <div className="ov-cross-v" style={{ left: pos.x }} />
        </>
      )}
      {pos && start.mode === "picker" && (
        <div className="ov-loupe" style={{ left: loupeLeft, top: loupeTop }}>
          <canvas ref={loupeRef} width={LOUPE} height={LOUPE} />
          <div className="ov-loupe-label">
            <i style={{ background: color }} />
            {color}
          </div>
        </div>
      )}
      <div className="ov-hint">
        {edit ? "Рисуйте на выделении" : HINTS[start.mode]} <span>{edit ? "· Enter — готово · Esc — отмена" : "· Esc — отмена"}</span>
      </div>
    </div>
  );
}
