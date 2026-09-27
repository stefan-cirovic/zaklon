import { useEffect, useState } from "react";
import { api } from "../api";
import type { Key } from "../i18n";

type T = (k: Key) => string;
type State = { checked: boolean; firewall_on: boolean; allowed: boolean; blocked: boolean; error: string | null; ok: boolean };

/** Laptop only: shows up when Windows Firewall would keep phones out, with a fix. */
export default function Firewall({ t }: { t: T }) {
  const [st, setSt] = useState<State | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api<State>("/api/firewall").then(setSt).catch(() => {});
  }, []);
  if (!st || st.ok) return null;
  const allow = async () => {
    setBusy(true);
    try {
      setSt(await api<State>("/api/firewall/allow", { method: "POST" }));
    } catch {
      /* the state stays as it was */
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="panel stack left notice firewall" role="alert">
      <h2>{t("firewallTitle")}</h2>
      <p style={{ margin: 0 }}>{st.blocked ? t("firewallBlocked") : t("firewallMissing")}</p>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("firewallHow")}</p>
      <div>
        <button className="btn" onClick={allow} disabled={busy}>{busy ? t("firewallWaiting") : t("firewallAllow")}</button>
      </div>
    </div>
  );
}
