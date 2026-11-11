import { useEffect, useState } from "react";
import { call, on } from "../api";
import { parseReminder, whenLabel } from "../lib/reminders";
import { IClose, IPlay } from "./Icons";

export interface Timer {
  id: string;
  title: string;
  atMs: number;
  kind: "timer" | "reminder";
  repeat: "" | "daily";
  durationMs: number;
}

const PRESETS = [5, 10, 15, 25, 45];
const newId = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 6);

export function startTimer(min: number) {
  const ms = Math.round(min * 60e3);
  return call<Timer[]>("timers_add", {
    timer: { id: "timer", title: "Таймер", atMs: Date.now() + ms, kind: "timer", repeat: "", durationMs: ms },
  });
}

const mmss = (ms: number) => {
  const s = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
};

/** Timer ring with presets + reminders written in plain words. */
export function TimerPanel({ setBusy }: { setBusy: (b: boolean) => void }) {
  const [list, setList] = useState<Timer[]>([]);
  const [pick, setPick] = useState(5);
  const [custom, setCustom] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    call<Timer[]>("timers_list").then(setList).catch(() => {});
    const un = on<Timer[]>("timers", setList);
    return () => {
      un.then((u) => u());
    };
  }, []);

  const timer = list.find((t) => t.kind === "timer");
  useEffect(() => {
    if (!timer) return;
    const id = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(id);
  }, [timer]);

  const left = timer ? timer.atMs - now : pick * 60e3;
  const total = timer ? timer.durationMs : pick * 60e3;
  const frac = timer ? Math.max(0, Math.min(1, left / total)) : 1;
  const R = 30;
  const C = 2 * Math.PI * R;

  const reminders = list.filter((t) => t.kind === "reminder");
  const parsed = text.trim() ? parseReminder(text) : null;

  const addReminder = async () => {
    if (!parsed) return;
    setList(await call<Timer[]>("timers_add", { timer: { id: newId(), title: parsed.title, atMs: parsed.at, kind: "reminder", repeat: parsed.repeat, durationMs: 0 } }));
    setText("");
  };

  return (
    <div className="card tm">
      <div className="tm-top">
        <svg className="tm-ring" viewBox="0 0 72 72">
          <circle cx="36" cy="36" r={R} className="track" />
          <circle cx="36" cy="36" r={R} className="fill" strokeDasharray={`${C * frac} ${C}`} transform="rotate(-90 36 36)" />
          <text x="36" y="41" textAnchor="middle">
            {mmss(left)}
          </text>
        </svg>
        <div className="tm-btns">
          {timer ? (
            <button className="tm-start stop" onClick={async () => setList(await call<Timer[]>("timers_remove", { id: timer.id }))}>
              Стоп
            </button>
          ) : (
            <button className="tm-start" onClick={async () => setList(await startTimer(pick))}>
              <IPlay size={13} /> Старт
            </button>
          )}
          {custom === null ? (
            <button className="tm-custom" onClick={() => setCustom("")}>
              ✎ Своё время
            </button>
          ) : (
            <form
              className="tm-custom-form"
              onSubmit={async (e) => {
                e.preventDefault();
                const v = parseFloat(custom.replace(",", "."));
                if (v > 0) {
                  setPick(v);
                  setList(await startTimer(v));
                }
                setCustom(null);
                setBusy(false);
              }}
            >
              <input autoFocus value={custom} placeholder="мин" onFocus={() => setBusy(true)} onBlur={() => { setBusy(false); setCustom(null); }} onChange={(e) => setCustom(e.target.value)} />
            </form>
          )}
        </div>
      </div>
      <div className="tm-presets">
        {PRESETS.map((m) => (
          <button key={m} className={!timer && pick === m ? "on" : ""} onClick={() => setPick(m)} disabled={!!timer}>
            {m}
          </button>
        ))}
        <span>мин</span>
      </div>

      <div className="tm-sub">🔔 Напоминания</div>
      <div className="tm-list">
        {reminders.length === 0 && <div className="tm-empty">Напиши ниже, о чём и когда: «позвонить маме завтра в 9», «таблетки каждый день в 23:00», «через 20 минут выключить плиту»</div>}
        {reminders.map((r) => (
          <div className="tm-item" key={r.id}>
            <div>
              <b>{r.title}</b>
              <small>
                {whenLabel(r.atMs)}
                {r.repeat === "daily" ? " · каждый день" : ""}
              </small>
            </div>
            <button onClick={async () => setList(await call<Timer[]>("timers_remove", { id: r.id }))} title="Удалить">
              <IClose size={12} />
            </button>
          </div>
        ))}
      </div>
      <form
        className="tm-input"
        onSubmit={(e) => {
          e.preventDefault();
          addReminder();
        }}
      >
        <input value={text} placeholder="Позвонить маме завтра в 9" onFocus={() => setBusy(true)} onBlur={() => setBusy(false)} onChange={(e) => setText(e.target.value)} />
        {text.trim() && <small className={parsed ? "" : "bad"}>{parsed ? `${whenLabel(parsed.at)}${parsed.repeat ? ", каждый день" : ""} · Enter` : "Не понял когда"}</small>}
      </form>
    </div>
  );
}
