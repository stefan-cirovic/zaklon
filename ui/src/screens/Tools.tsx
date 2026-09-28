import { useId, useState } from "react";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { Icon } from "../components/Icon";
import { GlyphIcon, ToolIcon } from "../components/ExplorerIcons";
import { TOOL_SECTIONS, TOOLS, toolCategories, type ToolId } from "../tools";

type T = (k: Key) => string;
type Props = {
  t: T;
  go: (tab: string) => void;
  /** The tool in the bar, the same for the whole household. */
  pinned: ToolId | null;
  /** Laptop only: pin a tool (replacing the one before) or none. */
  pin: ((id: ToolId | null) => Promise<void>) | null;
};

type Section = (typeof TOOL_SECTIONS)[number];
type Tool = (typeof TOOLS)[number];

/**
 * Every tool, sorted by topic (the same categories as the Add-ons folders):
 * each topic with what it is about, its tools, and a link to its guides in
 * Add-ons, so the tools and the guides for a topic are in one place. A tool
 * about several topics is under each. Each tool opens its screen, and one
 * can be pinned to the bar.
 */
export default function Tools({ t, go, pinned, pin }: Props) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const toggle = async (id: ToolId) => {
    if (!pin) return;
    setBusy(true);
    setErr(null);
    try {
      await pin(pinned === id ? null : id);
    } catch (e) {
      setErr(errText(t, e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="stack">
      <div className="page-head">
        <h1>{t("tools")}</h1>
        <p className="muted">{t("toolsIntro")}</p>
      </div>
      {err && <p className="error" role="alert">{err}</p>}
      <div className="topic-grid">
        {TOOL_SECTIONS.map((s) => (
          <TopicSection
            key={s.id}
            t={t}
            section={s}
            tools={TOOLS.filter((tool) => toolCategories(tool).includes(s.id))}
            go={go}
            pinned={pinned}
            toggle={pin ? toggle : null}
            busy={busy}
          />
        ))}
      </div>
      <p className="muted tools-note">{pin ? t("pinForHousehold") : t("pinOnLaptop")}</p>
    </div>
  );
}

function TopicSection({
  t,
  section,
  tools,
  go,
  pinned,
  toggle,
  busy,
}: {
  t: T;
  section: Section;
  tools: Tool[];
  go: (tab: string) => void;
  pinned: ToolId | null;
  toggle: ((id: ToolId) => void) | null;
  busy: boolean;
}) {
  const id = useId();
  const name = t(section.name);
  // Every topic has its folder in Add-ons; Downloads is Add-ons itself.
  const guides = section.id === "downloads" ? null : t(section.id === "maps" ? "topicMapsGuides" : "topicGuides");
  return (
    <section className="topic-panel" aria-labelledby={id}>
      <div className="topic-head">
        <span className="topic-icon">
          <GlyphIcon glyph={section.glyph} size={28} />
        </span>
        <div className="topic-text">
          <h2 id={id}>{name}</h2>
          <p className="muted">{t(section.desc)}</p>
        </div>
      </div>
      {tools.length > 0 && (
        <ul className="topic-tools">
          {tools.map((tool) => (
            <ToolCard key={tool.id} t={t} tool={tool} on={pinned === tool.id} go={go} toggle={toggle} busy={busy} />
          ))}
        </ul>
      )}
      {guides && (
        <a className="topic-guides" href={`#addons/${section.id}`} aria-label={`${guides}: ${name}`}>
          <span>{guides}</span>
          <ToolIcon name="chevron" size={16} />
        </a>
      )}
    </section>
  );
}

function ToolCard({
  t,
  tool,
  on,
  go,
  toggle,
  busy,
}: {
  t: T;
  tool: Tool;
  on: boolean;
  go: (tab: string) => void;
  toggle: ((id: ToolId) => void) | null;
  busy: boolean;
}) {
  const title = t(tool.title);
  return (
    <li className={"tool-card" + (on ? " pinned" : "")}>
      <button className="tool-open" onClick={() => go(tool.id)}>
        <span className="tool-icon">
          <Icon name={tool.id} size={26} />
        </span>
        <span className="tool-text">
          <span className="tool-title">
            {title}
            {on && (
              <span className="badge soon tool-pinned">
                <Icon name="pin" size={12} /> {t("pinned")}
              </span>
            )}
          </span>
          <span className="tool-desc">{t(tool.desc)}</span>
        </span>
      </button>
      {toggle && (
        <div className="tool-actions">
          <button
            className="btn secondary small"
            aria-label={`${on ? t("unpin") : t("pinToBar")}: ${title}`}
            onClick={() => toggle(tool.id)}
            disabled={busy}
          >
            <Icon name="pin" size={16} />
            {on ? t("unpin") : t("pinToBar")}
          </button>
        </div>
      )}
    </li>
  );
}
