import { useCallback, useEffect, useState } from "react";
import { api, type Device, type PairStart, type Status } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtDateTime, securityCode } from "../format";
import ConfirmButton from "../components/ConfirmButton";
import Qr from "../components/Qr";

type T = (k: Key) => string;
type Props = {
  status: Status | null;
  t: T;
  lang: Lang;
  setLang: (l: Lang) => void;
  refresh: () => void;
  /** True on the laptop; phones cannot pair other phones or see the data folder. */
  isHub: boolean;
  /** This phone's own device id (phones only). */
  ownDeviceId: string | null;
};

export default function Household({ status, t, lang, setLang, refresh, isHub, ownDeviceId }: Props) {
  if (!status) return <p className="muted">…</p>;
  if (!status.set_up) {
    return isHub ? <Setup t={t} lang={lang} setLang={setLang} onDone={refresh} defaultName={status.hub_name} /> : <p className="muted">{t("hubNotSetUp")}</p>;
  }
  return <Devices t={t} status={status} lang={lang} setLang={setLang} isHub={isHub} ownDeviceId={ownDeviceId} />;
}

type SetupProps = { t: T; lang: Lang; setLang: (l: Lang) => void; onDone: () => void; defaultName: string };

function Setup({ t, lang, setLang, onDone, defaultName }: SetupProps) {
  const [name, setName] = useState(defaultName);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (pw !== pw2) {
      setErr(t("passwordsDiffer"));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await api("/api/setup", { json: { password: pw, hub_name: name, language: lang } });
      onDone();
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="stack form" onSubmit={submit}>
      <div className="page-head">
        <h1>{t("setupTitle")}</h1>
        <p className="muted">{t("setupIntro")}</p>
      </div>
      <label className="field">
        {t("language")}
        <select value={lang} onChange={(e) => setLang(e.target.value as Lang)}>
          <option value="en">{t("english")}</option>
          <option value="sr">{t("serbian")}</option>
        </select>
      </label>
      <label className="field">
        {t("hubName")}
        <input type="text" value={name} onChange={(e) => setName(e.target.value)} maxLength={60} />
      </label>
      <label className="field">
        {t("password")}
        <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} minLength={8} required autoComplete="new-password" />
      </label>
      <label className="field">
        {t("passwordAgain")}
        <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} minLength={8} required autoComplete="new-password" />
      </label>
      <p className="muted" style={{ fontSize: 14 }}>{t("passwordRule")}</p>
      {err && <p className="error" role="alert">{err}</p>}
      <button className="btn" disabled={busy || pw.length < 8}>{t("finish")}</button>
    </form>
  );
}

type DevicesProps = { t: T; status: Status; lang: Lang; setLang: (l: Lang) => void; isHub: boolean; ownDeviceId: string | null };

function Devices({ t, status, lang, setLang, isHub, ownDeviceId }: DevicesProps) {
  const [devices, setDevices] = useState<Device[]>([]);
  const [pair, setPair] = useState<PairStart | null>(null);
  const [expiresAt, setExpiresAt] = useState(0);
  const [now, setNow] = useState(() => Date.now());
  const [paired, setPaired] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setDevices(await api<Device[]>("/api/devices"));
      setErr(null);
    } catch (e) {
      setErr(errText(t, e));
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  // While a code is shown: tick the countdown and watch for the new phone.
  useEffect(() => {
    if (!pair) return;
    const before = devices.length;
    const tick = setInterval(() => setNow(Date.now()), 1000);
    const poll = setInterval(async () => {
      try {
        const list = await api<Device[]>("/api/devices");
        setDevices(list);
        if (list.length > before) {
          setPaired(true);
          setPair(null);
        }
      } catch {
        /* keep polling */
      }
    }, 3000);
    return () => {
      clearInterval(tick);
      clearInterval(poll);
    };
    // Only restart when a new code is shown.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pair]);

  const start = async () => {
    setErr(null);
    setPaired(false);
    try {
      const p = await api<PairStart>("/api/pair/start", { method: "POST" });
      setPair(p);
      setExpiresAt(Date.now() + p.expires_in_secs * 1000);
      setNow(Date.now());
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  const remove = async (d: Device) => {
    try {
      await api(`/api/devices/${d.id}`, { method: "DELETE" });
      load();
    } catch (e) {
      setErr(errText(t, e));
    }
  };

  const host = pair?.payload.hosts[0] ?? status.addresses?.[0];
  const left = Math.max(0, Math.round((expiresAt - now) / 1000));
  const expired = pair !== null && left === 0;
  const installUrl = host && pair ? `http://${host}:${pair.payload.install_port}/get` : null;

  return (
    <div className="stack">
      <div className="row between wrap">
        <h1>{t("household")}</h1>
        <select value={lang} onChange={(e) => setLang(e.target.value as Lang)} style={{ width: "auto" }} aria-label={t("language")}>
          <option value="en">{t("english")}</option>
          <option value="sr">{t("serbian")}</option>
        </select>
      </div>
      {err && <p className="error" role="alert">{err}</p>}

      {isHub &&
        (pair ? (
          <div className="panel stack pair-panel">
            <h2>{t("pairTitle")}</h2>
            <p>{t("pairStep1")}</p>
            {installUrl && <Qr value={installUrl} size={180} label={t("qrDownload")} />}
            <p className="value" style={{ fontSize: 18, wordBreak: "break-all" }}>{installUrl ?? "–"}</p>
            <p>{t("pairStep2")}</p>
            {expired ? (
              <p className="warn">{t("codeExpired")}</p>
            ) : (
              <>
                <Qr value={JSON.stringify(pair.payload)} label={t("qrPair")} />
                <p className="muted">{t("pairCode")}</p>
                <div className="code" aria-label={t("pairCode")}>{pair.code}</div>
                <p className="muted" style={{ fontSize: 14 }}>
                  {t("securityCode")}: <strong className="sec-code">{securityCode(pair.payload.fp)}</strong>
                </p>
                <p className="muted" style={{ fontSize: 14 }}>
                  {t("codeValidFor")} {Math.floor(left / 60)}:{String(left % 60).padStart(2, "0")}
                </p>
              </>
            )}
            <div className="row actions">
              <button className="btn" onClick={start}>{t("newCode")}</button>
              <button className="btn secondary" onClick={() => setPair(null)}>{t("cancel")}</button>
            </div>
          </div>
        ) : (
          <div className="row">
            <button className="btn" onClick={start}>{t("addDevice")}</button>
            {paired && <span className="ok">{t("devicePaired")}</span>}
          </div>
        ))}

      <div>
        <h2>{t("pairedDevices")}</h2>
        {devices.length === 0 ? (
          <p className="muted">{t("noDevices")}</p>
        ) : (
          <div className="list">
            {devices.map((d) => {
              const own = d.id === ownDeviceId;
              return (
                <div className="item wrap" key={d.id}>
                  <div>
                    <div>
                      {d.name}
                      {own && <span className="muted"> · {t("thisDevice")}</span>}
                    </div>
                    <div className="muted" style={{ fontSize: 13 }}>
                      {d.platform} · {t("lastSeen")}: {d.last_seen ? fmtDateTime(d.last_seen) : t("never")}
                    </div>
                  </div>
                  {!own && (
                    <ConfirmButton label={t("remove")} confirmLabel={t("yesRemove")} cancelLabel={t("cancel")} onConfirm={() => remove(d)} />
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
      {isHub && (
        <div className="panel">
          <div className="label">{t("dataFolder")}</div>
          <div style={{ wordBreak: "break-all" }}>{status.root ?? "–"}</div>
        </div>
      )}
    </div>
  );
}
