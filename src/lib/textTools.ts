// Text transforms for the «Текст» tab.

export const upper = (s: string) => s.toLocaleUpperCase("ru-RU");
export const lower = (s: string) => s.toLocaleLowerCase("ru-RU");

/** "ПРИВЕТ. как ДЕЛА?" → "Привет. Как дела?" */
export function sentence(s: string): string {
  let out = "";
  let cap = true;
  for (const ch of lower(s)) {
    if (cap && /\p{L}/u.test(ch)) {
      out += ch.toLocaleUpperCase("ru-RU");
      cap = false;
    } else out += ch;
    if (/[.!?…]/.test(ch) || ch === "\n") cap = true;
  }
  return out;
}

const TR: Record<string, string> = {
  а: "a", б: "b", в: "v", г: "g", д: "d", е: "e", ё: "yo", ж: "zh", з: "z", и: "i", й: "y", к: "k", л: "l", м: "m",
  н: "n", о: "o", п: "p", р: "r", с: "s", т: "t", у: "u", ф: "f", х: "kh", ц: "ts", ч: "ch", ш: "sh", щ: "shch",
  ъ: "", ы: "y", ь: "", э: "e", ю: "yu", я: "ya",
};

/** Кириллица → латиница ("Щёлково" → "Shchyolkovo"). */
export function translit(s: string): string {
  let out = "";
  const chars = [...s];
  chars.forEach((ch, i) => {
    const low = ch.toLowerCase();
    const t = TR[low];
    if (t === undefined) {
      out += ch;
      return;
    }
    if (ch === low || !t) out += t;
    else {
      // ALL CAPS word → ALL CAPS translit, Capitalised → Capitalised
      const next = chars[i + 1];
      const wordUpper = next && next !== next.toLowerCase();
      out += wordUpper ? t.toUpperCase() : t.charAt(0).toUpperCase() + t.slice(1);
    }
  });
  return out;
}

const EN = "`qwertyuiop[]asdfghjkl;'zxcvbnm,./~QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?@#$^&";
const RU = "ёйцукенгшщзхъфывапролджэячсмитьбю.ЁЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭЯЧСМИТЬБЮ,\"№;:?";

function layoutMap(from: string, to: string) {
  const map = new Map<string, string>();
  for (let i = 0; i < from.length; i++) if (!map.has(from[i])) map.set(from[i], to[i]);
  return map;
}
const EN_RU = layoutMap(EN, RU);
const RU_EN = layoutMap(RU, EN);
const conv = (w: string, map: Map<string, string>) => [...w].map((ch) => map.get(ch) ?? ch).join("");

/**
 * "Ghbdtn" → "Привет", "руддщ" → "hello". When the text mixes both layouts,
 * only the words typed in the minority layout are switched ("ghbdtn, как дела" → "привет, как дела").
 */
export function fixLayout(s: string): string {
  const lat = (s.match(/[a-z]/gi) ?? []).length;
  const cyr = (s.match(/[а-яё]/gi) ?? []).length;
  if (!lat || !cyr) return conv(s, lat ? EN_RU : RU_EN);
  const toRu = cyr >= lat;
  return s.replace(/\S+/g, (w) => {
    const wl = /[a-z]/i.test(w);
    const wc = /[а-яё]/i.test(w);
    if (toRu && wl && !wc) {
      // "ghbdtn, как дела" → "привет, как дела": in mixed text a trailing comma was
      // typed after switching layouts. ("?" stays converted — it is «,» on the
      // Russian layout; "." too — words ending in «ю» are common.)
      const m = /^(.*[a-z].*?)(,+)$/i.exec(w);
      return m ? conv(m[1], EN_RU) + m[2] : conv(w, EN_RU);
    }
    if (!toRu && wc && !wl) return conv(w, RU_EN);
    return w;
  });
}

/** Collapses repeated spaces, trims lines, keeps at most one empty line. */
export function tidySpaces(s: string): string {
  return s
    .replace(/[ \t ]+/g, " ")
    .split("\n")
    .map((l) => l.trim())
    .join("\n")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}

/** Joins hard-wrapped lines into paragraphs ("сло-\nво" → "слово"). */
export function joinLines(s: string): string {
  return tidySpaces(s)
    .split(/\n{2,}/)
    .map((p) => p.replace(/(\p{L})-\n(\p{Ll})/gu, "$1$2").replace(/\n/g, " "))
    .join("\n\n");
}

export function counts(s: string) {
  return {
    chars: [...s].length,
    noSpaces: [...s.replace(/\s/g, "")].length,
    words: (s.match(/[\p{L}\p{N}][\p{L}\p{N}'’-]*/gu) ?? []).length,
    lines: s ? s.split("\n").length : 0,
  };
}

export function jsonPretty(s: string): string {
  try {
    return JSON.stringify(JSON.parse(s), null, 2);
  } catch {
    return s;
  }
}

export function jsonLine(s: string): string {
  try {
    return JSON.stringify(JSON.parse(s));
  } catch {
    return s;
  }
}

export function toBase64(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let bin = "";
  bytes.forEach((b) => (bin += String.fromCharCode(b)));
  return btoa(bin);
}

export function fromBase64(s: string): string {
  try {
    const bin = atob(s.trim());
    return new TextDecoder().decode(Uint8Array.from(bin, (c) => c.charCodeAt(0)));
  } catch {
    return s;
  }
}

export const capitalizeWords = (s: string) => s.toLocaleLowerCase("ru-RU").replace(/(^|[\s\-«"(])(\p{L})/gu, (_, a: string, b: string) => a + b.toLocaleUpperCase("ru-RU"));
