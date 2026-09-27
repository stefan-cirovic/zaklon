import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { api, ApiError } from "../api";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { fmtBytes } from "../format";
import Reader from "../components/Reader";
import PhoneAi from "./PhoneAi";

type T = (k: Key) => string;
type EngineState = "missing" | "no_model" | "stopped" | "starting" | "ready" | "failed";
type ModelChoice = { id: string; title_en: string; title_sr: string; size: number; installed: boolean; recommended: boolean };
type Overview = {
  engine: EngineState;
  engine_installed: boolean;
  selected: string | null;
  recommended: string;
  ram_total: number;
  models: ModelChoice[];
  books: number;
};
type Source = { n: number; title: string; url: string; book_title_en: string; book_title_sr: string };
type Proposal = {
  action: "add" | "use" | "shopping";
  item_id: string | null;
  name: string;
  quantity: number;
  unit: string;
  category: string;
  current: number | null;
};
type Answer = {
  id: string;
  from_supplies?: boolean;
  proposal?: Proposal | null;
  /** What happened to the proposal on this device. */
  outcome?: "done" | "canceled";
  question: string;
  status: "searching" | "starting" | "thinking" | "done" | "failed";
  text: string;
  sources: Source[];
  searched?: string[];
  grounded: boolean;
  language: string;
  tokens_per_second: number;
  error: string | null;
};
type PackState = { id: string; state: { status: string; bytes_done: number; bytes_total: number } };

const STORE = "zaklon.chat";
const KEEP = 20;

function loadChat(): Answer[] {
  try {
    const raw = localStorage.getItem(STORE);
    const list = raw ? (JSON.parse(raw) as Answer[]) : [];
    return Array.isArray(list) ? list.filter((a) => a.status === "done" || a.status === "failed") : [];
  } catch {
    return [];
  }
}

function saveChat(list: Answer[]) {
  try {
    localStorage.setItem(STORE, JSON.stringify(list.filter((a) => a.status === "done").slice(-KEEP)));
  } catch {
    /* private mode: just for this session */
  }
}

/** Models write **bold**; show it as bold, and "* item" lists with bullets. */
function bold(text: string) {
  const lines = text.replace(/^(\s*)[*-] /gm, "$1• ");
  return lines.split(/(\*\*[^*\n]+\*\*)/g).map((p, i) =>
    p.startsWith("**") && p.endsWith("**") && p.length > 4 ? <strong key={i}>{p.slice(2, -2)}</strong> : <Fragment key={i}>{p.replace(/\*\*/g, "")}</Fragment>,
  );
}

/** Answer text with [1]-style marks turned into buttons that open the source. */
function AnswerText({ text, sources, open }: { text: string; sources: Source[]; open: (s: Source) => void }) {
  const parts = text.split(/(\[\d+(?:,\s*\d+)*\])/g);
  return (
    <div className="answer-text">
      {parts.map((p, i) => {
        const m = p.match(/^\[(\d+(?:,\s*\d+)*)\]$/);
        if (!m) return <Fragment key={i}>{bold(p)}</Fragment>;
        const nums = m[1].split(",").map((n) => Number(n.trim()));
        return (
          <Fragment key={i}>
            {nums.map((n) => {
              const s = sources.find((x) => x.n === n);
              return s ? (
                <button key={n} className="cite" onClick={() => open(s)} title={s.title}>{n}</button>
              ) : null;
            })}
          </Fragment>
        );
      })}
    </div>
  );
}

