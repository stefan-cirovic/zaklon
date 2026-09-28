import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import type { Key, Lang } from "../i18n";
import { Icon } from "../components/Icon";
import { SettingsIcon } from "../components/SettingsIcon";
import {
  HELP_GROUPS,
  HELP_ICONS,
  HELP_TOPICS,
  helpHref,
  helpLoaded,
  helpRoute,
  loadHelp,
  parseInline,
  searchHelp,
  type HelpBlock,
  type HelpContent,
  type HelpTopicId,
} from "../help";

type T = (k: Key) => string;

/** The page and section in the address (#help/supplies/scan), following the back and forward buttons. */
function useRoute() {
  const [route, setRoute] = useState(() => helpRoute(location.hash));
  useEffect(() => {
    const onHash = () => setRoute(helpRoute(location.hash));
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);
  return route;
}

/** What was searched last, still there when you come back from a topic it found. */
let lastQuery = "";

/**
 * The user guide: a home with every topic in groups and a search, and a page
 * per topic (#help/<topic>), with the other topics beside it on a laptop and
 * its sections listed on a wide screen. The text is in help/en.ts and
 * help/sr.ts, loaded the first time Help opens.
 */
export default function Help({ t, lang }: { t: T; lang: Lang }) {
  const route = useRoute();
  const [loaded, setLoaded] = useState<{ lang: Lang; content: HelpContent } | null>(() => {
    const content = helpLoaded(lang);
    return content ? { lang, content } : null;
  });
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    setFailed(false);
    loadHelp(lang)
      .then((content) => alive && setLoaded({ lang, content }))
      .catch(() => alive && setFailed(true));
    return () => {
      alive = false;
    };
  }, [lang]);

  const content = loaded?.lang === lang ? loaded.content : null;
  if (!content) {
    return (
      <div className="stack">
        <div className="page-head">
          <h1>{t("help")}</h1>
        </div>
        {failed ? <p className="error" role="alert">{t("helpNotLoaded")}</p> : <p className="muted">{t("aiLoading")}</p>}
      </div>
    );
  }
  return route.topic ? <TopicPage t={t} content={content} id={route.topic} section={route.section} /> : <HelpHome t={t} content={content} />;
}

/** Text with its **bold** parts and [links](#screen). */
function Rich({ text }: { text: string }) {
  return (
    <>
      {parseInline(text).map((x, i) =>
        "text" in x ? <Fragment key={i}>{x.text}</Fragment> : "bold" in x ? <strong key={i}>{x.bold}</strong> : <a key={i} href={x.href}>{x.link}</a>,
      )}
    </>
  );
}

function Block({ t, block }: { t: T; block: HelpBlock }) {
  if ("p" in block) return <p><Rich text={block.p} /></p>;
  if ("steps" in block) {
    return (
      <ol className="help-steps">
        {block.steps.map((s, i) => <li key={i}><Rich text={s} /></li>)}
      </ol>
    );
  }
  if ("list" in block) {
    return (
      <ul className="help-list">
        {block.list.map((s, i) => <li key={i}><Rich text={s} /></li>)}
      </ul>
    );
  }
  const warn = "warn" in block;
  const text = "warn" in block ? block.warn : block.note;
  return (
    <div className={"help-callout" + (warn ? " important" : "")} role="note">
      <span className="help-callout-label">{t(warn ? "helpWarn" : "helpNote")}</span>
      <p><Rich text={text} /></p>
    </div>
  );
}

function TopicTile({ content, id }: { content: HelpContent; id: HelpTopicId }) {
  const topic = content[id];
  return (
    <a className="set-tile" href={helpHref(id)} data-topic={id}>
      <span className="set-tile-icon">
        <Icon name={HELP_ICONS[id]} size={24} />
      </span>
      <span className="set-tile-text">
        <span className="set-tile-title">{topic.title}</span>
        <span className="set-tile-desc">{topic.summary}</span>
      </span>
      <span className="set-tile-go">
        <SettingsIcon name="chevron" size={18} />
      </span>
    </a>
  );
}

