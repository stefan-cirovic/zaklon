import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, ApiError, clientForget, flushOutbox, clientState, getMode, type AppMode, type LinkSummary, type Status } from "./api";
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
  const [error, setError] = useState<string | null>(null);
  // Until the hub says otherwise: the device's own language (sr, hr, bs -> Serbian).
  const [lang, setLangState] = useState<Lang>(() => {
    const saved = readPref("zaklon.lang", "") as Lang;
    if (saved === "sr" || saved === "en") return saved;
    const device = typeof navigator !== "undefined" ? navigator.language.toLowerCase() : "en";
    return /^(sr|hr|bs|sh|cnr)/.test(device) ? "sr" : "en";
  });
  const [accent, setAccentState] = useState<Accent>(() => {
    const a = readPref("zaklon.accent", "green") as Accent;
    return ACCENTS.includes(a) ? a : "green";
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
    await clientForget().catch(() => {});
    setLink(await clientState().catch(() => null));
    setStatus(null);
    setNotice(why);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const s = await api<Status>("/api/status");
      setStatus(s);
      setError(null);
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
      // The laptop was reinstalled (new identity): this pairing can never work again.
      if (m.mode === "client" && /reinstalled or replaced/.test(String(e instanceof Error ? e.message : e))) {
        await unlinked("hubChanged");
        return;
      }
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
    let timer: ReturnType<typeof setTimeout> | undefined;
    let misses = 0;
    const schedule = () => {
      if (!alive) return;
      if (timer) clearTimeout(timer);
      const wait = connected.current ? POLL_SLOW : Math.min(POLL_FAST * 2 ** Math.min(misses, 5), 60000);
      timer = setTimeout(tick, wait);
    };
    const tick = async () => {
      if (typeof document !== "undefined" && document.hidden) {
        schedule();
        return;
      }
      await refresh();
      misses = connected.current ? 0 : misses + 1;
      schedule();
    };
    const onVisible = () => {
      if (!document.hidden) {
        misses = 0;
        if (timer) clearTimeout(timer);
        tick();
      }
    };
    document.addEventListener("visibilitychange", onVisible);
    tick();
    return () => {
      alive = false;
      document.removeEventListener("visibilitychange", onVisible);
      if (timer) clearTimeout(timer);
    };
  }, [refresh, link?.linked, mode?.mode]);

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
    await clientForget().catch(() => {});
    setLink(await clientState());
    setStatus(null);
  };

  const isHub = mode?.mode !== "client";

  if (mode?.mode === "client" && link && !link.linked) {
    return (
      <main className="center-screen">
        <Connect
          t={t}
          notice={notice ? t(notice) : null}
          onLinked={async () => {
            setNotice(null);
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
        {tab === "home" && <Home status={status} error={error ? errText(t, new Error(error)) : null} t={t} go={setTab} />}
        {tab === "household" && (
          <div className="stack">
            {link?.linked && (
              <div className="panel row between wrap">
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
      <nav className="nav" aria-label="Zaklon">
        <div className="brand">Zaklon</div>
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
    </div>
  );
}
