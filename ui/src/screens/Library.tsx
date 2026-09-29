import { useEffect, useId, useRef, useState } from "react";
import { api } from "../api";
import Reader from "../components/Reader";
import HelpLink from "../components/HelpLink";
import RichText from "../components/RichText";
import StarterSet from "../components/StarterSet";
import { Entries } from "../components/PackViews";
import { GlyphIcon } from "../components/ExplorerIcons";
import { Icon } from "../components/Icon";
import { SettingsIcon } from "../components/SettingsIcon";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { useVisiblePoll } from "../poll";
import { countWord, cyrToLat, fmtBytes, latinArticles } from "../format";
import { usePacks } from "../usePacks";
import { packTopics, isTopicId, topicOf, TOPICS, type TopicId } from "../topics";
import { toolsFor } from "../tools";
import { BUSY, type Book, type Entry, type LibraryReply, type Pack } from "../packs";

type T = (k: Key) => string;
type Props = { t: T; lang: Lang; isHub: boolean };

type Result = {
  title: string;
  url: string;
  snippet: string;
  book: string;
  book_title_en: string;
  book_title_sr: string;
  kind: "title" | "text";
};

/** The topic in the address (#library/water), or none for the Library itself. */
function topicFromHash(): TopicId | null {
  const [tab, topic] = location.hash.replace(/^#/, "").split(/[/?]/);
  return tab === "library" && isTopicId(topic) ? topic : null;
}

function useTopic(): TopicId | null {
  const [topic, setTopic] = useState(topicFromHash);
  useEffect(() => {
    const onHash = () => setTopic(topicFromHash());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  return topic;
}

/**
 * The Library: everything there is to read. At the top one search across
 * every installed book, below it the topics. A topic's page (#library/water)
 * has the guides on the hub, each opening straight into reading (and a
 * search in them), the ones to get, downloaded right there, and the way to
 * the topic's tool. Before anything is installed it starts with the
 * recommended set.
 */
export default function Library({ t, lang, isHub }: Props) {
  const topic = useTopic();
  const [lib, setLib] = useState<LibraryReply | null>(null);
  const [libErr, setLibErr] = useState<string | null>(null);
  const [reader, setReader] = useState<{ url: string; title: string } | null>(null);
  const packs = usePacks(t, lang, isHub);

  // The books on the hub; asked for more often while the library engine starts.
  useVisiblePoll(async () => {
    try {
      setLib(await api<LibraryReply>("/api/library"));
      setLibErr(null);
      return true;
    } catch (e) {
      setLibErr(errText(t, e));
      return false;
    }
  }, lib?.engine === "starting" ? 2000 : 15000);

  // A new place starts at the top.
  useEffect(() => {
    window.scrollTo(0, 0);
  }, [topic]);

  const byId = new Map<string, Pack>((packs.data?.packs ?? []).map((p) => [p.id, p]));
  const books = lib?.books ?? [];
  /** The topics a book is under: its own (a newer hub says them), else its pack's in the catalog. */
  const topicsOf = (b: Book) => packTopics(b.topics ?? byId.get(b.pack_id)?.topics);
  const bookTitle = (en: string, sr: string) => (lang === "sr" && sr ? sr : en);
  const open = (url: string, title: string) => setReader({ url, title });
  const err = libErr ?? packs.err;

  const shared = { t, lang, isHub, lib, books, topicsOf, bookTitle, open, err, packs };
  // The reader covers the page, which stays as it was (with what was searched) for when it closes.
  return (
    <>
      {topic ? <TopicPage {...shared} topic={topic} /> : <LibraryHome {...shared} />}
      {reader && <Reader t={t} lang={lang} url={reader.url} title={reader.title} onClose={() => setReader(null)} />}
    </>
  );
}

type Shared = {
  t: T;
  lang: Lang;
  isHub: boolean;
  lib: LibraryReply | null;
  books: Book[];
  topicsOf: (b: Book) => TopicId[];
  bookTitle: (en: string, sr: string) => string;
  open: (url: string, title: string) => void;
  err: string | null;
  packs: ReturnType<typeof usePacks>;
};

/** The state of the library engine, when it keeps books from being read. */
function EngineNote({ t, lib }: { t: T; lib: LibraryReply | null }) {
  if (lib?.engine === "starting") return <p className="muted">{t("engineStarting")}</p>;
  // Guides on the hub, but the engine that reads them was removed.
  if (lib?.engine === "missing" && lib.books.length > 0)
    return (
      <p className="warn">
        <RichText text={t("engineMissing")} />
      </p>
    );
  if (lib?.engine === "failed")
    return (
      <p className="error">
        <RichText text={t("engineFailed")} />
      </p>
    );
  return null;
}

/**
 * Search as you type, 300 ms after the last key, in every book or in a
 * topic's; stale replies are ignored. `null` while fewer than two letters
 * are typed.
 */
function useSearch(t: T, q: string, topic: TopicId | null) {
  const [results, setResults] = useState<Result[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const seq = useRef(0);
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
        const where = topic ? `&topic=${topic}` : "";
        const r = await api<Result[]>(`/api/library/search?q=${encodeURIComponent(query)}${where}&limit=30`);
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
  }, [q, t, topic]);
  return { results, searching, err };
}

/** The search box and, once something is typed, what it found. */
function Search({ t, lang, topic, placeholder, open, bookTitle }: { t: T; lang: Lang; topic: TopicId | null; placeholder: string; open: Shared["open"]; bookTitle: Shared["bookTitle"] }) {
  const [q, setQ] = useState("");
  const { results, searching, err } = useSearch(t, q, topic);
  const latin = latinArticles(lang);
  const show = (s: string) => (latin ? cyrToLat(s) : s);
  return (
    <div className="stack lib-search">
      <input
        type="search"
        className="search"
        aria-label={placeholder}
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder={placeholder}
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
      />
      {err && <p className="error" role="alert">{err}</p>}
      {results !== null && (
        <div className="list cols" role="region" aria-label={t("searchResults")}>
          {searching && results.length === 0 && <p className="muted">{t("aiLoading")}</p>}
          {!searching && results.length === 0 && <p className="muted">{t("noResults")}</p>}
          {results.map((r) => (
            <button key={r.url} className="item clickable result" onClick={() => open(r.url, r.title)}>
              <div>
                <div className="result-title">{show(r.title)}</div>
                {r.snippet && <div className="muted result-snippet">{show(r.snippet)}</div>}
                <div className="muted result-book">{bookTitle(r.book_title_en, r.book_title_sr)}</div>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** The Library itself: the search across every book, a start for an empty library, and the topics. */
function LibraryHome({ t, lang, isHub, lib, books, topicsOf, bookTitle, open, err, packs }: Shared) {
  const topicsId = useId();
  const data = packs.data;
  const knowledge = (data?.packs ?? []).filter((p) => p.category === "knowledge");
  const empty = lib !== null && books.length === 0;
  // The first guides are on their way: they show up under their topics once ready.
  const coming = knowledge.some((p) => BUSY.includes(p.state.status));
  const readable = lib?.engine === "running" && books.length > 0;
  return (
    <div className="stack library">
      <div className="page-head">
        <div className="title-line">
          <h1>{t("library")}</h1>
          <HelpLink t={t} topic="library" />
        </div>
        <p className="muted">{t("libraryIntro")}</p>
      </div>
      {err && <p className="error" role="alert">{err}</p>}
      {!lib && !err && <p className="muted">{t("aiLoading")}</p>}
      <EngineNote t={t} lib={lib} />
      {readable && <Search t={t} lang={lang} topic={null} placeholder={t("searchPlaceholder")} open={open} bookTitle={bookTitle} />}

      {empty && (
        <section className="panel stack left lib-start" aria-labelledby={`${topicsId}-start`}>
          <h2 id={`${topicsId}-start`}>{t("libStartTitle")}</h2>
          <p className="muted" style={{ margin: 0 }}>{isHub ? t("libStartIntro") : t("libStartPhone")}</p>
          {coming && (
            <p className="ok" style={{ margin: 0 }} role="status">
              <RichText text={t("libStartComing")} />
            </p>
          )}
          {isHub && data && <StarterSet t={t} lang={lang} packs={data.packs} sets={data.starter_sets ?? []} freeBytes={data.system.disk_free} onStarted={packs.load} embedded />}
        </section>
      )}

      <section className="stack" aria-labelledby={topicsId}>
        <h2 id={topicsId} className="section-title">{t("libTopics")}</h2>
        <ul className="lib-topics">
          {TOPICS.map((x) => {
            const here = books.filter((b) => topicsOf(b).includes(x.id));
            const offered = knowledge.filter((p) => !p.withdrawn && p.state.status !== "installed" && packTopics(p.topics).includes(x.id)).length;
            const line =
              here.length > 0
                ? `${t("libOnHub")}: ${here.length}`
                : data
                  ? `${offered} ${countWord(offered, [t("guideWord1"), t("guideWord5")], [t("guideWord1"), t("guideWord2"), t("guideWord5")])} ${t("libToGet")}`
                  : "";
            return (
              <li key={x.id}>
                <a className="lib-topic" href={`#library/${x.id}`} data-topic={x.id}>
                  <span className="lib-topic-icon">
                    <GlyphIcon glyph={x.glyph} size={26} />
                  </span>
                  <span className="lib-topic-text">
                    <span className="lib-topic-name">{t(x.name)}</span>
                    <span className="lib-topic-desc">{t(x.desc)}</span>
                    {line && <span className={"lib-topic-count" + (here.length > 0 ? " ok" : "")}>{line}</span>}
                  </span>
                  <span className="lib-topic-go" aria-hidden="true">
                    <SettingsIcon name="chevron" size={18} />
                  </span>
                </a>
              </li>
            );
          })}
        </ul>
      </section>
      <p className="muted tools-note">
        <RichText text={t("libraryElsewhere")} />
      </p>
    </div>
  );
}

/**
 * A topic's page: its tool, the guides on the hub (each opens to read, with
 * a search in them), and the ones to get, with the ones people download
 * themselves apart at the bottom.
 */
function TopicPage({ t, lang, lib, books, topicsOf, bookTitle, open, err, packs, topic }: Shared & { topic: TopicId }) {
  const x = topicOf(topic);
  const name = t(x.name);
  const head = useRef<HTMLHeadingElement>(null);
  const yoursId = useId();
  const moreId = useId();
  const byUserId = useId();
  useEffect(() => {
    head.current?.focus({ preventScroll: true });
  }, [topic]);

  const data = packs.data;
  const here = books.filter((b) => topicsOf(b).includes(topic));
  const byId = new Map<string, Pack>((data?.packs ?? []).map((p) => [p.id, p]));
  // To get: knowledge packs about the topic not on the hub yet (with the ones on their way).
  const toGet: Entry[] = (data?.packs ?? [])
    .filter((p) => p.category === "knowledge" && p.state.status !== "installed" && packTopics(p.topics).includes(topic))
    .map((p) => packs.packEntry(p, { removable: false }))
    // The ones recommended for the app's language first, else in the catalog's order.
    .sort((a, b) => Number(b.recommended) - Number(a.recommended));
  const offered = toGet.filter((e) => !e.byUser);
  const byUser = toGet.filter((e) => e.byUser);
  const tools = toolsFor(topic);
  const readable = lib?.engine === "running";

  return (
    <div className="stack library lib-topic-page">
      <div className="set-head lib-head">
        <a className="set-back" href="#library" aria-label={t("libBack")} title={t("libBack")}>
          <SettingsIcon name="back" size={20} />
        </a>
        <nav className="set-crumbs" aria-label={t("breadcrumb")}>
          <a href="#library">{t("library")}</a>
          <span aria-hidden="true">›</span>
        </nav>
        <h1 ref={head} tabIndex={-1}>{name}</h1>
        <HelpLink t={t} topic="library" section="topics" />
      </div>
      <p className="muted lib-topic-intro">{t(x.desc)}</p>
      {tools.map((tool) => (
        <a key={tool.id} className="lib-tool" href={`#${tool.id}`}>
          <span className="tool-icon">
            <Icon name={tool.id} size={22} />
          </span>
          <span className="lib-tool-text">
            <span className="muted lib-tool-label">{t("libToolFor")}</span>
            <span className="lib-tool-name">{t(tool.title)}</span>
          </span>
          <SettingsIcon name="chevron" size={18} />
        </a>
      ))}
      {err && <p className="error" role="alert">{err}</p>}

      <section className="stack lib-section" aria-labelledby={yoursId}>
        <h2 id={yoursId}>{t("libYourGuides")}</h2>
        {!lib ? (
          <p className="muted">{t("aiLoading")}</p>
        ) : here.length === 0 ? (
          <p className="muted">{t("libYourGuidesNone")}</p>
        ) : (
          <>
            <EngineNote t={t} lib={lib} />
            {readable && <Search t={t} lang={lang} topic={topic} placeholder={t("libSearchTopic")} open={open} bookTitle={bookTitle} />}
            <ul className="lib-books" aria-label={t("libYourGuides")}>
              {here.map((b) => {
                const pack = byId.get(b.pack_id);
                const e = pack ? packs.packEntry(pack, { removable: false }) : null;
                const title = bookTitle(b.title_en, b.title_sr);
                return (
                  <li key={b.name} className="lib-book" data-entry={b.pack_id}>
                    <button className="lib-book-open" onClick={() => open(b.home, title)} disabled={!readable} aria-label={`${t("libRead")}: ${title}`}>
                      <span className="entry-icon">
                        <GlyphIcon glyph={x.glyph} />
                      </span>
                      <span className="lib-book-text">
                        <span className="entry-name">{title}</span>
                        {e && (
                          <span className="muted small-text">
                            {fmtBytes(e.installedBytes)} · {e.license}
                          </span>
                        )}
                        {e?.note && <span className="offer-note small-text">{e.note}</span>}
                        {e?.update && <span className="warn small-text">{t("packUpdateAvailable")}</span>}
                      </span>
                      <span className="lib-book-read">
                        {t("libRead")}
                        <SettingsIcon name="chevron" size={16} />
                      </span>
                    </button>
                    {e?.update && <div className="lib-book-actions">{e.actions(true)}</div>}
                  </li>
                );
              })}
            </ul>
          </>
        )}
      </section>

      <section className="stack lib-section" aria-labelledby={moreId}>
        <h2 id={moreId}>{t("libGetMore")}</h2>
        {!data ? (
          <p className="muted">{t("aiLoading")}</p>
        ) : toGet.length === 0 ? (
          <p className="muted">{t("libGetMoreNone")}</p>
        ) : (
          <>
            <p className="muted folder-note">{t("libGetMoreIntro")}</p>
            {offered.length > 0 && <Entries t={t} entries={offered} view="tiles" glyph={x.glyph} label={`${t("libGetMore")}: ${name}`} />}
          </>
        )}
      </section>
      {byUser.length > 0 && (
        <section className="stack explorer-section by-user" aria-labelledby={byUserId}>
          <h2 id={byUserId} className="section-title">{t("offerUserGroup")}</h2>
          <p className="muted folder-note">{t("offerUserGroupIntro")}</p>
          <Entries t={t} entries={byUser} view="tiles" glyph={x.glyph} label={t("offerUserGroup")} />
        </section>
      )}
    </div>
  );
}
