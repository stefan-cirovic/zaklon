import { useEffect, useState } from "react";

type Props = {
  label: string;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void | Promise<void>;
  className?: string;
};

/**
 * A button that needs a second, deliberate tap: the first tap turns it into
 * "Yes, remove / No". Replaces the browser's confirm() dialog, which looks
 * foreign in the app and is easy to hit by accident.
 */
export default function ConfirmButton({ label, confirmLabel, cancelLabel, onConfirm, className = "btn danger" }: Props) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!asking) return;
    const id = setTimeout(() => setAsking(false), 6000);
    return () => clearTimeout(id);
  }, [asking]);

  if (!asking) {
    return (
      <button type="button" className={className} onClick={() => setAsking(true)}>
        {label}
      </button>
    );
  }
  return (
    <span className="confirm" role="group">
      <button
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
      <button type="button" className="btn secondary" onClick={() => setAsking(false)}>
        {cancelLabel}
      </button>
    </span>
  );
}
