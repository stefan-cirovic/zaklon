import type { Key, Lang } from "../i18n";
import type { IconName } from "../components/Icon";
import { fold } from "../settings";
import { HELP_TOPICS, type HelpBlock, type HelpContent, type HelpSection, type HelpTopicId } from "./types";

export { HELP_TOPICS, type HelpBlock, type HelpContent, type HelpSection, type HelpTopic, type HelpTopicId } from "./types";

/** The Help home: the topics in groups, in the order of the pages. */
export const HELP_GROUPS: { title: Key; topics: HelpTopicId[] }[] = [
  { title: "helpGroupStart", topics: ["start", "pairing"] },
  { title: "helpGroupScreens", topics: ["home", "assistant", "supplies", "library", "maps", "addons", "household"] },
  { title: "helpGroupTrouble", topics: ["offline", "troubleshooting"] },
];

export const HELP_ICONS: Record<HelpTopicId, IconName> = {
  start: "help",
  pairing: "phone",
  home: "home",
  assistant: "assistant",
  supplies: "supplies",
  library: "library",
  maps: "maps",
  addons: "addons",
  household: "household",
  offline: "offline",
  troubleshooting: "wrench",
};

export function isHelpTopic(id: unknown): id is HelpTopicId {
  return typeof id === "string" && (HELP_TOPICS as readonly string[]).includes(id);
}

/** The address of a help page, or of one section on it. */
export function helpHref(topic?: HelpTopicId, section?: string): string {
  return `#help${topic ? `/${topic}` : ""}${section ? `/${section}` : ""}`;
}

/** The topic and section in an address like "#help/supplies/scan" (nulls for the Help home). */
export function helpRoute(hash: string): { topic: HelpTopicId | null; section: string | null } {
  const [tab, topic, section] = hash.replace(/^#/, "").split("/");
  if (tab !== "help" || !isHelpTopic(topic)) return { topic: null, section: null };
  return { topic, section: section || null };
}

/** Each language in its own file, loaded when Help first opens (the rest of the app does not need it). */
const LOADERS: Record<Lang, () => Promise<{ default: HelpContent }>> = {
  en: () => import("./en"),
  sr: () => import("./sr"),
};
const loaded: Partial<Record<Lang, HelpContent>> = {};

export function helpLoaded(lang: Lang): HelpContent | null {
  return loaded[lang] ?? null;
}

export async function loadHelp(lang: Lang): Promise<HelpContent> {
  const content = loaded[lang] ?? (await LOADERS[lang]()).default;
  loaded[lang] = content;
  return content;
}

/** Text without its marks: "**Save**" -> "Save", "[Home](#home)" -> "Home". */
export function plainText(s: string): string {
  return s.replace(/\*\*(.+?)\*\*/g, "$1").replace(/\[([^\]]+)\]\((#[^)\s]*)\)/g, "$1");
}

/** The pieces of a text: plain text, **bold** and [links](#screen). */
export type Inline = { text: string } | { bold: string } | { link: string; href: string };

export function parseInline(s: string): Inline[] {
  const out: Inline[] = [];
  const re = /\*\*(.+?)\*\*|\[([^\]]+)\]\((#[^)\s]*)\)/g;
  let last = 0;
  for (let m = re.exec(s); m; m = re.exec(s)) {
    if (m.index > last) out.push({ text: s.slice(last, m.index) });
    out.push(m[1] !== undefined ? { bold: m[1] } : { link: m[2], href: m[3] });
    last = m.index + m[0].length;
  }
  if (last < s.length) out.push({ text: s.slice(last) });
  return out;
}

function blockText(b: HelpBlock): string {
  if ("p" in b) return b.p;
  if ("note" in b) return b.note;
  if ("warn" in b) return b.warn;
  return ("steps" in b ? b.steps : b.list).join(" ");
}

/** Folded like the settings search, and once more with "dj" as "d", so "uredaji" finds "uređaji" too. */
function searchable(s: string): string {
  const f = fold(plainText(s));
  return `${f} ${f.replace(/dj/g, "d")}`;
}

export type HelpHit = {
  topic: HelpTopicId;
  /** The sections that hold every word typed (none when the words are spread over the page). */
  sections: HelpSection[];
};

/**
 * The topics that have every word typed somewhere on their page, without
 * minding accents or case ("sifrovanje" finds "Šifrovanje"): first those with
 * every word in the title, then in the title or summary, then the rest, each
 * in the order of the pages.
 */
export function searchHelp(content: HelpContent, query: string): HelpHit[] {
  const words = fold(query).split(" ").filter(Boolean);
  if (words.length === 0) return [];
  const hits: { hit: HelpHit; rank: number }[] = [];
  for (const id of HELP_TOPICS) {
    const topic = content[id];
    const title = searchable(topic.title);
    const head = `${title} ${searchable(topic.summary)}`;
    const sections = topic.sections.map((s) => ({ s, text: searchable([s.title, ...s.body.map(blockText)].join(" ")) }));
    const all = `${head} ${sections.map((x) => x.text).join(" ")}`;
    if (!words.every((w) => all.includes(w))) continue;
    const rank = words.every((w) => title.includes(w)) ? 0 : words.every((w) => head.includes(w)) ? 1 : 2;
    const inSections = sections.filter((x) => words.every((w) => x.text.includes(w))).map((x) => x.s);
    hits.push({ hit: { topic: id, sections: inSections }, rank });
  }
  return hits.sort((a, b) => a.rank - b.rank).map((x) => x.hit);
}
