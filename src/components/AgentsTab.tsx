// «Агенты»: Claude Code / Codex sessions — who works, who waits for you.
// Waiting ones are on top; a permission or a question is answered right here.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { call, type AgentQuestion, type AgentSession, type AgentsPayload, type HookStatus } from "../api";
import { agentName, badge, elapsed, needsYou, isBusy, SOURCES } from "../lib/agents";
import { ClaudeMark, CodexMark, IBack, ICheck, IHand, ITerminal } from "./Icons";
import type { ToastData } from "./Toast";

export function SourceTag({ s }: { s: AgentSession }) {
  const src = SOURCES[s.source];
  return (
    <span className="ag-src" style={{ color: src.color }}>
      {s.source === "claude" ? <ClaudeMark size={13} color={src.color} /> : <CodexMark size={12} color={src.color} />}
      {src.name}
    </span>
  );
}

function ContextBar({ s, wide }: { s: AgentSession; wide?: boolean }) {
  const pct = Math.round(s.context * 100);
  const color = pct >= 80 ? "#ff6b5e" : SOURCES[s.source].color;
  return (
    <span className={`ag-ctx ${wide ? "wide" : ""}`}>
      <i>
        <b style={{ width: `${Math.max(4, pct)}%`, background: color }} />
      </i>
      <em style={pct >= 80 ? { color } : undefined}>{wide ? `контекст ${pct}%` : `${pct}%`}</em>
    </span>
  );
}

function Dot({ s }: { s: AgentSession }) {
  const color = SOURCES[s.source].color;
  if (needsYou(s)) return <span className="ag-dot ring" style={{ color }} />;
  return <span className={`ag-dot ${isBusy(s) ? "busy" : "rest"}`} style={{ background: isBusy(s) ? color : undefined }} />;
}

/** Ticks once a second for the timers. */
function useNow(ms = 1000) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(t);
  }, [ms]);
  return now;
}

export function headline(list: AgentSession[]): { icon: boolean; text: string } {
  const waiting = list.filter(needsYou).length;
  if (waiting) return { icon: true, text: `Нужны вы: ${waiting}` };
  const busy = list.filter(isBusy).length;
  if (busy) return { icon: false, text: `Работают: ${busy}` };
  return { icon: false, text: list.length ? `Отдыхают: ${list.length}` : "" };
}

