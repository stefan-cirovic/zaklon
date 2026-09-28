import { useState } from "react";
import type { Key } from "../i18n";
import { errText } from "../errors";
import { Icon } from "../components/Icon";
import { TOOLS, type ToolId } from "../tools";

type T = (k: Key) => string;
type Props = {
  t: T;
  go: (tab: string) => void;
  /** The tool in the bar, the same for the whole household. */
  pinned: ToolId | null;
  /** Laptop only: pin a tool (replacing the one before) or none. */
  pin: ((id: ToolId | null) => Promise<void>) | null;
};

/** Every tool, with what it is for; each opens its screen, and one can be pinned to the bar. */
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
      <ul className="tool-grid" aria-label={t("tools")}>
        {TOOLS.map((tool) => {
          const title = t(tool.title);
          const on = pinned === tool.id;
          return (
            <li key={tool.id} className={"tool-card" + (on ? " pinned" : "")}>
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
              {pin && (
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
        })}
        {/* Not a tool of its own (it cannot be pinned), but found here with them. */}
        <li className="tool-card help-card">
          <a className="tool-open" href="#help">
            <span className="tool-icon">
              <Icon name="help" size={26} />
            </span>
            <span className="tool-text">
              <span className="tool-title">{t("help")}</span>
              <span className="tool-desc">{t("helpToolDesc")}</span>
            </span>
          </a>
        </li>
      </ul>
      <p className="muted tools-note">{pin ? t("pinForHousehold") : t("pinOnLaptop")}</p>
    </div>
  );
}
