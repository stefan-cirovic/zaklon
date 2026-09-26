import { useEffect, useRef, useState } from "react";
import { api, contentBase } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";

type T = (k: Key) => string;
type Props = { t: T; lang: Lang; go: (tab: string) => void };

type Engine = "missing" | "idle" | "starting" | "running" | "failed";
type Book = { name: string; pack_id: string; title_en: string; title_sr: string; languages: string[]; home: string };
type LibraryReply = { engine: Engine; books: Book[] };
type Result = {
  title: string;
  url: string;
  snippet: string;
  book: string;
  book_title_en: string;
  book_title_sr: string;
  kind: "title" | "text";
};

export default function Library({ t, lang, go }: Props) {
  const [lib, setLib] = useState<LibraryReply | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [results, setResults] = useState<Result[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [reader, setReader] = useState<{ url: string; title: string } | null>(null);
  const [base, setBase] = useState<string | null>(null);
  const seq = useRef(0);

  useEffect(() => {
    contentBase().then(setBase).catch((e) => setErr(errText(t, e)));
  }, [t]);

  // Refresh the library state; poll while the engine is starting.
  useEffect(() => {
    let alive = true;
    const load = () =>
      api<LibraryReply>("/api/library")
        .then((r) => {
          if (!alive) return;
          setLib(r);
          setErr(null);
        })
        .catch((e) => alive && setErr(errText(t, e)));
    load();
    const id = setInterval(load, lib?.engine === "starting" ? 2000 : 15000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [lib?.engine, t]);

  // Search as you type, 300 ms after the last key; ignore stale replies.
  useEffect(() => {
    const query = q.trim();
    if (query.length < 2) {
      // Invalidate any search still in flight so its results do not appear later.
      seq.current++;
      setResults(null);
      setSearching(false);
      return;
    }
    const mine = ++seq.current;
    const timer = setTimeout(async () => {
      setSearching(true);
      try {
        const r = await api<Result[]>(`/api/library/search?q=${encodeURIComponent(query)}&limit=30`);
        if (mine === seq.current) {
          setResults(r);
          setErr(null);
        }
      } catch (e) {
        if (mine === seq.current) setErr(errText(t, e));
      } finally {
        if (mine === seq.current) setSearching(false);
      }
    }, 300);
    return () => clearTimeout(timer);
  }, [q, t]);

  const bookTitle = (en: string, sr: string) => (lang === "sr" && sr ? sr : en);

  if (reader && base !== null) {
    return (
      <div className="reader">
        <div className="reader-bar">
          <button className="btn secondary" onClick={() => setReader(null)}>← {t("back")}</button>
          <div className="reader-title">{reader.title}</div>
        </div>
        <iframe
          className="reader-frame"
          src={base + reader.url}
          title={reader.title}
          sandbox="allow-same-origin allow-popups"
          referrerPolicy="no-referrer"
        />
      </div>
    );
  }

  const engine = lib?.engine;
  return (
    <div className="stack">
      <div className="page-head">
        <h1>{t("library")}</h1>
        <p className="muted">{t("libraryIntro")}</p>
      </div>
      {err && <p className="error" role="alert">{err}</p>}

      {(engine === "missing" || engine === "idle") && (
        <div className="panel stack" style={{ textAlign: "center" }}>
          <p>{t("noBooks")}</p>
          <div>
            <button className="btn" onClick={() => go("addons")}>{t("goToAddons")}</button>
          </div>
        </div>
      )}
      {engine === "starting" && <p className="muted">{t("engineStarting")}</p>}
      {engine === "failed" && <p className="error">{t("engineFailed")}</p>}

      {engine === "running" && (
        <>
          <input
            type="search"
            className="search"
            aria-label={t("searchPlaceholder")}
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder={t("searchPlaceholder")}
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
          {results === null ? (
            <div>
              <h2>{t("books")}</h2>
              <div className="list">
                {lib!.books.map((b) => (
                  <button
                    key={b.name}
                    className="item clickable"
                    onClick={() => setReader({ url: b.home, title: bookTitle(b.title_en, b.title_sr) })}
                  >
                    <div>{bookTitle(b.title_en, b.title_sr)}</div>
                    <span className="muted">›</span>
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="list">
              {searching && results.length === 0 && <p className="muted">…</p>}
              {!searching && results.length === 0 && <p className="muted">{t("noResults")}</p>}
              {results.map((r) => (
                <button key={r.url} className="item clickable result" onClick={() => setReader({ url: r.url, title: r.title })}>
                  <div>
                    <div className="result-title">{r.title}</div>
                    {r.snippet && <div className="muted result-snippet">{r.snippet}</div>}
                    <div className="muted result-book">{bookTitle(r.book_title_en, r.book_title_sr)}</div>
                  </div>
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
