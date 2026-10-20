// The island lights up when an agent needs you: who, what for, the question.

import type { AgentSession } from "../api";
import { agentName, place, SOURCES } from "../lib/agents";
import { ClaudeMark, CodexMark } from "./Icons";

export function AgentPill({ s, more }: { s: AgentSession; more: number }) {
  const src = SOURCES[s.source];
  const what =
    s.status === "question" ? "Нужен ответ" : s.status === "permission" ? "Нужно разрешение" : "Ждёт вас";
  const line = s.request?.kind === "question" ? `Спрашивает: ${s.request.summary}` : s.request?.detail || s.action;
  return (
    <div className="agent-pill">
      <div className="agent-pill-icon" style={{ color: src.color }}>
        {s.source === "claude" ? <ClaudeMark size={26} color={src.color} /> : <CodexMark size={22} color={src.color} />}
      </div>
      <div className="agent-pill-text">
        <div className="agent-pill-title">
          <span style={{ color: src.color }}>{src.name}</span> {what}
          {more > 0 && <i>+{more}</i>}
        </div>
        <div className="agent-pill-who">
          {agentName(s)} · {place(s).where}
        </div>
        <div className="agent-pill-line">{line.replace(/^\$ /, "")}</div>
      </div>
    </div>
  );
}
