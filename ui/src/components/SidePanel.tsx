import { useEffect, useId, useRef, type ReactNode } from "react";
import { Icon } from "./Icon";

type Props = {
  /** Which edge it slides in from. */
  side: "left" | "right";
  title: string;
  closeLabel: string;
  onClose: () => void;
  children: ReactNode;
  className?: string;
};

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * A panel over the screen, along one edge, with the rest dimmed: a dialog.
 * Escape, the close button or a tap beside it closes it. Focus moves into it
 * when it opens, stays inside while it is open, and goes back to where it
 * was when it closes.
 */
export default function SidePanel({ side, title, closeLabel, onClose, children, className = "" }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();
  const close = useRef(onClose);
  useEffect(() => {
    close.current = onClose;
  });

  useEffect(() => {
    const before = document.activeElement as HTMLElement | null;
    const panel = ref.current;
    const first = panel?.querySelector<HTMLElement>("[data-autofocus]") ?? panel?.querySelector<HTMLElement>(FOCUSABLE);
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close.current();
        return;
      }
      if (e.key !== "Tab" || !panel) return;
      const items = [...panel.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null);
      if (items.length === 0) return;
      const [a, z] = [items[0], items[items.length - 1]];
      if (e.shiftKey && document.activeElement === a) {
        e.preventDefault();
        z.focus();
      } else if (!e.shiftKey && document.activeElement === z) {
        e.preventDefault();
        a.focus();
      }
    };
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("keydown", onKey);
      if (before && document.contains(before)) before.focus();
    };
  }, []);

  return (
    <>
      <div className="side-backdrop" onClick={() => close.current()} aria-hidden="true" />
      <div ref={ref} className={`side-panel ${side} ${className}`} role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <div className="side-panel-head">
          <h2 id={titleId}>{title}</h2>
          <button type="button" className="ghost-icon" onClick={() => close.current()} aria-label={closeLabel} title={closeLabel}>
            <Icon name="close" size={20} />
          </button>
        </div>
        <div className="side-panel-body">{children}</div>
      </div>
    </>
  );
}
