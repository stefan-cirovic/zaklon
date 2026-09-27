import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { api, ApiError, clientForget, flushOutbox, clientState, getMode, type AppMode, type LinkSummary, type Status } from "./api";
import { discardParked, onOfflineChange, onProbeRequest, parkedElsewhere, sendParkedHere } from "./offline";
import { makeT, type Key, type Lang } from "./i18n";
import { setFormatLang } from "./format";
import { errText } from "./errors";
import ConfirmButton from "./components/ConfirmButton";
import Home from "./screens/Home";
import Household from "./screens/Household";
import Connect from "./screens/Connect";
import Addons from "./screens/Addons";
import Library from "./screens/Library";
import Supplies from "./screens/Supplies";
import Maps from "./screens/Maps";
import Assistant from "./screens/Assistant";
import OfflineBanner from "./components/OfflineBanner";
import { Brand } from "./components/Brand";
import { ACCENTS, type Accent, type Look } from "./screens/HouseholdMore";

const TABS: { id: string; key: Key; ico: string }[] = [
  { id: "home", key: "home", ico: "⌂" },
  { id: "library", key: "library", ico: "≡" },
  { id: "maps", key: "maps", ico: "◎" },
  { id: "supplies", key: "supplies", ico: "▤" },
  { id: "assistant", key: "assistant", ico: "◇" },
  { id: "addons", key: "addons", ico: "⊕" },
];
const TAB_IDS = [...TABS.map((x) => x.id), "household"];

/** Poll every 2 s until the hub answers, then every 10 s. Never overlapping. */
const POLL_FAST = 2000;
const POLL_SLOW = 10000;
/** Phones: the on-device AI engine holds 1-3 GB; it stops when the app has been in the background this long. */
const AI_STOP_HIDDEN = 2 * 60 * 1000;

