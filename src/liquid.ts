// The liquid island: a canvas shape hanging from the top edge of the screen.
// Size changes are spring-driven (slight jelly overshoot), the bottom edge
// stretches toward the cursor and ripples with the system audio level.

export interface LiquidTarget {
  w: number;
  h: number;
  /** bottom corner radius */
  r: number;
  /** stretch toward the cursor */
  pull: boolean;
  /** ripple with audio */
  wave: boolean;
  /** equalizer bars on the right */
  bars: boolean;
  /** rim colour along the bottom edge, rgba */
  rim: [number, number, number, number];
  /** soft drop shadow (expanded states) */
  shadow: boolean;
}

class Spring {
  velocity = 0;
  target: number;
  constructor(public value: number, public k: number, public c: number) {
    this.target = value;
  }
  step(dt: number) {
    const a = -this.k * (this.value - this.target) - this.c * this.velocity;
    this.velocity += a * dt;
    this.value += this.velocity * dt;
  }
  settled(eps = 0.05) {
    return Math.abs(this.value - this.target) < eps && Math.abs(this.velocity) < eps * 8;
  }
  snap() {
    this.value = this.target;
    this.velocity = 0;
  }
}

interface Drop {
  x: number;
  y: number;
  vy: number;
  r: number;
  life: number;
}

const K = 0.5523; // bezier circle constant
/** Frame budget: on 120 Hz+ monitors draw every 2nd/3rd refresh (~60–80 fps).
 *  While music plays the island animates non-stop, so this halves its GPU cost.
 *  Slower displays (60–100 Hz) draw every frame. */
const MIN_FRAME_MS = 12;
/** refresh interval below which frames are skipped (≈ 110 Hz) */
const FAST_REFRESH_MS = 9;
const REACH = 150; // how far below the island the cursor still pulls

export class Liquid {
  private ctx: CanvasRenderingContext2D;
  private dpr = 1;
  private cw = 0;
  private ch = 0;
  private raf = 0;
  private last = 0;
  private t = 0;

  private w = new Spring(0, 230, 21);
  private h = new Spring(0, 260, 24);
  private r = new Spring(14, 220, 26);
  private pull = new Spring(0, 150, 9.5);
  private pullX = new Spring(500, 300, 32);
  private wave = new Spring(0, 40, 12);
  private bars = new Spring(0, 60, 14);
  private rim: [number, number, number, number] = [255, 255, 255, 0.12];
  private rimTarget: [number, number, number, number] = [255, 255, 255, 0.12];
  private shadow = new Spring(0, 80, 18);
  /** glass look: the screen behind the island, blurred inside the shape */
  private glass = false;
  private backdrop: { img: HTMLImageElement; x: number; y: number; w: number; h: number } | null = null;
  private springs = [this.w, this.h, this.r, this.pull, this.pullX, this.wave, this.bars, this.shadow];

  private level = 0;
  private levelTarget = 0;
  private barLevels = [0, 0, 0, 0];
  private cursor: { x: number; y: number; inside: boolean } | null = null;
  private target: LiquidTarget = {
    w: 0,
    h: 0,
    r: 14,
    pull: false,
    wave: false,
    bars: false,
    rim: [255, 255, 255, 0.12],
    shadow: false,
  };
  private drops: Drop[] = [];
  private lastDrop = 0;
  private prevPullTarget = 0;

  /** called every frame with the current (animated) box */
  onBox: ((w: number, h: number, r: number) => void) | null = null;

  private destroyed = false;
  private onResize = () => {
    this.resize();
    this.kick();
  };

  constructor(private canvas: HTMLCanvasElement) {
    this.ctx = canvas.getContext("2d")!;
    this.resize();
    window.addEventListener("resize", this.onResize);
  }

  destroy() {
    this.destroyed = true;
    if (this.raf) cancelAnimationFrame(this.raf);
    this.raf = 0;
    window.removeEventListener("resize", this.onResize);
    this.onBox = null;
  }

  private resize() {
    this.dpr = window.devicePixelRatio || 1;
    this.cw = window.innerWidth;
    this.ch = window.innerHeight;
    this.canvas.width = Math.round(this.cw * this.dpr);
    this.canvas.height = Math.round(this.ch * this.dpr);
    this.canvas.style.width = this.cw + "px";
    this.canvas.style.height = this.ch + "px";
  }

