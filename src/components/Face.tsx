// «Глазки»: a face living inside the liquid island. Idle eyes follow the cursor and blink,
// they look up and think while the AI writes, stare in alarm when a timer rings and
// smile when something is done.

import { useEffect, useRef, useState } from "react";
import { on, type Cursor } from "../api";

export type Mood = "idle" | "think" | "alarm" | "happy" | "sleep";

export function Face({ mood = "idle", size = 1 }: { mood?: Mood; size?: number }) {
  const ref = useRef<HTMLSpanElement>(null);
  const [look, setLook] = useState({ x: 0, y: 0 });
  const [blink, setBlink] = useState(false);

  useEffect(() => {
    if (mood !== "idle") return;
    const un = on<Cursor>("cursor", (c) => {
      const el = ref.current;
      if (!el) return;
      const r = el.getBoundingClientRect();
      const dx = c.x - (r.left + r.width / 2);
      const dy = c.y - (r.top + r.height / 2);
      const d = Math.hypot(dx, dy) || 1;
      // the farther the cursor, the more the pupils lean towards it (max 3 px)
      const k = Math.min(1, d / 260) * 3;
      setLook({ x: (dx / d) * k, y: (dy / d) * k * 0.8 });
    });
    return () => {
      un.then((u) => u());
    };
  }, [mood]);

  useEffect(() => {
    if (mood !== "idle") return;
    let t = 0;
    const next = () => {
      t = window.setTimeout(() => {
        setBlink(true);
        window.setTimeout(() => setBlink(false), 140);
        next();
      }, 2500 + Math.random() * 3500);
    };
    next();
    return () => window.clearTimeout(t);
  }, [mood]);

  const shift = mood === "think" ? { x: 2, y: -2 } : mood === "idle" ? look : { x: 0, y: 0 };
  return (
    <span ref={ref} className={`face face-${mood} ${blink ? "blink" : ""}`} style={{ ["--fs" as string]: size }}>
      {mood === "think" && (
        <span className="face-dots">
          <i />
          <i />
          <i />
        </span>
      )}
      {mood === "alarm" && <span className="face-bang">!</span>}
      {mood === "happy" ? (
        <svg className="face-smile" viewBox="0 0 36 16" width={36 * size} height={16 * size}>
          <path d="M3 9 Q8 2 13 9" />
          <path d="M23 9 Q28 2 33 9" />
          <path d="M14 13 Q18 16 22 13" />
        </svg>
      ) : (
        <>
          <i className="eye" style={{ transform: `translate(${shift.x}px, ${shift.y}px)` }} />
          <i className="eye" style={{ transform: `translate(${shift.x}px, ${shift.y}px)` }} />
        </>
      )}
    </span>
  );
}
