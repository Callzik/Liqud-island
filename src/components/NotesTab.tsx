import { useEffect, useMemo, useRef, useState } from "react";
import { call } from "../api";
import { ago } from "../lib/format";
import { IPin, IPlus, ISearch, ITrash } from "./Icons";
import { TimerPanel } from "./TimerPanel";

interface Note {
  id: string;
  text: string;
  pinned: boolean;
  updated: number;
}

const newId = () => Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
const titleOf = (n: Note) => n.text.trim().split("\n")[0].slice(0, 80) || "Новая заметка";
const previewOf = (n: Note) => n.text.trim().split("\n").slice(1).join(" ").trim().slice(0, 90);

/** «Заметки»: list, search, pin; saved to notes.json automatically. */
export function NotesTab({ setBusy }: { setBusy: (b: boolean) => void }) {
  const [notes, setNotes] = useState<Note[] | null>(null);
  const [sel, setSel] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const saveT = useRef(0);
  const pending = useRef<Note[] | null>(null);
  const editorRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    call<Note[]>("notes_load")
      .catch(() => [] as Note[])
      .then((n) => {
        const list = Array.isArray(n) ? n : [];
        setNotes(list);
        setSel(list[0]?.id ?? null);
      });
    // flush a pending save when the tab closes
    return () => {
      window.clearTimeout(saveT.current);
      if (pending.current) call("notes_save", { notes: pending.current });
    };
  }, []);

  const persist = (next: Note[]) => {
    setNotes(next);
    pending.current = next;
    window.clearTimeout(saveT.current);
    saveT.current = window.setTimeout(() => {
      call("notes_save", { notes: next });
      pending.current = null;
    }, 400);
  };

  const sorted = useMemo(() => {
    const query = q.trim().toLowerCase();
    return (notes ?? [])
      .filter((n) => !query || n.text.toLowerCase().includes(query))
      .sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.updated - a.updated);
  }, [notes, q]);

  const current = notes?.find((n) => n.id === sel) ?? null;

  const add = () => {
    const n: Note = { id: newId(), text: "", pinned: false, updated: Date.now() };
    persist([n, ...(notes ?? [])]);
    setSel(n.id);
    setQ("");
    window.setTimeout(() => editorRef.current?.focus(), 30);
  };
  const update = (patch: Partial<Note>) => {
    if (!current || !notes) return;
    persist(notes.map((n) => (n.id === current.id ? { ...n, ...patch, updated: patch.pinned === undefined ? Date.now() : n.updated } : n)));
  };
  const remove = () => {
    if (!current || !notes) return;
    const rest = notes.filter((n) => n.id !== current.id);
    persist(rest);
    setSel(rest[0]?.id ?? null);
  };

  if (notes === null) return <div className="notes" />;


  return (
    <div className="notes">
      <div className="notes-side">
        <div className="notes-search">
          <ISearch size={15} />
          <input
            value={q}
            placeholder="Поиск"
            spellCheck={false}
            onFocus={() => setBusy(true)}
            onBlur={() => setBusy(false)}
            onChange={(e) => setQ(e.target.value)}
          />
          <button className="icon-btn small" onClick={add} title="Новая заметка">
            <IPlus size={16} />
          </button>
        </div>
        <div className="notes-list">
          {sorted.length === 0 && <div className="hint notes-hint">{notes.length ? "Ничего не нашлось" : "Заметок пока нет"}</div>}
          {sorted.map((n) => (
            <button key={n.id} className={`note-item ${n.id === sel ? "sel" : ""}`} onClick={() => setSel(n.id)}>
              <span className="note-title">
                {n.pinned && <IPin size={12} />}
                {titleOf(n)}
              </span>
              <span className="note-sub">
                {ago(n.updated)} {previewOf(n) && `· ${previewOf(n)}`}
              </span>
            </button>
          ))}
        </div>
      </div>
      <div className="card notes-editor">
        {current ? (
          <>
            <div className="notes-head">
              <span className="muted">Изменено {ago(current.updated)}</span>
              <div className="head-actions">
                <button className={`icon-btn small ${current.pinned ? "on" : ""}`} onClick={() => update({ pinned: !current.pinned })} title={current.pinned ? "Открепить" : "Закрепить"}>
                  <IPin size={15} />
                </button>
                <button className="icon-btn small danger" onClick={remove} title="Удалить">
                  <ITrash size={15} />
                </button>
              </div>
            </div>
            <textarea
              ref={editorRef}
              value={current.text}
              placeholder="Первая строка станет заголовком"
              spellCheck={false}
              onFocus={() => setBusy(true)}
              onBlur={() => setBusy(false)}
              onChange={(e) => update({ text: e.target.value })}
            />
          </>
        ) : (
          <div className="chat-empty">
            <div className="chat-empty-title">Нет заметки</div>
            <button className="text-btn" onClick={add}>
              <IPlus size={15} /> Создать
            </button>
          </div>
        )}
      </div>
      <TimerPanel setBusy={setBusy} />
    </div>
  );
}
