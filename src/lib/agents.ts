// Helpers shared by the agents tab, the office and the island pill.

import type { AgentSession } from "../api";

export const SOURCES = {
  claude: { name: "Claude Code", color: "#e07b53", short: "Claude" },
  codex: { name: "Codex", color: "#8f94ff", short: "Codex" },
} as const;

export const needsYou = (s: AgentSession) => s.status === "permission" || s.status === "question" || s.status === "waiting";
export const isBusy = (s: AgentSession) => s.status === "working" || s.status === "thinking";

export const agentName = (s: AgentSession) => s.title || s.project || SOURCES[s.source].short;

/** Badge next to the name (only for those who wait). */
export function badge(s: AgentSession): string | null {
  switch (s.status) {
    case "permission":
      return "нужно разрешение";
    case "question":
      return "нужен ответ";
    case "waiting":
      return "ждёт вас";
    default:
      return null;
  }
}

/** Short "what is he doing" for the office labels. */
export function doing(s: AgentSession): string {
  if (needsYou(s)) return "нужны вы";
  if (s.status === "done" || s.status === "idle") return "отдыхает";
  const a = s.action;
  if (a.startsWith("Пишет") || a.startsWith("Правит")) return "пишет код";
  if (a.startsWith("Запускает тесты")) return "гоняет тесты";
  if (a.startsWith("Запускает")) return "запускает команду";
  if (a.startsWith("Ищет по коду")) return "ищет по коду";
  if (a.startsWith("Ищет в интернете")) return "гуглит";
  if (a.startsWith("Читает")) return "читает код";
  if (a.startsWith("Думает") || s.status === "thinking") return "думает";
  return a.length > 22 ? a.slice(0, 21).toLowerCase() + "…" : a.toLowerCase();
}

/** 31:17 / 2:46:57 */
export function elapsed(fromMs: number, now = Date.now()): string {
  const t = Math.max(0, Math.floor((now - fromMs) / 1000));
  const h = Math.floor(t / 3600);
  const m = Math.floor((t % 3600) / 60);
  const s = t % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

/** Stable small number from a session id (looks, desk order). */
export function hash(id: string): number {
  let h = 2166136261;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}