  get centerX() {
    return this.cw / 2;
  }

  setTarget(t: LiquidTarget) {
    this.target = t;
    this.w.target = t.w;
    this.h.target = t.h;
    this.r.target = t.r;
    this.wave.target = t.wave ? 1 : 0;
    this.bars.target = t.bars ? 1 : 0;
    this.shadow.target = t.shadow ? 1 : 0;
    this.rimTarget = t.rim;
    this.kick();
  }

  setCursor(x: number, y: number, inside: boolean) {
    this.cursor = { x, y, inside };
    this.kick();
  }

  clearCursor() {
    this.cursor = null;
    this.kick();
  }

  setGlass(on: boolean) {
    if (this.glass === on) return;
    this.glass = on;
    if (!on) this.backdrop = null;
    this.kick();
  }

  setBackdrop(img: HTMLImageElement, x: number, y: number, w: number, h: number) {
    if (!this.glass) return;
    this.backdrop = { img, x, y, w, h };
    this.kick();
  }

  /** Frosted glass inside the current path (called with the shape as clip). */
  private paintGlass(ctx: CanvasRenderingContext2D, L: number, R: number, H: number) {
    const b = this.backdrop;
    if (b) {
      ctx.save();
      ctx.filter = "blur(14px) saturate(170%)";
      // bleed past the edges so the blur doesn't fade to transparent at the border
      ctx.drawImage(b.img, b.x - 24, b.y - 24, b.w + 48, b.h + 48);
      ctx.restore();
    }
    // tint keeps text readable on any wallpaper
    ctx.fillStyle = "rgba(14,14,18,0.46)";
    ctx.fillRect(L - 2, 0, R - L + 4, H + 2);
    // soft sheen from above
    const g = ctx.createLinearGradient(0, 0, 0, Math.max(30, H));
    g.addColorStop(0, "rgba(255,255,255,0.16)");
    g.addColorStop(0.45, "rgba(255,255,255,0.04)");
    g.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = g;
    ctx.fillRect(L - 2, 0, R - L + 4, H + 2);
  }

  setAudio(level: number) {
    // perceptual curve: quiet music still moves a little
    this.levelTarget = Math.min(1, Math.pow(Math.max(0, level), 0.6));
    this.kick();
  }

  /** false until the first frame after a pause — that one is never skipped */
  private drawn = false;
  private prevTick = 0;
  private refreshMs = 1000 / 60;

  kick() {
    if (this.raf || this.destroyed) return;
    this.last = performance.now();
    this.drawn = false;
    this.prevTick = 0;
    this.raf = requestAnimationFrame(this.tick);
  }

  private tick = (now: number) => {
    this.raf = 0;
    // measured refresh interval (only between two consecutive callbacks)
    if (this.prevTick) {
      const d = now - this.prevTick;
      if (d > 0 && d < 50) this.refreshMs += (d - this.refreshMs) * 0.1;
    }
    this.prevTick = now;
    if (this.drawn && this.refreshMs < FAST_REFRESH_MS && now - this.last < MIN_FRAME_MS) {
      this.raf = requestAnimationFrame(this.tick);
      return;
    }
    this.drawn = true;
    const dt = Math.min(1 / 30, Math.max(0.001, (now - this.last) / 1000));
    this.last = now;
    this.t += dt;

    this.updatePull(now);
    for (const s of this.springs) s.step(dt);
    if (this.w.value < 0) this.w.value = 0;
    if (this.h.value < 0) this.h.value = 0;

    const attack = this.levelTarget > this.level ? 0.5 : 0.1;
    this.level += (this.levelTarget - this.level) * attack;
    for (let i = 0; i < 4; i++) {
      const wobble = 0.55 + 0.45 * Math.sin(this.t * (5.3 + i * 1.7) + i * 1.9);
      const goal = this.level * wobble;
      this.barLevels[i] += (goal - this.barLevels[i]) * 0.35;
    }
    for (let i = 0; i < 4; i++) {
      this.rim[i] += (this.rimTarget[i] - this.rim[i]) * (i === 3 ? 0.12 : 0.15);
    }
    this.stepDrops(dt);
    this.draw();
    this.onBox?.(this.w.value, this.h.value, this.r.value);

    const animating =
      !this.w.settled(0.1) ||
      !this.h.settled(0.1) ||
      !this.r.settled(0.1) ||
      !this.pull.settled(0.08) ||
      !this.pullX.settled(0.5) ||
      !this.wave.settled(0.01) ||
      !this.bars.settled(0.01) ||
      !this.shadow.settled(0.01) ||
      this.drops.length > 0 ||
      Math.abs(this.levelTarget - this.level) > 0.002 ||
      this.rim.some((v, i) => Math.abs(v - this.rimTarget[i]) > 0.5 * (i === 3 ? 0.01 : 1)) ||
      this.wave.value > 0.01 ||
      this.bars.value > 0.01;
    if (animating) this.raf = requestAnimationFrame(this.tick);
    else {
      for (const s of [this.w, this.h, this.r]) s.snap();
    }
  };

