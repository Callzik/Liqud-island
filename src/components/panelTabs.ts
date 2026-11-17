import type { Settings } from "../api";
import { IBuilding, IChat, IClipboard, IHome, IInbox, ILyrics, INote, ISun, ITerminal, ITextTool, IToggles } from "./Icons";

export type Tab = "home" | "clipboard" | "shelf" | "text" | "chat" | "notes" | "agents" | "office" | "weather" | "controls" | "lyrics" | "settings";

export const PANEL_TABS: { id: Exclude<Tab, "settings">; label: string; Icon: typeof IHome; on?: (s: Settings) => boolean }[] = [
  { id: "home", label: "Главная", Icon: IHome },
  { id: "clipboard", label: "Буфер", Icon: IClipboard },
  { id: "shelf", label: "Полка", Icon: IInbox },
  { id: "text", label: "Текст", Icon: ITextTool, on: (s) => s.textEnabled },
  { id: "chat", label: "Чат", Icon: IChat, on: (s) => s.aiEnabled },
  { id: "notes", label: "Заметки", Icon: INote, on: (s) => s.notesEnabled },
  { id: "agents", label: "Агенты", Icon: ITerminal, on: (s) => s.agentsEnabled },
  { id: "office", label: "Офис", Icon: IBuilding, on: (s) => s.agentsEnabled },
  { id: "weather", label: "Погода", Icon: ISun, on: (s) => s.weatherEnabled },
  { id: "controls", label: "Управление", Icon: IToggles },
  { id: "lyrics", label: "Песня", Icon: ILyrics },
];
