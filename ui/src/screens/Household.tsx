import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, type Device, type PairStart, type Status } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtDateTime, securityCode } from "../format";
import ConfirmButton from "../components/ConfirmButton";
import Qr from "../components/Qr";
import Backups, { SetupRestore } from "../components/Backups";
import Hotspot from "../components/Hotspot";
import Firewall from "../components/Firewall";
import Memory from "../components/Memory";
import { UpdateSettings, type UpdateState } from "../components/Updates";
import { Brand, Mark } from "../components/Brand";
import { SettingsIcon } from "../components/SettingsIcon";
import { categoriesFor, categoryOf, findSettings, householdRoute, settingsHref, type Category, type CategoryId, type Setting } from "../settings";
import { AboutZaklon, AiModel, Appearance, ChangePassword, LanguageSettings, Licenses, NetworkAddresses, Privacy, ThisHub, type Look } from "./HouseholdMore";

type T = (k: Key) => string;
type Props = {
  status: Status | null;
  /** The hub answers now (the status may be the last one it gave). */
  online: boolean;
  t: T;
  lang: Lang;
  setLang: (l: Lang) => void;
  refresh: () => void;
  /** True on the laptop; phones cannot pair other phones or see the data folder. */
  isHub: boolean;
  /** This phone's own device id (phones only). */
  ownDeviceId: string | null;
  look: Look;
  /** A phone's link to its hub, shown even while the hub does not answer. */
  top?: React.ReactNode;
};

/** How long (in 150 ms steps) a setting opened by its address is kept in view while the page loads. */
const KEEP_TICKS = 12;
/** The space above a setting scrolled to (scroll-margin-top of .set-anchor). */
const SCROLL_MARGIN = 20;

