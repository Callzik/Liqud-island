import { call, dragOut } from "../api";
import { WeatherIcon } from "./WeatherIcon";
import { Face } from "./Face";
import { IAlert, IBell, ICheck, ICopy, IDownload, IImage, IInbox, ILauncher, IMic, IPipette, IQr, IText, IVideo } from "./Icons";

export interface ToastAction {
  label: string;
  cmd: string;
  args?: Record<string, unknown>;
}

export interface ToastData {
  icon: "shelf" | "launcher" | "copy" | "check" | "error" | "image" | "text" | "qr" | "color" | "rain" | "mic" | "download" | "video" | "timer";
  /** weather code for the "rain" icon */
  wx?: number;
  title: string;
  subtitle?: string;
  tone?: "error" | "alarm";
  /** thumbnail (data url) shown instead of the icon */
  image?: string;
  /** colour sample shown instead of the icon */
  swatch?: string;
  actions?: ToastAction[];
  /** file the thumbnail drags out as */
  drag?: string;
  /** how long to show, ms */
  ms?: number;
}

const ICONS = { shelf: IInbox, launcher: ILauncher, copy: ICopy, check: ICheck, error: IAlert, image: IImage, text: IText, qr: IQr, color: IPipette, mic: IMic, download: IDownload, video: IVideo, timer: IBell };

export function Toast({ data, onDone, face }: { data: ToastData; onDone?: () => void; face?: boolean }) {
  const Icon = (ICONS as Record<string, typeof ICheck>)[data.icon] ?? ICheck;
  return (
    <div className={`toast ${data.tone === "error" ? "toast-error" : ""} ${data.tone === "alarm" ? "toast-alarm" : ""}`}>
      {face && !data.image && (data.tone === "alarm" || data.icon === "check") ? (
        <div className="toast-face">
          <Face mood={data.tone === "alarm" ? "alarm" : "happy"} />
        </div>
      ) : data.image ? (
        <img
          className={`toast-thumb ${data.drag ? "can-drag" : ""}`}
          src={data.image}
          alt=""
          draggable={false}
          title={data.drag ? "Перетащите в чат или папку" : undefined}
          onMouseDown={(e) => {
            if (!data.drag || e.button !== 0) return;
            e.stopPropagation();
            dragOut([data.drag], data.drag);
          }}
        />
      ) : data.icon === "rain" ? (
        <div className="toast-icon">
          <WeatherIcon code={data.wx ?? 61} size={24} />
        </div>
      ) : data.swatch ? (
        <div className="toast-swatch" style={{ background: data.swatch }} />
      ) : (
        <div className="toast-icon">
          <Icon size={20} />
        </div>
      )}
      <div className="toast-text">
        <div className="toast-title">{data.title}</div>
        {data.subtitle && <div className="toast-sub">{data.subtitle}</div>}
        {data.drag && <div className="toast-hint">Перетащи превью в чат или папку</div>}
      </div>
      {data.actions && data.actions.length > 0 && (
        <div className="toast-actions">
          {data.actions.map((a) => (
            <button
              key={a.label}
              className="toast-btn"
              onClick={(e) => {
                e.stopPropagation();
                call(a.cmd, a.args ?? {}).catch(() => {});
                onDone?.();
              }}
            >
              {a.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
