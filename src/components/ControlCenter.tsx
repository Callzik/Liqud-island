import { useCallback, useEffect, useRef, useState } from "react";
import { call } from "../api";
import { IBluetooth, IClose, IContrast, IList, ILock, IMoon, IPower, IRestart, IShield, ISliders, IWifi } from "./Icons";

export interface Net {
  ssid: string;
  signal: number;
  secure: boolean;
  connected: boolean;
  known: boolean;
}
export interface Vpn {
  name: string;
  connected: boolean;
}
export interface SysState {
  wifi: boolean | null;
  bluetooth: boolean | null;
  dark: boolean;
  vpn: Vpn[];
  ssid: string;
}

const err = (e: unknown) => String(e).replace(/^Error:\s*/, "");

function Bars({ level }: { level: number }) {
  const n = level > 75 ? 4 : level > 50 ? 3 : level > 25 ? 2 : 1;
  return (
    <span className="cc-bars" aria-label={`Сигнал ${level}%`}>
      {[1, 2, 3, 4].map((i) => (
        <i key={i} className={i <= n ? "on" : ""} style={{ height: 3 + i * 3 }} />
      ))}
    </span>
  );
}

function WifiList({ onClose, onChanged }: { onClose: () => void; onChanged: () => void }) {
  const [nets, setNets] = useState<Net[] | null>(null);
  const [error, setError] = useState("");
  const [ask, setAsk] = useState<string | null>(null);
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async (scan: boolean) => {
    try {
      setNets(await call<Net[]>("wifi_list", { scan }));
      setError("");
    } catch (e) {
      setError(err(e));
      setNets([]);
    }
  }, []);

  useEffect(() => {
    load(true);
    // the scan finishes in a few seconds: refresh while the list is open
    const t = setInterval(() => load(false), 4000);
    return () => clearInterval(t);
  }, [load]);

  async function connect(n: Net, password?: string) {
    if (n.secure && !n.known && password === undefined) {
      setAsk(n.ssid);
      setPw("");
      return;
    }
    setBusy(n.ssid);
    setError("");
    try {
      await call("wifi_connect", { ssid: n.ssid, password: password ?? null });
      setAsk(null);
      setTimeout(() => {
        load(false);
        onChanged();
      }, 2500);
    } catch (e) {
      setError(err(e));
    } finally {
      setTimeout(() => setBusy(null), 2500);
    }
  }

  async function disconnect() {
    setBusy("*");
    try {
      await call("wifi_disconnect");
    } catch (e) {
      setError(err(e));
    }
    setTimeout(() => {
      setBusy(null);
      load(false);
      onChanged();
    }, 1200);
  }

  return (
    <div className="card cc-nets">
      <div className="cc-nets-head">
        <span>Сети Wi-Fi</span>
        <button className="text-btn" onClick={() => call("sys_settings", { page: "wifi" })}>
          <ISliders size={13} /> Параметры
        </button>
        <button className="icon-btn" onClick={onClose} title="Закрыть">
          <IClose size={15} />
        </button>
      </div>
      <div className="cc-nets-list">
        {nets === null && <div className="hint">Ищу сети…</div>}
        {nets?.length === 0 && !error && <div className="hint">Сетей не видно</div>}
        {nets?.map((n) => (
          <div key={n.ssid} className="cc-net-wrap">
            <div className={`cc-net ${n.connected ? "on" : ""}`}>
              <Bars level={n.signal} />
              <div className="cc-net-name">
                <b>
                  {n.ssid} {n.secure && <ILock size={11} />}
                </b>
                <small>{n.connected ? "Подключено" : n.known ? "Сохранена" : n.secure ? "Защищённая" : "Открытая"}</small>
              </div>
              {n.connected ? (
                <button className="cc-btn" disabled={busy !== null} onClick={disconnect}>
                  Отключить
                </button>
              ) : (
                <button className="cc-btn accent" disabled={busy !== null} onClick={() => connect(n)}>
                  {busy === n.ssid ? "Подключаю…" : "Подключить"}
                </button>
              )}
            </div>
            {ask === n.ssid && (
              <form
                className="cc-pass"
                onSubmit={(e) => {
                  e.preventDefault();
                  connect(n, pw);
                }}
              >
                <input type="password" autoFocus placeholder="Пароль" value={pw} onChange={(e) => setPw(e.target.value)} />
                <button className="cc-btn light" type="submit" disabled={pw.length < 8 || busy !== null}>
                  Войти
                </button>
              </form>
            )}
          </div>
        ))}
      </div>
      {error && <div className="cc-error">{error}</div>}
    </div>
  );
}

