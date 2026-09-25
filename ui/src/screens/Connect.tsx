import { useState } from "react";
import { clientDiscover, clientPair, type DiscoveredHub, type PairPayload } from "../api";
import type { Key } from "../i18n";

type T = (k: Key) => string;
type Props = { t: T; onLinked: () => void };

async function scanQr(): Promise<string | null> {
  const { scan, Format, checkPermissions, requestPermissions } = await import("@tauri-apps/plugin-barcode-scanner");
  let perm = await checkPermissions();
  if (perm !== "granted") perm = await requestPermissions();
  if (perm !== "granted") return null;
  const result = await scan({ windowed: false, formats: [Format.QRCode] });
  return result.content;
}

export default function Connect({ t, onLinked }: Props) {
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
    try {
      const content = await scanQr();
      if (!content) {
        setErr(t("cameraDenied"));
        return;
      }
      const parsed = JSON.parse(content) as PairPayload;
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
      setErr((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const effective: PairPayload | null =
    payload ??
    (picked && code.length === 6
      ? { v: 1, hosts: [picked.host], port: picked.port, fp: picked.fp, code, name: picked.name, install_port: 8480 }
      : null);

  const doPair = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!effective) return;
    setBusy(true);
    setErr(null);
    try {
      await clientPair(effective, password, deviceName || "Phone");
      onLinked();
    } catch (ex) {
      setErr(String(ex));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="stack" style={{ maxWidth: 480 }}>
      <div>
        <h1>{t("connectTitle")}</h1>
        <p className="muted">{t("connectIntro")}</p>
      </div>

      {!effective && (
        <div className="stack">
          <button className="btn" onClick={doScan}>{t("scanQr")}</button>
          <button className="btn secondary" onClick={doDiscover} disabled={busy}>{t("findHubs")}</button>
          {hubs && hubs.length > 0 && (
            <div className="list">
              {hubs.map((h) => (
                <button key={h.id} className="item" style={{ textAlign: "left", cursor: "pointer" }} onClick={() => setPicked(h)}>
                  <div>
                    <div>{h.name}</div>
                    <div className="muted" style={{ fontSize: 13 }}>{h.host}:{h.port}</div>
                  </div>
                  <span className="muted">{picked?.id === h.id ? "✓" : "›"}</span>
                </button>
              ))}
            </div>
          )}
          {picked && (
            <label className="field">
              {t("pairCodeEntry")}
              <input type="text" inputMode="numeric" maxLength={6} value={code} onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))} />
            </label>
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
            <input type="password" value={password} onChange={(e) => setPassword(e.target.value)} required autoFocus />
          </label>
          <label className="field">
            {t("deviceName")}
            <input type="text" value={deviceName} onChange={(e) => setDeviceName(e.target.value)} maxLength={60} placeholder={t("deviceNameHint")} />
          </label>
          <div className="row">
            <button className="btn" disabled={busy || !password}>{t("pairNow")}</button>
            <button type="button" className="btn secondary" onClick={() => { setPayload(null); setPicked(null); setCode(""); }}>{t("cancel")}</button>
          </div>
        </form>
      )}

      {err && <p className="error">{err}</p>}
    </div>
  );
}
