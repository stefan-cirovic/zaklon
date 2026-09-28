// Saved conversations with the assistant. The hub keeps them; each device
// sees only its own (the laptop's for the laptop, each phone its own). A copy
// can be sent to another device of the household, where it is marked with
// the sender's name.

import type { Key } from "./i18n";

export type Source = { n: number; title: string; web?: boolean; url: string; book_title_en: string; book_title_sr: string };
export type Proposal = {
  action: "add" | "use" | "shopping" | "remember";
  item_id: string | null;
  name: string;
  quantity: number;
  unit: string;
  category: string;
  current: number | null;
};

/** An answer as the screen shows it: one being written (polled from the hub) or a saved one. */
export type Answer = {
  /** The hub's answer id (a saved turn without one uses its turn id). */
  id: string;
  /** The saved turn, when the conversation is saved on the hub. */
  turnId?: string;
  from_supplies?: boolean;
  proposal?: Proposal | null;
  /** What happened to the proposal. */
  outcome?: "done" | "canceled" | null;
  question: string;
  status: "searching" | "starting" | "thinking" | "done" | "failed";
  text: string;
  sources: Source[];
  searched?: string[];
  cited?: boolean;
  grounded: boolean;
  /** A fixed reply from the hub (a health question with no checked source), not from the model. */
  fixed?: boolean;
  language: string;
  tokens_per_second: number;
  error: string | null;
};

export type Summary = {
  id: string;
  /** Empty until the first question gives it one. */
  title: string;
  /** A copy sent from another device: "laptop" or a device id, and the sender's name then. */
  from_owner: string | null;
  from_name: string | null;
  created_at: string;
  updated_at: string;
  turns: number;
};

export type SavedTurn = {
  id: string;
  question: string;
  answer: string;
  status: "pending" | "done" | "failed";
  error: string | null;
  answer_id: string | null;
  sources: Source[];
  details: Partial<Pick<Answer, "grounded" | "cited" | "fixed" | "from_supplies" | "searched" | "proposal" | "language" | "tokens_per_second">>;
  outcome: "done" | "canceled" | null;
  created_at: string;
  answered_at: string | null;
};

export type Conversation = Summary & { turns: SavedTurn[] };

/** The owner name of the laptop, as the hub stores it. */
export const LAPTOP = "laptop";

/** The conversation open when the Assistant was left, opened again on return. */
const OPEN = "zaklon.chat.open";

export function readOpen(): string | null {
  try {
    return localStorage.getItem(OPEN) || null;
  } catch {
    return null;
  }
}

export function writeOpen(id: string | null) {
  try {
    if (id) localStorage.setItem(OPEN, id);
    else localStorage.removeItem(OPEN);
  } catch {
    /* private mode: just for this session */
  }
}

/**
 * A question typed on Home, waiting for the Assistant: the Assistant opens
 * with a new conversation and asks it there, in its usual way, as soon as it
 * knows the hub's assistant is ready (or puts it in the box when it cannot).
 */
let handedOver: string | null = null;

export function askInAssistant(question: string) {
  handedOver = question;
}

/** The question handed over from Home, if any (reading it does not take it). */
export function questionFromHome(): string | null {
  return handedOver;
}

export function clearQuestionFromHome() {
  handedOver = null;
}

/** Open a saved conversation when the Assistant is shown next. */
export function openInAssistant(id: string) {
  handedOver = null;
  writeOpen(id);
}

/** A saved turn as the screen shows it; one still pending is followed until its answer is done. */
export function toAnswer(t: SavedTurn): Answer {
  const d = t.details ?? {};
  return {
    id: t.answer_id ?? t.id,
    turnId: t.id,
    question: t.question,
    status: t.status === "pending" ? "searching" : t.status,
    text: t.answer,
    sources: Array.isArray(t.sources) ? t.sources : [],
    searched: d.searched ?? [],
    cited: d.cited,
    grounded: d.grounded ?? false,
    fixed: d.fixed ?? false,
    from_supplies: d.from_supplies ?? false,
    proposal: d.proposal ?? null,
    language: d.language ?? "",
    tokens_per_second: d.tokens_per_second ?? 0,
    error: t.error,
    outcome: t.outcome,
  };
}

/** Who sent a copy, as a line under its title; null for a conversation of this device's own. */
export function fromText(t: (k: Key) => string, c: Pick<Summary, "from_owner" | "from_name">): string | null {
  if (!c.from_owner && !c.from_name) return null;
  if (c.from_owner === LAPTOP) return t("aiFromLaptop");
  return t("aiFromDevice").replace("{name}", c.from_name ?? "?");
}

/** Lower case, without diacritics (and đ as dj), so "caj" finds "Čaj". */
export function fold(s: string): string {
  return s
    .toLowerCase()
    .replace(/đ/g, "dj")
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "");
}

/** Conversations whose title holds every word of `query` (used when the hub cannot be asked). */
export function filterByTitle(list: Summary[], query: string): Summary[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (words.length === 0) return list;
  return list.filter((c) => {
    const title = fold(c.title);
    return words.every((w) => title.includes(w));
  });
}

export type Group = { key: Key; items: Summary[] };

/** The list by when each was last used: today, yesterday, the 7 days before, and older. */
export function groupByRecency(list: Summary[], now = new Date()): Group[] {
  // Midnight `days` days ago, in local time (a day is not always 24 hours long).
  const midnight = (days: number) => new Date(now.getFullYear(), now.getMonth(), now.getDate() - days).getTime();
  const [today, yesterday, week] = [midnight(0), midnight(1), midnight(7)];
  const groups: Group[] = [
    { key: "aiToday", items: [] },
    { key: "aiYesterday", items: [] },
    { key: "aiPrevious7", items: [] },
    { key: "aiOlder", items: [] },
  ];
  for (const c of list) {
    const at = Date.parse(c.updated_at);
    const d = Number.isNaN(at) ? 0 : at;
    const i = d >= today ? 0 : d >= yesterday ? 1 : d >= week ? 2 : 3;
    groups[i].items.push(c);
  }
  return groups.filter((g) => g.items.length > 0);
}
