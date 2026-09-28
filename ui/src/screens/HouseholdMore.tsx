import { useEffect, useState } from "react";
import { api, type Status } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes, latinArticles, setLatinArticles } from "../format";
import { Brand } from "../components/Brand";

// The panels Household's category pages are built from (see Household.tsx).
// A setting the search can jump to carries the id "set-<setting>" (settings.ts).

type T = (k: Key) => string;

/** Amber is the Zaklon color and the default. */
export const ACCENTS = ["amber", "green", "white", "purple", "blue"] as const;
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
    <div className="set-rows">
      <div id="set-accent" className="set-anchor set-row" tabIndex={-1}>
        <div className="set-row-text">
          <span className="set-row-title" id="accent-title">{t("accentColor")}</span>
          <span className="set-row-desc">{t("lookThisDevice")}</span>
        </div>
        <div className="swatches" role="radiogroup" aria-labelledby="accent-title">
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
      </div>
      <label id="set-pure-black" className="set-anchor set-row check-line" tabIndex={-1}>
        <input type="checkbox" checked={look.oled} onChange={(e) => look.setOled(e.target.checked)} />
        <span className="set-row-title">{t("pureBlack")}</span>
      </label>
    </div>
  );
}

/** The app's language on this device, and the script Serbian articles are shown in. */
export function LanguageSettings({ t, lang, setLang }: { t: T; lang: Lang; setLang: (l: Lang) => void }) {
  // Not chosen yet: Latin follows the app's language (see latinArticles).
  const [latin, setLatin] = useState<boolean | null>(null);
  const shown = latin ?? latinArticles(lang);
  const toggleLatin = (on: boolean) => {
    setLatin(on);
    setLatinArticles(on);
  };
  return (
    <div className="set-rows">
      <div id="set-language" className="set-anchor set-row" tabIndex={-1}>
        <div className="set-row-text">
          <label className="set-row-title" htmlFor="app-language">{t("language")}</label>
          <span className="set-row-desc" id="app-language-hint">{t("languageHint")}</span>
        </div>
        <select id="app-language" className="set-row-select" value={lang} onChange={(e) => setLang(e.target.value as Lang)} aria-describedby="app-language-hint">
          <option value="en">{t("english")}</option>
          <option value="sr">{t("serbian")}</option>
        </select>
      </div>
      <label id="set-latin" className="set-anchor set-row check-line" tabIndex={-1}>
        <input type="checkbox" checked={shown} onChange={(e) => toggleLatin(e.target.checked)} />
        <span className="set-row-title">{t("latinArticles")}</span>
      </label>
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

/** What stays on the hub and the phones, and when Zaklon goes online. */
export function Privacy({ t }: { t: T }) {
  return (
    <div className="panel stack left">
      <h2>{t("privacy")}</h2>
      <ul className="plain">
        <li>{t("privacy1")}</li>
        <li>{t("privacy2")}</li>
        <li>{t("privacy3")}</li>
        <li>{t("privacy4")}</li>
      </ul>
    </div>
  );
}

/** Where phones on the same network find this laptop. */
export function NetworkAddresses({ t, status }: { t: T; status: Status }) {
  const list = status.addresses ?? [];
  return (
    <div className="panel stack left">
      <h2>{t("networkAddresses")}</h2>
      <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("addressesIntro")}</p>
      {list.length === 0 ? (
        <p style={{ margin: 0 }}>–</p>
      ) : (
        <div className="row wrap" style={{ gap: 8 }}>
          {list.map((a) => (
            <code className="url" key={a}>{a}</code>
          ))}
        </div>
      )}
    </div>
  );
}

type ModelChoice = { id: string; title_en: string; title_sr: string; size: number; installed: boolean; recommended: boolean };
type AiOverview = { selected: string | null; recommended: string; ram_total: number; models: ModelChoice[] };

/** Which of the hub's AI models the assistant uses (the whole household's choice). */
export function AiModel({ t, lang, isHub }: { t: T; lang: Lang; isHub: boolean }) {
  const [ov, setOv] = useState<AiOverview | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    api<AiOverview>("/api/assistant")
      .then(setOv)
      .catch((e) => setErr(errText(t, e)));
  }, [t]);

  const select = async (id: string) => {
    setBusy(true);
    setErr(null);
    try {
      await api("/api/assistant/model", { json: { id } });
      setOv(await api<AiOverview>("/api/assistant"));
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  const title = (m: ModelChoice) => (lang === "sr" ? m.title_sr : m.title_en);
  const installed = ov?.models.filter((m) => m.installed) ?? [];
  const rec = ov?.models.find((m) => m.id === ov.recommended);
  return (
    <div className="panel stack left">
      <h2>{t("catModels")}</h2>
      {err && <p className="error" role="alert">{err}</p>}
      {ov &&
        (installed.length === 0 ? (
          <p style={{ margin: 0 }}>{t("aiNeedsModel")}.</p>
        ) : (
          <label className="field">
            {t("aiModel")}
            <select value={ov.selected ?? ""} onChange={(e) => select(e.target.value)} disabled={busy}>
              {installed.map((m) => (
                <option key={m.id} value={m.id}>
                  {title(m)}
                  {m.recommended ? ` · ${t("recommended")}` : ""}
                </option>
              ))}
            </select>
          </label>
        ))}
      {ov && rec && (
        <p className="muted" style={{ margin: 0, fontSize: 14 }}>
          {t("aiRecommendedFor")} {fmtBytes(ov.ram_total)} {t("aiRecommendedMemory")}: <strong>{title(rec)}</strong> ({fmtBytes(rec.size)})
        </p>
      )}
      {isHub ? (
        <div>
          <a className="btn secondary" href="#addons">{t("goToAddons")}</a>
        </div>
      ) : (
        <p className="muted" style={{ margin: 0, fontSize: 14 }}>{t("aiModelsOnLaptop")}</p>
      )}
    </div>
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

/** Zaklon itself: the version, the license and where the source is. */
export function AboutZaklon({ t, status }: { t: T; status: Status | null }) {
  return (
    <div className="panel stack left">
      <Brand size={36} className="about-brand" />
      <p style={{ margin: 0 }}>Zaklon {status?.version ?? ""} · {t("aboutFree")}</p>
      <p className="muted" style={{ fontSize: 14, margin: 0 }}>{t("aboutSource")} github.com/stefan-cirovic/zaklon · zaklon.com</p>
    </div>
  );
}

const THIRD_PARTY: { name: string; role: Key; license: string }[] = [
  { name: "Kiwix (kiwix-serve)", role: "tpKiwix", license: "GPL-3.0-or-later" },
  { name: "llama.cpp", role: "tpLlama", license: "MIT" },
  { name: "CoMaps", role: "tpComaps", license: "Apache-2.0" },
  { name: "zxing-cpp", role: "tpZxing", license: "Apache-2.0" },
  { name: "OpenStreetMap", role: "tpOsm", license: "ODbL 1.0" },
  { name: "Wikipedia, Wiktionary", role: "tpWikipedia", license: "CC BY-SA 4.0" },
  { name: "Qwen, Gemma", role: "tpModels", license: "Apache-2.0" },
  { name: "Tauri, React, Rust", role: "tpFrameworks", license: "MIT / Apache-2.0" },
  { name: "Sora", role: "tpFont", license: "OFL-1.1" },
];

/** The other projects Zaklon builds on, each under its own license. */
export function Licenses({ t }: { t: T }) {
  return (
    <div className="panel stack left">
      <h2>{t("licenses")}</h2>
      <p className="muted" style={{ fontSize: 14, margin: 0 }}>{t("licensesIntro")}</p>
      <ul className="plain">
        {THIRD_PARTY.map((x) => (
          <li key={x.name}>
            <strong>{x.name}</strong> · {t(x.role)} · <span className="muted">{x.license}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
