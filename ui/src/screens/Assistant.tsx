import { Fragment, useCallback, useEffect, useRef, useState } from "react";
import { api, ApiError, inTauri } from "../api";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Key, Lang } from "../i18n";
import { errText } from "../errors";
import { useVisiblePoll, whenVisible } from "../poll";
import { fmtBytes, fmtQty } from "../format";
import { forget, onBackOnline } from "../offline";
import {
  clearQuestionFromHome,
  filterByTitle,
  questionFromHome,
  readOpen,
  toAnswer,
  writeOpen,
  type Answer,
  type Conversation,
  type SavedTurn,
  type Source,
  type Summary,
} from "../conversations";
import Reader from "../components/Reader";
import PhoneAi from "./PhoneAi";
import Memory from "../components/Memory";
import ConversationList from "../components/ConversationList";
import ChatTitle from "../components/ChatTitle";
import SidePanel from "../components/SidePanel";
import HelpLink from "../components/HelpLink";
import { Icon } from "../components/Icon";
import Suggestions from "../components/Suggestions";
import RichText from "../components/RichText";

type T = (k: Key) => string;
type EngineState = "missing" | "no_model" | "stopped" | "starting" | "ready" | "failed";
/** `fits`: the hub has the memory for it (an older hub does not say). */
type ModelChoice = { id: string; title_en: string; title_sr: string; size: number; installed: boolean; recommended: boolean; fits?: boolean };
type Overview = {
  engine: EngineState;
  engine_installed: boolean;
  selected: string | null;
  /** null: no model fits the hub's memory. */
  recommended: string | null;
  ram_total: number;
  models: ModelChoice[];
  books: number;
};
type PackState = { id: string; state: { status: string; bytes_done: number; bytes_total: number } };
/** The open conversation could not be shown: not kept on this phone away from the hub, or the hub failed. */
type Unshown = "notKept" | "failed" | null;

/** A hub from before saved conversations: the conversation stays on this device, as it did then. */
const STORE = "zaklon.chat";
const KEEP = 20;
/** How often the list is asked for again (a copy sent from another device shows up). */
const LIST_EVERY = 30_000;

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

/** The model the hub would answer with needs more memory than the hub has. */
function tooBig(ov: Overview): boolean {
  return ov.models.find((m) => m.id === ov.selected)?.fits === false;
}

/** No AI model fits the hub's memory: the assistant is not for that computer. */
function noAiThere(ov: Overview): boolean {
  return ov.recommended === null && ov.models.length > 0;
}

const NARROW = "(max-width: 899px)";

