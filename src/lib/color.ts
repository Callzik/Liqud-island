// Picks a vivid accent colour from album art for the rim and equalizer.

export type RGB = [number, number, number];

export const DEFAULT_ACCENT: RGB = [224, 88, 127];

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return [h / 6, s, l];
}

function hslToRgb(h: number, s: number, l: number): RGB {
  const hue = (p: number, q: number, t: number) => {
    if (t < 0) t += 1;
    if (t > 1) t -= 1;
    if (t < 1 / 6) return p + (q - p) * 6 * t;
    if (t < 1 / 2) return q;
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6;
    return p;
  };
  if (s === 0) return [l * 255, l * 255, l * 255];
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return [hue(p, q, h + 1 / 3) * 255, hue(p, q, h) * 255, hue(p, q, h - 1 / 3) * 255];
}

const cache = new Map<string, RGB>();

export function accentFrom(url: string): Promise<RGB> {
  const hit = cache.get(url);
  if (hit) return Promise.resolve(hit);
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      try {
        const c = document.createElement("canvas");
        c.width = c.height = 24;
        const ctx = c.getContext("2d")!;
        ctx.drawImage(img, 0, 0, 24, 24);
        const px = ctx.getImageData(0, 0, 24, 24).data;
        // hue histogram weighted by saturation
        const bins = new Array(24).fill(0).map(() => ({ w: 0, r: 0, g: 0, b: 0 }));
        for (let i = 0; i < px.length; i += 4) {
          const [h, s, l] = rgbToHsl(px[i], px[i + 1], px[i + 2]);
          if (l < 0.12 || l > 0.92) continue;
          const w = s * s * (1 - Math.abs(l - 0.55));
          const bin = bins[Math.floor(h * 24) % 24];
          bin.w += w;
          bin.r += px[i] * w;
          bin.g += px[i + 1] * w;
          bin.b += px[i + 2] * w;
        }
        const best = bins.reduce((a, b) => (b.w > a.w ? b : a));
        if (best.w < 0.5) return resolve(DEFAULT_ACCENT);
        const [h, s, l] = rgbToHsl(best.r / best.w, best.g / best.w, best.b / best.w);
        const rgb = hslToRgb(h, Math.max(0.55, Math.min(0.9, s * 1.15)), Math.max(0.55, Math.min(0.68, l)));
        cache.set(url, rgb);
        // keys are whole cover data: URLs (tens of KB each) — keep only recent ones
        if (cache.size > 24) cache.delete(cache.keys().next().value!);
        resolve(rgb);
      } catch {
        resolve(DEFAULT_ACCENT);
      }
    };
    img.onerror = () => resolve(DEFAULT_ACCENT);
    img.src = url;
  });
}

export const css = (c: RGB, a = 1) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`;

/** "#rrggbb" bright enough for dark text on top. */
export function isLightColor(hex: string): boolean {
  const m = /^#?([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
  if (!m) return false;
  const [r, g, b] = [m[1], m[2], m[3]].map((x) => parseInt(x, 16) / 255);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.62;
}
