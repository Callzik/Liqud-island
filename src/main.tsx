import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { Overlay } from "./components/Overlay";
import { call, type InitPayload } from "./api";
import "./styles.css";

// front-end errors go to the journal (%APPDATA%\dev.callzik.island\logs)
const report = (message: string) => void call("log_js", { level: "error", message }).catch(() => {});
window.addEventListener("error", (e) => report(`${e.message} @ ${e.filename}:${e.lineno}:${e.colno}`));
window.addEventListener("unhandledrejection", (e) => {
  const r = e.reason;
  report(`unhandled: ${r instanceof Error ? `${r.message} ${r.stack ?? ""}` : String(r)}`);
});

document.addEventListener("contextmenu", (e) => {
  // the native context menu makes no sense on the island; inputs keep theirs
  if (!(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) e.preventDefault();
});

if (location.hash.startsWith("#overlay")) {
  createRoot(document.getElementById("root")!).render(<Overlay />);
} else
call<InitPayload>("init").then((boot) => {
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App boot={boot} />
    </StrictMode>,
  );
});
