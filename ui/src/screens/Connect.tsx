import { useState } from "react";
import { clientDiscover, clientPair, clientPairFound, type DiscoveredHub, type PairPayload } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { securityCode } from "../format";
import { scanCode } from "../scan";
import { Brand } from "../components/Brand";

type T = (k: Key) => string;
type Props = { t: T; lang: Lang; setLang: (l: Lang) => void; onLinked: () => void; notice: string | null };

export default function Connect({ t, lang, setLang, onLinked, notice }: Props) {
  const [payload, setPayload] = useState<PairPayload | null>(null);
  const [hubs, setHubs] = useState<DiscoveredHub[] | null>(null);
  const [picked, setPicked] = useState<DiscoveredHub | null>(null);
  const [code, setCode] = useState("");
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
  // answer. The phone checks it with the code from the laptop before the
  // password is sent (pair_found in client.rs). The QR code carries the hub's
  // certificate itself.
  const found = payload === null ? picked : null;
  const ready = payload !== null || (found !== null && code.length === 6);

  const reset = () => {
    setPayload(null);
    setPicked(null);
    setCode("");
    setErr(null);
  };

  const doPair = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!ready) return;
    setBusy(true);
    setErr(null);
    const name = deviceName.trim() || t("defaultPhoneName");
    try {
      if (payload) await clientPair(payload, password, name);
      else if (found) await clientPairFound(found, code, password, name);
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
        <div className="brand-hero">
          <Brand size={72} layout="column" />
        </div>
        <h1>{t("connectTitle")}</h1>
        <p className="muted">{t("connectIntro")}</p>
        {/* The phone's language decides at first; each name is in its own language so it can be found. */}
        <select className="lang-pick" value={lang} onChange={(e) => setLang(e.target.value as Lang)} aria-label={t("language")}>
          <option value="en">English</option>
          <option value="sr">Srpski</option>
        </select>
      </div>
      {notice && <p className="warn" role="alert">{notice}</p>}

      {!payload && (
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
                    setErr(null);
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
        </div>
      )}

      {(payload || found) && (
        <form className="stack" onSubmit={doPair}>
          <div className="panel">
            <div className="label">{t("hubStatus")}</div>
            <div className="value" style={{ fontSize: 18 }}>{payload ? payload.name || payload.hosts[0] : found?.name || found?.host}</div>
          </div>
          {found && (
            <div className="stack">
              <label className="field">
                {t("pairCodeEntry")}
                <input
                  type="text"
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  maxLength={6}
                  value={code}
                  onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
                  required
                  autoFocus
                />
              </label>
              <p className="muted" style={{ fontSize: 14 }}>{t("pairCodeCheck")}</p>
              <p className="muted" style={{ fontSize: 14 }}>
                {t("securityCode")}: <strong className="sec-code">{securityCode(found.fp)}</strong>
              </p>
            </div>
          )}
          <label className="field">
            {t("password")}
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required autoFocus={payload !== null} autoComplete="current-password" />
          </label>
          <label className="field">
            {t("deviceName")}
            <input type="text" value={deviceName} onChange={(e) => setDeviceName(e.target.value)} maxLength={60} placeholder={t("deviceNameHint")} />
          </label>
          <div className="row actions">
            <button className="btn" disabled={busy || !password || !ready}>{t("pairNow")}</button>
            <button type="button" className="btn secondary" onClick={reset}>{t("cancel")}</button>
          </div>
        </form>
      )}

      {err && <p className="error" role="alert">{err}</p>}
    </div>
  );
}