/** Every topic in its group, and a search through all of them (Enter opens the first found). */
function HelpHome({ t, content }: { t: T; content: HelpContent }) {
  const [query, setQueryState] = useState(lastQuery);
  const setQuery = (q: string) => {
    lastQuery = q;
    setQueryState(q);
  };
  const hits = useMemo(() => searchHelp(content, query), [content, query]);
  const typed = query.trim() !== "";
  useEffect(() => {
    window.scrollTo(0, 0);
  }, []);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && hits[0]) {
      e.preventDefault();
      location.hash = helpHref(hits[0].topic, hits[0].sections[0]?.id).slice(1);
    } else if (e.key === "Escape" && query) {
      e.preventDefault();
      setQuery("");
    }
  };

  return (
    <div className="stack help-home">
      <div className="page-head">
        <h1>{t("help")}</h1>
        <p className="muted">{t("helpIntro")}</p>
      </div>
      <div className="set-search help-search" role="search">
        <SettingsIcon name="search" size={18} />
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKey}
          placeholder={t("helpSearch")}
          aria-label={t("helpSearch")}
          autoComplete="off"
          spellCheck={false}
          maxLength={200}
        />
      </div>
      <p role="status" className={typed && hits.length === 0 ? "muted help-none" : "sr-only"}>
        {!typed ? "" : hits.length === 0 ? t("helpNoResults") : `${t("helpFound")} ${hits.length}`}
      </p>
      {typed ? (
        hits.length > 0 && (
          <ul className="help-results">
            {hits.map((h) => {
              const topic = content[h.topic];
              return (
                <li key={h.topic} className="help-result">
                  <a className="help-result-main" href={helpHref(h.topic, h.sections[0]?.id)}>
                    <span className="set-tile-icon">
                      <Icon name={HELP_ICONS[h.topic]} size={22} />
                    </span>
                    <span className="set-tile-text">
                      <span className="set-tile-title">{topic.title}</span>
                      <span className="set-tile-desc">{topic.summary}</span>
                    </span>
                  </a>
                  {h.sections.length > 0 && (
                    <p className="help-found-in">
                      <span className="muted">{t("helpFoundIn")}:</span>{" "}
                      {h.sections.map((s, i) => (
                        <Fragment key={s.id}>
                          {i > 0 && ", "}
                          <a href={helpHref(h.topic, s.id)}>{s.title}</a>
                        </Fragment>
                      ))}
                    </p>
                  )}
                </li>
              );
            })}
          </ul>
        )
      ) : (
        HELP_GROUPS.map((g) => (
          <section key={g.title} className="stack help-group" aria-labelledby={`help-group-${g.title}`}>
            <h2 id={`help-group-${g.title}`} className="section-title">{t(g.title)}</h2>
            <ul className="set-tiles">
              {g.topics.map((id) => (
                <li key={id}>
                  <TopicTile content={content} id={id} />
                </li>
              ))}
            </ul>
          </section>
        ))
      )}
    </div>
  );
}

/** A topic's sections as links: beside the page on a wide screen, above it on a narrow one. */
function Contents({ t, content, id, className }: { t: T; content: HelpContent; id: HelpTopicId; className: string }) {
  return (
    <nav className={"help-toc " + className} aria-label={t("helpOnThisPage")}>
      <div className="help-toc-title">{t("helpOnThisPage")}</div>
      <ul>
        {content[id].sections.map((s) => (
          <li key={s.id}>
            <a href={helpHref(id, s.id)}>{s.title}</a>
          </li>
        ))}
      </ul>
    </nav>
  );
}

/** One topic: "Help › Supplies" with a way back, its sections, the screen it is about, and the topics before and after it. */
function TopicPage({ t, content, id, section }: { t: T; content: HelpContent; id: HelpTopicId; section: string | null }) {
  const topic = content[id];
  const title = useRef<HTMLHeadingElement>(null);
  const at = HELP_TOPICS.indexOf(id);
  const prev = at > 0 ? HELP_TOPICS[at - 1] : null;
  const next = at < HELP_TOPICS.length - 1 ? HELP_TOPICS[at + 1] : null;

  // A new page starts at the top with its title in focus, or at the section asked for.
  useEffect(() => {
    const el = section ? document.getElementById(`help-${section}`) : null;
    if (el) {
      el.scrollIntoView({ block: "start" });
      el.focus({ preventScroll: true });
    } else {
      window.scrollTo(0, 0);
      title.current?.focus({ preventScroll: true });
    }
  }, [id, section]);

  return (
    <div className="set-page help-page">
      <aside className="set-side">
        <nav aria-label={t("helpTopics")}>
          <ul className="set-nav">
            {HELP_TOPICS.map((x) => (
              <li key={x}>
                <a href={helpHref(x)} aria-current={x === id ? "page" : undefined}>
                  <Icon name={HELP_ICONS[x]} size={18} />
                  <span>{content[x].title}</span>
                </a>
              </li>
            ))}
          </ul>
        </nav>
      </aside>
      <div className="set-main">
        <div className="set-head">
          <a className="set-back" href="#help" aria-label={t("helpBackToHelp")} title={t("helpBackToHelp")}>
            <SettingsIcon name="back" size={20} />
          </a>
          <nav className="set-crumbs" aria-label={t("breadcrumb")}>
            <a href="#help">{t("help")}</a>
            <span aria-hidden="true">›</span>
          </nav>
          <h1 ref={title} tabIndex={-1}>{topic.title}</h1>
        </div>
        <div className="help-layout">
          <article className="help-article" aria-label={topic.title}>
            <p className="help-summary">{topic.summary}</p>
            {topic.sections.length >= 4 && <Contents t={t} content={content} id={id} className="help-toc-inline" />}
            {topic.sections.map((s) => (
              <section key={s.id} id={`help-${s.id}`} className="help-section" tabIndex={-1} aria-labelledby={`help-h-${s.id}`}>
                <h2 id={`help-h-${s.id}`}>{s.title}</h2>
                {s.body.map((b, i) => (
                  <Block key={i} t={t} block={b} />
                ))}
              </section>
            ))}
            {topic.open && (
              <p className="help-open">
                <a className="btn" href={topic.open.href}>{topic.open.label}</a>
              </p>
            )}
            <nav className="help-pager" aria-label={`${t("helpPrevious")} / ${t("helpNext")}`}>
              {prev && (
                <a className="prev" href={helpHref(prev)} rel="prev">
                  <span className="help-pager-label">‹ {t("helpPrevious")}</span>
                  <span>{content[prev].title}</span>
                </a>
              )}
              {next && (
                <a className="next" href={helpHref(next)} rel="next">
                  <span className="help-pager-label">{t("helpNext")} ›</span>
                  <span>{content[next].title}</span>
                </a>
              )}
            </nav>
          </article>
          <aside className="help-toc-side">
            <Contents t={t} content={content} id={id} className="" />
          </aside>
        </div>
      </div>
    </div>
  );
}