/** The page and setting in the address (#household/backups), following the back and forward buttons. */
function useRoute() {
  const [route, setRoute] = useState(() => householdRoute(location.hash));
  useEffect(() => {
    const onHash = () => setRoute(householdRoute(location.hash));
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  return route;
}

/**
 * Household, laid out like Windows Settings: the hub at the top, a search,
 * and a tile for each category; a category opens as its own page
 * (#household/<category>) with the other categories beside it on a laptop.
 * A phone sees only what applies to it. First run shows the setup instead.
 */
export default function Household({ status, online, t, lang, setLang, refresh, isHub, ownDeviceId, look, top }: Props) {
  const route = useRoute();
  const cats = useMemo(() => categoriesFor(isHub), [isHub]);
  const cat = cats.find((c) => c.id === route.cat) ?? null;
  const [query, setQuery] = useState("");
  // A search result for the page and setting already shown: open it again.
  const [jump, setJump] = useState(0);
  // Back on the tiles, the one just left has the focus (as in Windows Settings).
  const here = cat?.id ?? null;
  const [shown, setShown] = useState({ here, setting: route.setting });
  const [from, setFrom] = useState<CategoryId | null>(null);
  if (shown.here !== here || shown.setting !== route.setting) {
    setShown({ here, setting: route.setting });
    setFrom(here ? null : shown.here);
    setQuery("");
  }

  const landed = useCallback(() => setFrom(null), []);
  const go = useCallback((s: Setting) => {
    const href = settingsHref(s.cat, s.id);
    setQuery("");
    if (location.hash === href) setJump((n) => n + 1);
    else location.hash = href.slice(1);
  }, []);

  if (status?.set_up === false) {
    return isHub ? (
      <Setup t={t} lang={lang} setLang={setLang} onDone={refresh} defaultName={status.hub_name} />
    ) : (
      <div className="stack">
        {top}
        <p className="muted">{t("hubNotSetUp")}</p>
      </div>
    );
  }

  const search = (children: ReactNode) => (
    <SettingsSearch t={t} isHub={isHub} query={query} setQuery={setQuery} go={go}>
      {children}
    </SettingsSearch>
  );

  if (!cat) {
    return (
      <div className="stack settings-home">
        <h1>{t("household")}</h1>
        <HubCard t={t} status={status} online={online} isHub={isHub} />
        {isHub && status && <Firewall t={t} />}
        {search(<Tiles t={t} cats={cats} isHub={isHub} from={from} landed={landed} />)}
      </div>
    );
  }

  // Parts that need the hub wait for its first answer.
  const hub = (node: ReactNode) => (status ? node : <p className="muted wide">{t("loadingOrUnavailable")}</p>);
  let body: ReactNode;
  switch (cat.id) {
    case "devices":
      body = (
        <>
          {top && <Anchor id="hub-link" wide>{top}</Anchor>}
          {hub(status && <Devices t={t} status={status} isHub={isHub} ownDeviceId={ownDeviceId} />)}
        </>
      );
      break;
    case "network":
      body = hub(
        status && (
          <>
            <Anchor id="hotspot" wide><Hotspot t={t} /></Anchor>
            <Anchor id="firewall"><Firewall t={t} showOk /></Anchor>
            <Anchor id="addresses"><NetworkAddresses t={t} status={status} /></Anchor>
          </>
        ),
      );
      break;
    case "backups":
      body = hub(<div className="wide"><Backups t={t} /></div>);
      break;
    case "privacy":
      body = (
        <>
          {isHub && hub(<Anchor id="password"><ChangePassword t={t} /></Anchor>)}
          <Anchor id="privacy"><Privacy t={t} /></Anchor>
        </>
      );
      break;
    case "appearance":
      body = <div className="wide"><Appearance t={t} look={look} /></div>;
      break;
    case "language":
      body = <div className="wide"><LanguageSettings t={t} lang={lang} setLang={setLang} /></div>;
      break;
    case "assistant":
      body = hub(
        <>
          <Anchor id="model"><AiModel t={t} lang={lang} isHub={isHub} /></Anchor>
          <Anchor id="memory"><Memory t={t} version={0} expanded /></Anchor>
        </>,
      );
      break;
    case "updates":
      body = hub(
        <Anchor id="updates" wide>
          <div className="panel left"><UpdateSettings t={t} isHub={isHub} /></div>
        </Anchor>,
      );
      break;
    case "about":
      body = (
        <>
          <Anchor id="version"><AboutZaklon t={t} status={status} /></Anchor>
          {status && <Anchor id="this-hub"><ThisHub t={t} status={status} isHub={isHub} /></Anchor>}
          <Anchor id="licenses" wide><Licenses t={t} /></Anchor>
        </>
      );
      break;
  }

  return (
    <CategoryPage t={t} cat={cat} setting={route.setting} jump={jump}
      side={
        <>
          <HubMini t={t} status={status} online={online} />
          {search(<CategoryNav t={t} cats={cats} current={cat.id} />)}
        </>
      }
    >
      {body}
    </CategoryPage>
  );
}

/** A setting's place on its page: where a search result or an address like #household/network/hotspot opens it. */
function Anchor({ id, wide, children }: { id: string; wide?: boolean; children: ReactNode }) {
  return (
    <div id={`set-${id}`} className={"set-anchor" + (wide ? " wide" : "")} tabIndex={-1}>
      {children}
    </div>
  );
}

/** The top of Household: the hub, whether it runs, and a few facts about it. */
function HubCard({ t, status, online, isHub }: { t: T; status: Status | null; online: boolean; isHub: boolean }) {
  const [update, setUpdate] = useState<UpdateState | null>(null);
  // undefined: not known yet; null: there is none.
  const [lastBackup, setLastBackup] = useState<string | null | undefined>(undefined);
  const answered = status !== null;
  useEffect(() => {
    if (!answered) return;
    api<UpdateState>("/api/updates").then(setUpdate).catch(() => {});
    if (isHub) {
      api<{ backups: { created: string }[] }>("/api/backups")
        .then((r) => setLastBackup(r.backups[0]?.created ?? null))
        .catch(() => {});
    }
  }, [answered, isHub]);

  const facts: [string, ReactNode][] = [[t("devices"), status?.devices ?? "–"]];
  if (isHub) facts.push([t("addresses"), status?.addresses?.join(", ") || "–"]);
  facts.push([
    t("version"),
    <>
      {status?.version ?? "–"}
      {update?.newer && update.latest && (
        <>
          {" · "}
          <a href={settingsHref("updates", "updates")}>{t("newVersionFact")} {update.latest}</a>
        </>
      )}
    </>,
  ]);
  if (isHub) facts.push([t("lastBackup"), lastBackup ? fmtDateTime(lastBackup) : lastBackup === null ? t("noBackupYetShort") : "–"]);

  return (
    <section className="hub-card" aria-label={t("hubStatus")}>
      <div className="hub-id">
        <span className="hub-avatar">
          <Mark size={44} />
        </span>
        <div className="hub-id-text">
          <div className="hub-name">{status?.hub_name ?? "Zaklon"}</div>
          <div className="hub-state">
            <span className={"status-dot" + (online ? "" : " off")} aria-hidden="true" /> {online ? t("online") : t("offline")}
          </div>
        </div>
      </div>
      <dl className="hub-facts">
        {facts.map(([k, v]) => (
          <div key={k}>
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

/** The hub, small, at the top of the list of categories (laptop). */
function HubMini({ t, status, online }: { t: T; status: Status | null; online: boolean }) {
  return (
    <div className="hub-mini">
      <Mark size={32} />
      <div style={{ minWidth: 0 }}>
        <div className="hub-mini-name">{status?.hub_name ?? "Zaklon"}</div>
        <div className="hub-state">
          <span className={"status-dot" + (online ? "" : " off")} aria-hidden="true" /> {online ? t("online") : t("offline")}
        </div>
      </div>
    </div>
  );
}

type TilesProps = {
  t: T;
  cats: Category[];
  isHub: boolean;
  /** The category just left: its tile gets the focus, once (then `landed`). */
  from: CategoryId | null;
  landed: () => void;
};

function Tiles({ t, cats, isHub, from, landed }: TilesProps) {
  const list = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!from) return;
    list.current?.querySelector<HTMLElement>(`[data-cat="${from}"]`)?.focus();
    landed();
  }, [from, landed]);
  return (
    <ul className="set-tiles" aria-label={t("settingsCategories")} ref={list}>
      {cats.map((c) => (
        <li key={c.id}>
          <a className="set-tile" href={settingsHref(c.id)} data-cat={c.id}>
            <span className="set-tile-icon">
              <SettingsIcon name={c.id} size={24} />
            </span>
            <span className="set-tile-text">
              <span className="set-tile-title">{t(c.title)}</span>
              <span className="set-tile-desc">{t(!isHub && c.descPhone ? c.descPhone : c.desc)}</span>
            </span>
            <span className="set-tile-go">
              <SettingsIcon name="chevron" size={18} />
            </span>
          </a>
        </li>
      ))}
    </ul>
  );
}

function CategoryNav({ t, cats, current }: { t: T; cats: Category[]; current: CategoryId }) {
  return (
    <nav aria-label={t("settingsCategories")}>
      <ul className="set-nav">
        {cats.map((c) => (
          <li key={c.id}>
            <a href={settingsHref(c.id)} aria-current={c.id === current ? "page" : undefined}>
              <SettingsIcon name={c.id} size={18} />
              <span>{t(c.title)}</span>
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}

type SearchProps = {
  t: T;
  isHub: boolean;
  query: string;
  setQuery: (q: string) => void;
  go: (s: Setting) => void;
  /** Shown while nothing is typed: the tiles, or the list of categories. */
  children: ReactNode;
};

/**
 * "Find a setting": finds settings by name and keywords, in English and
 * Serbian, and opens the page at the one chosen. Enter opens the first; the
 * arrow keys move through the results.
 */
function SettingsSearch({ t, isHub, query, setQuery, go, children }: SearchProps) {
  const results = useMemo(() => findSettings(query, isHub, t), [query, isHub, t]);
  const box = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const typed = query.trim() !== "";

  const links = () => [...(list.current?.querySelectorAll<HTMLAnchorElement>("a") ?? [])];
  const onBoxKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && results[0]) {
      e.preventDefault();
      go(results[0]);
    } else if (e.key === "ArrowDown" && results.length > 0) {
      e.preventDefault();
      links()[0]?.focus();
    } else if (e.key === "Escape" && query) {
      e.preventDefault();
      setQuery("");
    }
  };
  const onListKey = (e: React.KeyboardEvent) => {
    const all = links();
    const i = all.indexOf(document.activeElement as HTMLAnchorElement);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      all[Math.min(i + 1, all.length - 1)]?.focus();
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      if (i <= 0) box.current?.focus();
      else all[i - 1]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      setQuery("");
      box.current?.focus();
    }
  };

  return (
    <div className="set-search-area">
      <div className="set-search" role="search">
        <SettingsIcon name="search" size={18} />
        <input
          ref={box}
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onBoxKey}
          placeholder={t("findSetting")}
          aria-label={t("findSetting")}
          autoComplete="off"
          spellCheck={false}
        />
      </div>
      <p role="status" className={typed && results.length === 0 ? "muted set-none" : "sr-only"}>
        {!typed ? "" : results.length === 0 ? t("noSettingFound") : `${t("settingsFound")} ${results.length}`}
      </p>
      {!typed
        ? children
        : results.length > 0 && (
            <ul className="set-results" ref={list} onKeyDown={onListKey}>
              {results.map((s) => (
                <li key={s.id}>
                  <a
                    href={settingsHref(s.cat, s.id)}
                    onClick={(e) => {
                      if (e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
                      e.preventDefault();
                      go(s);
                    }}
                  >
                    <span className="set-result-icon">
                      <SettingsIcon name={s.cat} size={20} />
                    </span>
                    <span className="set-result-text">
                      <span className="set-result-title">{t(s.title)}</span>
                      <span className="set-result-cat">{t(categoryOf(s.cat).title)}</span>
                    </span>
                  </a>
                </li>
              ))}
            </ul>
          )}
    </div>
  );
}

type PageProps = {
  t: T;
  cat: Category;
  /** The setting to open the page at (from the address). */
  setting: string | null;
  /** Changes when the same setting is asked for again. */
  jump: number;
  /** The list of categories beside the page (laptop). */
  side: ReactNode;
  children: ReactNode;
};

/** One category: "Household › Backups" with a way back, its settings, and the other categories beside it on a laptop. */
function CategoryPage({ t, cat, setting, jump, side, children }: PageProps) {
  const title = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    const find = () => (setting ? document.getElementById(`set-${setting}`) : null);
    const show = (el: HTMLElement) => {
      el.scrollIntoView({ block: "start" });
      el.focus({ preventScroll: true });
      el.classList.remove("flash");
      void el.offsetWidth; // restart the highlight
      el.classList.add("flash");
    };
    // A new page starts at the top with its title in focus, or at the setting asked for.
    let target = find();
    if (target) show(target);
    else {
      window.scrollTo(0, 0);
      title.current?.focus({ preventScroll: true });
    }
    if (!setting) return;
    // What loads a moment later (the setting itself, or what is above it) moves
    // it: keep it in place for a moment, unless the person scrolls or types.
    let ticks = 0;
    const stop = () => clearInterval(timer);
    const timer = setInterval(() => {
      if (++ticks > KEEP_TICKS) return stop();
      const el = find();
      if (!el) return;
      if (el !== target) {
        target = el;
        show(el);
        return;
      }
      // It could not take the focus while it was empty (hidden).
      if (document.activeElement === document.body) el.focus({ preventScroll: true });
      if (Math.abs(el.getBoundingClientRect().top - SCROLL_MARGIN) > 4) el.scrollIntoView({ block: "start" });
    }, 150);
    const events = ["wheel", "touchstart", "keydown"] as const;
    for (const e of events) window.addEventListener(e, stop, { passive: true });
    return () => {
      stop();
      for (const e of events) window.removeEventListener(e, stop);
    };
  }, [cat.id, setting, jump]);

  return (
    <div className="set-page">
      <aside className="set-side">{side}</aside>
      <div className="set-main">
        <div className="set-head">
          <a className="set-back" href="#household" aria-label={t("backToHousehold")} title={t("backToHousehold")}>
            <SettingsIcon name="back" size={20} />
          </a>
          <nav className="set-crumbs" aria-label={t("breadcrumb")}>
            <a href="#household">{t("household")}</a>
            <span aria-hidden="true">›</span>
          </nav>
          <h1 ref={title} tabIndex={-1}>{t(cat.title)}</h1>
        </div>
        <div className="set-panels" key={cat.id}>
          {children}
        </div>
      </div>
    </div>
  );
}

type SetupProps = { t: T; lang: Lang; setLang: (l: Lang) => void; onDone: () => void; defaultName: string };

/** The hub's suggested name, in the chosen language ("Zaklon on PC" / "Zaklon na PC"). */
function localName(name: string, lang: Lang) {
  return lang === "sr" ? name.replace(/^Zaklon on /, "Zaklon na ") : name.replace(/^Zaklon na /, "Zaklon on ");
}

function Setup({ t, lang, setLang, onDone, defaultName }: SetupProps) {
  const [typed, setTyped] = useState<string | null>(null);
  const name = typed ?? localName(defaultName, lang);
  const setName = (v: string) => setTyped(v);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [checkUpdates, setCheckUpdates] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // A backup of the previous hub is prepared instead: nothing to set up.
  const [restoring, setRestoring] = useState(false);
  const onRestoreReady = useCallback(() => setRestoring(true), []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (pw !== pw2) {
      setErr(t("passwordsDiffer"));
      return;
    }
    setBusy(true);
    setErr(null);
    try {
      await api("/api/setup", { json: { password: pw, hub_name: name, language: lang, check_updates: checkUpdates } });
      onDone();
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setBusy(false);
    }
  };

  const head = (
    <div className="page-head centered">
      <div className="brand-hero">
        <Brand size={64} layout="column" />
      </div>
      <h1>{t("setupTitle")}</h1>
      {!restoring && <p className="muted">{t("setupIntro")}</p>}
    </div>
  );

  // The restore panel stays in the same place, so it keeps its state when
  // the form steps aside.
  return (
    <div className="stack form setup">
      {restoring ? (
        head
      ) : (
        <form className="stack" onSubmit={submit}>
          {head}
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
          <label className="check-line">
            <input type="checkbox" checked={checkUpdates} onChange={(e) => setCheckUpdates(e.target.checked)} />
            <span>
              {t("updateSwitch")}
              <span className="muted" style={{ display: "block", fontSize: 14 }}>{t("setupUpdatesHint")}</span>
            </span>
          </label>
          {err && <p className="error" role="alert">{err}</p>}
          <button className="btn" disabled={busy || pw.length < 8}>{t("finish")}</button>
        </form>
      )}
      <SetupRestore t={t} onReady={onRestoreReady} />
    </div>
  );
}

const PLATFORMS: Record<string, string> = { android: "Android", ios: "iOS", windows: "Windows", macos: "macOS", linux: "Linux" };

/** "android" as people write it. */
function platformName(p: string): string {
  return PLATFORMS[p.toLowerCase()] ?? p.charAt(0).toUpperCase() + p.slice(1);
}

type DevicesProps = {
  t: T;
  status: Status;
  isHub: boolean;
  ownDeviceId: string | null;
};

/** Household > Devices: pairing a phone (laptop) and the phones paired. */
function Devices({ t, status, isHub, ownDeviceId }: DevicesProps) {
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
    <div className="stack wide">
      {err && <p className="error" role="alert">{err}</p>}

      {isHub && (
        <div id="set-add-phone" className="set-anchor" tabIndex={-1}>
          {pair ? (
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
            <div className="panel left set-action">
              <div className="set-row-text">
                <h2>{t("addDevice")}</h2>
                <span className="set-row-desc">{t("addDeviceIntro")}</span>
                {paired && <span className="ok" role="status">{t("devicePaired")}</span>}
              </div>
              <button className="btn" onClick={start}>{t("addDevice")}</button>
            </div>
          )}
        </div>
      )}

      <div id="set-paired" className="set-anchor stack" tabIndex={-1}>
        <h2>{t("pairedDevices")}</h2>
        {devices.length === 0 ? (
          <p className="muted" style={{ margin: 0 }}>{t("noDevices")}</p>
        ) : (
          <div className="list">
            {devices.map((d) => {
              const own = d.id === ownDeviceId;
              return (
                <div className="item" key={d.id}>
                  <div style={{ minWidth: 0, overflowWrap: "anywhere" }}>
                    <div>
                      {d.name}
                      {own && <span className="muted"> · {t("thisDevice")}</span>}
                    </div>
                    <div className="muted" style={{ fontSize: 13 }}>
                      {platformName(d.platform)} · {t("lastSeen")}: {d.last_seen ? fmtDateTime(d.last_seen) : t("never")}
                    </div>
                  </div>
                  {!own && isHub && (
                    <ConfirmButton label={t("remove")} confirmLabel={t("yesRemove")} cancelLabel={t("cancel")} onConfirm={() => remove(d)} />
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
