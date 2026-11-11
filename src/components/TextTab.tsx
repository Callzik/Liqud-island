import { useEffect, useState } from "react";
import { call } from "../api";
import { capitalizeWords, counts, fixLayout, fromBase64, joinLines, jsonLine, jsonPretty, lower, sentence, tidySpaces, toBase64, translit, upper } from "../lib/textTools";
import type { ToastData } from "./Toast";

const TOOLS: { label: string; title: string; fn: (s: string) => string }[] = [
  { label: "ВЕРХНИЙ", title: "Все буквы заглавные", fn: upper },
  { label: "нижний", title: "Все буквы строчные", fn: lower },
  { label: "Как в предложении", title: "Заглавная — только в начале предложений", fn: sentence },
  { label: "Транслит", title: "Кириллица → латиница", fn: translit },
  { label: "Раскладка", title: "Ghbdtn → Привет и обратно", fn: fixLayout },
  { label: "Пробелы", title: "Убрать лишние пробелы и пустые строки", fn: tidySpaces },
  { label: "Переносы", title: "Склеить строки в абзацы", fn: joinLines },
  { label: "Каждое Слово", title: "Заглавная в начале каждого слова", fn: capitalizeWords },
  { label: "JSON красиво", title: "Отформатировать JSON с отступами", fn: jsonPretty },
  { label: "JSON в строку", title: "Сжать JSON в одну строку", fn: jsonLine },
  { label: "→ Base64", title: "Закодировать в Base64", fn: toBase64 },
  { label: "Base64 →", title: "Раскодировать Base64", fn: fromBase64 },
];

/** «Текст»: quick transforms over the clipboard text. */
export function TextTab({
  toast,
  setBusy,
  onClose,
}: {
  toast: (t: ToastData, ms?: number) => void;
  setBusy: (b: boolean) => void;
  onClose: () => void;
}) {
  const [text, setText] = useState("");
  const [loaded, setLoaded] = useState(false);

  const fromClipboard = async () => {
    const t = await call<string | null>("clip_read_text").catch(() => null);
    setText(t ?? "");
    setLoaded(true);
  };
  useEffect(() => {
    fromClipboard();
  }, []);

  const [tr, setTr] = useState(false);
  const translate = async () => {
    setTr(true);
    try {
      const out = await call<string>("translate_text", { text, to: null });
      setText(out);
      await call("copy_text", { text: out }).catch(() => {});
      toast({ icon: "text", title: "Перевод готов", subtitle: "Уже в буфере" }, 2000);
    } catch (e) {
      toast({ icon: "error", tone: "error", title: "Не удалось перевести", subtitle: String(e) }, 3500);
    }
    setTr(false);
  };

  const c = counts(text);
  const nf = (n: number) => n.toLocaleString("ru-RU");

  return (
    <div className="tt">
      <div className="tt-tools">
        <button className="tt-tool tt-accent" title="Русский ⇄ английский" disabled={!text || tr} onClick={translate}>
          {tr ? "Перевожу…" : "Перевести"}
        </button>
        {TOOLS.map((t) => (
          <button key={t.label} className="tt-tool" title={t.title} disabled={!text} onClick={() => setText(t.fn(text))}>
            {t.label}
          </button>
        ))}
      </div>
      <textarea
        className="tt-area"
        value={text}
        spellCheck={false}
        placeholder={loaded ? "В буфере нет текста — вставьте или напишите здесь" : "Читаю буфер…"}
        onFocus={() => setBusy(true)}
        onBlur={() => setBusy(false)}
        onChange={(e) => setText(e.target.value)}
      />
      <div className="tt-foot">
        <span className="tt-count">
          {nf(c.chars)} симв. · {nf(c.noSpaces)} без пробелов · {nf(c.words)} {c.words % 10 === 1 && c.words % 100 !== 11 ? "слово" : c.words % 10 >= 2 && c.words % 10 <= 4 && (c.words % 100 < 12 || c.words % 100 > 14) ? "слова" : "слов"} ·{" "}
          {nf(c.lines)} стр.
        </span>
        <button className="text-btn" onClick={fromClipboard} title="Заново взять текст из буфера">
          Из буфера
        </button>
        <button
          className="text-btn"
          disabled={!text}
          onClick={async () => {
            await call("copy_text", { text });
            toast({ icon: "copy", title: "Скопировано", subtitle: `${nf(c.chars)} симв.` });
          }}
        >
          В буфер
        </button>
        <button
          className="tt-paste"
          disabled={!text}
          onClick={async () => {
            onClose();
            const ok = await call<boolean>("paste_text", { text });
            if (!ok) toast({ icon: "error", tone: "error", title: "Не удалось вставить" });
          }}
        >
          Вставить
        </button>
      </div>
    </div>
  );
}