  private updatePull(now: number) {
    const W = this.w.value;
    const H = this.h.value;
    const L = this.centerX - W / 2;
    const R = this.centerX + W / 2;
    let goal = 0;
    const c = this.cursor;
    if (this.target.pull && c && !c.inside && W > 40) {
      const dy = c.y - H;
      const inX = c.x > L - 30 && c.x < R + 30;
      if (inX && dy > 0 && dy < REACH) {
        const k = dy / REACH;
        goal = dy * 0.82 * (1 - k * k);
      }
      const margin = Math.min(W / 2 - 1, this.r.value + 18);
      this.pullX.target = Math.max(L + margin, Math.min(R - margin, c.x));
    }
    // a droplet tears off when the cursor escapes a long stretch
    if (
      goal === 0 &&
      this.prevPullTarget > 24 &&
      this.pull.value > 26 &&
      c &&
      c.y > H + REACH * 0.75 &&
      now - this.lastDrop > 900
    ) {
      this.lastDrop = now;
      this.drops.push({ x: this.pullX.value, y: H + this.pull.value * 0.92, vy: 90, r: 3.2 + Math.random() * 1.6, life: 1 });
    }
    this.prevPullTarget = goal;
    this.pull.target = goal;
  }

  private stepDrops(dt: number) {
    for (const d of this.drops) {
      d.vy += 1300 * dt;
      d.y += d.vy * dt;
      d.life -= dt * 1.4;
    }
    this.drops = this.drops.filter((d) => d.life > 0 && d.y < this.ch + 20);
  }

  /** vertical offset of the bottom edge at x */
  private disp(x: number, L: number, W: number, rr: number): number {
    const inner = W - 2 * rr;
    if (inner <= 0) return 0;
    const u = (x - L - rr) / inner;
    if (u <= 0 || u >= 1) return 0;
    const env = Math.pow(Math.sin(Math.PI * u), 0.75);
    let d = 0;
    const p = this.pull.value;
    if (Math.abs(p) > 0.05) {
      const sigma = 26 + Math.abs(p) * 0.55;
      const dx = x - this.pullX.value;
      d += p * Math.exp(-(dx * dx) / (2 * sigma * sigma)) * env;
    }
    const wv = this.wave.value;
    if (wv > 0.001) {
      const amp = wv * (1.6 + this.level * 7.5);
      const k1 = (2 * Math.PI) / 40;
      const k2 = (2 * Math.PI) / 97;
      d += amp * env * (0.85 * Math.sin(k1 * (x - L) - this.t * 5.4) + 0.15 * Math.sin(k2 * (x - L) + this.t * 2.3));
    }
    return d;
  }

  private shapePath(ctx: CanvasRenderingContext2D, L: number, R: number, H: number, rr: number, f: number) {
    const W = R - L;
    ctx.beginPath();
    ctx.moveTo(L - f, 0);
    ctx.quadraticCurveTo(L, 0, L, f);
    ctx.lineTo(L, Math.max(f, H - rr));
    ctx.bezierCurveTo(L, H - rr + rr * K, L + rr - rr * K, H, L + rr, H);
    for (let x = L + rr + 3; x < R - rr; x += 3) ctx.lineTo(x, H + this.disp(x, L, W, rr));
    ctx.lineTo(R - rr, H);
    ctx.bezierCurveTo(R - rr + rr * K, H, R, H - rr + rr * K, R, Math.max(f, H - rr));
    ctx.lineTo(R, f);
    ctx.quadraticCurveTo(R, 0, R + f, 0);
    ctx.closePath();
  }

