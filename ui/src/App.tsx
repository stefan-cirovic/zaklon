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
import Tools from "./screens/Tools";
import Help from "./screens/Help";
import OfflineBanner from "./components/OfflineBanner";
import { BrandLink } from "./components/Brand";
import { Icon, type IconName } from "./components/Icon";
import { isToolId, toolOf, TOOLS, type ToolId } from "./tools";
import { ACCENTS, type Accent, type Look } from "./screens/HouseholdMore";

/** The bar: these four, and the tool the household pinned (after Assistant). Every tool is on the Tools screen. */
const NAV: { id: string; key: Key; icon: IconName }[] = [
  { id: "home", key: "home", icon: "home" },
  { id: "assistant", key: "assistant", icon: "assistant" },
  { id: "tools", key: "tools", icon: "tools" },
  { id: "household", key: "household", icon: "household" },
];
/** Help is opened from Tools and from the "How it works" link on each screen. */
const TAB_IDS = [...NAV.map((x) => x.id), ...TOOLS.map((x) => x.id), "help"];

/** Poll every 2 s until the hub answers, then every 10 s. Never overlapping. */
const POLL_FAST = 2000;
const POLL_SLOW = 10000;
/** How often the pinned tool is asked for again (it changes rarely, on the laptop). */
const PINNED_EVERY = 60_000;
/** Phones: the on-device AI engine holds 1-3 GB; it stops when the app has been in the background this long. */
const AI_STOP_HIDDEN = 2 * 60 * 1000;

function readPref(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function writePref(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* private mode: just for this session */
  }
}

/** The screen in the address; a screen may add a place of its own after a slash ("#addons/maps", "#household/backups"). */
function tabFromHash(): string {
  const id = typeof location !== "undefined" ? location.hash.replace("#", "").split("/")[0] : "";
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
  // The household's pinned tool, as last heard from the hub (so the bar does not jump at start).
  const [pinned, setPinnedState] = useState<ToolId | null>(() => {
    const p = readPref("zaklon.pinned", "");
    return isToolId(p) ? p : null;
  });
  const pinnedAt = useRef(0);
  const setPinned = useCallback((tool: unknown) => {
    const p = isToolId(tool) ? tool : null;
    setPinnedState(p);
    writePref("zaklon.pinned", p ?? "");
  }, []);
  const loadPinned = useCallback(async () => {
    pinnedAt.current = Date.now();
    try {
      setPinned((await api<{ tool: string | null }>("/api/pinned-tool")).tool);
    } catch {
      /* keep the last known one */
    }
  }, [setPinned]);
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

  /** Open a screen, or a place in it ("addons/models"). */
  const setTab = (id: string) => {
    setTabState(id.split("/")[0]);
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
    setPinned(null);
    pinnedAt.current = 0;
  }, [setPinned]);

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
      if (s.set_up && Date.now() - pinnedAt.current > PINNED_EVERY) loadPinned();
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
  }, [unlinked, loadPinned]);

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
    setPinned(null);
    pinnedAt.current = 0;
  };

  const isHub = mode?.mode !== "client";
  /** Laptop only: the household's pinned tool (it replaces the one before), or none. */
  const pin = async (id: ToolId | null) => {
    const r = await api<{ tool: string | null }>("/api/pinned-tool", { json: { tool: id } });
    setPinned(r?.tool ?? null);
    pinnedAt.current = Date.now();
  };
  const bar: typeof NAV = pinned ? [NAV[0], NAV[1], { id: pinned, key: toolOf(pinned).title, icon: pinned }, NAV[2], NAV[3]] : NAV;
  // A tool that is not in the bar belongs under Tools there, and so does Help.
  const barTab = (isToolId(tab) && tab !== pinned) || tab === "help" ? "tools" : tab;
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

  // The assistant fills the window like a chat app: its list and conversation scroll on their own.
  const fill = tab === "assistant" && !setupOnly;

  return (
    <div className={"shell" + (fill ? " fill" : "")}>
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
          <Household
            status={status}
            online={!!status && !error}
            t={t}
            lang={lang}
            setLang={setLang}
            refresh={refresh}
            isHub={isHub}
            ownDeviceId={link?.device_id ?? null}
            look={look}
            top={
              link?.linked && (
                <div className="panel left row between wrap">
                  <div>
                    <div className="label">{t("linkedTo")}</div>
                    <div>
                      {link.hub_name} · {link.last_host ?? link.hosts[0]}
                    </div>
                  </div>
                  <ConfirmButton label={t("forgetHub")} confirmLabel={t("yesForget")} cancelLabel={t("cancel")} onConfirm={forget} />
                </div>
              )
            }
          />
        )}
        {tab === "tools" && <Tools t={t} go={setTab} pinned={pinned} pin={isHub ? pin : null} />}
        {tab === "library" && <Library t={t} lang={lang} go={setTab} />}
        {tab === "maps" && <Maps t={t} lang={lang} isHub={isHub} />}
        {tab === "supplies" && <Supplies t={t} />}
        {tab === "assistant" && <Assistant t={t} lang={lang} isHub={isHub} go={setTab} />}
        {tab === "addons" && <Addons t={t} lang={lang} isHub={isHub} />}
        {tab === "help" && <Help t={t} lang={lang} />}
        {status?.version && !fill && <p className="muted footer-note">Zaklon {status.version}</p>}
      </main>
      {!setupOnly && (
        <nav className="nav" aria-label={t("mainNav")}>
          <BrandLink label={t("zaklonWebsite")} />
          <div className={"nav-items" + (bar.length > 4 ? " five" : "")}>
            {bar.map((x) => (
              <button
                key={x.id}
                className={barTab === x.id ? "active" : ""}
                // The tool open under Tools: its section, not the page itself.
                aria-current={barTab === x.id ? (tab === x.id ? "page" : "true") : undefined}
                onClick={() => setTab(x.id)}
              >
                <Icon name={x.icon} />
                <span className="nav-label">{t(x.key)}</span>
              </button>
            ))}
          </div>
        </nav>
      )}
    </div>
  );
}
