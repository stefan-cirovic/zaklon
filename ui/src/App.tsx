import { useCallback, useEffect, useState } from "react";
import { api, getMode, type AppMode, type Status } from "./api";
import { makeT, type Key, type Lang } from "./i18n";
import Home from "./screens/Home";
import Household from "./screens/Household";
import Placeholder from "./screens/Placeholder";

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
  const [tab, setTab] = useState("home");
  const [mode, setMode] = useState<AppMode | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lang, setLangState] = useState<Lang>((readPref("zaklon.lang", "") as Lang) || "en");
  const [accent] = useState(readPref("zaklon.accent", "green"));
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
    getMode().then(setMode);
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 10000);
    return () => clearInterval(id);
  }, [refresh]);

  useEffect(() => {
    document.documentElement.dataset.accent = accent;
  }, [accent]);

  useEffect(() => {
    if (status && !status.set_up) setTab("household");
  }, [status]);

  return (
    <div className="shell">
      <main className="content">
        {tab === "home" && <Home status={status} error={error} t={t} go={setTab} />}
        {tab === "household" && (
          <Household status={status} t={t} lang={lang} setLang={setLang} refresh={refresh} />
        )}
        {tab === "library" && <Placeholder title={t("library")} text={t("comingSoon")} />}
        {tab === "maps" && <Placeholder title={t("maps")} text={t("comingSoon")} />}
        {tab === "supplies" && <Placeholder title={t("supplies")} text={t("comingSoon")} />}
        {tab === "assistant" && <Placeholder title={t("assistant")} text={t("comingSoon")} />}
        {tab === "addons" && <Placeholder title={t("addons")} text={t("comingSoon")} />}
        {mode && (
          <p className="muted" style={{ marginTop: 32, fontSize: 12 }}>
            {mode.mode} · {mode.platform} · {mode.version}
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
