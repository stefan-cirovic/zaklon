import type { Key } from "../i18n";
import { isTopicId, topicOf, upgradedTopic } from "../topics";
import { isToolId, toolOf } from "../tools";
import { markFromChat, type Suggestion } from "../conversations";
import { GlyphIcon, type Glyph } from "./ExplorerIcons";

type T = (k: Key) => string;

/** At most this many under an answer. */
const MAX = 3;

/**
 * A suggestion as this app opens it: a tool it has, or a topic's guides in
 * the Library; only its own addresses. An answer saved before the Library
 * had topics links to Add-ons (#addons/water) or to the Library as a tool:
 * those lead to the Library's topic now, and the guides for the maps (which
 * are a tool) are left out.
 */
function current(s: Suggestion): Suggestion | null {
  if (!/^#[\w/?=&:,.-]+$/.test(s.link)) return null;
  if (s.kind === "tool") {
    if (s.id === "library" && s.link === "#library") return { kind: "guides", id: "reference", topic: "reference", link: "#library/reference", label_key: "aiSugLookUp" };
    const ok = isToolId(s.id) && (s.link === `#${s.id}` || s.link.startsWith(`#${s.id}?`) || s.link.startsWith(`#tools/${s.id}?`));
    return ok ? s : null;
  }
  if (s.kind !== "guides" || (s.link !== `#library/${s.topic}` && s.link !== `#addons/${s.topic}`)) return null;
  const topic = upgradedTopic(s.topic);
  if (!isTopicId(topic)) return null;
  return { ...s, id: topic, topic, link: `#library/${topic}`, label_key: topic === "reference" ? "aiSugLookUp" : "aiSugGuides" };
}

function label(t: T, s: Suggestion): string {
  const name = isTopicId(s.topic) ? t(topicOf(s.topic).name) : s.topic;
  const text = t(s.label_key as Key);
  // A text this app does not have (a newer hub): the tool's name, or the topic's guides.
  if (text === s.label_key) return s.kind === "tool" && isToolId(s.id) ? t(toolOf(s.id).title) : t("aiSugGuides").replace("{topic}", name);
  return text.replace("{topic}", name);
}

function glyphOf(s: Suggestion): Glyph {
  if (isTopicId(s.topic)) return topicOf(s.topic).glyph;
  return s.topic === "maps" ? "map" : "book";
}

/**
 * The tools and guides for the question, under its answer: a calculator
 * filled in from what was asked comes first (amber), then the guides of the
 * question's topics in the Library. Each opens its screen; the conversation
 * stays, and the screen offers the way back to it.
 */
export default function Suggestions({ t, list }: { t: T; list: Suggestion[] | undefined }) {
  const shown: Suggestion[] = [];
  for (const s of list ?? []) {
    const c = current(s);
    if (c && !shown.some((x) => x.link === c.link)) shown.push(c);
  }
  if (shown.length === 0) return null;
  return (
    <div className="ai-suggest" role="group" aria-label={t("aiSuggestions")}>
      {shown.slice(0, MAX).map((s) => (
        <a key={s.link} className={"ai-suggest-chip" + (s.kind === "tool" ? " tool" : "")} href={s.link} onClick={markFromChat}>
          <GlyphIcon glyph={glyphOf(s)} size={16} />
          <span>{label(t, s)}</span>
        </a>
      ))}
    </div>
  );
}
