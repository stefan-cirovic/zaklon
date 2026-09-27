import { useEffect, useRef, useState } from "react";

type Props = {
  label: string;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void | Promise<void>;
  className?: string;
  /** Accessible name for the first button, for icon-only labels like "×". */
  ariaLabel?: string;
};

/**
 * A button that needs a second, deliberate tap: the first tap turns it into
 * "Yes, remove / No". Replaces the browser's confirm() dialog, which looks
 * foreign in the app and is easy to hit by accident.
 *
 * Focus moves to "Yes" when the question appears and back to the original
 * button when it closes, so keyboard and screen reader users don't get lost.
 * There is no auto-cancel timer: the question stays until answered.
 */
export default function ConfirmButton({ label, confirmLabel, cancelLabel, onConfirm, className = "btn danger", ariaLabel }: Props) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  const firstRef = useRef<HTMLButtonElement>(null);
  const yesRef = useRef<HTMLButtonElement>(null);
  const wasAsking = useRef(false);

  useEffect(() => {
    if (asking) {
      yesRef.current?.focus();
    } else if (wasAsking.current) {
      firstRef.current?.focus();
    }
    wasAsking.current = asking;
  }, [asking]);

  if (!asking) {
    return (
      <button ref={firstRef} type="button" className={className} aria-label={ariaLabel} onClick={() => setAsking(true)}>
        {label}
      </button>
    );
  }
  return (
    <span
      className="confirm"
      role="group"
      aria-label={ariaLabel ?? label}
      onKeyDown={(e) => {
        if (e.key === "Escape" && !busy) setAsking(false);
      }}
    >
      <button
        ref={yesRef}
        type="button"
        className="btn danger-solid"
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          try {
            await onConfirm();
          } finally {
            setBusy(false);
            setAsking(false);
          }
        }}
      >
        {confirmLabel}
      </button>
      <button
        type="button"
        className="btn secondary"
        onClick={() => setAsking(false)}
      >
        {cancelLabel}
      </button>
    </span>
  );
}