function readPref(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function tabFromHash(): string {
  const id = typeof location !== "undefined" ? location.hash.replace("#", "") : "";
  return TAB_IDS.includes(id) ? id : "home";
}

export default function App() {
  const [tab, setTabState] = useState(tabFromHash);
  const [mode, setMode] = useState<AppMode | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  // When the hub last answered; the status shown after that is marked as old.
  const [statusAt, setStatusAt] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Until the hub says otherwise: the device's own language (sr, hr, bs -> Serbian).
  const [lang, setLangState] = useState<Lang>(() => {
    const saved = readPref("zaklon.lang", "") as Lang;
    if (saved === "sr" || saved === "en") return saved;
    const device = typeof navigator !== "undefined" ? navigator.language.toLowerCase() : "en";
    return /^(sr|hr|bs|sh|cnr)/.test(device) ? "sr" : "en";
  });
  const [accent, setAccentState] = useState<Accent>(() => {
    const a = readPref("zaklon.accent", "amber") as Accent;
    return ACCENTS.includes(a) ? a : "amber";
  });
  const [oled, setOledState] = useState(() => readPref("zaklon.oled", "0") === "1");
  const look: Look = {
    accent,
    oled,
    setAccent: (a) => {
      setAccentState(a);
      try {
        localStorage.setItem("zaklon.accent", a);
      } catch {
        /* private mode: just for this session */
      }
    },
    setOled: (on) => {
      setOledState(on);
      try {
        localStorage.setItem("zaklon.oled", on ? "1" : "0");
      } catch {
        /* private mode: just for this session */
      }
    },
  };
  const [link, setLink] = useState<LinkSummary | null>(null);
  const [notice, setNotice] = useState<Key | null>(null);
  // Shopping list changes set aside when this phone was unlinked.
  const [parked, setParked] = useState(0);
  // Shows the offer of changes set aside for another hub (one that was
  // replaced) again whenever the phone's waiting changes change.
  const [, offlineChanged] = useReducer((n: number) => n + 1, 0);
  const [parkedErr, setParkedErr] = useState<string | null>(null);
  // A different Zaklon hub keeps answering where this phone's hub did.
  const [hubChanged, setHubChanged] = useState(false);
  const connected = useRef(false);
  setFormatLang(lang);
  // Stable between renders so screens can safely depend on it.
  const t = useMemo(() => makeT(lang), [lang]);

  const setTab = (id: string) => {
    setTabState(id);
    if (location.hash !== `#${id}`) location.hash = id;
  };

  // Follow the address: back/forward buttons and links to "#supplies" etc.
  useEffect(() => {
    const onHash = () => setTabState(tabFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const setLang = (l: Lang) => {
    setLangState(l);
    try {
      localStorage.setItem("zaklon.lang", l);
    } catch {
      /* ignore */
    }
  };

  /** This phone was removed on the hub: drop the link and say why. */
  const unlinked = useCallback(async (why: Key) => {
    setParked(await clientForget().catch(() => 0));
    setLink(await clientState().catch(() => null));
    setStatus(null);
    setStatusAt(null);
    setHubChanged(false);
    setNotice(why);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const s = await api<Status>("/api/status");
      setStatus(s);
      setStatusAt(Date.now());
      setError(null);
      setHubChanged(false);
      connected.current = true;
      // Back in reach: send what waited on this phone.
      flushOutbox().catch(() => {});
      if (!readPref("zaklon.lang", "")) setLangState(s.language);
    } catch (e) {
      connected.current = false;
      const m = await getMode();
      if (m.mode === "client" && e instanceof ApiError && e.status === 401) {
        await unlinked("removedFromHub");
        return;
      }
      // Another Zaklon hub keeps answering where ours did (the laptop was
      // reinstalled?). Not unpaired automatically: the phone keeps its copy
      // and its waiting changes until the person chooses to pair again.
      setHubChanged(m.mode === "client" && /reinstalled or replaced/.test(String(e instanceof Error ? e.message : e)));
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [unlinked]);

  useEffect(() => {
    getMode()
      .then(async (m) => {
        setMode(m);
        if (m.mode === "client") setLink(await clientState());
      })
      .catch(() => {});
  }, []);

  // One request at a time: the next one starts only after the previous finished.
  // A phone away from the hub asks less and less often (2 s, then up to a
  // minute) so it does not drain the battery at the shop, and not at all while
  // the app is in the background; coming back to the app asks right away.
  // An unpaired phone does not ask.
  useEffect(() => {
    if (mode?.mode === "client" && link && !link.linked) return;
    let alive = true;
    let running = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let misses = 0;
    const schedule = () => {
      if (!alive) return;
      if (timer) clearTimeout(timer);
      const wait = connected.current ? POLL_SLOW : Math.min(POLL_FAST * 2 ** Math.min(misses, 5), 60000);
      timer = setTimeout(tick, wait);
    };
    const tick = async () => {
      if (running) return;
      if (typeof document !== "undefined" && document.hidden) {
        schedule();
        return;
      }
      running = true;
      await refresh();
      running = false;
      misses = connected.current ? 0 : misses + 1;
      schedule();
    };
    const now = () => {
      if (running) return;
      if (timer) clearTimeout(timer);
      tick();
    };
    const onVisible = () => {
      if (!document.hidden) {
        misses = 0;
        now();
      }
    };
    document.addEventListener("visibilitychange", onVisible);
    // A screen was answered from the phone's copy: look for the hub now.
    const stopProbe = onProbeRequest(() => {
      if (!connected.current) now();
    });
    tick();
    return () => {
      alive = false;
      document.removeEventListener("visibilitychange", onVisible);
      stopProbe();
      if (timer) clearTimeout(timer);
    };
  }, [refresh, link?.linked, mode?.mode]);

  // Phones: the AI engine on the phone holds a lot of memory. When the app
  // has been in the background for a while, stop it (starting it again takes
  // a few seconds).
  useEffect(() => {
    if (mode?.mode !== "client") return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const onVisibility = () => {
      if (timer) clearTimeout(timer);
      timer = undefined;
      if (document.hidden) timer = setTimeout(() => invoke("local_ai_stop").catch(() => {}), AI_STOP_HIDDEN);
    };
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      if (timer) clearTimeout(timer);
    };
  }, [mode?.mode]);

  // Changes set aside for another hub: offered once this phone is paired.
  useEffect(() => onOfflineChange(offlineChanged), []);
  const elsewhere = mode?.mode === "client" && link?.linked ? parkedElsewhere(link.hub_id) : [];

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
    document.documentElement.dataset.oled = oled ? "1" : "0";
    document.documentElement.lang = lang === "sr" ? "sr-Latn" : "en";
  }, [accent, oled, lang]);

  const needsSetup = status !== null && !status.set_up;
  useEffect(() => {
    if (needsSetup) setTabState("household");
  }, [needsSetup]);

  const forget = async () => {
    const n = await clientForget().catch(() => 0);
    setLink(await clientState());
    setStatus(null);
    setStatusAt(null);
    setNotice(null);
    setHubChanged(false);
    setParked(n);
  };

  const isHub = mode?.mode !== "client";
  // Setup comes first: until then there is nothing to go to.
  const setupOnly = isHub && needsSetup;
  let homeError = error ? errText(t, new Error(error)) : null;
  // On the laptop the hub is this computer, not something on the Wi-Fi.
  if (isHub && homeError === t("errUnreachable")) homeError = t("errHubStopped");

  if (mode?.mode === "client" && link && !link.linked) {
    return (
      <main className="center-screen">
        <Connect
          t={t}
          lang={lang}
          setLang={setLang}
          notice={
            [notice ? t(notice) : "", parked > 0 ? `${t("outboxParked")} ${parked}. ${t("outboxParkedAfter")}` : ""].filter(Boolean).join(" ") || null
          }
          onLinked={async () => {
            setNotice(null);
            setParked(0);
            setLink(await clientState());
            refresh();
          }}
        />
      </main>
    );
  }

  return (
    <div className="shell">
      <main className="content">
        {!isHub && <OfflineBanner t={t} />}
        {!isHub && hubChanged && link?.linked && (
          <div className="panel notice stack" role="alert">
            <p style={{ margin: 0 }}>{t("hubChangedAsk")}</p>
            <div className="row">
              <ConfirmButton label={t("pairAgain")} confirmLabel={t("yesPairAgain")} cancelLabel={t("cancel")} className="btn" onConfirm={() => unlinked("hubChanged")} />
            </div>
          </div>
        )}
        {!isHub &&
          elsewhere.map((p) => (
            <div className="panel notice stack" role="status" key={p.hub_id}>
              <p style={{ margin: 0 }}>
                {t("parkedOther")}
                {p.hub_name ? ` (${p.hub_name})` : ""}: {p.count}.
              </p>
              <div className="row wrap">
                <button className="btn" onClick={() => sendParkedHere(p.hub_id).then(() => flushOutbox()).catch((e) => setParkedErr(errText(t, e)))}>
                  {t("parkedSendHere")}
                </button>
                <ConfirmButton
                  label={t("discard")}
                  confirmLabel={t("yesDiscard")}
                  cancelLabel={t("cancel")}
                  className="btn secondary"
                  onConfirm={() => discardParked(p.hub_id).catch((e) => setParkedErr(errText(t, e)))}
                />
              </div>
              {parkedErr && <p className="error" role="alert">{parkedErr}</p>}
            </div>
          ))}
        {tab === "home" && <Home status={status} statusAt={statusAt} error={homeError} t={t} go={setTab} phone={!isHub} />}
        {tab === "household" && (
          <div className="stack">
            {link?.linked && (
              <div className="panel left row between wrap">
                <div>
                  <div className="label">{t("linkedTo")}</div>
                  <div>
                    {link.hub_name} · {link.last_host ?? link.hosts[0]}
                  </div>
                </div>
                <ConfirmButton label={t("forgetHub")} confirmLabel={t("yesForget")} cancelLabel={t("cancel")} onConfirm={forget} />
              </div>
            )}
            <Household status={status} t={t} lang={lang} setLang={setLang} refresh={refresh} isHub={isHub} ownDeviceId={link?.device_id ?? null} look={look} />
          </div>
        )}
        {tab === "library" && <Library t={t} lang={lang} go={setTab} />}
        {tab === "maps" && <Maps t={t} lang={lang} isHub={isHub} />}
        {tab === "supplies" && <Supplies t={t} />}
        {tab === "assistant" && <Assistant t={t} lang={lang} isHub={isHub} go={setTab} />}
        {tab === "addons" && <Addons t={t} lang={lang} isHub={isHub} />}
        {status?.version && <p className="muted footer-note">Zaklon {status.version}</p>}
      </main>
      {!setupOnly && (
        <nav className="nav" aria-label="Zaklon">
          <Brand size={28} className="brand" />
          {TABS.map((x) => (
            <button key={x.id} className={tab === x.id ? "active" : ""} aria-current={tab === x.id ? "page" : undefined} onClick={() => setTab(x.id)}>
              <span className="ico" aria-hidden="true">{x.ico}</span>
              <span>{t(x.key)}</span>
            </button>
          ))}
          <button
            className={"wide-only" + (tab === "household" ? " active" : "")}
            aria-current={tab === "household" ? "page" : undefined}
            onClick={() => setTab("household")}
          >
            <span className="ico" aria-hidden="true">⚙</span>
            <span>{t("household")}</span>
          </button>
          <button
            className={"more-only" + (tab === "household" ? " active" : "")}
            aria-current={tab === "household" ? "page" : undefined}
            onClick={() => setTab("household")}
          >
            <span className="ico" aria-hidden="true">⚙</span>
            <span>{t("more")}</span>
          </button>
        </nav>
      )}
    </div>
  );
}
