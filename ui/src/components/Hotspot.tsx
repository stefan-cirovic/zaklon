import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Key } from "../i18n";
import { errText } from "../errors";
import Qr from "./Qr";

type T = (k: Key) => string;
type State = { supported: boolean; on: boolean; ssid: string; passphrase: string; clients: number; error: string | null; qr: string | null };

/** Laptop only: turn the laptop into the household's Wi-Fi network when there is no router. */
export default function Hotspot({ t }: { t: T }) {
  const [st, setSt] = useState<State | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(() => {
    api<State>("/api/hotspot").then(setSt).catch(() => {});
  }, []);
  useEffect(load, [load]);

  const act = async (what: "start" | "stop") => {
    setBusy(true);
    setErr(null);
    try {
      setSt(await api<State>(`/api/hotspot/${what}`, { method: "POST" }));
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  if (!st) return null;
  const ours = st.on && st.ssid === "Zaklon";
  return (
    <div className="panel stack left">
      <h2>{t("hotspotTitle")}</h2>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("hotspotIntro")}</p>
      {err && <p className="error" role="alert">{err}</p>}
      {st.error === "no Wi-Fi adapter" ? (
        <p className="muted" style={{ margin: 0 }}>{t("hotspotNoWifi")}</p>
      ) : (
        st.error && <p className="warn" style={{ margin: 0, fontSize: 14 }}>{t("hotspotFailed")} ({st.error})</p>
      )}
      {ours ? (
        <>
          <div className="qr-line">
            {st.qr && <Qr value={st.qr} size={160} label={t("hotspotQr")} />}
            <dl className="facts">
              <div><dt>{t("hotspotName")}</dt><dd>{st.ssid}</dd></div>
              <div><dt>{t("hotspotPassword")}</dt><dd className="code-inline">{st.passphrase}</dd></div>
              <div><dt>{t("hotspotClients")}</dt><dd>{st.clients}</dd></div>
            </dl>
          </div>
          <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("hotspotJoin")}</p>
          <div>
            <button className="btn secondary" onClick={() => act("stop")} disabled={busy}>{t("hotspotStop")}</button>
          </div>
        </>
      ) : st.error === "no Wi-Fi adapter" ? null : (
        <div>
          <button className="btn" onClick={() => act("start")} disabled={busy || !st.supported}>{busy ? t("hotspotStarting") : t("hotspotStart")}</button>
        </div>
      )}
    </div>
  );
}