export default function Assistant({ t, lang, isHub, go }: { t: T; lang: Lang; isHub: boolean; go: (tab: string) => void }) {
  const [ov, setOv] = useState<Overview | null>(null);
  const [hubDown, setHubDown] = useState(false);
  const [chat, setChat] = useState<Answer[]>(loadChat);
  const [question, setQuestion] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [reader, setReader] = useState<Source | null>(null);
  const [downloads, setDownloads] = useState<PackState[]>([]);
  const [showPhone, setShowPhone] = useState(false);
  const endRef = useRef<HTMLDivElement | null>(null);
  const asking = useRef(false);
  const [confirming, setConfirming] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setOv(await api<Overview>("/api/assistant"));
      setHubDown(false);
    } catch {
      setHubDown(true);
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // While a model or the engine downloads, follow it.
  const modelIds = ov?.models.map((m) => m.id) ?? [];
  const downloading = downloads.some((d) => ["queued", "downloading", "verifying"].includes(d.state.status));
  const pollDownloads = useCallback(async () => {
    try {
      const c = await api<{ packs: PackState[] }>("/api/catalog");
      setDownloads(c.packs.filter((p) => p.id === "llama-cpp" || modelIds.includes(p.id)));
    } catch {
      /* try again */
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelIds.join(",")]);
  useEffect(() => {
    if (!ov || ov.engine !== "missing" && ov.engine !== "no_model") return;
    pollDownloads();
    const id = setInterval(() => {
      pollDownloads();
      load();
    }, downloading ? 1500 : 8000);
    return () => clearInterval(id);
  }, [ov, downloading, pollDownloads, load]);

  const busy = chat.some((a) => !["done", "failed"].includes(a.status));
  const current = chat.find((a) => !["done", "failed"].includes(a.status));

  // Follow the answer being written.
  useEffect(() => {
    if (!current) return;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let failures = 0;
    let lastStatus = "";
    // One request at a time, so replies cannot arrive out of order. A phone
    // that drops Wi-Fi for a moment keeps waiting; only a lost answer (404)
    // or a long silence ends it.
    const tick = async () => {
      try {
        const a = await api<Answer>(`/api/assistant/answers/${current.id}`);
        if (!alive) return;
        failures = 0;
        setChat((list) => {
          const next = list.map((x) => (x.id === a.id ? { ...a, outcome: x.outcome } : x));
          if (a.status === "done" || a.status === "failed") saveChat(next);
          return next;
        });
        if (a.status !== lastStatus) {
          lastStatus = a.status;
          load();
        }
        if (a.status === "done" || a.status === "failed") return;
      } catch (e) {
        if (!alive) return;
        failures += 1;
        const lost = e instanceof ApiError && e.status === 404;
        if (lost || failures >= 40) {
          setChat((list) => list.map((x) => (x.id === current.id ? { ...x, status: "failed", error: String(e instanceof Error ? e.message : e) } : x)));
          return;
        }
      }
      timer = setTimeout(tick, failures ? 1500 : 400);
    };
    tick();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
    };
  }, [current?.id, load]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [chat.length, current?.text.length]);

  const ask = async (e: React.FormEvent) => {
    e.preventDefault();
    const q = question.trim();
    if (!q || busy || asking.current) return;
    asking.current = true;
    setErr(null);
    const history = chat
      .filter((a) => a.status === "done")
      .slice(-2)
      .map((a) => ({ question: a.question, answer: a.text }));
    try {
      const r = await api<{ id: string }>("/api/assistant/ask", { json: { question: q, language: lang, history } });
      setChat((list) => [
        ...list,
        { id: r.id, question: q, status: "searching", text: "", sources: [], grounded: false, language: lang, tokens_per_second: 0, error: null },
      ]);
      setQuestion("");
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      asking.current = false;
    }
  };

  const clear = () => {
    setChat([]);
    saveChat([]);
  };

  const download = async (id: string) => {
    setErr(null);
    try {
      await api(`/api/packs/${id}/download`, { method: "POST" });
      pollDownloads();
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  const select = async (id: string) => {
    try {
      await api("/api/assistant/model", { json: { id } });
      load();
    } catch (ex) {
      setErr(errText(t, ex));
    }
  };

  // Carry out a proposed change through the normal supplies API.
  const confirm = async (a: Answer) => {
    const p = a.proposal;
    if (!p || confirming) return;
    setErr(null);
    setConfirming(a.id);
    try {
      if (p.action === "add" && p.item_id) await api(`/api/items/${p.item_id}/adjust`, { json: { delta: p.quantity } });
      else if (p.action === "add") await api("/api/items", { json: { name: p.name, quantity: p.quantity, unit: p.unit, category: p.category } });
      else if (p.action === "use" && p.item_id) await api(`/api/items/${p.item_id}/adjust`, { json: { delta: -p.quantity } });
      else if (p.action === "shopping")
        await api("/api/shopping", { json: { text: p.name, quantity: p.quantity > 0 ? p.quantity : null, unit: p.quantity > 0 ? p.unit : null, item_id: p.item_id } });
      setOutcome(a.id, "done");
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setConfirming(null);
    }
  };
  const setOutcome = (id: string, outcome: "done" | "canceled") =>
    setChat((list) => {
      const next = list.map((x) => (x.id === id ? { ...x, outcome } : x));
      saveChat(next);
      return next;
    });

  const title = (m: ModelChoice) => (lang === "sr" && m.title_sr ? m.title_sr : m.title_en);
  const bookTitle = (s: Source) => (lang === "sr" && s.book_title_sr ? s.book_title_sr : s.book_title_en);

  if (reader) {
    return <Reader t={t} lang={lang} url={reader.url} title={reader.title} onClose={() => setReader(null)} />;
  }

  // A phone away from home (or a hub without the assistant) uses its own model.
  const hubReady = !!ov && ov.engine !== "missing" && ov.engine !== "no_model";
  if (!isHub && (hubDown || (ov && !hubReady))) {
    return (
      <div className="stack">
        <div className="page-head">
          <h1>{t("assistant")}</h1>
          <p className="muted">{hubDown ? t("aiAwayFromHub") : t("aiHubHasNoModel")}</p>
        </div>
        <PhoneAi t={t} lang={lang} />
      </div>
    );
  }

  const rec = ov?.models.find((m) => m.id === ov.recommended) ?? ov?.models[0];
  const stateOf = (id: string) => downloads.find((d) => d.id === id)?.state;
  const installedModels = ov?.models.filter((m) => m.installed) ?? [];
  const statusText: Record<Answer["status"], Key> = {
    searching: "aiSearching",
    starting: "aiStarting",
    thinking: "aiWriting",
    done: "aiWriting",
    failed: "aiWriting",
  };

  return (
    <div className="stack assistant">
      <div className="page-head">
        <h1>{t("assistant")}</h1>
        <p className="muted">{t("assistantIntro")}</p>
      </div>
      {err && <p className="error" role="alert">{err}</p>}

      {ov && !hubReady && rec && (
        <div className="panel stack">
          <h2>{t("aiNeedsModel")}</h2>
          <p className="muted" style={{ margin: 0 }}>
            {t("aiRecommendedFor")} {fmtBytes(ov.ram_total)}: <strong>{title(rec)}</strong> ({fmtBytes(rec.size)})
          </p>
          {(() => {
            const st = stateOf(rec.id);
            const eng = stateOf("llama-cpp");
            const active = [st, eng].find((s) => s && ["queued", "downloading", "verifying"].includes(s.status));
            if (active) {
              const pct = active.bytes_total ? Math.round((active.bytes_done / active.bytes_total) * 100) : 0;
              return (
                <>
                  <div className="bar"><i style={{ width: `${pct}%` }} /></div>
                  <p className="muted" style={{ margin: 0, fontSize: 13 }}>{t("aiDownloading")} {fmtBytes(active.bytes_done)} / {fmtBytes(active.bytes_total)}</p>
                </>
              );
            }
            return (
              <div className="row wrap">
                <button className="btn" onClick={() => download(rec.id)}>{t("download")}</button>
                <button className="btn secondary" onClick={() => go("addons")}>{t("aiOtherModels")}</button>
              </div>
            );
          })()}
        </div>
      )}

      {ov && hubReady && (
        <div className="row between wrap model-line">
          <label className="row model-pick">
            <span className="muted" style={{ fontSize: 14 }}>{t("aiModel")}</span>
            <select value={ov.selected ?? ""} onChange={(e) => select(e.target.value)} disabled={busy}>
              {installedModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {title(m)}{m.recommended ? ` · ${t("recommended")}` : ""}
                </option>
              ))}
            </select>
          </label>
          <span className="muted" style={{ fontSize: 13 }}>
            {ov.engine === "ready" ? t("aiReady") : ov.engine === "starting" ? t("aiStarting") : ov.engine === "failed" ? t("aiFailed") : t("aiSleeping")}
            {ov.books === 0 && ` · ${t("aiNoLibrary")}`}
          </span>
        </div>
      )}

      {hubReady && (
        <>
          {ov?.books === 0 && (
            <p className="warn" style={{ margin: 0, fontSize: 14 }}>
              {t("aiNoLibraryLong")} <a href="#addons">{t("addons")}</a>.
            </p>
          )}
          <div className="chat">
            {chat.length === 0 && <p className="muted">{t("aiEmptyChat")}</p>}
            {chat.map((a) => (
              <div key={a.id} className="exchange">
                <div className="q">{a.question}</div>
                <div className="a panel left">
                  {a.status === "failed" ? (
                    <p className="error" style={{ margin: 0 }}>{errText(t, new Error(a.error ?? ""))}</p>
                  ) : (
                    <>
                      {a.text ? <AnswerText text={a.text} sources={a.sources} open={setReader} /> : <p className="muted" style={{ margin: 0 }}>{t(statusText[a.status])}</p>}
                      {a.status === "done" && a.proposal && (
                        <div className="row wrap proposal">
                          {a.outcome === "done" ? (
                            <span className="ok">✓ {t("aiDone")} · <a href="#supplies">{t("supplies")}</a></span>
                          ) : a.outcome === "canceled" ? (
                            <span className="muted">{t("aiCancelled")}</span>
                          ) : (
                            <>
                              <button className="btn" onClick={() => confirm(a)} disabled={confirming === a.id}>{t("aiConfirm")}</button>
                              <button className="btn secondary" onClick={() => setOutcome(a.id, "canceled")}>{t("cancel")}</button>
                            </>
                          )}
                        </div>
                      )}
                      {a.status === "done" && a.from_supplies && !a.proposal && (
                        <div className="muted" style={{ fontSize: 13, marginTop: 8 }}>{t("aiFromSupplies")} · <a href="#supplies">{t("supplies")}</a></div>
                      )}
                      {a.status === "done" && !a.grounded && <p className="warn" style={{ margin: "8px 0 0", fontSize: 13 }}>{t("aiNotGrounded")}</p>}
                      {a.sources.length > 0 && (a.status === "done" || a.text) && (
                        <div className="sources">
                          <div className="label">{t("aiSources")}</div>
                          {a.sources.map((s) => (
                            <button key={s.n} className="source" onClick={() => setReader(s)}>
                              <span className="cite static">{s.n}</span> {s.title} <span className="muted">· {bookTitle(s)}</span>
                            </button>
                          ))}
                        </div>
                      )}
                      {a.status === "done" && (a.tokens_per_second > 0 || (a.searched?.length ?? 0) > 0) && (
                        <div className="muted" style={{ fontSize: 12, marginTop: 6 }}>
                          {a.searched && a.searched.length > 0 && !a.from_supplies && `${t("aiSearched")}: ${a.searched.join(", ")}`}
                          {a.searched && a.searched.length > 0 && !a.from_supplies && a.tokens_per_second > 0 && " · "}
                          {a.tokens_per_second > 0 && `${a.tokens_per_second.toFixed(1)} ${t("tokensPerSecond")}`}
                        </div>
                      )}
                    </>
                  )}
                </div>
              </div>
            ))}
            <div ref={endRef} />
          </div>
          <form className="stack ask-form" onSubmit={ask}>
            <textarea
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  (e.currentTarget.form as HTMLFormElement).requestSubmit();
                }
              }}
              placeholder={t("askExample")}
              aria-label={t("askSomething")}
              rows={2}
              maxLength={2000}
            />
            <div className="row between wrap">
              <button className="btn" disabled={busy || !question.trim()}>{busy ? t("aiThinking") : t("ask")}</button>
              {chat.length > 0 && !busy && <button type="button" className="btn secondary small" onClick={clear}>{t("aiNewChat")}</button>}
            </div>
          </form>
        </>
      )}

      {!isHub && (
        <div className="stack">
          <button className="btn secondary" onClick={() => setShowPhone(!showPhone)} aria-expanded={showPhone}>
            {showPhone ? "▴ " : "▾ "}{t("aiOnThisPhone")}
          </button>
          {showPhone && <PhoneAi t={t} lang={lang} />}
        </div>
      )}
    </div>
  );
}
