import { useCallback, useEffect, useState } from "react";
import { api, clientForget, clientState, getMode, type AppMode, type LinkSummary, type Status } from "./api";
import { makeT, type Key, type Lang } from "./i18n";
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

function readPref(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

export default function App() {
  const TAB_IDS = ["home", "library", "maps", "supplies", "assistant", "addons", "household"];
  const initialTab = typeof location !== "undefined" ? location.hash.replace("#", "") : "";
  const [tab, setTabState] = useState(TAB_IDS.includes(initialTab) ? initialTab : "home");
  const setTab = (id: string) => {
    setTabState(id);
    try {
      history.replaceState(null, "", `#${id}`);
    } catch {
      /* ignore */
    }
  };
  const [mode, setMode] = useState<AppMode | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lang, setLangState] = useState<Lang>((readPref("zaklon.lang", "") as Lang) || "en");
  const [accent] = useState(readPref("zaklon.accent", "green"));
  const [link, setLink] = useState<LinkSummary | null>(null);
  const t = makeT(lang);

  const setLang = (l: Lang) => {
    setLangState(l);
    try {
      localStorage.setItem("zaklon.lang", l);
    } catch {
      /* ignore */
    }
  };

  const refresh = useCallback(async () => {
    try {
      const s = await api<Status>("/api/status");
      setStatus(s);
      setError(null);
      if (!readPref("zaklon.lang", "")) setLangState(s.language);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    getMode().then(async (m) => {
      setMode(m);
      if (m.mode === "client") setLink(await clientState());
    });
  }, []);

  // Poll quickly until the hub answers for the first time, then every 10 s.
  useEffect(() => {
    refresh();
    const id = setInterval(refresh, status ? 10000 : 2000);
    return () => clearInterval(id);
  }, [refresh, status]);

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
  }, [accent]);

  useEffect(() => {
    if (status && !status.set_up) setTab("household");
  }, [status]);

  const forget = async () => {
    await clientForget();
    setLink(await clientState());
    setStatus(null);
  };

  if (mode?.mode === "client" && link && !link.linked) {
    return (
      <main className="center-screen">
        <Connect t={t} onLinked={async () => { setLink(await clientState()); refresh(); }} />
      </main>
    );
  }

  return (
    <div className="shell">
      <main className="content">
        {tab === "home" && <Home status={status} error={error} t={t} go={setTab} />}
        {tab === "household" && (
          <div className="stack">
            {link?.linked && (
              <div className="panel row between">
                <div>
                  <div className="label">{t("linkedTo")}</div>
                  <div>{link.hub_name} · {link.last_host ?? link.hosts[0]}</div>
                </div>
                <button className="btn danger" onClick={forget}>{t("forgetHub")}</button>
              </div>
            )}
            <Household status={status} t={t} lang={lang} setLang={setLang} refresh={refresh} />
          </div>
        )}
        {tab === "library" && <Library t={t} lang={lang} go={setTab} />}
        {tab === "maps" && <Placeholder title={t("maps")} text={t("comingSoon")} />}
        {tab === "supplies" && <Supplies t={t} />}
        {tab === "assistant" && <Placeholder title={t("assistant")} text={t("comingSoon")} />}
        {tab === "addons" && <Addons t={t} lang={lang} isHub={mode?.mode === "hub"} />}
        {mode && (
          <p className="muted" style={{ marginTop: 32, fontSize: 12 }}>
            {[mode.mode, mode.platform, mode.version || status?.version].filter(Boolean).join(" · ")}
          </p>
        )}
      </main>
      <nav className="nav">
        <div className="brand">Zaklon</div>
        {TABS.map((x) => (
          <button key={x.id} className={tab === x.id ? "active" : ""} onClick={() => setTab(x.id)}>
            <span className="ico">{x.ico}</span>
            <span>{t(x.key)}</span>
          </button>
        ))}
        <button className={"wide-only" + (tab === "household" ? " active" : "")} onClick={() => setTab("household")}>
          <span className="ico">⚙</span>
          <span>{t("household")}</span>
        </button>
        <button className={"more-only" + (tab === "household" ? " active" : "")} onClick={() => setTab("household")}>
          <span className="ico">⚙</span>
          <span>{t("more")}</span>
        </button>
      </nav>
    </div>
  );
}
