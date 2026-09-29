import type { Key } from "../i18n";
import { TOPICS } from "../topics";
import { isToolId, toolOf } from "../tools";
import { markFromChat, type Suggestion } from "../conversations";
import { GlyphIcon } from "./ExplorerIcons";

type T = (k: Key) => string;

/** At most this many under an answer. */
const MAX = 3;

/** A suggestion this app can open: a tool it has, or a topic's folder in Add-ons; only its own addresses. */
function usable(s: Suggestion): boolean {
  if (!/^#[\w/?=&:,.-]+$/.test(s.link)) return false;
  if (s.kind === "tool") return isToolId(s.id) && (s.link === `#${s.id}` || s.link.startsWith(`#${s.id}?`) || s.link.startsWith(`#tools/${s.id}?`));
  return s.kind === "guides" && TOPICS.some((x) => x.id === s.topic) && s.link === `#addons/${s.topic}`;
}

function label(t: T, s: Suggestion): string {
  const topic = TOPICS.find((x) => x.id === s.topic);
  const name = topic ? t(topic.name) : s.topic;
  const text = t(s.label_key as Key);
  // A text this app does not have (a newer hub): the tool's name, or the topic's guides.
  if (text === s.label_key) return s.kind === "tool" && isToolId(s.id) ? t(toolOf(s.id).title) : `${t("topicGuides")}: ${name}`;
  return text.replace("{topic}", name);
}

/**
 * The tools and guides for the question, under its answer: a calculator
 * filled in from what was asked comes first (amber), then the guides of the
 * question's topics. Each opens its screen; the conversation stays, and the
 * screen offers the way back to it.
 */
export default function Suggestions({ t, list }: { t: T; list: Suggestion[] | undefined }) {
  const shown = (list ?? []).filter(usable).slice(0, MAX);
  if (shown.length === 0) return null;
  return (
    <div className="ai-suggest" role="group" aria-label={t("aiSuggestions")}>
      {shown.map((s) => {
        const glyph = TOPICS.find((x) => x.id === s.topic)?.glyph ?? "book";
        return (
          <a key={s.link} className={"ai-suggest-chip" + (s.kind === "tool" ? " tool" : "")} href={s.link} onClick={markFromChat}>
            <GlyphIcon glyph={glyph} size={16} />
            <span>{label(t, s)}</span>
          </a>
        );
      })}
    </div>
  );
}
