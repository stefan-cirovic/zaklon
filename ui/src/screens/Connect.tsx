import { useState } from "react";
import { clientDiscover, clientPair, type DiscoveredHub, type PairPayload } from "../api";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { securityCode } from "../format";
import { scanCode } from "../scan";

type T = (k: Key) => string;
type Props = { t: T; onLinked: () => void; notice: string | null };

export default function Connect({ t, onLinked, notice }: Props) {
  const [payload, setPayload] = useState<PairPayload | null>(null);
  const [hubs, setHubs] = useState<DiscoveredHub[] | null>(null);
  const [picked, setPicked] = useState<DiscoveredHub | null>(null);
  const [code, setCode] = useState("");
  const [matches, setMatches] = useState(false);
  const [password, setPassword] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const doScan = async () => {
    setErr(null);
    const r = await scanCode("qr");
    if (!r.ok) {
      if (r.reason === "denied") setErr(t("cameraDenied"));
      return;
    }
    try {
      const parsed = JSON.parse(r.text) as PairPayload;
      if (parsed.v !== 1 || !Array.isArray(parsed.hosts) || !parsed.fp || !parsed.code) throw new Error("bad");
      setPayload(parsed);
    } catch {
      setErr(t("notAPairingCode"));
    }
  };

  const doDiscover = async () => {
    setErr(null);
    setBusy(true);
    try {
      const found = await clientDiscover();
      setHubs(found);
      if (found.length === 0) setErr(t("noHubsFound"));
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  // A hub found on the network is only a suggestion: anyone on the Wi-Fi can
  // answer. The person must confirm the security code matches the laptop's
  // screen before the password is sent. (The QR code carries it already.)
  const fromDiscovery = payload === null && picked !== null;
  const effective: PairPayload | null =
    payload ??
    (picked && code.length === 6 && matches
      ? { v: 1, hosts: [picked.host], port: picked.port, fp: picked.fp, code, name: picked.name, install_port: 8480 }
      : null);

  const reset = () => {
    setPayload(null);
    setPicked(null);
    setCode("");
    setMatches(false);
    setErr(null);
  };

  const doPair = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!effective) return;
    setBusy(true);
    setErr(null);
    try {
      await clientPair(effective, password, deviceName.trim() || t("defaultPhoneName"));
      onLinked();
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="stack">
      <div>
        <h1>{t("connectTitle")}</h1>
        <p className="muted">{t("connectIntro")}</p>
      </div>
      {notice && <p className="warn" role="alert">{notice}</p>}

      {!effective && (
        <div className="stack">
          <button className="btn" onClick={doScan}>{t("scanQr")}</button>
          <button className="btn secondary" onClick={doDiscover} disabled={busy}>{t("findHubs")}</button>
          {hubs && hubs.length > 0 && (
            <div className="list">
              {hubs.map((h) => (
                <button
                  key={h.id + h.host}
                  className="item clickable"
                  aria-pressed={picked?.host === h.host}
                  onClick={() => {
                    setPicked(h);
                    setMatches(false);
                  }}
                >
                  <div>
                    <div>{h.name}</div>
                    <div className="muted" style={{ fontSize: 13 }}>{h.host}:{h.port}</div>
                  </div>
                  <span className="muted" aria-hidden="true">{picked?.host === h.host ? "✓" : "›"}</span>
                </button>
              ))}
            </div>
          )}
          {fromDiscovery && picked && (
            <div className="panel stack">
              <label className="field">
                {t("pairCodeEntry")}
                <input type="text" inputMode="numeric" maxLength={6} value={code} onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))} />
              </label>
              <p>
                {t("securityCode")}: <strong className="sec-code">{securityCode(picked.fp)}</strong>
              </p>
              <label className="check-line">
                <input type="checkbox" checked={matches} onChange={(e) => setMatches(e.target.checked)} />
                <span>{t("securityMatches")}</span>
              </label>
            </div>
          )}
        </div>
      )}

      {effective && (
        <form className="stack" onSubmit={doPair}>
          <div className="panel">
            <div className="label">{t("hubStatus")}</div>
            <div className="value" style={{ fontSize: 18 }}>{effective.name || effective.hosts[0]}</div>
          </div>
          <label className="field">
            {t("password")}
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required autoFocus autoComplete="current-password" />
          </label>
          <label className="field">
            {t("deviceName")}
            <input type="text" value={deviceName} onChange={(e) => setDeviceName(e.target.value)} maxLength={60} placeholder={t("deviceNameHint")} />
          </label>
          <div className="row actions">
            <button className="btn" disabled={busy || !password}>{t("pairNow")}</button>
            <button type="button" className="btn secondary" onClick={reset}>{t("cancel")}</button>
          </div>
        </form>
      )}

      {err && <p className="error" role="alert">{err}</p>}
    </div>
  );
}
