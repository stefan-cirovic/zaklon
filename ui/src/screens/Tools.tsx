import { useState } from "react";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { Icon } from "../components/Icon";
import HelpLink from "../components/HelpLink";
import RichText from "../components/RichText";
import { TOOLS, type ToolId } from "../tools";
import { topicOf, type TopicId } from "../topics";

type T = (k: Key) => string;
type Props = {
  t: T;
  go: (tab: string) => void;
  /** The tool in the bar, the same for the whole household. */
  pinned: ToolId | null;
  /** Laptop only: pin a tool (replacing the one before) or none. */
  pin: ((id: ToolId | null) => Promise<void>) | null;
};

type Tool = (typeof TOOLS)[number];

/**
 * The tools, each once, as a grid of tiles: each opens its screen, and one
 * can be pinned to the bar. A tool about topics of the Library has a quiet
 * line to their guides. What there is to read is in the Library, and what
 * is on the hub in Settings › Storage & Downloads: the line at the bottom
 * says so.
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
        <div className="title-line">
          <h1>{t("tools")}</h1>
          <HelpLink t={t} topic="tools" />
        </div>
        <p className="muted">{t("toolsIntro")}</p>
      </div>
      {err && <p className="error" role="alert">{err}</p>}
      <ul className="tool-grid" aria-label={t("tools")}>
        {TOOLS.map((tool) => (
          <ToolCard key={tool.id} t={t} tool={tool} on={pinned === tool.id} go={go} toggle={pin ? toggle : null} busy={busy} />
        ))}
      </ul>
      <p className="muted tools-note">{pin ? t("pinForHousehold") : t("pinOnLaptop")}</p>
      <p className="muted tools-note">
        <RichText text={t("toolsElsewhere")} />
      </p>
    </div>
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
  const topics = tool.topics as readonly TopicId[];
  return (
    <li className={"tool-card" + (on ? " pinned" : "")} data-tool={tool.id}>
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
      {(topics.length > 0 || toggle) && (
        <div className="tool-actions">
          {topics.length > 0 && (
            <p className="tool-guides">
              {t("toolGuides")}:{" "}
              {topics.map((id, i) => (
                <span key={id}>
                  {i > 0 && " · "}
                  <a href={`#library/${id}`} aria-label={`${t("toolGuides")}: ${t(topicOf(id).name)}`}>
                    {t(topicOf(id).name)}
                  </a>
                </span>
              ))}
            </p>
          )}
          {toggle && (
            <button
              className="btn secondary small"
              aria-label={`${on ? t("unpin") : t("pinToBar")}: ${title}`}
              onClick={() => toggle(tool.id)}
              disabled={busy}
            >
              <Icon name="pin" size={16} />
              {on ? t("unpin") : t("pinToBar")}
            </button>
          )}
        </div>
      )}
    </li>
  );
}
