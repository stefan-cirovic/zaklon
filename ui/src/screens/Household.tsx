import { useEffect, useState } from "react";
import { api, type Device, type PairStart, type Status } from "../api";
import type { Key, Lang } from "../i18n";
import Qr from "../components/Qr";

type T = (k: Key) => string;
type Props = { status: Status | null; t: T; lang: Lang; setLang: (l: Lang) => void; refresh: () => void };

export default function Household({ status, t, lang, setLang, refresh }: Props) {
  if (!status) return <p className="muted">…</p>;
  if (!status.set_up) {
    return <Setup t={t} lang={lang} setLang={setLang} onDone={refresh} defaultName={status.hub_name} />;
  }
  return <Devices t={t} status={status} lang={lang} setLang={setLang} />;
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
      setErr((ex as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="stack" onSubmit={submit} style={{ maxWidth: 480 }}>
      <div>
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
        <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} minLength={8} required />
      </label>
      <label className="field">
        {t("passwordAgain")}
        <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} minLength={8} required />
      </label>
      <p className="muted" style={{ fontSize: 14 }}>{t("passwordRule")}</p>
      {err && <p className="error">{err}</p>}
      <button className="btn" disabled={busy || pw.length < 8}>{t("finish")}</button>
    </form>
  );
}

type DevicesProps = { t: T; status: Status; lang: Lang; setLang: (l: Lang) => void };

function Devices({ t, status, lang, setLang }: DevicesProps) {
  const [devices, setDevices] = useState<Device[]>([]);
  const [pair, setPair] = useState<PairStart | null>(null);
  const [paired, setPaired] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const load = () =>
    api<Device[]>("/api/devices")
      .then(setDevices)
      .catch((e) => setErr((e as Error).message));

  useEffect(() => {
    load();
  }, []);

  useEffect(() => {
    if (!pair) return;
    const before = devices.length;
    const id = setInterval(async () => {
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
    return () => clearInterval(id);
  }, [pair, devices.length]);

  const start = async () => {
    setErr(null);
    setPaired(false);
    try {
      setPair(await api<PairStart>("/api/pair/start", { method: "POST" }));
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  const remove = async (d: Device) => {
    try {
      await api(`/api/devices/${d.id}`, { method: "DELETE" });
      load();
    } catch (e) {
      setErr((e as Error).message);
    }
  };

  const host = pair?.payload.hosts[0] ?? status.addresses?.[0];

  return (
    <div className="stack">
      <div className="row between">
        <h1>{t("household")}</h1>
        <select value={lang} onChange={(e) => setLang(e.target.value as Lang)} style={{ width: "auto" }}>
          <option value="en">{t("english")}</option>
          <option value="sr">{t("serbian")}</option>
        </select>
      </div>
      {err && <p className="error">{err}</p>}
      {pair ? (
        <div className="panel stack">
          <h2>{t("pairTitle")}</h2>
          <p>{t("pairStep1")}</p>
          <p className="value" style={{ fontSize: 20 }}>
            {host ? `http://${host}:${pair.payload.install_port}/get` : "–"}
          </p>
          <p>{t("pairStep2")}</p>
          <Qr value={JSON.stringify(pair.payload)} />
          <p className="muted">{t("pairCode")}</p>
          <div className="code">{pair.code}</div>
          <p className="muted" style={{ fontSize: 14 }}>{t("pairExpires")}</p>
          <div>
            <button className="btn secondary" onClick={() => setPair(null)}>{t("cancel")}</button>
          </div>
        </div>
      ) : (
        <div className="row">
          <button className="btn" onClick={start}>{t("addDevice")}</button>
          {paired && <span className="ok">{t("devicePaired")}</span>}
        </div>
      )}
      <div>
        <h2>{t("pairedDevices")}</h2>
        {devices.length === 0 ? (
          <p className="muted">{t("noDevices")}</p>
        ) : (
          <div className="list">
            {devices.map((d) => (
              <div className="item" key={d.id}>
                <div>
                  <div>{d.name}</div>
                  <div className="muted" style={{ fontSize: 13 }}>
                    {d.platform} · {t("lastSeen")}: {d.last_seen ? new Date(d.last_seen).toLocaleString() : t("never")}
                  </div>
                </div>
                <button className="btn danger" onClick={() => remove(d)}>{t("remove")}</button>
              </div>
            ))}
          </div>
        )}
      </div>
      <div className="panel">
        <div className="label">{t("dataFolder")}</div>
        <div>{status.root ?? "–"}</div>
      </div>
    </div>
  );
}