/** A phone-sized screen: the list of conversations becomes a panel opened from the top. */
function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(() => typeof window !== "undefined" && window.matchMedia(NARROW).matches);
  useEffect(() => {
    const m = window.matchMedia(NARROW);
    const on = () => setNarrow(m.matches);
    on();
    m.addEventListener("change", on);
    return () => m.removeEventListener("change", on);
  }, []);
  return narrow;
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
  const narrow = useNarrow();
  const [ov, setOv] = useState<Overview | null>(null);
  const [hubDown, setHubDown] = useState(false);
  // Saved conversations: whether the hub keeps them (null until known; an
  // older hub does not), this device's list, and the one that is open.
  const [saved, setSaved] = useState<boolean | null>(null);
  const [list, setList] = useState<Summary[] | null>(null);
  // A question typed on Home: it starts a new conversation here (read without
  // taking it, so a second render sees it too; it is taken once shown).
  const [fromHome] = useState(questionFromHome);
  const pendingAsk = useRef(fromHome);
  useEffect(() => clearQuestionFromHome(), []);
  const [openId, setOpenId] = useState<string | null>(() => (questionFromHome() ? null : readOpen()));
  const [conv, setConv] = useState<Summary | null>(null);
  const [unshown, setUnshown] = useState<Unshown>(null);
  const [chat, setChat] = useState<Answer[]>([]);
  // Answers shown from before (a saved conversation, the last visit) are not read out again.
  const restored = useRef(new Set<string>());
  // A conversation the screen already shows (it was just started here): not loaded again.
  const shown = useRef<string | null>(null);
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<Summary[] | null>(null);
  const [drawer, setDrawer] = useState(false);
  const [memoryOpen, setMemoryOpen] = useState(false);
  const [question, setQuestion] = useState(() => questionFromHome() ?? "");
  const [err, setErr] = useState<string | null>(null);
  const [reader, setReader] = useState<Source | null>(null);
  const [downloads, setDownloads] = useState<PackState[]>([]);
  const [showPhone, setShowPhone] = useState(false);
  const endRef = useRef<HTMLDivElement | null>(null);
  const box = useRef<HTMLTextAreaElement | null>(null);
  const asking = useRef(false);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [memoryVersion, setMemoryVersion] = useState(0);
  // Online research is off unless switched on, for this conversation only.
  const [online, setOnline] = useState(false);

  const load = useCallback(async () => {
    try {
      setOv(await api<Overview>("/api/assistant"));
      setHubDown(false);
    } catch {
      setHubDown(true);
    }
  }, []);

  const loadList = useCallback(async () => {
    try {
      const l = await api<Summary[]>("/api/conversations");
      // A hub from before saved conversations answers with its page, not a list.
      if (!Array.isArray(l)) throw new SyntaxError("not a list");
      setList(l);
      setSaved(true);
    } catch (e) {
      if (e instanceof SyntaxError || (e instanceof ApiError && (e.status === 404 || e.status === 405))) {
        setSaved(false);
        setList([]);
      }
      // Out of reach with no copy on this phone: keep what is shown.
    }
  }, []);

  useEffect(() => {
    load();
    // Start loading the model now, so the first answer comes sooner.
    api("/api/assistant/warm", { json: { language: lang } }).catch(() => {});
    // Once per visit to the screen.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [load]);
  useVisiblePoll(loadList, LIST_EVERY);
  // A phone back home: the hub's assistant and the fresh list again.
  useEffect(
    () =>
      onBackOnline(() => {
        load();
        loadList();
      }),
    [load, loadList],
  );

  // A hub without saved conversations: the conversation of the last visit, kept on this device.
  useEffect(() => {
    if (saved !== false) return;
    const old = loadChat();
    old.forEach((a) => restored.current.add(a.id));
    setChat(old);
    setOpenId(null);
  }, [saved]);

  const loadConversation = useCallback(async (id: string) => {
    try {
      const c = await api<Conversation>(`/api/conversations/${encodeURIComponent(id)}`);
      if (!c || !Array.isArray(c.turns)) throw new SyntaxError("not a conversation");
      // Another one was opened meanwhile.
      if (shown.current !== id) return;
      const answers = c.turns.map(toAnswer);
      answers.filter((a) => a.status === "done" || a.status === "failed").forEach((a) => restored.current.add(a.id));
      setConv(c);
      setChat(answers);
      setUnshown(null);
    } catch (e) {
      if (shown.current !== id) return;
      setConv(null);
      setChat([]);
      if (e instanceof ApiError && e.status === 404) {
        // Gone (deleted, or this phone was paired with another hub since): a new conversation instead.
        shown.current = null;
        setOpenId(null);
        return;
      }
      setUnshown(isHub ? "failed" : "notKept");
    }
  }, [isHub]);

  // Open the chosen conversation (or a new one), and remember it for the next visit.
  useEffect(() => {
    if (saved === false) return;
    writeOpen(openId);
    if (!openId) {
      setConv(null);
      setChat([]);
      setUnshown(null);
      return;
    }
    if (shown.current === openId) return;
    shown.current = openId;
    setChat([]);
    loadConversation(openId);
  }, [openId, saved, loadConversation]);

  // Search: the hub looks in titles and questions; away from it, the titles on this phone.
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setFound(null);
      return;
    }
    let alive = true;
    const local = () => filterByTitle(list ?? [], q);
    if (hubDown) {
      setFound(local());
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const r = await api<Summary[]>(`/api/conversations?q=${encodeURIComponent(q)}`);
        if (alive) setFound(Array.isArray(r) ? r : local());
      } catch {
        if (alive) setFound(local());
      }
    }, 250);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
    // The list itself changes often; a search is run again when its words change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, hubDown]);

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
  const needsDownload = !!ov && (ov.engine === "missing" || ov.engine === "no_model");
  useVisiblePoll(async () => {
    await Promise.all([pollDownloads(), load()]);
  }, needsDownload ? (downloading ? 1500 : 8000) : null);

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
    // or a long silence ends it. Nothing is asked while the app is in the
    // background; the answer is fetched on return.
    const tick = async () => {
      await whenVisible();
      if (!alive) return;
      try {
        const a = await api<Answer>(`/api/assistant/answers/${current.id}`);
        if (!alive) return;
        failures = 0;
        setChat((list) => {
          const next = list.map((x) => (x.id === a.id ? { ...a, turnId: x.turnId, outcome: x.outcome } : x));
          if (saved === false && (a.status === "done" || a.status === "failed")) saveChat(next);
          return next;
        });
        if (a.status !== lastStatus) {
          lastStatus = a.status;
          load();
        }
        if (a.status === "done" || a.status === "failed") {
          loadList();
          return;
        }
      } catch (e) {
        if (!alive) return;
        failures += 1;
        const lost = e instanceof ApiError && e.status === 404;
        if (lost && openId && saved) {
          // The hub restarted meanwhile: the saved conversation says what became of it.
          loadConversation(openId);
          return;
        }
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.id, load]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [chat.length, current?.text.length, openId]);

  // The box grows with the question, up to a few lines.
  useEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, [question]);

  const readOnly = !isHub && hubDown;

  // Another conversation: online research is off again (it is switched on per conversation).
  const newChat = () => {
    setDrawer(false);
    setErr(null);
    setOnline(false);
    if (saved === false) {
      setChat([]);
      saveChat([]);
      return;
    }
    setOpenId(null);
    if (!narrow) box.current?.focus();
  };

  const openChat = (id: string) => {
    setDrawer(false);
    if (id === openId) {
      // Shown already; one that could not be loaded is tried again.
      if (unshown) loadConversation(id);
      return;
    }
    setErr(null);
    setOnline(false);
    shown.current = null;
    setOpenId(id);
  };

  const ask = (e: React.FormEvent) => {
    e.preventDefault();
    void send(question.trim());
  };

  const send = async (q: string) => {
    if (!q || busy || asking.current) return;
    asking.current = true;
    setErr(null);
    // The conversation so far, for a hub that does not keep conversations (one that does uses its own).
    const history = chat
      .filter((a) => a.status === "done")
      .slice(-2)
      .map((a) => ({ question: a.question, answer: a.text }));
    const where = saved === false ? {} : openId ? { conversation: openId } : { new_conversation: true };
    try {
      const r = await api<{ id: string; conversation?: Summary; turn?: SavedTurn }>("/api/assistant/ask", { json: { question: q, language: lang, history, online, ...where } });
      const c = r.conversation;
      if (c) {
        // A new conversation is in the list at once, with its title.
        if (c.id !== openId) {
          shown.current = c.id;
          setOpenId(c.id);
        }
        setConv(c);
        setList((l) => [c, ...(l ?? []).filter((x) => x.id !== c.id)]);
      }
      setChat((list) => [
        ...list,
        { id: r.id, turnId: r.turn?.id, question: q, status: "searching", text: "", sources: [], grounded: false, language: lang, tokens_per_second: 0, error: null },
      ]);
      setQuestion("");
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      asking.current = false;
    }
  };

  // The question from Home is asked in a new conversation as soon as the hub's
  // assistant is known to be ready. When it cannot be asked (no AI model yet)
  // it waits in the question box; a phone away from the hub gets it in the
  // box of its own AI.
  useEffect(() => {
    const q = pendingAsk.current;
    if (!q) return;
    if (hubDown) {
      pendingAsk.current = null;
      return;
    }
    if (!ov || saved === null) return;
    pendingAsk.current = null;
    const ready = ov.engine !== "missing" && ov.engine !== "no_model" && !tooBig(ov);
    if (ready && openId === null) void send(q);
  });

  // Stop the answer being written; the hub keeps what was written so far.
  const stop = (id: string) => {
    api(`/api/assistant/answers/${id}/cancel`, { method: "POST" }).catch(() => {});
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
      else if (p.action === "remember") await api("/api/memory", { json: { text: p.name } });
      else if (p.action === "shopping")
        await api("/api/shopping", { json: { text: p.name, quantity: p.quantity > 0 ? p.quantity : null, unit: p.quantity > 0 ? p.unit : null, item_id: p.item_id } });
      setOutcome(a, "done");
      if (p.action === "remember") setMemoryVersion((v) => v + 1);
    } catch (ex) {
      setErr(errText(t, ex));
    } finally {
      setConfirming(null);
    }
  };
  const setOutcome = (a: Answer, outcome: "done" | "canceled") => {
    setChat((list) => {
      const next = list.map((x) => (x.id === a.id ? { ...x, outcome } : x));
      if (saved === false) saveChat(next);
      return next;
    });
    // Kept with the conversation, so the change is not offered again when it is opened later.
    if (openId && a.turnId) {
      api(`/api/conversations/${encodeURIComponent(openId)}/turns/${encodeURIComponent(a.turnId)}`, { method: "PATCH", json: { outcome } }).catch(() => {});
    }
  };

  const renamed = (c: Summary) => {
    setConv(c);
    setList((l) => (l ?? []).map((x) => (x.id === c.id ? c : x)));
    loadList();
  };
  const deleted = () => {
    if (openId) forget(`/api/conversations/${openId}`);
    setList((l) => (l ?? []).filter((x) => x.id !== openId));
    shown.current = null;
    setOpenId(null);
    loadList();
  };

  const title = (m: ModelChoice) => (lang === "sr" && m.title_sr ? m.title_sr : m.title_en);
  const bookTitle = (s: Source) => (lang === "sr" && s.book_title_sr ? s.book_title_sr : s.book_title_en);

  const openSource = (src: Source) => {
    if (src.web) {
      if (inTauri()) openUrl(src.url).catch(() => window.open(src.url, "_blank", "noopener"));
      else window.open(src.url, "_blank", "noopener");
    } else setReader(src);
  };

  if (reader) {
    return <Reader t={t} lang={lang} url={reader.url} title={reader.title} onClose={() => setReader(null)} />;
  }

  // A phone away from home (or a hub without the assistant) uses its own model for new questions.
  // A hub whose model needs more memory than it has cannot answer either.
  const hubReady = !!ov && ov.engine !== "missing" && ov.engine !== "no_model" && !tooBig(ov);
  const phoneOwnAi = !isHub && (hubDown || (!!ov && !hubReady));
  const noAi = !!ov && noAiThere(ov);
  const whyPhoneAi = hubDown ? t("aiAwayFromHub") : noAi || (!!ov && tooBig(ov)) ? t("aiHubNoMemory") : t("aiHubHasNoModel");
  const rec = ov?.recommended ? ov.models.find((m) => m.id === ov.recommended) : undefined;
  const stateOf = (id: string) => downloads.find((d) => d.id === id)?.state;
  const installedModels = ov?.models.filter((m) => m.installed) ?? [];
  const statusText: Record<Answer["status"], Key> = {
    searching: "aiSearching",
    starting: "aiStarting",
    thinking: "aiWriting",
    done: "aiNoAnswer",
    failed: "aiWriting",
  };
  // Screen readers hear only the latest answer: its status while it works, then the final text once.
  const latest = chat[chat.length - 1];
  const announce = !latest || restored.current.has(latest.id)
    ? ""
    : latest.status === "failed"
      ? errText(t, new Error(latest.error ?? ""))
      : latest.status === "done"
        ? latest.text.replace(/\[\d+(?:,\s*\d+)*\]/g, "").replace(/\*\*/g, "")
        : t(statusText[latest.status]);
  const showing = openId !== null && saved !== false;
  const canAsk = hubReady && !readOnly && (!showing || conv !== null);

  const side = (head: React.ReactNode) => (
    <ConversationList
      t={t}
      head={head}
      list={query.trim() ? found : list}
      query={query}
      setQuery={setQuery}
      openId={openId}
      open={openChat}
      newChat={newChat}
      showMemory={() => {
        setDrawer(false);
        setMemoryOpen(true);
      }}
    />
  );

  // Nothing fits this computer: say so, and that everything else still works.
  const notHere = ov && !hubReady && noAi && (
    <div className="panel stack left">
      <h2>{t("aiNotHere")}</h2>
      <p className="muted" style={{ margin: 0 }}>{t("aiNotHereLong")}</p>
    </div>
  );
  const needsModel = notHere || (ov && !hubReady && rec && (
    <div className="panel stack left">
      <h2>{t("aiNeedsModel")}</h2>
      {tooBig(ov) && <p className="warn" style={{ margin: 0 }}>{t("errAiTooBig")}</p>}
      <p className="muted" style={{ margin: 0 }}>
        {t("aiRecommendedFor")} {fmtBytes(ov.ram_total)} {t("aiRecommendedMemory")}: <strong>{title(rec)}</strong> ({fmtBytes(rec.size)})
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
              <p className="muted" style={{ margin: 0, fontSize: 13 }}>
                {/* The engine comes along with the model: say which of the two these bytes are. */}
                {t("aiDownloading")} {active === eng ? t("aiEngine") : t("aiModel")} · {fmtBytes(active.bytes_done)} / {fmtBytes(active.bytes_total)}
              </p>
            </>
          );
        }
        return (
          <div className="row wrap">
            <button className="btn" onClick={() => download(rec.id)}>{t("download")}</button>
            <button className="btn secondary" onClick={() => go("settings/assistant/model")}>{t("aiOtherModels")}</button>
          </div>
        );
      })()}
    </div>
  ));

  // The model and whether it runs: under the question box on a wide screen, and on a phone
  // (where room is short) on the screen a new conversation starts with.
  const modelLine = ov && hubReady && (
    <div className="row model-line">
      <label className="row model-pick">
        <span>{t("aiModel")}</span>
        <select value={ov.selected ?? ""} onChange={(e) => select(e.target.value)} disabled={busy}>
          {installedModels.map((m) => (
            <option key={m.id} value={m.id} disabled={m.fits === false}>
              {title(m).match(/\(([^)]+)\)/)?.[1] ?? title(m)}{m.recommended ? ` · ${t("recommended")}` : ""}
              {m.fits === false ? ` · ${t("aiNeedsMoreMemory")}` : ""}
            </option>
          ))}
        </select>
      </label>
      <span>
        {ov.engine === "ready" ? t("aiReady") : ov.engine === "starting" ? t("aiStarting") : ov.engine === "failed" ? t("aiFailed") : t("aiSleeping")}
        {ov.books === 0 && ` · ${t("aiNoLibrary")}`}
        {isHub && ov.engine === "ready" && !busy && (
          <>
            {" · "}
            <button type="button" className="link-btn" onClick={() => api("/api/assistant/stop", { method: "POST" }).then(load).catch(() => {})}>{t("aiFreeMemory")}</button>
          </>
        )}
      </span>
    </div>
  );

  // What a new conversation starts with.
  const start = (
    <div className="stack chat-start">
      {phoneOwnAi ? (
        <>
          <p className="muted" style={{ margin: 0 }}>{whyPhoneAi}</p>
          <PhoneAi t={t} lang={lang} question={fromHome ?? undefined} />
        </>
      ) : (
        <>
          <div className="chat-hello">
            <span className="chat-hello-icon"><Icon name="assistant" size={28} /></span>
            <p>{t("assistantIntro")}</p>
            {hubReady && <p className="muted">{t("aiEmptyChat")}</p>}
          </div>
          {!ov && !err && <p className="muted">{t("aiLoading")}</p>}
          {needsModel}
          {hubReady && ov?.books === 0 && (
            <p className="warn" style={{ margin: 0, fontSize: 14 }}>
              <RichText text={t("aiNoLibraryLong")} />
            </p>
          )}
          {narrow && modelLine && <div className="start-model">{modelLine}</div>}
          {!isHub && hubReady && (
            <div className="stack">
              <button className="btn secondary" onClick={() => setShowPhone(!showPhone)} aria-expanded={showPhone}>
                {showPhone ? "▴ " : "▾ "}{t("aiOnThisPhone")}
              </button>
              {showPhone && <PhoneAi t={t} lang={lang} />}
            </div>
          )}
        </>
      )}
    </div>
  );

  return (
    <div className="assistant">
      {!narrow && <aside className="convo-side">{side(<h1>{t("assistant")}</h1>)}</aside>}
      <section className="chat-pane" aria-label={conv?.title || t("aiNewChat")}>
        {narrow && (
          <div className="chat-top">
            <button type="button" className="ghost-icon" onClick={() => setDrawer(true)} aria-label={t("aiConversations")} title={t("aiConversations")} aria-expanded={drawer}>
              <Icon name="list" />
            </button>
            <h1>{t("assistant")}</h1>
            <HelpLink t={t} topic="assistant" className="ghost-icon" />
            <button type="button" className="ghost-icon" onClick={newChat} aria-label={t("aiNewChat")} title={t("aiNewChat")}>
              <Icon name="plus" />
            </button>
          </div>
        )}
        {showing && conv ? (
          <div className="chat-head">
            <ChatTitle key={conv.id} t={t} conv={conv} readOnly={readOnly} onRenamed={renamed} onDeleted={deleted} />
            {!narrow && <HelpLink t={t} topic="assistant" />}
          </div>
        ) : (
          !narrow && (
            <div className="chat-head">
              <div className="chat-title"><h2 className="muted">{t("aiNewChat")}</h2></div>
              <HelpLink t={t} topic="assistant" />
            </div>
          )
        )}
        <div className="chat-scroll">
          <div className="chat-column">
            {err && <p className="error" role="alert">{err}</p>}
            {readOnly && showing && <p className="muted chat-note">{t("aiReadOnly")}</p>}
            {/* An open conversation must not hide the way to ask: a phone away from
                the hub (or a hub without a model) asks its own AI in a new one. */}
            {showing && phoneOwnAi && (
              <div className="panel notice stack chat-note">
                <p style={{ margin: 0 }}>{whyPhoneAi}</p>
                <div className="row">
                  <button className="btn" onClick={newChat}>{t("aiAskThisPhone")}</button>
                </div>
              </div>
            )}
            {showing && isHub && !!ov && !hubReady && needsModel}
            {showing && unshown && <p className="muted chat-note">{t(unshown === "notKept" ? "aiNotKept" : "errGeneric")}</p>}
            {showing && !conv && !unshown && <p className="muted">{t("aiLoading")}</p>}
            {!showing && chat.length === 0 && start}
            <div className="sr-only" aria-live="polite" aria-atomic="true">{announce}</div>
            {chat.length > 0 && (
              <div className="chat">
                {chat.map((a) => (
                  <div key={a.id} className="exchange">
                    <div className="q">{a.question}</div>
                    <div className="a">
                      {a.status === "failed" ? (
                        <p className="error" style={{ margin: 0 }}>{errText(t, new Error(a.error ?? ""))}</p>
                      ) : (
                        <>
                          {a.text ? <AnswerText text={a.text} sources={a.sources} open={openSource} /> : <p className="muted thinking" style={{ margin: 0 }}>{t(statusText[a.status])}</p>}
                          {a.status === "done" && a.proposal && (
                            <div className="row wrap proposal">
                              {a.outcome === "done" ? (
                                <span className="ok">✓ {t("aiDone")}{a.proposal.action !== "remember" && <> · <a href="#supplies">{t("supplies")}</a></>}</span>
                              ) : a.outcome === "canceled" ? (
                                <span className="muted">{t("aiCancelled")}</span>
                              ) : readOnly ? null : (
                                <>
                                  <button className="btn" onClick={() => confirm(a)} disabled={confirming === a.id}>{t("aiConfirm")}</button>
                                  <button className="btn secondary" onClick={() => setOutcome(a, "canceled")}>{t("cancel")}</button>
                                </>
                              )}
                            </div>
                          )}
                          {a.status === "done" && a.from_supplies && !a.proposal && (
                            <div className="muted" style={{ fontSize: 13, marginTop: 8 }}>{t("aiFromSupplies")} · <a href="#supplies">{t("supplies")}</a></div>
                          )}
                          {a.status === "done" && !a.grounded && !a.fixed && (
                            <p className="warn" style={{ margin: "8px 0 0", fontSize: 13 }}>{t(a.sources.length > 0 ? "aiUncited" : "aiNotGrounded")}</p>
                          )}
                          {a.status === "done" && a.grounded && !a.from_supplies && !a.proposal && a.sources.length > 0 && a.cited === false && (
                            <p className="muted" style={{ margin: "8px 0 0", fontSize: 13 }}>{t("aiNotCited")}</p>
                          )}
                          {a.sources.length > 0 && (a.status === "done" || a.text) && (
                            <div className="sources">
                              <div className="label">{t("aiSources")}</div>
                              {a.sources.map((s) => (
                                <button key={s.n} className={"source" + (s.web ? " web" : "")} onClick={() => openSource(s)}>
                                  <span className="cite static">{s.n}</span> {s.title} <span className="muted">· {bookTitle(s)}{s.web ? ` (${t("aiInternet")})` : ""}</span>
                                </button>
                              ))}
                            </div>
                          )}
                          {a.status === "done" && (a.tokens_per_second > 0 || (a.searched?.length ?? 0) > 0) && (
                            <div className="muted" style={{ fontSize: 12, marginTop: 6 }}>
                              {a.searched && a.searched.length > 0 && !a.from_supplies && `${t("aiSearched")}: ${a.searched.join(", ")}`}
                              {a.searched && a.searched.length > 0 && !a.from_supplies && a.tokens_per_second > 0 && " · "}
                              {a.tokens_per_second > 0 && `${fmtQty(Math.round(a.tokens_per_second * 10) / 10)} ${t("tokensPerSecond")}`}
                            </div>
                          )}
                        </>
                      )}
                      {/* The tools and guides for the question: there even when the AI could not answer. */}
                      {(a.status === "done" || a.status === "failed") && <Suggestions t={t} list={a.suggestions} />}
                    </div>
                  </div>
                ))}
              </div>
            )}
            <div ref={endRef} />
          </div>
        </div>
        {canAsk && (
          <form className="composer" onSubmit={ask}>
            <div className="composer-box">
              <textarea
                ref={box}
                value={question}
                onChange={(e) => setQuestion(e.target.value)}
                onKeyDown={(e) => {
                  // Enter asks on a keyboard; on a phone it starts a new line (the button asks).
                  if (e.key === "Enter" && !e.shiftKey && !narrow) {
                    e.preventDefault();
                    (e.currentTarget.form as HTMLFormElement).requestSubmit();
                  }
                }}
                placeholder={t("askExample")}
                aria-label={t("askSomething")}
                rows={1}
                maxLength={2000}
              />
              {current ? (
                <button type="button" className="send-btn" onClick={() => stop(current.id)} aria-label={t("aiStop")} title={t("aiStop")}>
                  <Icon name="stop" size={18} />
                </button>
              ) : (
                <button className="send-btn" disabled={busy || !question.trim()} aria-label={t("ask")} title={t("ask")}>
                  <Icon name="send" size={20} />
                </button>
              )}
            </div>
            <div className="composer-foot">
              <label className="check-line online-switch">
                <input type="checkbox" checked={online} onChange={(e) => setOnline(e.target.checked)} />
                <span>{t("aiOnline")}</span>
              </label>
              {!narrow && modelLine}
            </div>
          </form>
        )}
      </section>
      {narrow && drawer && (
        <SidePanel side="left" title={t("aiConversations")} closeLabel={t("aiClose")} onClose={() => setDrawer(false)} className="convo-drawer">
          {side(null)}
        </SidePanel>
      )}
      {memoryOpen && <Memory t={t} version={memoryVersion} onClose={() => setMemoryOpen(false)} />}
    </div>
  );
}
