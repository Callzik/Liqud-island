// «Позвонить маме завтра в 9», «таблетки каждый день в 23:00», «через 20 минут выключить плиту».

export interface Parsed {
  at: number;
  title: string;
  repeat: "" | "daily";
}

const UNITS: [RegExp, number][] = [
  [/^с(ек|екунд[уы]?)?$/, 1e3],
  [/^м(ин|инут[уы]?|инуту)?$/, 60e3],
  [/^ч(ас|аса|асов)?$/, 3600e3],
  [/^д(ень|ня|ней)?$/, 86400e3],
];

const WORDS: Record<string, number> = { полчаса: 30, минуту: 1, час: 60, пару: 2, полторы: 1.5 };

function unitMs(u: string): number | null {
  for (const [re, ms] of UNITS) if (re.test(u)) return ms;
  return null;
}

function atTime(base: Date, h: number, m: number) {
  const d = new Date(base);
  d.setHours(h, m, 0, 0);
  return d;
}

export function parseReminder(input: string, now = new Date()): Parsed | null {
  let text = " " + input.trim() + " ";
  let at: Date | null = null;
  let repeat: Parsed["repeat"] = "";

  // «через 20 минут», «через полчаса», «через час»
  const rel = /\sчерез\s+(\d+(?:[.,]\d+)?|полчаса|час|минуту|пару|полторы)\s*([а-яё]*)\s/i.exec(text);
  if (rel) {
    const nStr = rel[1].toLowerCase();
    let n = WORDS[nStr] ?? parseFloat(nStr.replace(",", "."));
    let unit = rel[2].toLowerCase();
    let ms: number | null;
    if (nStr === "полчаса" || nStr === "час" || nStr === "минуту") {
      ms = 60e3;
      if (nStr === "минуту") n = 1;
      // the word after belongs to the title
      text = text.replace(rel[0], " " + unit + " ");
    } else {
      ms = unitMs(unit) ?? 60e3;
      text = text.replace(rel[0], " ");
    }
    at = new Date(now.getTime() + n * ms);
  }

  // «каждый день»
  if (/\sкаждый\s+день\s/i.test(text)) {
    repeat = "daily";
    text = text.replace(/\sкаждый\s+день\s/i, " ");
  }

  // day words
  let day = 0;
  const dayRe = /\s(сегодня|завтра|послезавтра)\s/i.exec(text);
  if (dayRe) {
    day = { сегодня: 0, завтра: 1, послезавтра: 2 }[dayRe[1].toLowerCase() as "сегодня"] ?? 0;
    text = text.replace(dayRe[0], " ");
  }

  // «в 9», «в 9:30», «в 23.00», «в 9 утра», «в 7 вечера»
  const tm = /\sв\s+(\d{1,2})(?:[:.](\d{2}))?(?:\s+(утра|дня|вечера|ночи))?\s/i.exec(text);
  if (tm && !at) {
    let h = parseInt(tm[1], 10);
    const m = tm[2] ? parseInt(tm[2], 10) : 0;
    const part = tm[3]?.toLowerCase();
    if ((part === "вечера" || part === "дня") && h < 12) h += 12;
    if (part === "ночи" && h === 12) h = 0;
    if (h > 23 || m > 59) return null;
    const base = new Date(now);
    base.setDate(base.getDate() + day);
    let d = atTime(base, h, m);
    if (!dayRe && d.getTime() <= now.getTime()) d = new Date(d.getTime() + 86400e3);
    at = d;
    text = text.replace(tm[0], " ");
  } else if (dayRe && !at) {
    const base = new Date(now);
    base.setDate(base.getDate() + day);
    at = atTime(base, 9, 0);
  }

  if (!at) return null;
  const title = text.replace(/\s+/g, " ").trim().replace(/^(напомни(ть)?|напоминание)\s+/i, "");
  return { at: at.getTime(), title: title ? title[0].toUpperCase() + title.slice(1) : "Напоминание", repeat };
}

export function whenLabel(at: number, now = Date.now()) {
  const d = new Date(at);
  const t = d.toLocaleTimeString("ru-RU", { hour: "2-digit", minute: "2-digit" });
  const day0 = new Date(now);
  day0.setHours(0, 0, 0, 0);
  const diff = Math.floor((at - day0.getTime()) / 86400e3);
  if (diff === 0) return `сегодня в ${t}`;
  if (diff === 1) return `завтра в ${t}`;
  return `${d.toLocaleDateString("ru-RU", { day: "numeric", month: "short" })} в ${t}`;
}
