import { memo, useCallback, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";
import { call, type Settings } from "../api";
import { chat, type ChatAttachment, type ChatMsg } from "../lib/chatStore";
import { Markdown } from "../lib/markdown";
import { IChat, IClose, IFile, IImage, IPaperclip, IPlus, ISend, IStop } from "./Icons";
import type { ToastData } from "./Toast";

export interface ChatProps {
  settings: Settings;
  toast: (t: ToastData, ms?: number) => void;
  setBusy: (b: boolean) => void;
  /** question sent from the launcher ("Спросить ИИ") */
  ask: { text: string; n: number } | null;
  onAsked: () => void;
  /** App routes files dropped on the panel here while this tab is open */
  setDropHandler: (fn: ((paths: string[]) => void) | null) => void;
  openSettings: () => void;
}

function Chip({ a, onRemove }: { a: ChatAttachment; onRemove?: () => void }) {
  return (
    <span className="chat-chip" title={a.name}>
      {a.kind === "image" && a.dataUrl ? <img src={a.dataUrl} alt="" /> : a.kind === "image" ? <IImage size={14} /> : <IFile size={14} />}
      <span className="chat-chip-name">{a.name}</span>
      <span className="chat-chip-label">· {a.label}</span>
      {onRemove && (
        <button onClick={onRemove} title="Убрать">
          <IClose size={11} />
        </button>
      )}
    </span>
  );
}

/** One message. Memoised: while a reply streams only that message re-renders,
 *  not the markdown of the whole dialogue. */
const Msg = memo(function Msg({ m, canRetry, onCopy }: { m: ChatMsg; canRetry: boolean; onCopy: (code: string) => void }) {
  if (m.role === "user")
    return (
      <div className="msg msg-user">
        {m.attachments && (
          <div className="msg-files">
            {m.attachments.map((a, i) => (
              <Chip key={i} a={a} />
            ))}
          </div>
        )}
        {m.text && <div className="msg-bubble">{m.text}</div>}
      </div>
    );
  return (
    <div className="msg msg-ai">
      {m.text && (
        <div className="md">
          <Markdown text={m.text} onCopy={onCopy} />
        </div>
      )}
      {m.pending && !m.text && (
        <div className="ly-dots chat-dots">
          <i />
          <i />
          <i />
        </div>
      )}
      {m.pending && m.text && <span className="chat-caret" />}
      {m.error && (
        <div className="msg-error">
          {m.error}
          {/* only the last answer can be retried: retry resends the last question */}
          {canRetry && (
            <button className="text-btn" onClick={() => chat.retry()}>
              Повторить
            </button>
          )}
        </div>
      )}
    </div>
  );
});

export function Chat({ settings, toast, setBusy, ask, onAsked, setDropHandler, openSettings }: ChatProps) {
  const st = useSyncExternalStore(chat.subscribe, chat.get);
  const [text, setText] = useState("");
  const [files, setFiles] = useState<ChatAttachment[]>([]);
  const [loadingFiles, setLoadingFiles] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    chat.load();
  }, []);

  const addPaths = async (paths: string[]) => {
    setLoadingFiles((n) => n + paths.length);
    for (const p of paths) {
      try {
        const a = await call<ChatAttachment>("ai_attach", { path: p });
        setFiles((f) => [...f, a]);
      } catch (e) {
        toast({ icon: "error", tone: "error", title: "Файл не прикрепился", subtitle: String(e) }, 3500);
      } finally {
        setLoadingFiles((n) => n - 1);
      }
    }
    inputRef.current?.focus();
  };

  const addRef = useRef(addPaths);
  addRef.current = addPaths;
  useEffect(() => {
    setDropHandler((paths) => addRef.current(paths));
    return () => setDropHandler(null);
  }, [setDropHandler]);

  const toastRef = useRef(toast);
  toastRef.current = toast;
  const copyCode = useCallback((code: string) => {
    call("copy_text", { text: code })
      .then(() => toastRef.current({ icon: "copy", title: "Код скопирован" }))
      .catch((e) => toastRef.current({ icon: "error", tone: "error", title: "Не скопировалось", subtitle: String(e) }));
  }, []);

  // question from the launcher (a reply still streaming is cut short — the new
  // question wins instead of being silently dropped)
  useEffect(() => {
    if (!ask) return;
    chat.load().then(() => {
      if (chat.get().streaming) chat.stop();
      chat.send(ask.text);
      onAsked();
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ask?.n]);

  // keep the newest message in view while it streams (unless the user scrolled up)
  useLayoutEffect(() => {
    const el = listRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [st.messages]);

  // autosize the input
  useLayoutEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.min(120, el.scrollHeight)}px`;
  }, [text]);

  const send = () => {
    if (loadingFiles > 0) return;
    if (chat.send(text, files)) {
      setText("");
      setFiles([]);
      stick.current = true;
    }
  };

  const pick = async () => {
    setBusy(true);
    try {
      const paths = await call<string[]>("pick_files", { title: "Прикрепить к сообщению" });
      if (paths.length) await addPaths(paths);
    } catch {
      /* dialog failed to open — nothing to attach */
    } finally {
      setBusy(false);
    }
  };

  if (!settings.aiEnabled) {
    return (
      <div className="chat-empty">
        <IChat size={30} />
        <div className="chat-empty-title">ИИ-чат выключен</div>
        <button className="text-btn" onClick={openSettings}>
          Включить в настройках
        </button>
      </div>
    );
  }

  const streaming = !!st.streaming;
  const server = settings.aiUrl.replace(/^https?:\/\//, "").replace(/\/v1\/?$/, "");

  return (
    <div className="chat">
      <div className="chat-head">
        <div className="chat-src">
          <button className="on">Остров</button>
          <button onClick={() => call("ai_web", { site: "gemini" }).catch((e) => toast({ icon: "error", tone: "error", title: "Не открылся Gemini", subtitle: String(e) }))} title="Gemini в своём окне — войди в аккаунт Google один раз">
            Gemini ↗
          </button>
          <button onClick={() => call("ai_web", { site: "deepseek" }).catch((e) => toast({ icon: "error", tone: "error", title: "Не открылся DeepSeek", subtitle: String(e) }))} title="DeepSeek в своём окне — войди в аккаунт один раз">
            DeepSeek ↗
          </button>
          <button onClick={() => call("ai_web", { site: "translate" }).catch((e) => toast({ icon: "error", tone: "error", title: "Не открылся переводчик", subtitle: String(e) }))} title="Google Переводчик в своём окне: любой язык → русский">
            Перевод ↗
          </button>
        </div>
        <span className="chat-model" title={settings.aiUrl}>
          <IChat size={14} />
          {settings.aiModel || "модель сервера"} <i>· {server}</i>
        </span>
        {st.messages.length > 0 && (
          <button className="text-btn" onClick={() => chat.clear()} title="Начать новый диалог">
            <IPlus size={15} /> Новый диалог
          </button>
        )}
      </div>

      <div
        className="chat-list"
        ref={listRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
      >
        {st.messages.length === 0 && (
          <div className="chat-empty">
            <IChat size={30} />
            <div className="chat-empty-title">Спросите что-нибудь</div>
            <div className="hint">Можно прикрепить картинку, PDF, DOCX или код — или перетащить файл сюда</div>
          </div>
        )}
        {st.messages.map((m, i) => (
          <Msg key={m.id} m={m} canRetry={i === st.messages.length - 1 && !streaming} onCopy={copyCode} />
        ))}
      </div>

      <div className="chat-composer">
        {(files.length > 0 || loadingFiles > 0) && (
          <div className="chat-files">
            {files.map((a, i) => (
              <Chip key={i} a={a} onRemove={() => setFiles((f) => f.filter((_, j) => j !== i))} />
            ))}
            {loadingFiles > 0 && <span className="chat-chip loading">Читаю файл…</span>}
          </div>
        )}
        <div className="chat-input">
          <button className="icon-btn" onClick={pick} title="Прикрепить файл">
            <IPaperclip size={18} />
          </button>
          <textarea
            ref={inputRef}
            value={text}
            rows={1}
            placeholder="Сообщение…"
            spellCheck={false}
            onFocus={() => setBusy(true)}
            onBlur={() => setBusy(false)}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
          />
          {streaming ? (
            <button className="icon-btn send" onClick={() => chat.stop()} title="Остановить">
              <IStop size={16} />
            </button>
          ) : (
            <button className="icon-btn send" onClick={send} disabled={(!text.trim() && !files.length) || loadingFiles > 0} title="Отправить (Enter)">
              <ISend size={17} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