export function AgentsTab({
  agents,
  selected,
  setSelected,
  toast,
  setBusy,
  openSettings,
}: {
  agents: AgentsPayload;
  selected: string | null;
  setSelected: (id: string | null) => void;
  toast: (t: ToastData, ms?: number) => void;
  setBusy: (b: boolean) => void;
  openSettings: () => void;
}) {
  const list = agents.sessions;
  const now = useNow();
  const [hooks, setHooks] = useState<HookStatus | null>(null);
  const cur = selected ? list.find((s) => s.id === selected) ?? null : null;
  const head = headline(list);

  useEffect(() => {
    call<HookStatus>("agents_hooks", { action: "status" }).then(setHooks).catch(() => {});
  }, []);

  // a session that went away while open: back to the list
  useEffect(() => {
    if (selected && !cur) {
      const t = window.setTimeout(() => setSelected(null), 2600);
      return () => window.clearTimeout(t);
    }
  }, [selected, cur, setSelected]);

  const connect = async () => {
    try {
      const st = await call<HookStatus>("agents_hooks", { action: "install" });
      setHooks(st);
      toast({ icon: "check", title: "Claude Code подключён", subtitle: "Новые события появятся здесь" });
    } catch (e) {
      toast({ icon: "error", tone: "error", title: "Не подключилось", subtitle: String(e) }, 4000);
    }
  };

  return (
    <div className="agents">
      <div className="ag-head">
        {cur ? (
          <button className="text-btn ag-back" onClick={() => setSelected(null)}>
            <IBack size={16} /> Все сессии
          </button>
        ) : (
          <span className="ag-title">Агенты</span>
        )}
        <span className="ag-sum">
          {head.icon && <IHand size={15} />}
          {head.text}
        </span>
      </div>

      {cur ? (
        <Detail s={cur} now={now} setBusy={setBusy} />
      ) : selected ? (
        <div className="ag-empty">
          <ICheck size={22} />
          <div>Сессия закончилась</div>
        </div>
      ) : list.length === 0 ? (
        <div className="ag-empty">
          <ITerminal size={26} />
          {hooks && !hooks.installed ? (
            <>
              <div className="ag-empty-title">Подключите Claude Code</div>
              <div className="hint">
                Остров покажет все сессии: кто пишет код, кто думает, кто ждёт вас. Разрешения и вопросы — прямо отсюда.
                Добавит хуки в {hooks.settingsPath || "~/.claude/settings.json"} (копия старого файла сохранится рядом).
              </div>
              <button className="ag-btn primary" onClick={connect}>
                Подключить
              </button>
            </>
          ) : (
            <>
              <div className="ag-empty-title">Пока никто не работает</div>
              <div className="hint">
                Запустите <code>claude</code> или <code>codex</code> в терминале — сессия появится здесь. Двойной Ctrl — офис.
              </div>
            </>
          )}
        </div>
      ) : (
        <>
          <div className="ag-list">
            {list.map((s) => (
              <button key={s.id} className={`ag-row ${needsYou(s) ? "hot" : ""}`} onClick={() => setSelected(s.id)}>
                <Dot s={s} />
                <span className="ag-main">
                  <span className="ag-name">
                    <b>{agentName(s)}</b>
                    {badge(s) && <i className="ag-badge">{badge(s)}</i>}
                  </span>
                  <span className="ag-line">
                    {s.project && <span>{s.project}</span>}
                    <SourceTag s={s} />
                    <span className="ag-act">· {s.action}</span>
                  </span>
                </span>
                <ContextBar s={s} />
                <span className="ag-time">{elapsed(s.startedMs, now)}</span>
              </button>
            ))}
          </div>
          <div className="ag-foot">
            <span>
              Всего: {list.length}
              {list.length > 4 ? " · прокрутите список" : ""}
            </span>
            {hooks && (!hooks.installed || hooks.stale) && (
              <button className="text-btn" onClick={connect}>
                {hooks.stale ? "Обновить хуки Claude Code" : "Подключить Claude Code"}
              </button>
            )}
            {hooks?.installed && !hooks.stale && (
              <button className="text-btn" onClick={openSettings}>
                Настройки
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

function Countdown({ startedMs, timeoutMs, now }: { startedMs: number; timeoutMs: number; now: number }) {
  const left = Math.max(0, startedMs + timeoutMs - now);
  return (
    <div className="ag-count">
      <i>
        <b style={{ width: `${(left / timeoutMs) * 100}%` }} />
      </i>
      <span>{elapsed(now - left, now)}</span>
    </div>
  );
}

function Detail({ s, now, setBusy }: { s: AgentSession; now: number; setBusy: (b: boolean) => void }) {
  const [sent, setSent] = useState<string | null>(null);
  const req = s.request;
  const lastReq = useRef<string | null>(req?.id ?? null);

  useEffect(() => {
    if (req && req.id !== lastReq.current) {
      lastReq.current = req.id;
      setSent(null);
    }
  }, [req]);

  const answer = (decision: string, text?: string, done?: string) => {
    if (!req) return;
    call("agents_answer", { session: s.id, request: req.id, decision, text: text ?? null })
      .then(() => setSent(done ?? null))
      .catch(() => setSent("Вопрос уже закрыт"));
  };

  return (
    <div className="ag-detail">
      <div className="ag-dtop">
        <Dot s={s} />
        <b className="ag-dname">{agentName(s)}</b>
        {badge(s) && <i className="ag-badge big">{badge(s)}</i>}
        <span className="ag-time">{elapsed(s.startedMs, now)}</span>
      </div>
      <div className="ag-dmeta">
        <SourceTag s={s} />
        {s.project && <span className="ag-proj">{s.project}</span>}
        <ContextBar s={s} wide />
        <span className="ag-acts">{s.actions} действий</span>
      </div>

      {sent ? (
        <div className="ag-sent">
          <ICheck size={16} /> {sent}
        </div>
      ) : req?.kind === "permission" ? (
        <>
          <div className="ag-ask">
            <span className="ag-ask-icon">
              <ITerminal size={18} />
            </span>
            <span>
              <span className="ag-ask-title">{req.title}</span>
              <b>{req.summary || req.tool}</b>
            </span>
          </div>
          {req.detail && <pre className="ag-code">{req.detail.startsWith("$ ") ? <><em>$</em>{req.detail.slice(1)}</> : req.detail}</pre>}
          <Countdown startedMs={req.startedMs} timeoutMs={req.timeoutMs} now={now} />
          <PermissionKeys onAllow={() => answer("allow", undefined, "Разрешено — Claude продолжает")} />
          <div className="ag-actions">
            <span className="hint">Enter — разрешить</span>
            <button className="ag-btn" onClick={() => answer("chat", undefined, "Ответьте в терминале")}>
              В терминале
            </button>
            <button className="ag-btn" onClick={() => answer("deny", undefined, "Отказано — Claude ищет другой путь")}>
              Запретить
            </button>
            <button className="ag-btn primary" onClick={() => answer("allow", undefined, "Разрешено — Claude продолжает")}>
              Разрешить ⏎
            </button>
          </div>
        </>
      ) : req?.kind === "question" ? (
        <Questions
          key={req.id}
          questions={req.questions}
          countdown={<Countdown startedMs={req.startedMs} timeoutMs={req.timeoutMs} now={now} />}
          setBusy={setBusy}
          onAnswer={(text) => answer("answer", text, "Ответ отправлен — Claude продолжает")}
          onChat={() => answer("chat", undefined, "Ответьте в терминале")}
        />
      ) : (
        <>
          <div className="ag-ask">
            <span className="ag-ask-icon">
              <ITerminal size={18} />
            </span>
            <span>
              <span className="ag-ask-title">{needsYou(s) ? "Ждёт вас" : isBusy(s) ? "Сейчас" : "Последнее"}</span>
              <b>{s.action}</b>
            </span>
          </div>
          <div className="ag-actions">
            <span className="hint">{s.cwd}</span>
            <button className="ag-btn primary" onClick={() => call("agents_focus", { session: s.id }).catch(() => {})}>
              Открыть терминал
            </button>
          </div>
        </>
      )}
    </div>
  );
}

/** Enter allows (when nothing else has focus). */
function PermissionKeys({ onAllow }: { onAllow: () => void }) {
  const fn = useRef(onAllow);
  fn.current = onAllow;
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || e.repeat) return;
      if (document.activeElement instanceof HTMLInputElement || document.activeElement instanceof HTMLTextAreaElement) return;
      e.preventDefault();
      fn.current();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);
  return null;
}

function Questions({
  questions,
  countdown,
  setBusy,
  onAnswer,
  onChat,
}: {
  questions: AgentQuestion[];
  countdown: ReactNode;
  setBusy: (b: boolean) => void;
  onAnswer: (text: string) => void;
  onChat: () => void;
}) {
  const [step, setStep] = useState(0);
  // per question: chosen labels; "other" text separately
  const [picked, setPicked] = useState<string[][]>(() => questions.map(() => []));
  const [other, setOther] = useState<string[]>(() => questions.map(() => ""));
  const [otherOn, setOtherOn] = useState<boolean[]>(() => questions.map(() => false));
  const q = questions[step];
  const last = step === questions.length - 1;

  const chosen = (i: number) => (otherOn[i] && other[i].trim() ? [...picked[i], other[i].trim()] : picked[i]);
  const ready = chosen(step).length > 0;

  const finish = (pk = picked) => {
    const lines = questions.map((qq, i) => {
      const a = otherOn[i] && other[i].trim() ? [...pk[i], other[i].trim()] : pk[i];
      return `${qq.question} → ${a.join(", ") || "(без ответа)"}`;
    });
    onAnswer(lines.join("\n"));
  };

  const next = (pk = picked) => {
    if (last) finish(pk);
    else setStep(step + 1);
  };

  const pick = (label: string) => {
    const cur = picked[step];
    const nextPk = picked.map((p, i) =>
      i !== step ? p : q.multiSelect ? (cur.includes(label) ? cur.filter((x) => x !== label) : [...cur, label]) : [label],
    );
    setPicked(nextPk);
    if (!q.multiSelect) setOtherOn(otherOn.map((o, i) => (i === step ? false : o)));
    return nextPk;
  };

  const ref = useRef({ pick, next, ready, q, step });
  ref.current = { pick, next, ready, q, step };
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (document.activeElement instanceof HTMLInputElement) return;
      const r = ref.current;
      const n = Number(e.key);
      if (n >= 1 && n <= r.q.options.length) {
        e.preventDefault();
        r.pick(r.q.options[n - 1].label);
      } else if (n === r.q.options.length + 1) {
        e.preventDefault();
        setOtherOn((o) => o.map((v, i) => (i === r.step ? true : v)));
      } else if (e.key === "Enter" && r.ready) {
        e.preventDefault();
        r.next();
      }
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, []);

  if (!q) return null;
  return (
    <>
      <div className="ag-qhead">
        <span className="ag-asks">● Claude спрашивает{questions.length > 1 ? ` · ${step + 1} из ${questions.length}` : ""}</span>
        <b>{q.question}</b>
      </div>
      <div className="ag-opts">
        {q.options.map((o, i) => {
          const on = picked[step].includes(o.label);
          return (
            <button key={o.label} className={`ag-opt ${on ? "on" : ""}`} onClick={() => pick(o.label)}>
              <i>{i + 1}</i>
              <span>
                <b>{o.label}</b>
                {o.description && <small>{o.description}</small>}
              </span>
              {on && (
                <span className="ag-tick">
                  <ICheck size={12} />
                </span>
              )}
            </button>
          );
        })}
        <div className={`ag-opt other ${otherOn[step] ? "on" : ""}`} onClick={() => {
            if (otherOn[step]) return;
            setOtherOn(otherOn.map((v, i) => (i === step ? true : v)));
            if (!q.multiSelect) setPicked(picked.map((p, i) => (i === step ? [] : p)));
          }}
        >
          <i>{q.options.length + 1}</i>
          {otherOn[step] ? (
            <input
              autoFocus
              value={other[step]}
              placeholder="Свой ответ…"
              onFocus={() => setBusy(true)}
              onBlur={() => setBusy(false)}
              onChange={(e) => setOther(other.map((v, i) => (i === step ? e.target.value : v)))}
              onKeyDown={(e) => {
                if (e.key === "Enter" && other[step].trim()) {
                  e.preventDefault();
                  if (!q.multiSelect) setPicked(picked.map((p, i) => (i === step ? [] : p)));
                  next(q.multiSelect ? picked : picked.map((p, i) => (i === step ? [] : p)));
                }
              }}
            />
          ) : (
            <span>
              <b>Другое…</b>
            </span>
          )}
        </div>
      </div>
      {countdown}
      <div className="ag-actions">
        <span className="hint">Цифра — выбрать</span>
        <button className="ag-btn" onClick={onChat}>
          В терминале
        </button>
        <button className="ag-btn primary" disabled={!ready} onClick={() => next()}>
          {last ? "Ответить ⏎" : "Дальше ⏎"}
        </button>
      </div>
    </>
  );
}
