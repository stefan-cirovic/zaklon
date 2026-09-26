import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, ApiError, clientForget, clientState, getMode, type AppMode, type LinkSummary, type Status } from "./api";
import { makeT, type Key, type Lang } from "./i18n";
import { setFormatLang } from "./format";
import { errText } from "./errors";
import ConfirmButton from "./components/ConfirmButton";
import Home from "./screens/Home";
import Household from "./screens/Household";
import Connect from "./screens/Connect";
import Placeholder from "./screens/Placeholder";
import Addons from "./screens/Addons";
import Library from "./screens/Library";
import Supplies from "./screens/Supplies";

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
  const [lang, setLangState] = useState<Lang>((readPref("zaklon.lang", "") as Lang) || "en");
  const [accent] = useState(readPref("zaklon.accent", "green"));
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
      if (!readPref("zaklon.lang", "")) setLangState(s.language);
    } catch (e) {
      connected.current = false;
      const m = await getMode();
      if (m.mode === "client" && e instanceof ApiError && e.status === 401) {
        await unlinked("removedFromHub");
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
  useEffect(() => {
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      await refresh();
      if (alive) timer = setTimeout(tick, connected.current ? POLL_SLOW : POLL_FAST);
    };
    tick();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
    };
  }, [refresh, link?.linked]);

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
    document.documentElement.lang = lang === "sr" ? "sr-Latn" : "en";
  }, [accent, lang]);

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
            <Household status={status} t={t} lang={lang} setLang={setLang} refresh={refresh} isHub={isHub} ownDeviceId={link?.device_id ?? null} />
          </div>
        )}
        {tab === "library" && <Library t={t} lang={lang} go={setTab} />}
        {tab === "maps" && <Placeholder title={t("maps")} text={t("comingSoon")} />}
        {tab === "supplies" && <Supplies t={t} />}
        {tab === "assistant" && <Placeholder title={t("assistant")} text={t("comingSoon")} />}
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
