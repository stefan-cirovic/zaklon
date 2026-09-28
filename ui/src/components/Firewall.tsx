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
 * Laptop only: shows up when Windows Firewall would keep phones out, with a fix
 * (Windows asks the computer's administrator to confirm it): let Zaklon in, or,
 * when Windows treats the home network as public, make it private.
 * With `showOk` (Settings › Network) it also says when all is well.
 */
export default function Firewall({ t, showOk = false }: { t: T; showOk?: boolean }) {
  const [st, setSt] = useState<State | null>(null);
  const [busy, setBusy] = useState(false);
  // "Make this network private" was tried, and Windows still treats it as public.
  const [stillPublic, setStillPublic] = useState(false);
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
  const fix = async (path: string) => {
    setBusy(true);
    try {
      const next = await api<State>(path, { method: "POST" });
      setSt(next);
      return next;
    } catch {
      /* the state stays as it was */
      return null;
    } finally {
      setBusy(false);
    }
  };
  const makePrivate = async () => {
    const next = await fix("/api/firewall/private");
    setStillPublic(!next || (!next.ok && next.public_network));
  };
  // The rules are in place; only the network's type in Windows keeps phones out.
  const onlyNetwork = st.public_network && st.allowed && !st.blocked;
  return (
    <div className="panel stack left notice firewall" role="alert">
      <h2>{t("firewallTitle")}</h2>
      <p style={{ margin: 0 }}>{st.blocked ? t("firewallBlocked") : onlyNetwork ? t("firewallPublic") : t("firewallMissing")}</p>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{onlyNetwork ? t("firewallPublicHome") : t("firewallHow")}</p>
      <div>
        {onlyNetwork ? (
          <button className="btn" onClick={makePrivate} disabled={busy}>{busy ? t("firewallWaiting") : t("firewallMakePrivate")}</button>
        ) : (
          <button className="btn" onClick={() => fix("/api/firewall/allow")} disabled={busy}>{busy ? t("firewallWaiting") : t("firewallAllow")}</button>
        )}
      </div>
      {onlyNetwork && stillPublic && !busy && <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("firewallStillPublic")}</p>}
    </div>
  );
}
