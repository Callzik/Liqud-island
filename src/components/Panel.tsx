import type { AgentsPayload, Clip, Media, PinItem, Settings, ShelfItem, Volume } from "../api";
import type { RGB } from "../lib/color";
import { IBuilding, IChat, IClipboard, IHome, IInbox, ILyrics, INote, IPin, ISliders, ISun, ITerminal, ITextTool } from "./Icons";
import { AgentsTab } from "./AgentsTab";
import { Office } from "./Office";
import { needsYou } from "../lib/agents";
import { Home } from "./Home";
import { Lyrics } from "./Lyrics";
import { Chat } from "./Chat";
import { Weather } from "./Weather";
import { TextTab } from "./TextTab";
import { NotesTab } from "./NotesTab";
import type { WeatherPayload } from "../lib/weather";
import { ClipboardTab } from "./ClipboardTab";
import { ShelfTab } from "./ShelfTab";
import { SettingsTab } from "./SettingsTab";
import type { ToastData } from "./Toast";
import type { LyricsState } from "../lib/useLyrics";

export type Tab = "home" | "clipboard" | "shelf" | "text" | "chat" | "notes" | "agents" | "office" | "weather" | "lyrics" | "settings";

// order as in the promo video; «Песня» (lyrics) comes last
const TABS: { id: Exclude<Tab, "settings">; label: string; Icon: typeof IHome; on?: (s: Settings) => boolean }[] = [
  { id: "home", label: "Главная", Icon: IHome },
  { id: "clipboard", label: "Буфер", Icon: IClipboard },
  { id: "shelf", label: "Полка", Icon: IInbox },
  { id: "text", label: "Текст", Icon: ITextTool, on: (s) => s.textEnabled },
  { id: "chat", label: "Чат", Icon: IChat, on: (s) => s.aiEnabled },
  { id: "notes", label: "Заметки", Icon: INote, on: (s) => s.notesEnabled },
  { id: "agents", label: "Агенты", Icon: ITerminal, on: (s) => s.agentsEnabled },
  { id: "office", label: "Офис", Icon: IBuilding, on: (s) => s.agentsEnabled },
  { id: "weather", label: "Погода", Icon: ISun, on: (s) => s.weatherEnabled },
  { id: "lyrics", label: "Песня", Icon: ILyrics },
];

export interface PanelProps {
  tab: Tab;
  setTab: (t: Tab) => void;
  pinned: boolean;
  setPinned: (p: boolean) => void;
  settings: Settings;
  saveSettings: (patch: Partial<Settings>) => void;
  media: Media | null;
  mediaAt: number;
  cover: string | null;
  lyrics: LyricsState;
  accent: RGB;
  volume: Volume;
  clips: Clip[];
  shelf: ShelfItem[];
  setShelf: (s: ShelfItem[]) => void;
  pins: PinItem[];
  setPins: (p: PinItem[]) => void;
  version: string;
  onClose: () => void;
  onLauncher: () => void;
  toast: (t: ToastData, ms?: number) => void;
  setBusy: (b: boolean) => void;
  ask: { text: string; n: number } | null;
  onAsked: () => void;
  setDropHandler: (fn: ((paths: string[]) => void) | null) => void;
  weather: WeatherPayload | null;
  agents: AgentsPayload;
  agentSel: string | null;
  setAgentSel: (id: string | null) => void;
}

export function Panel(p: PanelProps) {
  const waiting = p.agents.sessions.filter(needsYou).length;
  return (
    <div className="panel">
      <header className="panel-head">
        <nav className="tabs">
          {TABS.filter((t) => !t.on || t.on(p.settings)).map(({ id, label, Icon }) => (
            <button key={id} className={`tab ${p.tab === id ? "active" : ""}`} onClick={() => p.setTab(id)} title={label}>
              <Icon size={18} />
              {p.tab === id && <span>{label}</span>}
              {id === "shelf" && p.shelf.length > 0 && p.tab !== id && <i className="tab-badge">{p.shelf.length}</i>}
              {id === "agents" && p.tab !== id && waiting > 0 && <i className="tab-badge hot">{waiting}</i>}
            </button>
          ))}
        </nav>
        <div className="head-actions">
          <button
            className={`icon-btn ${p.pinned ? "on" : ""}`}
            onClick={() => p.setPinned(!p.pinned)}
            title={p.pinned ? "Открепить" : "Закрепить открытым"}
          >
            <IPin size={18} />
          </button>
          <button
            className={`icon-btn ${p.tab === "settings" ? "on" : ""}`}
            onClick={() => p.setTab(p.tab === "settings" ? "home" : "settings")}
            title="Настройки"
          >
            <ISliders size={18} />
          </button>
        </div>
      </header>
      <section className="panel-body">
        {p.tab === "home" && <Home {...p} />}
        {p.tab === "lyrics" && <Lyrics media={p.media} mediaAt={p.mediaAt} accent={p.accent} lyrics={p.lyrics} />}
        {p.tab === "chat" && (
          <Chat
            settings={p.settings}
            toast={p.toast}
            setBusy={p.setBusy}
            ask={p.ask}
            onAsked={p.onAsked}
            setDropHandler={p.setDropHandler}
            openSettings={() => p.setTab("settings")}
          />
        )}
        {p.tab === "weather" && <Weather weather={p.weather} settings={p.settings} openSettings={() => p.setTab("settings")} />}
        {p.tab === "text" && <TextTab toast={p.toast} setBusy={p.setBusy} onClose={p.onClose} />}
        {p.tab === "notes" && <NotesTab setBusy={p.setBusy} />}
        {p.tab === "clipboard" && <ClipboardTab clips={p.clips} onClose={p.onClose} toast={p.toast} />}
        {p.tab === "shelf" && <ShelfTab shelf={p.shelf} setShelf={p.setShelf} toast={p.toast} setBusy={p.setBusy} />}
        {p.tab === "agents" && (
          <AgentsTab
            agents={p.agents}
            selected={p.agentSel}
            setSelected={p.setAgentSel}
            toast={p.toast}
            setBusy={p.setBusy}
            openSettings={() => p.setTab("settings")}
          />
        )}
        {p.tab === "office" && (
          <Office
            agents={p.agents}
            onOpen={(id) => {
              p.setAgentSel(id);
              p.setTab("agents");
            }}
          />
        )}
        {p.tab === "settings" && <SettingsTab settings={p.settings} save={p.saveSettings} version={p.version} setBusy={p.setBusy} />}
      </section>
    </div>
  );
}
