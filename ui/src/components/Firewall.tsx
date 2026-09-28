import { useEffect, useState } from "react";
import { api } from "../api";
import type { Key } from "../i18n";

type T = (k: Key) => string;
type State = {
  checked: boolean;
  firewall_on: boolean;
  allowed: boolean;
  blocked: boolean;
  /** The network the laptop is on is one Zaklon's rules leave out (Windows treats it as public). */
  public_network: boolean;
  error: string | null;
  ok: boolean;
};

/**
 * Laptop only: shows up when Windows Firewall would keep phones out, with a fix.
 * With `showOk` (Household > Network) it also says when all is well.
 */
export default function Firewall({ t, showOk = false }: { t: T; showOk?: boolean }) {
  const [st, setSt] = useState<State | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api<State>("/api/firewall").then(setSt).catch(() => {});
  }, []);
  // Windows takes a few seconds to answer.
  if (!st || st.ok) {
    return showOk ? (
      <div className={"panel stack left " + (st ? "firewall-ok" : "firewall-wait")}>
        <h2>{t("firewallName")}</h2>
        <p className="muted" style={{ margin: 0, fontSize: 14 }}>{st ? t("firewallOk") : t("firewallChecking")}</p>
      </div>
    ) : null;
  }
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
  // The rules are in place; only the network's type in Windows keeps phones out.
  const onlyNetwork = st.public_network && st.allowed && !st.blocked;
  return (
    <div className="panel stack left notice firewall" role="alert">
      <h2>{t("firewallTitle")}</h2>
      <p style={{ margin: 0 }}>{st.blocked ? t("firewallBlocked") : onlyNetwork ? t("firewallPublic") : t("firewallMissing")}</p>
      {!onlyNetwork && (
        <>
          <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("firewallHow")}</p>
          <div>
            <button className="btn" onClick={allow} disabled={busy}>{busy ? t("firewallWaiting") : t("firewallAllow")}</button>
          </div>
        </>
      )}
    </div>
  );
}
