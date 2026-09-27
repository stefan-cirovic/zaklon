import { useEffect, useState } from "react";
import { api, type Status } from "../api";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";
import { UpdateSettings } from "../components/Updates";

type T = (k: Key) => string;

export const ACCENTS = ["green", "white", "purple", "blue", "amber"] as const;
export type Accent = (typeof ACCENTS)[number];
const ACCENT_KEY: Record<Accent, Key> = {
  green: "accentGreen",
  white: "accentWhite",
  purple: "accentPurple",
  blue: "accentBlue",
  amber: "accentAmber",
};

export type Look = { accent: Accent; setAccent: (a: Accent) => void; oled: boolean; setOled: (on: boolean) => void };

/** Accent color and pure black, remembered on this device. */
export function Appearance({ t, look }: { t: T; look: Look }) {
  return (
    <div className="panel stack left">
      <h2>{t("appearance")}</h2>
      <div className="label">{t("accentColor")}</div>
      <div className="swatches" role="radiogroup" aria-label={t("accentColor")}>
        {ACCENTS.map((a) => (
          <button
            key={a}
            role="radio"
            aria-checked={look.accent === a}
            aria-label={t(ACCENT_KEY[a])}
            title={t(ACCENT_KEY[a])}
            className={`swatch swatch-${a}${look.accent === a ? " on" : ""}`}
            onClick={() => look.setAccent(a)}
          />
        ))}
      </div>
      <label className="check-line">
        <input type="checkbox" checked={look.oled} onChange={(e) => look.setOled(e.target.checked)} />
        <span>{t("pureBlack")}</span>
      </label>
      <p className="muted" style={{ fontSize: 13, margin: 0 }}>{t("lookThisDevice")}</p>
    </div>
  );
}

/** Anyone sitting at the laptop may change the household password. */
export function ChangePassword({ t }: { t: T }) {
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [done, setDone] = useState(false);
  const [busy, setBusy] = useState(false);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setDone(false);
    if (pw !== pw2) {
      setErr(t("passwordsDiffer"));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await api("/api/password", { json: { new_password: pw } });
      setDone(true);
      setPw("");
      setPw2("");
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="panel stack left" onSubmit={submit}>
      <h2>{t("householdPassword")}</h2>
      <p className="muted" style={{ fontSize: 14, margin: 0 }}>{t("changePasswordIntro")}</p>
      <label className="field">
        {t("newPassword")}
        <input type="password" value={pw} onChange={(e) => setPw(e.target.value)} minLength={8} required autoComplete="new-password" />
      </label>
      <label className="field">
        {t("passwordAgain")}
        <input type="password" value={pw2} onChange={(e) => setPw2(e.target.value)} minLength={8} required autoComplete="new-password" />
      </label>
      {err && <p className="error" role="alert">{err}</p>}
      {done && <p className="ok" role="status">{t("passwordChanged")}</p>}
      <div>
        <button className="btn" disabled={busy || pw.length < 8}>{t("changePasswordBtn")}</button>
      </div>
    </form>
  );
}

type Hardware = { cpu: string; cores: number; ram_total: number; ram_free: number; os: string };
type System = { disk_free: number; disk_total: number; battery_percent: number | null; plugged_in: boolean };

/** What the hub runs on. */
export function ThisHub({ t, status, isHub }: { t: T; status: Status; isHub: boolean }) {
  const [hw, setHw] = useState<Hardware | null>(null);
  const [sys, setSys] = useState<System | null>(null);
  useEffect(() => {
    api<Hardware>("/api/hardware").then(setHw).catch(() => {});
    api<System>("/api/system").then(setSys).catch(() => {});
  }, []);
  const rows: [Key, string][] = [
    ["hubName", status.hub_name],
    ["version", status.version],
  ];
  if (hw) {
    rows.push(["computer", hw.os]);
    if (hw.cpu) rows.push(["processor", `${hw.cpu} · ${hw.cores} ${t("threads")}`]);
    rows.push(["memory", `${fmtBytes(hw.ram_total)} (${fmtBytes(hw.ram_free)} ${t("free")})`]);
  }
  if (sys) rows.push(["diskFree", `${fmtBytes(sys.disk_free)} ${t("of")} ${fmtBytes(sys.disk_total)}`]);
  if (isHub && status.addresses?.length) rows.push(["networkAddresses", status.addresses.join(", ")]);
  if (isHub && status.root) rows.push(["dataFolder", status.root]);
  return (
    <div className="panel stack left">
      <h2>{t("thisHub")}</h2>
      <dl className="facts">
        {rows.map(([k, v]) => (
          <div key={k}>
            <dt>{t(k)}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

const THIRD_PARTY: { name: string; role: Key; license: string }[] = [
  { name: "Kiwix (kiwix-serve)", role: "tpKiwix", license: "GPL-3.0-or-later" },
  { name: "llama.cpp", role: "tpLlama", license: "MIT" },
  { name: "CoMaps", role: "tpComaps", license: "Apache-2.0" },
  { name: "OpenStreetMap", role: "tpOsm", license: "ODbL 1.0" },
  { name: "Wikipedia, Wiktionary", role: "tpWikipedia", license: "CC BY-SA 4.0" },
  { name: "Qwen, Gemma", role: "tpModels", license: "Apache-2.0" },
  { name: "Tauri, React, Rust", role: "tpFrameworks", license: "MIT / Apache-2.0" },
];

/** Version, license, privacy and the other projects Zaklon builds on. */
export function About({ t, status, isHub }: { t: T; status: Status; isHub: boolean }) {
  return (
    <div className="panel stack left">
      <h2>{t("about")}</h2>
      <p style={{ margin: 0 }}>Zaklon {status.version} · {t("aboutFree")}</p>
      <p className="muted" style={{ fontSize: 14, margin: 0 }}>{t("aboutSource")} github.com/stefan-cirovic/zaklon · zaklon.com</p>
      <UpdateSettings t={t} isHub={isHub} />
      <details>
        <summary>{t("privacy")}</summary>
        <ul className="plain">
          <li>{t("privacy1")}</li>
          <li>{t("privacy2")}</li>
          <li>{t("privacy3")}</li>
          <li>{t("privacy4")}</li>
        </ul>
      </details>
      <details>
        <summary>{t("licenses")}</summary>
        <p className="muted" style={{ fontSize: 14 }}>{t("licensesIntro")}</p>
        <ul className="plain">
          {THIRD_PARTY.map((x) => (
            <li key={x.name}>
              <strong>{x.name}</strong> · {t(x.role)} · <span className="muted">{x.license}</span>
            </li>
          ))}
        </ul>
      </details>
    </div>
  );
}