const POWER = [
  { id: "lock", label: "Блокировка", Icon: ILock, confirm: false },
  { id: "sleep", label: "Сон", Icon: IMoon, confirm: false },
  { id: "restart", label: "Перезагрузка", Icon: IRestart, confirm: true },
  { id: "shutdown", label: "Выключение", Icon: IPower, confirm: true },
] as const;

export function ControlCenter() {
  const [st, setSt] = useState<SysState | null>(null);
  const [nets, setNets] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState("");
  const [armed, setArmed] = useState<string | null>(null);
  const armT = useRef<number>(0);

  const refresh = useCallback(async () => {
    try {
      setSt(await call<SysState>("sys_state"));
    } catch (e) {
      setError(err(e));
    }
  }, []);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, 5000);
    return () => clearInterval(t);
  }, [refresh]);

  async function run(id: string, fn: () => Promise<unknown>) {
    setBusy(id);
    setError("");
    try {
      await fn();
    } catch (e) {
      setError(err(e));
    }
    await refresh();
    setBusy(null);
  }

  const vpn = st?.vpn[0];
  const vpnOn = !!st?.vpn.some((v) => v.connected);

  const tiles = [
    {
      id: "wifi",
      label: "Wi-Fi",
      Icon: IWifi,
      on: !!st?.wifi,
      sub: st?.wifi == null ? "Нет адаптера" : st.wifi ? st.ssid || "Включён" : "Выключен",
      off: st?.wifi == null,
      toggle: () => run("wifi", () => call("sys_radio", { kind: "wifi", on: !st?.wifi })),
      more: () => setNets(true),
    },
    {
      id: "bluetooth",
      label: "Bluetooth",
      Icon: IBluetooth,
      on: !!st?.bluetooth,
      sub: st?.bluetooth == null ? "Нет адаптера" : st.bluetooth ? "Включён" : "Выключен",
      off: st?.bluetooth == null,
      toggle: () => run("bluetooth", () => call("sys_radio", { kind: "bluetooth", on: !st?.bluetooth })),
      more: () => call("sys_settings", { page: "bluetooth" }),
    },
    {
      id: "dark",
      label: "Тёмная тема",
      Icon: IContrast,
      on: !!st?.dark,
      sub: st?.dark ? "Включена" : "Выключена",
      off: false,
      toggle: () => run("dark", () => call("sys_dark", { dark: !st?.dark })),
      more: null,
    },
    {
      id: "vpn",
      label: "VPN",
      Icon: IShield,
      on: vpnOn,
      sub: vpn ? (vpnOn ? st!.vpn.find((v) => v.connected)!.name : vpn.name) : "Не настроен",
      off: false,
      toggle: () =>
        vpn
          ? run("vpn", () => {
              const cur = st!.vpn.find((v) => v.connected) ?? vpn;
              return call("sys_vpn", { name: cur.name, on: !cur.connected });
            })
          : call("sys_settings", { page: "vpn" }),
      more: () => call("sys_settings", { page: "vpn" }),
    },
  ];

  function power(id: string, confirm: boolean) {
    if (confirm && armed !== id) {
      setArmed(id);
      clearTimeout(armT.current);
      armT.current = window.setTimeout(() => setArmed(null), 3000);
      return;
    }
    setArmed(null);
    run(id, () => call("sys_power", { action: id }));
  }

  return (
    <div className="cc">
      {nets ? (
        <WifiList onClose={() => setNets(false)} onChanged={refresh} />
      ) : (
        <div className="cc-tiles">
          {tiles.map((t) => (
            <div key={t.id} className={`card cc-tile ${t.on ? "on" : ""} ${t.off ? "dim" : ""}`}>
              <div className="cc-tile-top">
                <button className="cc-toggle" disabled={t.off || busy !== null} onClick={t.toggle} title={t.on ? "Выключить" : "Включить"}>
                  {busy === t.id ? <span className="cc-spin" /> : <t.Icon size={18} />}
                </button>
                {t.more && (
                  <button className="cc-more" onClick={t.more} title={t.id === "wifi" ? "Сети" : "Параметры"}>
                    <IList size={14} />
                  </button>
                )}
              </div>
              <div className="cc-tile-label">
                <b>{t.label}</b>
                <small>{t.sub}</small>
              </div>
            </div>
          ))}
        </div>
      )}
      <div className="cc-power">
        {POWER.map(({ id, label, Icon, confirm }) => (
          <button key={id} className={`card cc-pbtn ${armed === id ? "armed" : ""}`} disabled={busy !== null} onClick={() => power(id, confirm)}>
            <Icon size={17} />
            <span>{armed === id ? "Ещё раз" : label}</span>
          </button>
        ))}
      </div>
      {error && !nets && <div className="cc-error">{error}</div>}
    </div>
  );
}