  private bottomPath(ctx: CanvasRenderingContext2D, L: number, R: number, H: number, rr: number, f: number, inset: number) {
    const W = R - L;
    const top = Math.max(f, H - rr);
    ctx.beginPath();
    ctx.moveTo(L + inset, top - inset);
    ctx.bezierCurveTo(L + inset, H - rr + rr * K - inset, L + rr - rr * K, H - inset, L + rr, H - inset);
    for (let x = L + rr + 3; x < R - rr; x += 3) ctx.lineTo(x, H + this.disp(x, L, W, rr) - inset);
    ctx.lineTo(R - rr, H - inset);
    ctx.bezierCurveTo(R - rr + rr * K, H - inset, R - inset, H - rr + rr * K - inset, R - inset, top - inset);
  }

  private draw() {
    const ctx = this.ctx;
    ctx.setTransform(this.dpr, 0, 0, this.dpr, 0, 0);
    ctx.clearRect(0, 0, this.cw, this.ch);
    const W = this.w.value;
    const H = this.h.value;
    if (W < 2 || H < 1) return;
    const L = this.centerX - W / 2;
    const R = this.centerX + W / 2;
    const f = Math.min(12, H * 0.45, W * 0.1);
    const rr = Math.max(0, Math.min(this.r.value, H - f, W / 2 - 1));

    // shape
    ctx.save();
    if (this.shadow.value > 0.01) {
      ctx.shadowColor = `rgba(0,0,0,${0.45 * this.shadow.value})`;
      ctx.shadowBlur = 28 * this.shadow.value;
      ctx.shadowOffsetY = 8 * this.shadow.value;
    }
    this.shapePath(ctx, L, R, H, rr, f);
    ctx.fillStyle = this.glass ? "rgba(0,0,0,0.22)" : "#000";
    ctx.fill();
    ctx.restore();

    if (this.glass) {
      ctx.save();
      this.shapePath(ctx, L, R, H, rr, f);
      ctx.clip();
      this.paintGlass(ctx, L, R, H);
      // thin light edge (inside the clip, so only the inner half shows)
      this.shapePath(ctx, L, R, H, rr, f);
      ctx.lineWidth = 2;
      ctx.strokeStyle = "rgba(255,255,255,0.22)";
      ctx.stroke();
      ctx.restore();
    }

    // rim along the bottom edge, inside the shape
    const [cr, cg, cb, ca] = this.rim;
    if (ca > 0.01) {
      ctx.save();
      this.shapePath(ctx, L, R, H, rr, f);
      ctx.clip();
      this.bottomPath(ctx, L, R, H, rr, f, 3.2);
      ctx.lineWidth = 2.6;
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.strokeStyle = `rgba(${cr | 0},${cg | 0},${cb | 0},${ca})`;
      ctx.stroke();
      ctx.restore();
    }

    // equalizer bars
    const bv = this.bars.value;
    if (bv > 0.01) {
      ctx.save();
      ctx.globalAlpha = Math.min(1, bv);
      ctx.strokeStyle = `rgb(${cr | 0},${cg | 0},${cb | 0})`;
      ctx.lineWidth = 3.4;
      ctx.lineCap = "round";
      const cy = Math.min(H / 2 - 1, 22);
      for (let i = 0; i < 4; i++) {
        const x = R - 18 - (3 - i) * 7.5;
        const hh = (3 + this.barLevels[i] * 9 + i * 0.8) * bv;
        ctx.beginPath();
        ctx.moveTo(x, cy - hh);
        ctx.lineTo(x, cy + hh);
        ctx.stroke();
      }
      ctx.restore();
    }

    // torn-off droplets
    for (const d of this.drops) {
      ctx.save();
      ctx.globalAlpha = Math.max(0, Math.min(1, d.life * 1.5));
      ctx.fillStyle = this.glass ? "rgba(30,30,36,0.55)" : "#000";
      ctx.beginPath();
      ctx.ellipse(d.x, d.y, d.r * 0.9, d.r * (1 + Math.min(0.5, d.vy / 1600)), 0, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "rgba(255,255,255,0.35)";
      ctx.beginPath();
      ctx.arc(d.x - d.r * 0.3, d.y - d.r * 0.35, d.r * 0.28, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
  }
}
